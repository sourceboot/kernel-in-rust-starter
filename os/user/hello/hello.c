/* hello -- lab 25: an ordinary static musl program, run unmodified.
 *
 *   hello <16 hex digits>        (and 14 bytes on stdin: t<12 hex digits>\n)
 *
 * Built against musl's own start-up, stdio, malloc and syscall wrappers
 * (user/build.sh, layout "musl"), so before main runs the kernel has already
 * met arch_prctl (musl's TLS), set_tid_address and an ioctl on stdout; then
 * stdio reads with readv (an fread of 14 bytes: 13 into the caller's buffer and
 * the rest into stdio's own, in one call -- fgets would read a byte at a time
 * with read) and writes with writev. It also asks for the calls a
 * program takes for granted -- the ids, the CPU set, the clocks, a sleep, the
 * signal tables, a futex -- and checks each answer. It prints two lines:
 *
 *   hello, musl: <16 hex: splitmix64(a ^ w)> t<12 hex>
 *   ids 0 0 0 0 pid <getpid> cpus 1 slept <n> ms realtime <s> sig ok futex ok
 *
 * a = argv[1], w = the stdin word's 48 bits; n = 20 + a % 40 milliseconds,
 * measured on CLOCK_MONOTONIC across the nanosleep; s = CLOCK_REALTIME's
 * seconds, which gettimeofday must agree with to the second. A failed check
 * prints `hello: <what>` instead, on stdout, and exits 1.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <inttypes.h>
#include <sched.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/time.h>
#include <time.h>
#include <unistd.h>

static uint64_t splitmix64(uint64_t x) {
    x += 0x9e3779b97f4a7c15ull;
    x = (x ^ (x >> 30)) * 0xbf58476d1ce4e5b9ull;
    x = (x ^ (x >> 27)) * 0x94d049bb133111ebull;
    return x ^ (x >> 31);
}

static int fail(const char *what) {
    printf("hello: %s\n", what);
    return 1;
}

static int64_t ms_of(const struct timespec *t) {
    return (int64_t)t->tv_sec * 1000 + t->tv_nsec / 1000000;
}

static volatile sig_atomic_t never;
static void on_usr1(int sig) { never = sig; }

int main(int argc, char **argv) {
    char *end;
    if (argc != 2 || strlen(argv[1]) != 16) return fail("usage: hello <16 hex digits>");
    uint64_t a = strtoull(argv[1], &end, 16);
    if (*end) return fail("argv[1] is not 16 hex digits");

    /* stdin: one word, t + 12 hex digits and a newline (stdio reads it with readv). */
    char line[16] = {0};
    if (fread(line, 1, 14, stdin) != 14) return fail("fewer than 14 bytes on stdin");
    if (line[0] != 't' || line[13] != '\n') return fail("stdin is not t<12 hex digits> and a newline");
    line[13] = 0;
    uint64_t w = strtoull(line + 1, &end, 16);
    if (*end) return fail("the stdin line is not t<12 hex digits>");

    /* ids: all four 0, the caller's pid. */
    unsigned ids[4] = {getuid(), getgid(), geteuid(), getegid()};
    pid_t pid = getpid();

    /* the CPU set: exactly one CPU, number 0. */
    cpu_set_t set;
    CPU_ZERO(&set);
    if (sched_getaffinity(0, sizeof set, &set) != 0) return fail("sched_getaffinity failed");
    if (CPU_COUNT(&set) != 1 || !CPU_ISSET(0, &set)) return fail("sched_getaffinity: not exactly CPU 0");

    /* setpgid and setsid: asked, as a daemon asks. Their answers are not
     * checked (this kernel answers 0; Linux's setsid answers the new session,
     * or EPERM for a group leader) -- only that each is a call the kernel has. */
    (void)setpgid(0, 0);
    (void)setsid();

    /* a sleep on the monotonic clock: it must last at least what it asked. */
    int n = 20 + (int)(a % 40);
    struct timespec t0, t1, req = {0, (long)n * 1000000L}, rem = {7, 7};
    if (clock_gettime(CLOCK_MONOTONIC, &t0) != 0) return fail("clock_gettime(CLOCK_MONOTONIC) failed");
    if (nanosleep(&req, &rem) != 0) return fail("nanosleep failed");
    if (clock_gettime(CLOCK_MONOTONIC, &t1) != 0) return fail("clock_gettime(CLOCK_MONOTONIC) failed");
    int64_t slept = ms_of(&t1) - ms_of(&t0);
    if (slept < n) {
        printf("hello: nanosleep of %d ms returned after %" PRId64 " ms\n", n, slept);
        return 1;
    }
    struct timespec cpu;
    if (clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &cpu) != 0) return fail("clock_gettime(CLOCK_PROCESS_CPUTIME_ID) failed");

    /* the wall clock: REALTIME, and gettimeofday on the same second. */
    struct timespec rt;
    struct timeval tv;
    if (clock_gettime(CLOCK_REALTIME, &rt) != 0) return fail("clock_gettime(CLOCK_REALTIME) failed");
    if (gettimeofday(&tv, 0) != 0) return fail("gettimeofday failed");
    if (tv.tv_sec < rt.tv_sec || tv.tv_sec > rt.tv_sec + 1) return fail("gettimeofday and CLOCK_REALTIME disagree");

    /* the signal tables: what is stored comes back. */
    struct sigaction sa, old;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = on_usr1;
    sa.sa_flags = SA_RESTART;
    sigemptyset(&sa.sa_mask);
    sigaddset(&sa.sa_mask, SIGUSR2);
    if (sigaction(SIGUSR1, &sa, 0) != 0) return fail("sigaction(SIGUSR1, set) failed");
    if (sigaction(SIGUSR1, 0, &old) != 0) return fail("sigaction(SIGUSR1, get) failed");
    if (old.sa_handler != on_usr1 || !(old.sa_flags & SA_RESTART) || !sigismember(&old.sa_mask, SIGUSR2))
        return fail("sigaction did not give back what was stored");
    sigset_t block, was, now;
    sigemptyset(&block);
    sigaddset(&block, SIGUSR2);
    if (sigprocmask(SIG_BLOCK, &block, &was) != 0) return fail("sigprocmask(SIG_BLOCK) failed");
    if (sigprocmask(SIG_SETMASK, 0, &now) != 0) return fail("sigprocmask(get) failed");
    if (!sigismember(&now, SIGUSR2) || sigismember(&was, SIGUSR2)) return fail("sigprocmask did not keep the mask");
    if (sigprocmask(SIG_SETMASK, &was, 0) != 0) return fail("sigprocmask(SIG_SETMASK) failed");

    /* a futex: WAKE finds nobody; WAIT on a changed value does not wait. */
    static int word = 5;
    if (syscall(SYS_futex, &word, 1 /* FUTEX_WAKE */, 1, 0, 0, 0) != 0) return fail("futex(FUTEX_WAKE) did not return 0");
    errno = 0;
    if (syscall(SYS_futex, &word, 0 /* FUTEX_WAIT */, 6, 0, 0, 0) != -1 || errno != EAGAIN)
        return fail("futex(FUTEX_WAIT) on a changed value did not fail with EAGAIN");

    printf("hello, musl: %016" PRIx64 " t%012" PRIx64 "\n", splitmix64(a ^ w), w);
    printf("ids %u %u %u %u pid %d cpus 1 slept %d ms realtime %lld sig ok futex ok\n",
           ids[0], ids[1], ids[2], ids[3], (int)pid, n, (long long)rt.tv_sec);
    return 0;
}
