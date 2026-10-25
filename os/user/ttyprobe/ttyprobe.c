/* ttyprobe -- lab 33: the terminal calls a full-screen program makes, one
 * at a time, each with the run's nonce.
 *
 *   ttyprobe tcgets <16 hex digits>
 *   ttyprobe tcsets <16 hex digits>
 *   ttyprobe raw <16 hex digits>
 *   ttyprobe access <16 hex digits>
 *   ttyprobe trunc <16 hex digits>
 *
 * tcgets: TCGETS on /dev/tty into a buffer whose tail holds bytes drawn
 * from the nonce; exactly 36 bytes (Linux's struct termios) are written.
 * It prints
 *
 *   ttyprobe tcgets <the 36 bytes in hex> <the nonce>
 *
 * tcsets: a termios made from the one read (ICANON and ECHO off, ICRNL
 * flipped, VMIN and VTIME drawn from the nonce) is set with TCSETS and read
 * back the same; then the first is set again and read back the same:
 *
 *   ttyprobe tcsets ok <the nonce>
 *
 * raw: ICANON, ECHO, ISIG and ICRNL off, VMIN 1, VTIME 0; it prints
 * `ttyprobe raw ready <the nonce>`, then reads what is typed until six
 * bytes came or 20 s passed. The reads are O_NONBLOCK, 20 ms apart, with
 * no poll: what a raw read returns is the read's own answer (busybox's
 * line editor and vi poll first; rows 6 and 7 of ktest-33 take that
 * path). It puts the termios back and prints
 *
 *   ttyprobe raw <the bytes in hex> <the nonce>
 *
 * access: access(2) as root on a file it makes with mode 0, one with mode
 * 0644, a name that is not there, /tmp and /bin/busybox; then X_OK on a
 * file whose only execute bit is the group's, one whose only execute bit
 * is other's (their read and write bits drawn from the nonce), and a
 * directory made with mode 0 -- root may run the two files (any of the
 * three execute bits will do) and search the directory (a directory needs
 * none); each is checked by stat to carry the mode it was made with, and
 * removed after (added 2026-10-23, slice 8 fix round):
 *
 *   ttyprobe access ok <the nonce>
 *
 * trunc: ftruncate(2) cuts a file of 12,411 bytes drawn from the nonce to
 * 100 (its blocks freed, the 100 bytes kept, the offset unmoved), grows it
 * to 5000 (the new bytes read as zeros), and refuses a negative length, a
 * file opened read-only and a pipe with EINVAL:
 *
 *   ttyprobe trunc ok <the nonce>
 *
 * A failed check prints `ttyprobe: <what>` and exits 1.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

#define TCGETS 0x5401
#define TCSETS 0x5402
#define T_IFLAG 0
#define T_LFLAG 12
#define T_CC 17
#define V_TIME 5
#define V_MIN 6
#define F_ICRNL 0400u
#define F_ISIG 01u
#define F_ICANON 02u
#define F_ECHO 010u

static uint64_t nonce;

static uint64_t splitmix64(uint64_t x) {
    x += 0x9e3779b97f4a7c15ull;
    x = (x ^ (x >> 30)) * 0xbf58476d1ce4e5b9ull;
    x = (x ^ (x >> 27)) * 0x94d049bb133111ebull;
    return x ^ (x >> 31);
}

static void die(const char *what) {
    printf("ttyprobe: %s\n", what);
    exit(1);
}

static void dief(const char *fmt, long a, long b) {
    printf("ttyprobe: ");
    printf(fmt, a, b);
    printf("\n");
    exit(1);
}

static void hex(const unsigned char *b, size_t n) {
    for (size_t i = 0; i < n; i++)
        printf("%02x", b[i]);
}

static uint32_t word(const unsigned char *t, int at) {
    uint32_t w;
    memcpy(&w, t + at, 4);
    return w;
}

static void set_word(unsigned char *t, int at, uint32_t w) {
    memcpy(t + at, &w, 4);
}

static int tty_flags(int flags) {
    int fd = open("/dev/tty", O_RDWR | flags);
    if (fd < 0)
        dief("open(\"/dev/tty\") failed, errno %ld%.0ld", errno, 0);
    return fd;
}

static int tty(void) {
    return tty_flags(0);
}

static void tcget(int fd, unsigned char t[36]) {
    long r = syscall(SYS_ioctl, fd, TCGETS, t);
    if (r != 0)
        dief("ioctl(TCGETS) returned %ld, errno %ld", r, errno);
}

static void tcset(int fd, const unsigned char t[36]) {
    long r = syscall(SYS_ioctl, fd, TCSETS, t);
    if (r != 0)
        dief("ioctl(TCSETS) returned %ld, errno %ld", r, errno);
}

static void tcgets(void) {
    unsigned char buf[64], canary[64];
    for (int i = 0; i < 64; i++)
        canary[i] = buf[i] = (unsigned char)splitmix64(nonce + (uint64_t)i);
    int fd = tty();
    tcget(fd, buf);
    if (memcmp(buf + 36, canary + 36, 28))
        die("TCGETS wrote past the 36 bytes of struct termios");
    if (!memcmp(buf, canary, 36))
        die("TCGETS wrote nothing");
    printf("ttyprobe tcgets ");
    hex(buf, 36);
    printf(" %016" PRIx64 "\n", nonce);
}

static void tcsets(void) {
    unsigned char t0[36], t1[36], back[36];
    int fd = tty();
    tcget(fd, t0);
    memcpy(t1, t0, 36);
    set_word(t1, T_LFLAG, word(t0, T_LFLAG) & ~(F_ICANON | F_ECHO));
    set_word(t1, T_IFLAG, word(t0, T_IFLAG) ^ F_ICRNL);
    t1[T_CC + V_MIN] = (unsigned char)(1 + nonce % 200);
    t1[T_CC + V_TIME] = (unsigned char)((nonce >> 8) % 200);
    tcset(fd, t1);
    tcget(fd, back);
    if (memcmp(back, t1, 36)) {
        tcset(fd, t0);
        printf("ttyprobe: TCGETS after TCSETS read ");
        hex(back, 36);
        printf(", not the termios set, ");
        hex(t1, 36);
        printf("\n");
        exit(1);
    }
    tcset(fd, t0);
    tcget(fd, back);
    if (memcmp(back, t0, 36))
        die("TCGETS after setting the first termios again did not read it back");
    printf("ttyprobe tcsets ok %016" PRIx64 "\n", nonce);
}

static void raw(void) {
    unsigned char t0[36], t1[36], got[16];
    size_t n = 0;
    int fd = tty_flags(O_NONBLOCK);
    tcget(fd, t0);
    memcpy(t1, t0, 36);
    set_word(t1, T_LFLAG, word(t0, T_LFLAG) & ~(F_ICANON | F_ECHO | F_ISIG));
    set_word(t1, T_IFLAG, word(t0, T_IFLAG) & ~F_ICRNL);
    t1[T_CC + V_MIN] = 1;
    t1[T_CC + V_TIME] = 0;
    tcset(fd, t1);
    printf("ttyprobe raw ready %016" PRIx64 "\n", nonce);
    struct timespec tick = {0, 20 * 1000 * 1000};
    for (int waited = 0; n < 6 && waited < 1000; waited++) {
        ssize_t k = read(fd, got + n, sizeof got - n);
        if (k > 0)
            n += (size_t)k;
        else if (k == 0 || errno != EAGAIN)
            break;
        else
            nanosleep(&tick, 0);
    }
    tcset(fd, t0);
    printf("ttyprobe raw ");
    hex(got, n);
    printf(" %016" PRIx64 "\n", nonce);
}

static void want_access(const char *path, int mode, int err) {
    errno = 0;
    int r = access(path, mode);
    if ((err == 0 && r != 0) || (err != 0 && (r != -1 || errno != err))) {
        printf("ttyprobe: access(\"%s\", %d) returned %d errno %d; root's answer is %d errno %d\n", path, mode, r, r ? errno : 0, err ? -1 : 0, err);
        exit(1);
    }
}

/* What it was made with, read back, so an access answer is about that mode. */
static void want_mode(const char *path, mode_t mode) {
    struct stat st;
    if (stat(path, &st)) {
        printf("ttyprobe: stat(\"%s\") failed, errno %d\n", path, errno);
        exit(1);
    }
    if ((st.st_mode & (S_IFMT | 07777)) != mode) {
        printf("ttyprobe: %s has mode 0%o after it was made with mode 0%o\n", path, (unsigned)(st.st_mode & (S_IFMT | 07777)), (unsigned)mode);
        exit(1);
    }
}

static void accessp(void) {
    char none[32], ro[32], rw[32];
    unsigned w = (unsigned)splitmix64(nonce);
    snprintf(none, sizeof none, "/tmp/n%08x", w);
    snprintf(ro, sizeof ro, "/tmp/z%08x", w);
    snprintf(rw, sizeof rw, "/tmp/r%08x", w);
    int fd = open(ro, O_WRONLY | O_CREAT | O_TRUNC, 0);
    int fd2 = open(rw, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0 || fd2 < 0)
        die("could not make the two files in /tmp");
    close(fd);
    close(fd2);
    want_access(ro, F_OK, 0);
    want_access(none, F_OK, ENOENT);
    want_access(none, W_OK, ENOENT);
    want_access(ro, R_OK | W_OK, 0);
    want_access(rw, R_OK | W_OK, 0);
    want_access(rw, X_OK, EACCES);
    want_access(ro, X_OK, EACCES);
    want_access("/bin/busybox", X_OK, 0);
    want_access("/tmp", X_OK | W_OK, 0);
    want_access(rw, 8, EINVAL);
    unlink(ro);
    unlink(rw);

    /* X_OK by any execute bit, not the owner's alone; a directory needs none. */
    char gx[32], ox[32], dir[32];
    snprintf(gx, sizeof gx, "/tmp/g%08x", w);
    snprintf(ox, sizeof ox, "/tmp/o%08x", w);
    snprintf(dir, sizeof dir, "/tmp/d%08x", w);
    mode_t gm = 0010 | ((mode_t)splitmix64(nonce ^ 2) & 0666);
    mode_t om = 0001 | ((mode_t)splitmix64(nonce ^ 3) & 0666);
    fd = open(gx, O_WRONLY | O_CREAT | O_TRUNC, gm);
    fd2 = open(ox, O_WRONLY | O_CREAT | O_TRUNC, om);
    if (fd < 0 || fd2 < 0)
        die("could not make the two files with one execute bit in /tmp");
    close(fd);
    close(fd2);
    if (mkdir(dir, 0))
        die("could not make a directory with mode 0 in /tmp");
    want_mode(gx, S_IFREG | gm);
    want_mode(ox, S_IFREG | om);
    want_mode(dir, S_IFDIR);
    want_access(gx, X_OK, 0);
    want_access(ox, X_OK, 0);
    want_access(dir, X_OK, 0);
    want_access(dir, R_OK | W_OK | X_OK, 0);
    unlink(gx);
    unlink(ox);
    rmdir(dir);
    printf("ttyprobe access ok %016" PRIx64 "\n", nonce);
}

static void want_einval(int fd, off_t len, const char *what) {
    errno = 0;
    int r = ftruncate(fd, len);
    if (r != -1 || errno != EINVAL) {
        printf("ttyprobe: ftruncate on %s returned %d errno %d; Linux answers -1 EINVAL\n", what, r, r ? errno : 0);
        exit(1);
    }
}

static void trunc_(void) {
    enum { FULL = 12411, CUT = 100, GROWN = 5000 };
    static unsigned char data[FULL], back[GROWN];
    char path[32];
    struct stat full, cut, grown;
    snprintf(path, sizeof path, "/tmp/t%08x", (unsigned)splitmix64(nonce ^ 1));
    for (int i = 0; i < FULL; i++)
        data[i] = (unsigned char)splitmix64(nonce + (uint64_t)i) | 1;
    int fd = open(path, O_RDWR | O_CREAT | O_TRUNC, 0644);
    if (fd < 0)
        die("could not make the file in /tmp");
    if (write(fd, data, FULL) != FULL)
        die("could not write the file");
    if (fstat(fd, &full))
        die("fstat failed");
    if (ftruncate(fd, CUT))
        dief("ftruncate(fd, 100) returned -1 errno %ld%.0ld", errno, 0);
    if (fstat(fd, &cut) || cut.st_size != CUT)
        dief("after ftruncate(fd, 100) fstat says %ld bytes, not 100%.0ld", (long)cut.st_size, 0);
    if (!(cut.st_blocks < full.st_blocks))
        dief("ftruncate to 100 bytes freed no blocks: st_blocks %ld, and %ld at 12411 bytes", (long)cut.st_blocks, (long)full.st_blocks);
    if (lseek(fd, 0, SEEK_CUR) != FULL)
        die("ftruncate moved the file offset");
    if (ftruncate(fd, GROWN))
        dief("ftruncate(fd, 5000) returned -1 errno %ld%.0ld", errno, 0);
    if (fstat(fd, &grown) || grown.st_size != GROWN)
        dief("after ftruncate(fd, 5000) fstat says %ld bytes, not 5000%.0ld", (long)grown.st_size, 0);
    if (lseek(fd, 0, SEEK_SET) != 0 || read(fd, back, GROWN) != GROWN)
        die("could not read the 5000 bytes back");
    if (memcmp(back, data, CUT))
        die("the first 100 bytes changed");
    for (int i = CUT; i < GROWN; i++)
        if (back[i])
            dief("byte %ld of the grown file is %ld, not 0", i, back[i]);
    want_einval(fd, -1, "a negative length");
    int ro = open(path, O_RDONLY);
    want_einval(ro, 0, "a file opened O_RDONLY");
    int p[2];
    if (pipe(p))
        die("pipe failed");
    want_einval(p[1], 0, "a pipe");
    close(ro);
    close(fd);
    unlink(path);
    printf("ttyprobe trunc ok %016" PRIx64 "\n", nonce);
}

int main(int argc, char **argv) {
    char *end;
    if (argc != 3 || strlen(argv[2]) != 16)
        die("usage: ttyprobe tcgets|tcsets|raw|access|trunc <16 hex digits>");
    nonce = strtoull(argv[2], &end, 16);
    if (*end)
        die("the nonce is not 16 hex digits");
    setvbuf(stdout, 0, _IONBF, 0);
    if (!strcmp(argv[1], "tcgets"))
        tcgets();
    else if (!strcmp(argv[1], "tcsets"))
        tcsets();
    else if (!strcmp(argv[1], "raw"))
        raw();
    else if (!strcmp(argv[1], "access"))
        accessp();
    else if (!strcmp(argv[1], "trunc"))
        trunc_();
    else
        die("usage: ttyprobe tcgets|tcsets|raw|access|trunc <16 hex digits>");
    return 0;
}
