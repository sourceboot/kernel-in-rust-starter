/* sigprobe -- lab 31: what the mask holds, what a send from a process
 * names, and what an ignored signal leaves alone.
 *
 *   sigprobe dfl <word>
 *   sigprobe order <word>
 *   sigprobe uncatch <word>
 *   sigprobe nap <word>
 *   sigprobe queue <word>
 *   sigprobe send <word>
 *
 * Every signal call is the raw syscall (rt_sigprocmask 14, rt_sigaction 13,
 * kill 62), so what is printed is what the kernel answered, not what libc
 * made of it. Each mode prints one line that starts with <word>.
 *
 * dfl: SIGCHLD, SIGWINCH and SIGURG, each with its action set to SIG_DFL,
 * sent to itself. The default of each is to do nothing, so it prints
 *
 *   <word> dfl
 *
 * and exits 0. (ash catches SIGCHLD, so a shell that sends itself one
 * never meets its default.)
 *
 * order: SIGHUP, SIGUSR1 and SIGUSR2 blocked (SIG_BLOCK), SIGHUP's action
 * SIG_IGN; the program sends itself SIGUSR2, SIGUSR1 and SIGHUP, prints
 *
 *   <word> held
 *
 * and unblocks the three (SIG_UNBLOCK). All three were pending; on the way
 * back from that one call SIGHUP is dropped (ignored) and SIGUSR1, the
 * lowest that acts, ends the program: the shell sees 128 + 10 = 138. If it
 * returns, it prints `<word> survived` and exits 0.
 *
 * uncatch: SIGKILL, SIGSTOP and SIGUSR2 blocked (SIG_BLOCK), then the mask
 * read back, rt_sigaction(9, SIG_IGN) and rt_sigaction(19, a handler); it
 * prints
 *
 *   <word> <the mask, hex> <rt_sigaction 9's answer> <rt_sigaction 19's answer>
 *
 * (Linux: `800 -22 -22`, the mask SIGUSR2's bit alone) and sends itself
 * SIGKILL, which ends it whatever it asked for (137). If it returns, it
 * prints `<word> survived` and exits 0.
 *
 * nap: SIGUSR1's action SIG_IGN; a child sleeps 200 ms, sends its parent
 * SIGUSR1, SIGWINCH and SIGURG, writes one byte to a pipe (`x` when each
 * `kill` answered 0, `n` when one did not) and exits, while
 * the parent is in one nanosleep of 600 ms. None of the three acts, so the
 * sleep runs its course and returns 0. The parent sleeps again, up to four
 * more times, until the child's byte was there when a sleep ended; then it
 * prints
 *
 *   <word> <the first non-zero nanosleep answer, else 0>
 *
 * and exits 0; it exits 2 if the byte never came (the child never sent),
 * and 3 if it is `n` (a `kill` failed, so nothing was sent).
 *
 * queue: first SIGWINCH blocked, sent to itself and unblocked: taken then,
 * its default does nothing, so it is dropped. Then SIGUSR2 blocked, its
 * action SIG_IGN, sent to itself, its action set back to SIG_DFL; it prints
 *
 *   <word> queued
 *
 * and unblocks it. Blocked, the signal waited even though its action was
 * to ignore it (Linux keeps a blocked signal, since its action may change
 * before the unblock), so the unblock ends the program (140). If it
 * returns, it prints `<word> survived` and exits 0.
 *
 * send: a child that exits at once and is not yet waited for (a zombie),
 * then kill(<that child>, 0), kill(0, 0), kill(-1, 0) and kill(0, 15) (no
 * process groups here: a pid of 0 or below names no process). It prints
 *
 *   <word> <kill(child, 0)> <kill(0, 0)> <kill(-1, 0)> <kill(0, 15)>
 *
 * (here: `0 -3 -3 -3`) and exits 0.
 *
 * A wrong argument prints `sigprobe: usage` and exits 1.
 */
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#define SYS_read 0
#define SYS_write 1
#define SYS_close 3
#define SYS_poll 7
#define SYS_rt_sigaction 13
#define SYS_rt_sigprocmask 14
#define SYS_pipe 22
#define SYS_nanosleep 35
#define SYS_getpid 39
#define SYS_fork 57
#define SYS_exit 60
#define SYS_wait4 61
#define SYS_kill 62
#define SYS_getppid 110

#define SIG_BLOCK 0
#define SIG_UNBLOCK 1

#define SIGHUP 1
#define SIGKILL 9
#define SIGUSR1 10
#define SIGUSR2 12
#define SIGTERM 15
#define SIGCHLD 17
#define SIGSTOP 19
#define SIGURG 23
#define SIGWINCH 28

static long sc(long n, long a, long b, long c, long d) {
    long r;
    register long r10 __asm__("r10") = d;
    __asm__ volatile("syscall" : "=a"(r) : "a"(n), "D"(a), "S"(b), "d"(c), "r"(r10) : "rcx", "r11", "memory");
    return r;
}

static uint64_t bit(int n) {
    return 1ull << (n - 1);
}

static long mask(int how, uint64_t set) {
    return sc(SYS_rt_sigprocmask, how, (long)&set, 0, 8);
}

/* x86-64's struct kernel_sigaction: handler, flags, restorer, mask. */
static long action(int sig, uint64_t handler) {
    uint64_t act[4] = {handler, 0, 0, 0};
    return sc(SYS_rt_sigaction, sig, (long)act, 0, 8);
}

static void say(const char *s) {
    sc(SYS_write, 1, (long)s, (long)strlen(s), 0);
}

static void nap_ms(long ms) {
    long t[2] = {ms / 1000, (ms % 1000) * 1000000};
    sc(SYS_nanosleep, (long)t, 0, 0, 0);
}

static void handler(int sig) {
    (void)sig;
}

static int survived(const char *w) {
    char line[128];
    snprintf(line, sizeof line, "%s survived\n", w);
    say(line);
    return 0;
}

static int dfl(const char *w) {
    char line[128];
    long me = sc(SYS_getpid, 0, 0, 0, 0);
    int sigs[3] = {SIGCHLD, SIGWINCH, SIGURG};
    for (int i = 0; i < 3; i++) {
        action(sigs[i], 0);
        sc(SYS_kill, me, sigs[i], 0, 0);
    }
    snprintf(line, sizeof line, "%s dfl\n", w);
    say(line);
    return 0;
}

static int order(const char *w) {
    char line[128];
    long me = sc(SYS_getpid, 0, 0, 0, 0);
    uint64_t set = bit(SIGHUP) | bit(SIGUSR1) | bit(SIGUSR2);
    mask(SIG_BLOCK, set);
    action(SIGHUP, 1);
    sc(SYS_kill, me, SIGUSR2, 0, 0);
    sc(SYS_kill, me, SIGUSR1, 0, 0);
    sc(SYS_kill, me, SIGHUP, 0, 0);
    snprintf(line, sizeof line, "%s held\n", w);
    say(line);
    mask(SIG_UNBLOCK, set);
    return survived(w);
}

static int uncatch(const char *w) {
    char line[128];
    uint64_t old = 0;
    mask(SIG_BLOCK, bit(SIGKILL) | bit(SIGSTOP) | bit(SIGUSR2));
    sc(SYS_rt_sigprocmask, SIG_BLOCK, 0, (long)&old, 8);
    long a9 = action(SIGKILL, 1);
    long a19 = action(SIGSTOP, (uint64_t)(uintptr_t)handler);
    snprintf(line, sizeof line, "%s %llx %ld %ld\n", w, (unsigned long long)old, a9, a19);
    say(line);
    sc(SYS_kill, sc(SYS_getpid, 0, 0, 0, 0), SIGKILL, 0, 0);
    return survived(w);
}

static int nap(const char *w) {
    char line[128];
    int p[2];
    action(SIGUSR1, 1);
    if (sc(SYS_pipe, (long)p, 0, 0, 0) != 0) {
        return 2;
    }
    long child = sc(SYS_fork, 0, 0, 0, 0);
    if (child == 0) {
        long parent = sc(SYS_getppid, 0, 0, 0, 0);
        nap_ms(200);
        long k = sc(SYS_kill, parent, SIGUSR1, 0, 0);
        k |= sc(SYS_kill, parent, SIGWINCH, 0, 0);
        k |= sc(SYS_kill, parent, SIGURG, 0, 0);
        sc(SYS_write, p[1], (long)(k == 0 ? "x" : "n"), 1, 0);
        sc(SYS_exit, 0, 0, 0, 0);
    }
    if (child < 0) {
        return 2;
    }
    sc(SYS_close, p[1], 0, 0, 0);
    long first = 0;
    int sent = 0;
    for (int i = 0; i < 5 && !sent; i++) {
        long t[2] = {0, 600000000};
        long r = sc(SYS_nanosleep, (long)t, 0, 0, 0);
        if (r != 0 && first == 0) {
            first = r;
        }
        struct {
            int fd;
            short events, revents;
        } pfd = {p[0], 1, 0};
        sent = sc(SYS_poll, (long)&pfd, 1, 0, 0) == 1;
    }
    char got = 0;
    if (sent) {
        sc(SYS_read, p[0], (long)&got, 1, 0);
    }
    int st = 0;
    sc(SYS_wait4, child, (long)&st, 0, 0);
    snprintf(line, sizeof line, "%s %ld\n", w, first);
    say(line);
    return !sent ? 2 : got == 'x' ? 0 : 3;
}

static int send(const char *w) {
    char line[160];
    int p[2];
    if (sc(SYS_pipe, (long)p, 0, 0, 0) != 0) {
        return 2;
    }
    long child = sc(SYS_fork, 0, 0, 0, 0);
    if (child == 0) {
        sc(SYS_exit, 0, 0, 0, 0);
    }
    if (child < 0) {
        return 2;
    }
    /* The child's end of the pipe closes when it exits: EOF, then a moment
     * for its status to be recorded. */
    sc(SYS_close, p[1], 0, 0, 0);
    char b;
    while (sc(0 /* read */, p[0], (long)&b, 1, 0) > 0) {
    }
    nap_ms(100);
    long z = sc(SYS_kill, child, 0, 0, 0);
    long k0 = sc(SYS_kill, 0, 0, 0, 0);
    long km = sc(SYS_kill, -1, 0, 0, 0);
    long kt = sc(SYS_kill, 0, SIGTERM, 0, 0);
    int st = 0;
    sc(SYS_wait4, child, (long)&st, 0, 0);
    snprintf(line, sizeof line, "%s %ld %ld %ld %ld\n", w, z, k0, km, kt);
    say(line);
    return 0;
}

static int queue(const char *w) {
    char line[128];
    long me = sc(SYS_getpid, 0, 0, 0, 0);
    mask(SIG_BLOCK, bit(SIGWINCH));
    sc(SYS_kill, me, SIGWINCH, 0, 0);
    mask(SIG_UNBLOCK, bit(SIGWINCH));
    mask(SIG_BLOCK, bit(SIGUSR2));
    action(SIGUSR2, 1);
    sc(SYS_kill, me, SIGUSR2, 0, 0);
    action(SIGUSR2, 0);
    snprintf(line, sizeof line, "%s queued\n", w);
    say(line);
    mask(SIG_UNBLOCK, bit(SIGUSR2));
    return survived(w);
}

int main(int argc, char **argv) {
    if (argc == 3 && strcmp(argv[1], "dfl") == 0) {
        return dfl(argv[2]);
    }
    if (argc == 3 && strcmp(argv[1], "order") == 0) {
        return order(argv[2]);
    }
    if (argc == 3 && strcmp(argv[1], "uncatch") == 0) {
        return uncatch(argv[2]);
    }
    if (argc == 3 && strcmp(argv[1], "nap") == 0) {
        return nap(argv[2]);
    }
    if (argc == 3 && strcmp(argv[1], "queue") == 0) {
        return queue(argv[2]);
    }
    if (argc == 3 && strcmp(argv[1], "send") == 0) {
        return send(argv[2]);
    }
    say("sigprobe: usage\n");
    return 1;
}
