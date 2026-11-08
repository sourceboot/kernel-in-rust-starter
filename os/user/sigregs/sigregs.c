/* sigregs -- lab 32: what a signal handler may not disturb, and how a wait
 * a signal ends comes back.
 *
 *   sigregs regs <16 hex digits>
 *   sigregs restart <16 hex digits>
 *   sigregs suspend <16 hex digits>
 *   sigregs rflags <16 hex digits>
 *   sigregs ring3 <16 hex digits>
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
 * rflags (added 2026-10-29, slice 9): a SIGUSR1 handler rewrites the
 * frame's saved eflags -- IOPL 3, IF clear, NT set, DF set and the six
 * arithmetic flags to a pattern drawn from the nonce -- and the flags are
 * read with pushfq on the first instruction after the `syscall` the signal
 * came back through. DF and the arithmetic flags are FIX_EFLAGS bits, so
 * they come back from the frame; IOPL, IF and NT are not, so they stay what
 * the program had. (Fix round 1, 2026-10-29:) the action's sa_mask holds
 * SIGUSR2, and the handler reads the blocked mask (SIGUSR1 and SIGUSR2 must
 * be in it) and its own frame pointer (16-aligned: the handler was entered
 * as after a `call`, rsp + 8 16-aligned); after rt_sigreturn the mask must
 * be the one the program had, which (fix round 2) it sets empty first, so a
 * bit the handler's mask leaves behind shows. It prints
 *
 *   sigregs rflags kept <16 hex: the nonce>
 *
 * ring3 (added 2026-10-29, slice 9): children, each with a SIGUSR1
 * handler that forges one thing rt_sigreturn returns to: (1) rsp in the
 * kernel half, with rip on a user-mode routine that exits without touching
 * the stack, so a kernel that returns to it shows as that exit; (2) rip in
 * the kernel half (a kernel that returns there takes a #PF in ring 3 at
 * that rip); (3) cs and ss of ring 0 in the frame's csgsfs. (1) and (2)
 * must end with SIGSEGV; (3) must go on in ring 3 with its own cs and ss,
 * or end with SIGSEGV. Fix round 1 (2026-10-29) adds: (4) and (5), (1)
 * and (2) at 0x0000800000000000, the first address past the user half
 * (non-canonical: the ring-3 fetch (5) makes there is a #GP on QEMU 10.2.1
 * and 11.1.1 and a #PF on 8.2.2). The program cannot tell a SIGSEGV that
 * rt_sigreturn sent from one a fault at the forged rip caused, so the
 * vehicle watches: since 2026-10-31 (G1928) it loads, for this mode, a
 * copy of the kernel's IDT whose #GP and #PF gates note any fault taken
 * in ring 3 at a rip at or past 0x0000800000000000 and then go on to the
 * kernel's own entries (ktest-32.rs, `watch`). It no longer reads
 * mm::last_kill, which a later fault in the mode overwrote. (6) fpstate
 * in the kernel half; (7) rt_sigreturn made by hand, no handler run, with rsp in the
 * kernel half; all four must end with SIGSEGV; (8) a reserved MXCSR bit
 * (31) in the frame's FXSAVE area: the program must go on with it clear,
 * or end with SIGSEGV; (9) SIGKILL and SIGSTOP in uc_sigmask: the SIGKILL
 * its parent then sends must still end it. Fix round 2 (2026-10-29) adds:
 * (10) fpstate at 0x0000400000000000, unmapped, in the user half; (11)
 * fpstate whose 512 bytes cross the end of the user half; (12) rt_sigreturn
 * made by hand, no handler run, with rsp at 0x0000400000000000; all three
 * must end with SIGSEGV, as a kernel that checks only where a pointer starts
 * and then reads it does not; (13) SIGKILL and SIGSTOP in the action's
 * sa_mask, and (14) SIGKILL in the mask rt_sigsuspend is given: the SIGKILL
 * its parent sends while the handler runs, or while it sleeps, must end it.
 * Fix round 3 (2026-10-29) adds two ranges that start on a mapped page and
 * run on into an unmapped one (two pages mapped, the second unmapped again):
 * (15) fpstate 256 bytes before the hole; (16) rt_sigreturn made by hand,
 * no handler run, with rsp 64 bytes before it. Both must end with SIGSEGV,
 * as a kernel that checks the first page and the end's arithmetic, then
 * reads, does not.
 * It prints
 *
 *   sigregs ring3 <16 hex: the nonce>
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
#include <sys/mman.h>
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

/* ── rflags ── */

#define F_CF (1ull << 0)
#define F_PF (1ull << 2)
#define F_AF (1ull << 4)
#define F_ZF (1ull << 6)
#define F_SF (1ull << 7)
#define F_IF (1ull << 9)
#define F_DF (1ull << 10)
#define F_OF (1ull << 11)
#define F_IOPL (3ull << 12)
#define F_NT (1ull << 14)
#define F_ARITH (F_CF | F_PF | F_AF | F_ZF | F_SF | F_OF)

static volatile uint64_t rf_arith; /* the arithmetic flags the handler writes */
static volatile int rf_hit;
static volatile uint64_t rf_mask;  /* the blocked mask inside the handler */
static volatile uint64_t rf_frame; /* the handler's frame pointer */

static uint64_t blocked(void) {
    sigset_t s;
    sigprocmask(SIG_BLOCK, 0, &s);
    return ((uint64_t *)&s)[0];
}

static void on_rflags(int sig, siginfo_t *si, void *ctx) {
    (void)sig;
    (void)si;
    ucontext_t *uc = ctx;
    uint64_t f = (uint64_t)uc->uc_mcontext.gregs[REG_EFL];
    f = (f & ~(F_ARITH | F_IF)) | rf_arith | F_IOPL | F_NT | F_DF;
    uc->uc_mcontext.gregs[REG_EFL] = (greg_t)f;
    rf_mask = blocked();
    rf_frame = (uint64_t)(uintptr_t)__builtin_frame_address(0);
    rf_hit++;
}

static void rflags(uint64_t nonce) {
    static const uint64_t BITS[6] = {F_CF, F_PF, F_AF, F_ZF, F_SF, F_OF};
    uint64_t pick = splitmix64(nonce ^ 0x72666c6167ull), arith = 0;
    for (int i = 0; i < 6; i++)
        if (pick >> i & 1)
            arith |= BITS[i];
    rf_arith = arith;
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = on_rflags;
    sa.sa_flags = SA_SIGINFO;
    sigaddset(&sa.sa_mask, SIGUSR2);
    if (sigaction(SIGUSR1, &sa, 0) != 0)
        die("sigaction(SIGUSR1) failed");
    sigset_t none;
    sigemptyset(&none);
    sigprocmask(SIG_SETMASK, &none, 0); /* the mask the program had, whatever the shell passed down */
    uint64_t mask0 = blocked(), mask1;
    uint64_t good, after, me = (uint64_t)getpid(), nr = SYS_kill;
    __asm__ volatile("pushfq\n pop %0" : "=r"(good)::"memory");
    /* kill(getpid(), SIGUSR1) by hand: the handler runs on the way back from
     * this `syscall`, and pushfq is the first instruction rt_sigreturn returns
     * to. The flags are put back at once (popfq cannot raise IOPL, and sets IF
     * only where IOPL allows it). rsp steps over the red zone first; lea
     * leaves the flags alone. */
    __asm__ volatile("lea -128(%%rsp), %%rsp\n"
                     "syscall\n"
                     "pushfq\n"
                     "pop %0\n"
                     "push %3\n"
                     "popfq\n"
                     "lea 128(%%rsp), %%rsp\n"
                     : "=&r"(after), "+a"(nr)
                     : "D"(me), "r"(good), "S"((uint64_t)SIGUSR1)
                     : "rcx", "r11", "memory", "cc");
    mask1 = blocked();
    if (rf_hit != 1) {
        printf("sigregs: the SIGUSR1 handler ran %d times on the way back from kill(getpid(), SIGUSR1); want once, "
               "before kill returns\n",
               rf_hit);
        exit(1);
    }
    uint64_t want = 1ull << (SIGUSR1 - 1) | 1ull << (SIGUSR2 - 1);
    if ((rf_mask & want) != want) {
        printf("sigregs: inside the SIGUSR1 handler the blocked mask is %#" PRIx64 ", without %s: while a handler "
               "runs, its own signal and its action's sa_mask (here SIGUSR2) are blocked\n",
               rf_mask, rf_mask & 1ull << (SIGUSR1 - 1) ? "SIGUSR2 (bit 11)" : "SIGUSR1 (bit 9)");
        exit(1);
    }
    if (rf_frame & 15) {
        printf("sigregs: the SIGUSR1 handler's frame pointer is %#" PRIx64 ", not 16-aligned: it was entered with "
               "rsp %#" PRIx64 ", where a function's rsp + 8 is 16-aligned, as after a call\n",
               rf_frame, rf_frame + 8);
        exit(1);
    }
    if (mask1 != mask0) {
        printf("sigregs: after rt_sigreturn the blocked mask is %#" PRIx64 ", where it was %#" PRIx64 " before the "
               "signal: the mask comes back from the frame's uc_sigmask\n",
               mask1, mask0);
        exit(1);
    }
    if (after & F_IOPL) {
        printf("sigregs: after rt_sigreturn IOPL is %d (eflags %#" PRIx64 "): the frame's IOPL came back; IOPL is the "
               "kernel's, never the frame's\n",
               (int)(after >> 12 & 3), after);
        exit(1);
    }
    if (!(after & F_IF)) {
        printf("sigregs: after rt_sigreturn IF is clear (eflags %#" PRIx64 "): the frame's IF came back; IF is the "
               "kernel's, never the frame's\n",
               after);
        exit(1);
    }
    if (after & F_NT) {
        printf("sigregs: after rt_sigreturn NT is set (eflags %#" PRIx64 "): the frame's NT came back; only the "
               "FIX_EFLAGS bits come from the frame\n",
               after);
        exit(1);
    }
    if (!(after & F_DF) || (after & F_ARITH) != arith) {
        printf("sigregs: after rt_sigreturn eflags is %#" PRIx64 ", where the handler wrote DF set and the arithmetic "
               "flags %#" PRIx64 " into the frame: the FIX_EFLAGS bits come back from the frame\n",
               after, arith);
        exit(1);
    }
    printf("sigregs rflags kept %016" PRIx64 "\n", nonce);
}

/* ── ring3 ── */

static volatile int r3_what;             /* which field the handler forges */
static volatile uint64_t r3_rsp, r3_rip; /* the kernel-half values it writes */
#define R3_PAST ((uint64_t)0x0000800000000000ull) /* the first address past the user half */
#define R3_HOLE ((uint64_t)0x0000400000000000ull) /* user half, mapped by nothing */
static int r3_ready = -1;                /* children 9, 13, 14: the pipe end that tells the parent */
static volatile uint64_t r3_edge;       /* children 15, 16: the end of a mapped page, nothing after it */
volatile int r3_code;                    /* what r3_escape exits with */

void r3_escape(void);
__asm__(".text\n"
        ".globl r3_escape\n"
        "r3_escape:\n"
        "  mov r3_code(%rip), %edi\n"
        "  mov $60, %eax\n"
        "  syscall\n"
        "  ud2\n");

static void on_ring3(int sig, siginfo_t *si, void *ctx) {
    (void)sig;
    (void)si;
    ucontext_t *uc = ctx;
    greg_t *g = uc->uc_mcontext.gregs;
    if (r3_what == 1 || r3_what == 4) {
        g[REG_RSP] = (greg_t)(r3_what == 1 ? r3_rsp : R3_PAST);
        g[REG_RIP] = (greg_t)(uintptr_t)r3_escape;
    } else if (r3_what == 2 || r3_what == 5) {
        g[REG_RIP] = (greg_t)(r3_what == 2 ? r3_rip : R3_PAST);
    } else if (r3_what == 6) {
        uc->uc_mcontext.fpregs = (fpregset_t)(uintptr_t)r3_rsp;
    } else if (r3_what == 10 || r3_what == 11) {
        uc->uc_mcontext.fpregs = (fpregset_t)(uintptr_t)(r3_what == 10 ? R3_HOLE : R3_PAST - 256);
    } else if (r3_what == 15) {
        uc->uc_mcontext.fpregs = (fpregset_t)(uintptr_t)(r3_edge - 256);
    } else if (r3_what == 13) {
        /* running with the action's sa_mask, SIGKILL in it: tell the parent, then wait for its SIGKILL */
        (void)!write(r3_ready, "k", 1);
        nap(3000);
        _exit(123);
    } else if (r3_what == 14) {
        _exit(123); /* rt_sigsuspend slept through SIGKILL; the parent's SIGUSR1 ended the sleep */
    } else if (r3_what == 8) {
        if (uc->uc_mcontext.fpregs)
            uc->uc_mcontext.fpregs->mxcsr |= 0x80000000u;
    } else if (r3_what == 9) {
        ((uint64_t *)&uc->uc_sigmask)[0] |= 1ull << (SIGKILL - 1) | 1ull << (SIGSTOP - 1);
    } else {
        /* cs 0x08 and ss 0x10, the kernel's selectors, where csgsfs keeps
         * cs (bits 0-15) and ss (bits 48-63). */
        uint64_t v = (uint64_t)g[REG_CSGSFS];
        g[REG_CSGSFS] = (greg_t)((v & 0x0000ffffffff0000ull) | 0x08 | (0x10ull << 48));
    }
}

/* A child that forges field `what` and sends itself SIGUSR1; what waitpid
 * saw of it. */
static int r3_child(int what) {
    int ready[2] = {-1, -1};
    if ((what == 9 || what == 13 || what == 14) && pipe(ready) != 0)
        die("pipe failed");
    pid_t kid = fork();
    if (kid < 0)
        die("fork failed");
    if (kid == 0) {
        if (what == 7) {
            /* rt_sigreturn by hand, with no handler run and rsp in the kernel half */
            __asm__ volatile("mov %0, %%rsp\n mov $15, %%eax\n syscall\n ud2" ::"r"(r3_rsp) : "memory");
        }
        if (what == 12) {
            /* the same, with rsp at an unmapped user-half address */
            __asm__ volatile("mov %0, %%rsp\n mov $15, %%eax\n syscall\n ud2" ::"r"(R3_HOLE) : "memory");
        }
        if (what == 15 || what == 16) {
            /* two pages, the second unmapped again: a mapped page with a hole after it */
            unsigned char *p = mmap(0, 8192, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
            if (p == MAP_FAILED || munmap(p + 4096, 4096) != 0)
                _exit(119);
            memset(p, 0, 4096);
            r3_edge = (uint64_t)(uintptr_t)(p + 4096);
        }
        if (what == 16) {
            /* rt_sigreturn by hand, with rsp 64 bytes before the hole */
            __asm__ volatile("mov %0, %%rsp\n mov $15, %%eax\n syscall\n ud2" ::"r"(r3_edge - 64) : "memory");
        }
        r3_ready = ready[1];
        uint16_t cs0, ss0, cs1, ss1;
        __asm__ volatile("mov %%cs, %0\n mov %%ss, %1" : "=r"(cs0), "=r"(ss0));
        r3_what = what;
        struct sigaction sa;
        memset(&sa, 0, sizeof sa);
        sa.sa_sigaction = on_ring3;
        sa.sa_flags = SA_SIGINFO;
        if (what == 13)
            ((uint64_t *)&sa.sa_mask)[0] |= 1ull << (SIGKILL - 1) | 1ull << (SIGSTOP - 1);
        if (sigaction(SIGUSR1, &sa, 0) != 0)
            _exit(120);
        if (what == 14) {
            /* asleep with every signal but SIGUSR1 blocked, SIGKILL included in what is asked */
            uint64_t m = ~(1ull << (SIGUSR1 - 1));
            (void)!write(ready[1], "k", 1);
            syscall(SYS_rt_sigsuspend, &m, 8);
            _exit(125);
        }
        kill(getpid(), SIGUSR1);
        if (what == 8) {
            unsigned mx;
            __asm__ volatile("stmxcsr %0" : "=m"(mx));
            _exit(mx >> 31 ? 124 : 0);
        }
        if (what == 9) {
            /* back from the handler with the frame's mask: tell the parent, then wait for its SIGKILL */
            (void)!write(ready[1], "k", 1);
            nap(3000);
            _exit(123);
        }
        if (what != 3)
            _exit(121); /* came back to where it was: the forged field was not used */
        __asm__ volatile("mov %%cs, %0\n mov %%ss, %1" : "=r"(cs1), "=r"(ss1));
        _exit(cs1 == cs0 && ss1 == ss0 ? 0 : 122);
    }
    int st;
    if (what == 9 || what == 13 || what == 14) {
        char c;
        close(ready[1]);
        if (read(ready[0], &c, 1) == 1) {
            if (what == 14)
                nap(100); /* the child is asleep in rt_sigsuspend by now */
            kill(kid, SIGKILL);
        }
        close(ready[0]);
        if (what == 14) {
            /* SIGKILL ends it at once; a child still there after 3 s slept through it */
            for (int i = 0; i < 60; i++) {
                pid_t w = waitpid(kid, &st, WNOHANG);
                if (w == kid)
                    return st;
                if (w != 0)
                    die("waitpid of a ring3 child failed");
                nap(50);
            }
            kill(kid, SIGUSR1);
        }
    }
    if (waitpid(kid, &st, 0) != kid)
        die("waitpid of a ring3 child failed");
    return st;
}

/* A child that must end with SIGSEGV; `what` names its frame for the message. */
static void r3_segv(int what, const char *frame) {
    int st = r3_child(what);
    if (!WIFSIGNALED(st) || WTERMSIG(st) != SIGSEGV) {
        printf("sigregs: a program whose %s ended with status %#x; want killed by SIGSEGV (11) in rt_sigreturn\n",
               frame, st);
        exit(1);
    }
}

static void ring3(uint64_t nonce) {
    r3_rsp = 0xffff800000000000ull | (splitmix64(nonce ^ 0x727370ull) & 0x00007ffffffff000ull);
    r3_rip = 0xffff800000000000ull | (splitmix64(nonce ^ 0x726970ull) & 0x00007ffffffffff0ull);
    r3_code = 64 + (int)(nonce & 31);
    int st = r3_child(1);
    if (WIFEXITED(st) && WEXITSTATUS(st) == r3_code) {
        printf("sigregs: rt_sigreturn returned to ring 3 with rsp %016" PRIx64 ", the kernel half, from the frame: "
               "a frame whose rsp is outside the user half ends the program with SIGSEGV\n",
               (uint64_t)r3_rsp);
        exit(1);
    }
    if (!WIFSIGNALED(st) || WTERMSIG(st) != SIGSEGV) {
        printf("sigregs: a program whose frame had rsp %016" PRIx64 " ended with status %#x; want killed by SIGSEGV "
               "(11) in rt_sigreturn\n",
               (uint64_t)r3_rsp, st);
        exit(1);
    }
    st = r3_child(2);
    if (!WIFSIGNALED(st) || WTERMSIG(st) != SIGSEGV) {
        printf("sigregs: a program whose frame had rip %016" PRIx64 " ended with status %#x; want killed by SIGSEGV "
               "(11) in rt_sigreturn\n",
               (uint64_t)r3_rip, st);
        exit(1);
    }
    st = r3_child(4);
    if (WIFEXITED(st) && WEXITSTATUS(st) == r3_code) {
        printf("sigregs: rt_sigreturn returned to ring 3 with rsp %016" PRIx64 ", the first address past the user "
               "half, from the frame: a frame whose rsp is outside the user half ends the program with SIGSEGV\n",
               R3_PAST);
        exit(1);
    }
    if (!WIFSIGNALED(st) || WTERMSIG(st) != SIGSEGV) {
        printf("sigregs: a program whose frame had rsp %016" PRIx64 " ended with status %#x; want killed by SIGSEGV "
               "(11) in rt_sigreturn\n",
               R3_PAST, st);
        exit(1);
    }
    r3_segv(5, "frame had rip 0000800000000000, the first address past the user half,");
    st = r3_child(8);
    if (WIFEXITED(st) && WEXITSTATUS(st) == 124) {
        printf("sigregs: after a frame whose FXSAVE area set MXCSR bit 31, a reserved bit, the program's MXCSR has "
               "it set: rt_sigreturn masks the frame's MXCSR with the CPU's MXCSR_MASK\n");
        exit(1);
    }
    if (!(WIFEXITED(st) && WEXITSTATUS(st) == 0) && !(WIFSIGNALED(st) && WTERMSIG(st) == SIGSEGV)) {
        printf("sigregs: a program whose frame set MXCSR bit 31 ended with status %#x; want it to go on with the bit "
               "clear (exit 0) or be killed by SIGSEGV\n",
               st);
        exit(1);
    }
    st = r3_child(9);
    if (!WIFSIGNALED(st) || WTERMSIG(st) != SIGKILL) {
        printf("sigregs: a program whose frame's uc_sigmask held SIGKILL and SIGSTOP, sent SIGKILL after "
               "rt_sigreturn, ended with status %#x; want killed by SIGKILL (9): the mask comes back with SIGKILL and "
               "SIGSTOP unblocked\n",
               st);
        exit(1);
    }
    st = r3_child(13);
    if (!WIFSIGNALED(st) || WTERMSIG(st) != SIGKILL) {
        printf("sigregs: a program whose SIGUSR1 action's sa_mask held SIGKILL and SIGSTOP, sent SIGKILL while its "
               "handler ran, ended with status %#x; want killed by SIGKILL (9): a handler's mask never blocks SIGKILL "
               "or SIGSTOP\n",
               st);
        exit(1);
    }
    st = r3_child(14);
    if (!WIFSIGNALED(st) || WTERMSIG(st) != SIGKILL) {
        printf("sigregs: a program asleep in rt_sigsuspend with SIGKILL in the mask it passed, sent SIGKILL, ended "
               "with status %#x; want killed by SIGKILL (9): the mask rt_sigsuspend installs never blocks SIGKILL or "
               "SIGSTOP\n",
               st);
        exit(1);
    }
    r3_segv(10, "frame's fpstate was 0000400000000000, unmapped, in the user half,");
    r3_segv(11, "frame's fpstate was 00007fffffffff00, its 512 bytes crossing the end of the user half,");
    r3_segv(12, "rt_sigreturn, made with no handler run, had rsp 0000400000000000, unmapped, in the user half,");
    r3_segv(15, "frame's fpstate began 256 bytes before the end of a mapped page, its 512 bytes running on into an "
                "unmapped one,");
    r3_segv(16, "rt_sigreturn, made with no handler run, had rsp 64 bytes before the end of a mapped page, its "
                "frame running on into an unmapped one,");
    r3_segv(6, "frame's fpstate was in the kernel half");
    r3_segv(7, "rt_sigreturn, made with no handler run, had rsp in the kernel half,");
    st = r3_child(3);
    if (WIFEXITED(st) && WEXITSTATUS(st) == 122) {
        printf("sigregs: after a frame with cs 0x08 and ss 0x10, the program's cs or ss is not what it was: "
               "rt_sigreturn returns to ring 3 with the user's selectors\n");
        exit(1);
    }
    if (!(WIFEXITED(st) && WEXITSTATUS(st) == 0) && !(WIFSIGNALED(st) && WTERMSIG(st) == SIGSEGV)) {
        printf("sigregs: a program whose frame had cs 0x08 and ss 0x10 ended with status %#x; want it to go on in "
               "ring 3 (exit 0) or be killed by SIGSEGV\n",
               st);
        exit(1);
    }
    printf("sigregs ring3 %016" PRIx64 "\n", nonce);
}

int main(int argc, char **argv) {
    char *end;
    if (argc != 3 || strlen(argv[2]) != 16)
        die("usage: sigregs regs|restart|suspend|rflags|ring3 <16 hex digits>");
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
    else if (!strcmp(argv[1], "rflags"))
        rflags(nonce);
    else if (!strcmp(argv[1], "ring3"))
        ring3(nonce);
    else
        die("usage: sigregs regs|restart|suspend|rflags|ring3 <16 hex digits>");
    return 0;
}
