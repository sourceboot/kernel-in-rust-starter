/* evtest -- lab 26: the keyboard the way a Linux program finds it.
 *
 *   evtest probe     the device, before any key is typed
 *   evtest           the keys someone types
 *
 * Finds its device as doomgeneric's linuxvt backend does: an `event*` entry
 * of /dev/input, opened read-only for EVIOCGBIT(0) (EV_KEY must be there)
 * and EVIOCGBIT(EV_KEY) (KEY_ENTER and KEY_A must be), closed, opened again
 * O_NONBLOCK and grabbed (EVIOCGRAB 1); a second open's grab must be
 * refused with -EBUSY, and so must its release, with -EINVAL: it does not
 * hold the grab. The grab follows the open that holds it, however many
 * readers are open: with the second still open, the holder lets go (0), a
 * third open takes the grab (0) and closes, a fourth takes it (0) and lets
 * go (0), and the holder takes it back (0); `follow` is the first of those
 * that failed, or 0. A reader that let go holds nothing: once the fourth
 * has closed, the second's grab is still refused, and `kept` is what it
 * returned (-EBUSY). It prints
 *
 *   caps <entry> ev <bytes>:<first byte, hex> key <bytes>:<KEY_ENTER><KEY_A><KEY_UP> lack <n> spare <n> grab <0|-errno> busy <0|-errno> steal <0|-errno> follow <0|-errno> kept <0|-errno>
 *
 * `lack`: how many of the codes a set-1 key map gives (1 to 88 but 84) the
 * key bitmap leaves clear; `spare`: how many bits it sets that no key of a
 * keyboard has (0, 84, and every code from 256, the buttons).
 *
 * `evtest probe` then asks poll about the files no key is needed for and
 * lets the grab go, one line each, and exits:
 *
 *   files <ok|what failed>
 *   end release <0|-errno> again <0|-errno>
 *
 * `files`: the empty device (0 at once; 0 after waiting 30 ms), a pipe
 * (empty: the write end POLLOUT only; a byte: POLLIN; no writer left:
 * POLLHUP; no reader left: the write end POLLOUT and POLLERR), and fds
 * that are not open (POLLNVAL) or negative (ignored).
 *
 * `evtest` with no argument, after `caps`, prints in this order:
 *
 *   ready
 *   block <ok|what the read returned>
 *   keys <event> <event> ... time <ok|what>
 *   inject
 *   poll <ok|what failed>
 *   eagain <ok|what read returned>
 *   end release <0|-errno> again <0|-errno>
 *
 * Before `ready` it asks poll about /dev/console (POLLIN once the line
 * already typed into it is there, POLLOUT always; 0 once it is read),
 * and clears O_NONBLOCK. Right after `ready`, before anyone has typed, it
 * reads one event, which must sleep until the first key comes (`block`);
 * it sets O_NONBLOCK again and reads the rest, one event per
 * pass of a loop that waits in poll(50 ms) between reads (as doomgeneric
 * does, but reading even when poll said no, so a wrong poll costs this
 * line nothing), until 20 events or 5 seconds. Each EV_KEY prints
 * as <code>+ (pressed, 1), <code>- (released, 0) or <code>?<value>; each
 * EV_SYN SYN_REPORT as `.`; anything else as t<type>:<code>=<value>. `time`
 * checks every event's timestamp: microseconds under a million, never
 * going back, inside [ready, the last read] by CLOCK_REALTIME.
 * After `inject` someone puts one key in (press and release at once);
 * poll(3 s) must wake for it within 2 s with POLLIN, poll(0) must say
 * POLLIN before each of its four events and 0 after them. Last, a read of
 * the empty device must be -1 with EAGAIN; then the grab is let go
 * (0) and let go again (-EINVAL).
 * It exits 0 once every line is printed; `evtest: <what>` and 1 when it
 * cannot go on.
 */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/input.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <time.h>
#include <unistd.h>

#define DEV "/dev/input/"
#define EVENTS 20

static char bad[1024];

static void miss(const char *what, long got) {
    size_t n = strlen(bad);
    snprintf(bad + n, sizeof bad - n, "%s%s %ld", n ? "; " : "", what, got);
}

static int64_t ns(clockid_t c) {
    struct timespec t;
    clock_gettime(c, &t);
    return (int64_t)t.tv_sec * 1000000000 + t.tv_nsec;
}

static int fail(const char *what) {
    printf("evtest: %s: %s\n", what, strerror(errno));
    return 1;
}

static int bit(const unsigned char *b, int n) {
    return (b[n / 8] >> (n % 8)) & 1;
}

/* The key bitmap's codes no set-1 key map leaves out, and those no key has. */
static void key_codes(const unsigned char *key, int *lack, int *spare) {
    *lack = *spare = 0;
    for (int c = 1; c <= 88; c++)
        if (c != 84 && !bit(key, c)) ++*lack;
    *spare = bit(key, 0) + bit(key, 84);
    for (int c = 256; c <= KEY_MAX; c++) *spare += bit(key, c);
}

static char *show(char *out, const struct input_event *e) {
    if (e->type == EV_KEY && (e->value == 0 || e->value == 1))
        sprintf(out, " %u%c", e->code, e->value ? '+' : '-');
    else if (e->type == EV_KEY)
        sprintf(out, " %u?%d", e->code, e->value);
    else if (e->type == EV_SYN && e->code == SYN_REPORT && e->value == 0)
        strcpy(out, " .");
    else
        sprintf(out, " t%u:%u=%d", e->type, e->code, e->value);
    return out + strlen(out);
}

/* poll on n fds, each {fd, events}; returns poll's result, revents into out. */
static int ask(int n, const int *fds, const short *events, short *out, int ms) {
    struct pollfd p[4];
    for (int i = 0; i < n; i++) p[i] = (struct pollfd){fds[i], events[i], 0x7f00};
    int r = poll(p, n, ms);
    for (int i = 0; i < n; i++) out[i] = p[i].revents;
    return r;
}

/* `evtest probe`: poll on the files no key is needed for. */
static void files(int fd) {
    short rev[4];
    int r = ask(1, &fd, (short[]){POLLIN}, rev, 0);
    if (r != 0 || rev[0] != 0) miss("poll(0) on the empty device returned", r ? r : rev[0]);
    int64_t t0 = ns(CLOCK_MONOTONIC);
    r = ask(1, &fd, (short[]){POLLIN}, rev, 30);
    int64_t waited = (ns(CLOCK_MONOTONIC) - t0) / 1000000;
    if (r != 0 || rev[0] != 0) miss("poll(30) on the empty device returned", r ? r : rev[0]);
    if (waited < 30) miss("poll(30) on the empty device came back after (ms)", waited);

    int pp[2];
    if (pipe(pp) != 0) {
        miss("pipe: errno", errno);
    } else {
        r = ask(2, pp, (short[]){POLLIN, POLLOUT}, rev, 0);
        if (r != 1 || rev[0] != 0 || rev[1] != POLLOUT) miss("poll on an empty pipe's two ends: revents", rev[0] << 8 | rev[1]);
        if (write(pp[1], "x", 1) != 1) miss("write to the pipe: errno", errno);
        r = ask(2, pp, (short[]){POLLIN, POLLOUT}, rev, 0);
        if (r != 2 || rev[0] != POLLIN || rev[1] != POLLOUT) miss("poll on a pipe holding a byte: revents", rev[0] << 8 | rev[1]);
        close(pp[1]);
        char c;
        if (read(pp[0], &c, 1) != 1) miss("read from the pipe: errno", errno);
        r = ask(1, pp, (short[]){POLLIN}, rev, 0);
        if (r != 1 || rev[0] != POLLHUP) miss("poll on an empty pipe with no writer: revents", rev[0]);
        close(pp[0]);
    }
    if (pipe(pp) != 0) {
        miss("pipe: errno", errno);
    } else {
        close(pp[0]);
        r = ask(1, &pp[1], (short[]){POLLOUT}, rev, 0);
        if (r != 1 || rev[0] != (POLLOUT | POLLERR)) miss("poll on a pipe's write end with no reader: revents", rev[0]);
        close(pp[1]);
    }

    r = ask(2, (int[]){-1, 29}, (short[]){POLLIN, POLLIN}, rev, 0);
    if (r != 1 || rev[0] != 0 || rev[1] != POLLNVAL) miss("poll on fds -1 and 29 (not open): revents", rev[0] << 8 | rev[1]);
}

/* `evtest`: the console, holding the line typed into it before the run. */
static void console(void) {
    short rev[4];
    int con = open("/dev/console", O_RDWR);
    if (con < 0) {
        miss("open /dev/console: errno", errno);
        return;
    }
    int r = ask(2, (int[]){con, con}, (short[]){POLLIN, POLLOUT}, rev, 1000);
    if (r != 2 || rev[0] != POLLIN || rev[1] != POLLOUT) miss("poll(POLLIN, POLLOUT) on a console holding a line: revents", rev[0] << 8 | rev[1]);
    char line[16] = {0};
    ssize_t n = read(con, line, sizeof line - 1);
    if (n != 3 || strcmp(line, "zq\n") != 0) miss("reading that line returned", n);
    r = ask(1, &con, (short[]){POLLIN}, rev, 0);
    if (r != 0) miss("poll(0) on the console after reading its line: revents", rev[0]);
    close(con);
}

/* Let the grab go, twice: the second must be -EINVAL. */
static void let_go(int fd) {
    int rel = ioctl(fd, EVIOCGRAB, 0) == 0 ? 0 : -errno;
    int again = ioctl(fd, EVIOCGRAB, 0) == 0 ? 0 : -errno;
    printf("end release %d again %d\n", rel, again);
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    int probe = argc > 1 && strcmp(argv[1], "probe") == 0;
    char name[64] = "", path[96];
    DIR *d = opendir("/dev/input");
    if (!d) return fail("opendir /dev/input");
    for (struct dirent *e; (e = readdir(d));)
        if (strncmp(e->d_name, "event", 5) == 0 && !name[0]) snprintf(name, sizeof name, "%s", e->d_name);
    closedir(d);
    if (!name[0]) {
        errno = ENOENT;
        return fail("no event* entry in /dev/input");
    }
    snprintf(path, sizeof path, DEV "%s", name);

    unsigned char ev[8] = {0}, key[KEY_MAX / 8 + 1] = {0};
    int fd = open(path, O_RDONLY);
    if (fd < 0) return fail("open read-only");
    int nev = ioctl(fd, EVIOCGBIT(0, sizeof ev), ev);
    int nkey = ioctl(fd, EVIOCGBIT(EV_KEY, sizeof key), key);
    close(fd);
    fd = open(path, O_RDONLY | O_NONBLOCK);
    if (fd < 0) return fail("open O_NONBLOCK");
    int grab = ioctl(fd, EVIOCGRAB, 1) == 0 ? 0 : -errno;
    int other = open(path, O_RDONLY | O_NONBLOCK);
    int busy = other < 0 ? -1000 : ioctl(other, EVIOCGRAB, 1) == 0 ? 0 : -errno;
    /* The second open never held the grab, so it cannot let it go. */
    int steal = other < 0 ? -1000 : ioctl(other, EVIOCGRAB, 0) == 0 ? 0 : -errno;
    /* The grab is the holder's, not the last reader's: with `other` still
     * open the holder lets go, and a holder that closes lets go too. */
    int follow = -1000, kept = -1000;
    if (other >= 0) {
        follow = ioctl(fd, EVIOCGRAB, 0) == 0 ? 0 : -errno;
        int third = open(path, O_RDONLY | O_NONBLOCK);
        int took = third < 0 ? -1000 : ioctl(third, EVIOCGRAB, 1) == 0 ? 0 : -errno;
        if (third >= 0) close(third);
        /* A reader that held the grab and let it go. */
        int fourth = open(path, O_RDONLY | O_NONBLOCK);
        int was = fourth < 0 ? -1000 : ioctl(fourth, EVIOCGRAB, 1) == 0 ? 0 : -errno;
        int gave = fourth < 0 ? -1000 : ioctl(fourth, EVIOCGRAB, 0) == 0 ? 0 : -errno;
        int back = ioctl(fd, EVIOCGRAB, 1) == 0 ? 0 : -errno;
        if (!follow) follow = took;
        if (!follow) follow = was;
        if (!follow) follow = gave;
        if (!follow) follow = back;
        /* Its close leaves the holder's grab where it is. */
        if (fourth >= 0) close(fourth);
        kept = ioctl(other, EVIOCGRAB, 1) == 0 ? 0 : -errno;
        close(other);
        /* If the second took it, the holder takes it back for what follows. */
        if (kept == 0) (void)ioctl(fd, EVIOCGRAB, 1);
    }
    int lack, spare;
    key_codes(key, &lack, &spare);
    printf("caps %s ev %d:%02x key %d:%d%d%d lack %d spare %d grab %d busy %d steal %d follow %d kept %d\n", name, nev, ev[0], nkey,
           bit(key, KEY_ENTER), bit(key, KEY_A), bit(key, KEY_UP), lack, spare, grab, busy, steal, follow, kept);

    if (probe) {
        files(fd);
        printf("files %s\n", bad[0] ? bad : "ok");
        let_go(fd);
        close(fd);
        return 0;
    }

    console();
    int fl = fcntl(fd, F_GETFL);
    if (fl < 0 || fcntl(fd, F_SETFL, fl & ~O_NONBLOCK) != 0) return fail("fcntl F_SETFL without O_NONBLOCK");
    struct input_event got[EVENTS];
    int n = 0;
    int64_t start = ns(CLOCK_REALTIME);
    printf("ready\n");
    /* Nobody has typed yet: this read must sleep until the first key. */
    ssize_t b = read(fd, &got[0], sizeof got[0]);
    int berr = errno;
    if (fcntl(fd, F_SETFL, fl) != 0) return fail("fcntl F_SETFL O_NONBLOCK");
    if (b == (ssize_t)sizeof got[0]) {
        n = 1;
        printf("block ok\n");
    } else {
        printf("block: a read of event0 without O_NONBLOCK, made before any key was typed, returned %zd (errno %d)\n", b, b < 0 ? berr : 0);
    }
    int64_t until = ns(CLOCK_MONOTONIC) + 5000000000LL;
    while (n < EVENTS && ns(CLOCK_MONOTONIC) < until) {
        short rev;
        ask(1, &fd, (short[]){POLLIN}, &rev, 50);
        if (read(fd, &got[n], sizeof got[n]) == (ssize_t)sizeof got[n]) n++;
    }
    int64_t end = ns(CLOCK_REALTIME);
    char line[EVENTS * 16 + 64] = "keys", *at = line + 4;
    const char *time = "ok";
    int64_t last = 0;
    for (int i = 0; i < n; i++) {
        at = show(at, &got[i]);
        int64_t us = (int64_t)got[i].input_event_sec * 1000000 + got[i].input_event_usec;
        if (got[i].input_event_usec < 0 || got[i].input_event_usec >= 1000000) time = "usec out of range";
        else if (us < last) time = "went back";
        else if (us < start / 1000 || us > end / 1000) time = "outside [ready, last read]";
        last = us;
    }
    printf("%s time %s\n", line, time);

    short rev;
    int r = ask(1, &fd, (short[]){POLLIN}, &rev, 0);
    if (r != 0) miss("poll(0) on the device after the keys returned", r);
    printf("inject\n");
    int64_t t0 = ns(CLOCK_MONOTONIC);
    r = ask(1, &fd, (short[]){POLLIN}, &rev, 3000);
    int64_t waited = (ns(CLOCK_MONOTONIC) - t0) / 1000000;
    if (r != 1 || rev != POLLIN) miss("poll(3000) as a key came in: revents", rev);
    if (waited >= 2000) miss("poll(3000) as a key came in returned after (ms)", waited);
    char in[96] = "", *ip = in;
    for (int i = 0; i < 4; i++) {
        r = ask(1, &fd, (short[]){POLLIN}, &rev, 0);
        if (r != 1 || rev != POLLIN) miss("poll(0) before an injected event: revents", rev);
        struct input_event e;
        if (read(fd, &e, sizeof e) == (ssize_t)sizeof e) ip = show(ip, &e);
    }
    if (strcmp(in, " 88+ . 88- .") != 0) {
        size_t k = strlen(bad);
        snprintf(bad + k, sizeof bad - k, "%sthe injected key read as \"%s\"", k ? "; " : "", in);
    }
    r = ask(1, &fd, (short[]){POLLIN}, &rev, 0);
    if (r != 0) miss("poll(0) after the injected key: revents", rev);
    printf("poll %s\n", bad[0] ? bad : "ok");

    struct input_event e;
    ssize_t got1 = read(fd, &e, sizeof e);
    if (got1 == -1 && errno == EAGAIN) printf("eagain ok\n");
    else printf("eagain: read of the empty device returned %zd (errno %d)\n", got1, got1 < 0 ? errno : 0);

    let_go(fd);
    close(fd);
    return 0;
}
