/*
 * user/hostile-spin/hostile-spin.c -- lab 12: a program that never makes a syscall,
 * and the registers-survive-preemption row.
 *
 * Protocol (CONTRACT §A.3): argv = ["hostile-spin", <nonce>, <index>] (the reporter)
 * or ["hostile-spin", <nonce>, <index>, "noise"] (the noise instance).
 *
 * The reporter loads seven values derived from the nonce (§A.2 derive, id
 * regs.survive.preempt) into five callee-saved general registers (rbx,
 * r12-r15) and two SSE registers (xmm0, xmm1), spins CHECKS times through
 * ITERS iterations of a loop that touches none of them, and after every
 * ITERS iterations compares all seven with the values in memory. Then:
 *     ok <index> - regs.survive.preempt      all CHECKS comparisons matched
 *     not ok <index> - regs.survive.preempt  one did not (a '#' line names the register)
 * and it spins on forever, still checking, with no syscall: the vehicle's
 * K row kill.spin.preempted watches another process make progress meanwhile.
 *
 * The noise instance prints one '#' line and then rewrites the same seven
 * registers with different values on every iteration, forever, with no
 * syscall and no row. Run beside the reporter on one CPU, it is what makes a
 * kernel that does not save and restore a task's registers -- FXSAVE/FXRSTOR
 * for xmm, the trap frame for the GPRs -- fail the reporter's row: the course
 * kernel itself and every other pre-23 program carry no SSE, so without a
 * second SSE writer a missing FXSAVE would go unseen.
 *
 * This is the one pre-23 program with SSE instructions in it, on purpose
 * (CONTRACT §A.8, amended 2026-10-17): they are the movq lines in the inline
 * asm below, and build.sh's layout `flat-sse` allows exactly those.
 * rdtsc brackets every check; the min and max interval go on a '#' line (a max
 * far above the min is a check the process was descheduled during).
 */
#include "../sys.h"

#define ID     "regs.survive.preempt"
#define ITERS  (1ULL << 20)       /* loop iterations between two checks */
#define CHECKS 64ULL              /* checks before the row is reported */

/* v[0..6]: rbx r12 r13 r14 r15 xmm0 xmm1; v[7] last tsc, v[8] min interval, v[9] max interval. */
static const char *const REG[7] = { "rbx", "r12", "r13", "r14", "r15", "xmm0", "xmm1" };

/* Loads v[0..6] into the registers, then runs `checks` checks of `iters` iterations.
 * Returns 0 when every check matched, else 1 + the index of the register that differed. */
static u64 spin_check(u64 *v, u64 iters, u64 checks)
{
    u64 res, done = 0;
    __asm__ volatile (
        "    mov    0(%[v]), %%rbx\n"
        "    mov    8(%[v]), %%r12\n"
        "    mov   16(%[v]), %%r13\n"
        "    mov   24(%[v]), %%r14\n"
        "    mov   32(%[v]), %%r15\n"
        "    movq  40(%[v]), %%xmm0\n"
        "    movq  48(%[v]), %%xmm1\n"
        "    rdtsc\n"
        "    shl   $32, %%rdx\n"
        "    or    %%rdx, %%rax\n"
        "    mov   %%rax, 56(%[v])\n"
        "1:  mov   %[n], %%rcx\n"
        "2:  dec   %%rcx\n"
        "    jnz   2b\n"
        "    mov   $1, %[res]\n"
        "    cmp   0(%[v]), %%rbx\n"
        "    jne   9f\n"
        "    mov   $2, %[res]\n"
        "    cmp   8(%[v]), %%r12\n"
        "    jne   9f\n"
        "    mov   $3, %[res]\n"
        "    cmp   16(%[v]), %%r13\n"
        "    jne   9f\n"
        "    mov   $4, %[res]\n"
        "    cmp   24(%[v]), %%r14\n"
        "    jne   9f\n"
        "    mov   $5, %[res]\n"
        "    cmp   32(%[v]), %%r15\n"
        "    jne   9f\n"
        "    mov   $6, %[res]\n"
        "    movq  %%xmm0, %%rax\n"
        "    cmp   40(%[v]), %%rax\n"
        "    jne   9f\n"
        "    mov   $7, %[res]\n"
        "    movq  %%xmm1, %%rax\n"
        "    cmp   48(%[v]), %%rax\n"
        "    jne   9f\n"
        "    rdtsc\n"
        "    shl   $32, %%rdx\n"
        "    or    %%rdx, %%rax\n"
        "    mov   %%rax, %%rcx\n"
        "    sub   56(%[v]), %%rcx\n"
        "    mov   %%rax, 56(%[v])\n"
        "    cmp   64(%[v]), %%rcx\n"
        "    jae   3f\n"
        "    mov   %%rcx, 64(%[v])\n"
        "3:  cmp   72(%[v]), %%rcx\n"
        "    jbe   4f\n"
        "    mov   %%rcx, 72(%[v])\n"
        "4:  inc   %[done]\n"
        "    cmp   %[m], %[done]\n"
        "    jb    1b\n"
        "    xor   %[res], %[res]\n"
        "9:\n"
        : [res] "=&r"(res), [done] "+&r"(done)
        : [v] "r"(v), [n] "r"(iters), [m] "r"(checks)
        : "rax", "rcx", "rdx", "rbx", "r12", "r13", "r14", "r15", "cc", "memory");
    return res;
}

/* The noise instance: rewrites the seven registers with v[0..6] on every iteration, forever. */
static __attribute__((noreturn)) void spin_noise(const u64 *v)
{
    __asm__ volatile (
        "1:  mov    0(%[v]), %%rbx\n"
        "    mov    8(%[v]), %%r12\n"
        "    mov   16(%[v]), %%r13\n"
        "    mov   24(%[v]), %%r14\n"
        "    mov   32(%[v]), %%r15\n"
        "    movq  40(%[v]), %%xmm0\n"
        "    movq  48(%[v]), %%xmm1\n"
        "    jmp   1b\n"
        :
        : [v] "r"(v)
        : "rbx", "r12", "r13", "r14", "r15", "memory");
    __builtin_unreachable();
}

int main(int argc, char **argv, char **envp)
{
    struct line l = { 0, {0} };
    u64 index = 0;
    (void)envp;

    if (argc < 3 || !dec_parse(argv[2], &index)) {
        line_row(&l, 0, 0, ID);
        return 2;
    }
    const char *nonce = argv[1];
    int noise = argc >= 4 && streq(argv[3], "noise");
    int good_nonce = nonce_valid(nonce);

    /* The pattern: v[k] = splitmix64(seed + k), seed = derive(nonce, ID); the noise
     * instance uses the complement of the seed, so its seven values differ. */
    u64 seed = good_nonce ? derive(nonce, ID) : 0;
    if (noise) seed = ~seed;
    u64 v[10];
    for (int k = 0; k < 7; k++) v[k] = splitmix64(seed + (u64)k);
    v[7] = 0;
    v[8] = ~0ULL;
    v[9] = 0;

    line_note(&l, noise ? "hostile-spin: noise instance, seed=" : "hostile-spin: seed=");
    line_hex64(&l, seed);
    line_str(&l, noise ? ", rewriting rbx r12-r15 xmm0 xmm1 forever" : ", checking rbx r12-r15 xmm0 xmm1");
    line_end(&l);
    if (noise) spin_noise(v);

    u64 res = spin_check(v, ITERS, CHECKS);
    if (!good_nonce) {
        line_note(&l, "nonce is not 32 hex characters");
        line_end(&l);
    }
    if (res) {
        line_note(&l, "regs.survive.preempt: ");
        line_str(&l, REG[res - 1]);
        line_str(&l, " changed while this process spun (expected ");
        line_hex64(&l, v[res - 1]);
        line_str(&l, ")");
        line_end(&l);
    } else {
        line_note(&l, "regs.survive.preempt: ");
        line_dec(&l, CHECKS);
        line_str(&l, " checks x ");
        line_dec(&l, ITERS);
        line_str(&l, " iterations; tsc per check min=");
        line_dec(&l, v[8]);
        line_str(&l, " max=");
        line_dec(&l, v[9]);
        line_end(&l);
    }
    line_row(&l, good_nonce && res == 0, index, ID);

    /* Spin on, with no syscall, still checking; a later change is named once. */
    int named = res != 0;
    for (;;) {
        u64 r = spin_check(v, ITERS, CHECKS);
        if (r && !named) {
            named = 1;
            line_note(&l, "regs.survive.preempt: after the row, ");
            line_str(&l, REG[r - 1]);
            line_str(&l, " changed");
            line_end(&l);
        }
    }
}
