/* sigregs -- lab 32: what a signal handler may not disturb, and how a wait
 * a signal ends comes back.
 *
 *   sigregs regs <16 hex digits>
 *   sigregs restart <16 hex digits>
 *   sigregs suspend <16 hex digits>
 *
 * regs: every general register but rsp, and xmm1, holds a value drawn from
 * the nonce (splitmix64), and the program spins on a flag with no syscall,
 * so the SIGUSR1 a forked child sends arrives through an interrupt. The
 * handler checks what it was given -- the signal in rdi and in the
 * siginfo, and the ucontext's saved r12, rcx, r11 and xmm1 equal to what the
 * spin loop holds -- then overwrites every register it may and xmm1, and
 * sets the flag. After rt_sigreturn the loop stores every register, and
 * each must be the value it held before. It prints
 *
 *   sigregs regs <16 hex: a fold of the restored values>
 *
 * restart: a SIGUSR1 handler installed with SA_RESTART; one child exits
 * with status (nonce & 0x7f) after a second child has sent the parent
 * SIGUSR1, while the parent waits in wait4 for the first. The handler finds
 * in its ucontext that it interrupted wait4 and that wait4 is to start
 * again (rax = 61, rip on the `syscall`); wait4 then returns that child and
 * its status. It prints
 *
 *   sigregs restart <pid matched> status <n>
 *
 * suspend: SIGUSR1 blocked, a child sends it, and rt_sigsuspend with an
 * empty mask returns -1/EINTR once the handler has run; SIGUSR1 is blocked
 * again afterwards. It prints
 *
 *   sigregs suspend EINTR <16 hex: the nonce>
 *
 * A failed check prints `sigregs: <what>` and exits 1.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <inttypes.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <time.h>
#include <ucontext.h>
#include <unistd.h>

static uint64_t splitmix64(uint64_t x) {
    x += 0x9e3779b97f4a7c15ull;
    x = (x ^ (x >> 30)) * 0xbf58476d1ce4e5b9ull;
    x = (x ^ (x >> 27)) * 0x94d049bb133111ebull;
    return x ^ (x >> 31);
}

static void die(const char *what) {
    printf("sigregs: %s\n", what);
    exit(1);
}

static void nap(long ms) {
    struct timespec t = {ms / 1000, (ms % 1000) * 1000000};
    while (nanosleep(&t, &t) != 0 && errno == EINTR) {
    }
}

/* ── regs ── */

/* The spin loop's registers, in this order. */
static const char *const NAMES[15] = {"rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "r8",
                                      "r9",  "r10", "r11", "r12", "r13", "r14", "r15"};
uint64_t sr_want[15], sr_got[15];
uint64_t sr_xwant[2], sr_xgot[2];
volatile int sr_hit;
static volatile int bad;           /* what the handler found wrong; 0 = nothing */
static volatile uint64_t bad_seen; /* and the value it saw */

void sr_spin(void);
__asm__(".text\n"
        ".globl sr_spin\n"
        "sr_spin:\n"
        "  push %rbx\n push %rbp\n push %r12\n push %r13\n push %r14\n push %r15\n"
        "  movdqu sr_xwant(%rip), %xmm1\n"
        "  mov sr_want+0(%rip), %rax\n"
        "  mov sr_want+8(%rip), %rbx\n"
        "  mov sr_want+16(%rip), %rcx\n"
        "  mov sr_want+24(%rip), %rdx\n"
        "  mov sr_want+32(%rip), %rsi\n"
        "  mov sr_want+40(%rip), %rdi\n"
        "  mov sr_want+48(%rip), %rbp\n"
        "  mov sr_want+56(%rip), %r8\n"
        "  mov sr_want+64(%rip), %r9\n"
        "  mov sr_want+72(%rip), %r10\n"
        "  mov sr_want+80(%rip), %r11\n"
        "  mov sr_want+88(%rip), %r12\n"
        "  mov sr_want+96(%rip), %r13\n"
        "  mov sr_want+104(%rip), %r14\n"
        "  mov sr_want+112(%rip), %r15\n"
        "1: cmpl $0, sr_hit(%rip)\n"
        "  je 1b\n"
        "  mov %rax, sr_got+0(%rip)\n"
        "  mov %rbx, sr_got+8(%rip)\n"
        "  mov %rcx, sr_got+16(%rip)\n"
        "  mov %rdx, sr_got+24(%rip)\n"
        "  mov %rsi, sr_got+32(%rip)\n"
        "  mov %rdi, sr_got+40(%rip)\n"
        "  mov %rbp, sr_got+48(%rip)\n"
        "  mov %r8, sr_got+56(%rip)\n"
        "  mov %r9, sr_got+64(%rip)\n"
        "  mov %r10, sr_got+72(%rip)\n"
        "  mov %r11, sr_got+80(%rip)\n"
        "  mov %r12, sr_got+88(%rip)\n"
        "  mov %r13, sr_got+96(%rip)\n"
        "  mov %r14, sr_got+104(%rip)\n"
        "  mov %r15, sr_got+112(%rip)\n"
        "  movdqu %xmm1, sr_xgot(%rip)\n"
        "  pop %r15\n pop %r14\n pop %r13\n pop %r12\n pop %rbp\n pop %rbx\n"
        "  ret\n");

static void on_regs(int sig, siginfo_t *si, void *ctx) {
    ucontext_t *uc = ctx;
    greg_t *g = uc->uc_mcontext.gregs;
    uint64_t x0 = 0, x1 = 0;
    if (uc->uc_mcontext.fpregs) {
        memcpy(&x0, &uc->uc_mcontext.fpregs->_xmm[1].element[0], 8);
        memcpy(&x1, &uc->uc_mcontext.fpregs->_xmm[1].element[2], 8);
    }
    if (sig != SIGUSR1)
        bad = 1, bad_seen = (uint64_t)sig;
    else if (si->si_signo != SIGUSR1)
        bad = 2, bad_seen = (uint64_t)si->si_signo;
    else if ((uint64_t)g[REG_R12] != sr_want[11])
        bad = 3, bad_seen = (uint64_t)g[REG_R12];
    else if ((uint64_t)g[REG_RCX] != sr_want[2])
        bad = 4, bad_seen = (uint64_t)g[REG_RCX];
    else if ((uint64_t)g[REG_R11] != sr_want[10])
        bad = 5, bad_seen = (uint64_t)g[REG_R11];
    else if (!uc->uc_mcontext.fpregs)
        bad = 6, bad_seen = 0;
    else if (x0 != sr_xwant[0] || x1 != sr_xwant[1])
        bad = 7, bad_seen = x0;
    /* Everything the handler may overwrite, overwritten: a kernel that
     * returned through the registers as the handler left them fails. */
    __asm__ volatile("mov $0x5151515151515151, %%rax\n"
                     "mov %%rax, %%rbx\n mov %%rax, %%rcx\n mov %%rax, %%rdx\n"
                     "mov %%rax, %%rsi\n mov %%rax, %%rdi\n mov %%rax, %%r8\n"
                     "mov %%rax, %%r9\n mov %%rax, %%r10\n mov %%rax, %%r11\n"
                     "mov %%rax, %%r12\n mov %%rax, %%r13\n mov %%rax, %%r14\n"
                     "mov %%rax, %%r15\n movq %%rax, %%xmm1\n"
                     "std\n cld\n"
                     :
                     :
                     : "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13", "r14",
                       "r15", "xmm1", "cc", "memory");
    sr_hit = 1;
}

static void regs(uint64_t nonce) {
    for (int i = 0; i < 15; i++)
        sr_want[i] = splitmix64(nonce + (uint64_t)i);
    sr_xwant[0] = splitmix64(nonce ^ 0x786d6d31ull);
    sr_xwant[1] = splitmix64(sr_xwant[0]);
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = on_regs;
    sa.sa_flags = SA_SIGINFO;
    if (sigaction(SIGUSR1, &sa, 0) != 0)
        die("sigaction(SIGUSR1) failed");
    pid_t parent = getpid();
    pid_t kid = fork();
    if (kid < 0)
        die("fork failed");
    if (kid == 0) {
        nap(50);
        if (kill(parent, SIGUSR1) != 0) {
            printf("sigregs: kill(%d, SIGUSR1) failed: errno %d\n", (int)parent, errno);
            _exit(1);
        }
        _exit(0);
    }
    sr_spin();
    int st;
    if (waitpid(kid, &st, 0) != kid || !WIFEXITED(st) || WEXITSTATUS(st) != 0)
        die("the child that sends SIGUSR1 did not exit 0");
    static const char *const WHAT[] = {"", "the handler's first argument is not SIGUSR1",
                                       "the siginfo's si_signo is not SIGUSR1",
                                       "the ucontext's saved r12 is not the spin loop's r12",
                                       "the ucontext's saved rcx is not the spin loop's rcx",
                                       "the ucontext's saved r11 is not the spin loop's r11",
                                       "the ucontext has no fpstate (uc_mcontext.fpregs is NULL)",
                                       "the fpstate's saved xmm1 is not the spin loop's xmm1"};
    if (bad) {
        printf("sigregs: in the handler: %s (saw %016" PRIx64 ")\n", WHAT[bad], (uint64_t)bad_seen);
        exit(1);
    }
    for (int i = 0; i < 15; i++) {
        if (sr_got[i] != sr_want[i]) {
            printf("sigregs: %s is %016" PRIx64 " after the handler returned, was %016" PRIx64 " before it ran\n",
                   NAMES[i], sr_got[i], sr_want[i]);
            exit(1);
        }
    }
    if (sr_xgot[0] != sr_xwant[0] || sr_xgot[1] != sr_xwant[1]) {
        printf("sigregs: xmm1 is %016" PRIx64 "%016" PRIx64 " after the handler returned, was %016" PRIx64
               "%016" PRIx64 " before it ran\n",
               sr_xgot[1], sr_xgot[0], sr_xwant[1], sr_xwant[0]);
        exit(1);
    }
    uint64_t fold = nonce;
    for (int i = 0; i < 15; i++)
        fold = splitmix64(fold ^ sr_got[i]);
    fold = splitmix64(fold ^ sr_xgot[0]);
    fold = splitmix64(fold ^ sr_xgot[1]);
    printf("sigregs regs %016" PRIx64 "\n", fold);
}

/* ── restart ── */

static volatile int calls;
static volatile int64_t seen_rax;
static volatile int at_syscall;

static void on_restart(int sig, siginfo_t *si, void *ctx) {
    (void)sig;
    (void)si;
    ucontext_t *uc = ctx;
    const unsigned char *ip = (const unsigned char *)uc->uc_mcontext.gregs[REG_RIP];
    seen_rax = uc->uc_mcontext.gregs[REG_RAX];
    at_syscall = ip[0] == 0x0f && ip[1] == 0x05;
    calls++;
}

static void restart(uint64_t nonce) {
    int code = (int)(nonce & 0x7f);
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = on_restart;
    sa.sa_flags = SA_SIGINFO | SA_RESTART;
    if (sigaction(SIGUSR1, &sa, 0) != 0)
        die("sigaction(SIGUSR1, SA_RESTART) failed");
    int go[2];
    if (pipe(go) != 0)
        die("pipe failed");
    pid_t parent = getpid();
    pid_t waited = fork();
    if (waited < 0)
        die("fork failed");
    if (waited == 0) {
        /* Ends only after the other child has signalled the parent. */
        char c;
        close(go[1]);
        if (read(go[0], &c, 1) != 1)
            _exit(126);
        nap(20);
        _exit(code);
    }
    pid_t sender = fork();
    if (sender < 0)
        die("fork failed");
    if (sender == 0) {
        close(go[0]);
        nap(100);
        kill(parent, SIGUSR1);
        if (write(go[1], "g", 1) != 1)
            _exit(1);
        _exit(0);
    }
    close(go[0]);
    close(go[1]);
    int st = 0;
    long r = syscall(SYS_wait4, waited, &st, 0, 0);
    if (r < 0) {
        printf("sigregs: wait4 under SA_RESTART returned -1, errno %d (%s); a handler with SA_RESTART "
               "restarts it\n",
               errno, errno == EINTR ? "EINTR" : "not EINTR");
        exit(1);
    }
    if (calls != 1) {
        printf("sigregs: the SIGUSR1 handler ran %d times while the parent waited; want once\n", calls);
        exit(1);
    }
    if (seen_rax != SYS_wait4 || !at_syscall) {
        printf("sigregs: the handler's saved context has rax %" PRId64 " and rip %s the syscall instruction; "
               "a restarted wait4 has rax %d and rip on it\n",
               (int64_t)seen_rax, at_syscall ? "on" : "not on", SYS_wait4);
        exit(1);
    }
    if (r != waited || !WIFEXITED(st) || WEXITSTATUS(st) != code) {
        printf("sigregs: the restarted wait4 returned %ld status %#x; want pid %d exited %d\n", r, st,
               (int)waited, code);
        exit(1);
    }
    waitpid(sender, &st, 0);
    printf("sigregs restart pid matched status %d\n", code);
}

/* ── suspend ── */

static volatile int woke;
static void on_suspend(int sig) {
    (void)sig;
    woke++;
}

static void suspend(uint64_t nonce) {
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = on_suspend;
    sa.sa_flags = SA_RESTART; /* rt_sigsuspend is -EINTR all the same */
    if (sigaction(SIGUSR1, &sa, 0) != 0)
        die("sigaction(SIGUSR1) failed");
    sigset_t block, empty, now;
    sigemptyset(&block);
    sigaddset(&block, SIGUSR1);
    sigemptyset(&empty);
    if (sigprocmask(SIG_BLOCK, &block, 0) != 0)
        die("sigprocmask(SIG_BLOCK) failed");
    pid_t parent = getpid();
    pid_t kid = fork();
    if (kid < 0)
        die("fork failed");
    if (kid == 0) {
        nap(50);
        kill(parent, SIGUSR1);
        _exit(0);
    }
    int r = sigsuspend(&empty);
    int e = errno;
    if (r != -1 || e != EINTR) {
        printf("sigregs: rt_sigsuspend returned %d errno %d; want -1 EINTR\n", r, e);
        exit(1);
    }
    if (woke != 1) {
        printf("sigregs: rt_sigsuspend returned before the handler ran (it ran %d times)\n", woke);
        exit(1);
    }
    if (sigprocmask(SIG_BLOCK, 0, &now) != 0 || !sigismember(&now, SIGUSR1)) {
        printf("sigregs: after rt_sigsuspend SIGUSR1 is no longer blocked; the mask it replaced comes back\n");
        exit(1);
    }
    int st;
    waitpid(kid, &st, 0);
    printf("sigregs suspend EINTR %016" PRIx64 "\n", nonce);
}

int main(int argc, char **argv) {
    char *end;
    if (argc != 3 || strlen(argv[2]) != 16)
        die("usage: sigregs regs|restart|suspend <16 hex digits>");
    uint64_t nonce = strtoull(argv[2], &end, 16);
    if (*end)
        die("the nonce is not 16 hex digits");
    setvbuf(stdout, 0, _IONBF, 0);
    if (!strcmp(argv[1], "regs"))
        regs(nonce);
    else if (!strcmp(argv[1], "restart"))
        restart(nonce);
    else if (!strcmp(argv[1], "suspend"))
        suspend(nonce);
    else
        die("usage: sigregs regs|restart|suspend <16 hex digits>");
    return 0;
}
