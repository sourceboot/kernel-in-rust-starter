/* tls -- lab 25: a thread-local and SSE registers, kept across preemption.
 *
 *   tls <16 hex digits> t<12 hex digits> <0|1> <ms>
 *
 * Two of these run at once, with different words and slots (the last-but-one
 * argument), each spinning for <ms> milliseconds twice. musl's start-up has
 * already pointed the FS base at this program's thread block (arch_prctl
 * ARCH_SET_FS) before main runs; a C thread-local lives there.
 *
 *   Phase A: musl's thread block. Every turn of the loop reads %fs:0 (musl's
 *   self pointer) and the thread-local `tl_word` through %fs, and compares
 *   xmm8-xmm15 with what was loaded into them.
 *   Phase B: the program points the FS base at a block of its own (ARCH_SET_FS
 *   with an address that differs between the two copies, read back with
 *   ARCH_GET_FS), and the loop reads %fs:0 and %fs:8 from it. Then FS goes
 *   back to musl's block (ARCH_SET_FS) and the thread-local is read once more.
 *
 * A kernel that does not give each process its own FS base, or its own SSE
 * registers, shows the other copy's values here. The loop also counts the
 * gaps in the time-stamp counter longer than 0.2 ms: each is a stretch when
 * something else had the CPU, so the count says how often this copy was
 * preempted. It prints one line,
 *
 *   tls <16 hex: splitmix64(a ^ w ^ tl_word)> t<12 hex> gaps <A> <B> turns <A> <B>
 *
 * or `tls: <what>` on a failed check, and exits 1.
 *
 * Time: the counter's rate is the middle of three 20 ms measurements against
 * CLOCK_MONOTONIC, and each phase ends at <ms> by the counter or by the clock,
 * whichever comes first, so a host pause cannot stretch the run (2026-10-23).
 */
#define _GNU_SOURCE
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <time.h>

#define ARCH_SET_FS 0x1002
#define ARCH_GET_FS 0x1003

static uint64_t splitmix64(uint64_t x) {
    x += 0x9e3779b97f4a7c15ull;
    x = (x ^ (x >> 30)) * 0xbf58476d1ce4e5b9ull;
    x = (x ^ (x >> 27)) * 0x94d049bb133111ebull;
    return x ^ (x >> 31);
}

/* In .tdata: musl copies the initial value from the PT_TLS image. */
static _Thread_local uint64_t tl_word = 0x5b007b0071500025ull;

/* The block phase B points FS at: 64 slots of 64 bytes. */
static uint64_t alt[64 * 8] __attribute__((aligned(64)));

/* A raw arch_prctl: musl's syscall() would set errno through %fs. */
static long arch_prctl(long code, uint64_t addr) {
    long r;
    __asm__ volatile("syscall" : "=a"(r) : "a"((long)SYS_arch_prctl), "D"(code), "S"(addr) : "rcx", "r11", "memory");
    return r;
}

/* What one spin reads and counts. Offsets are fixed: the asm names them. */
struct spin {
    uint64_t deadline; /*  0: stop when the TSC passes this */
    uint64_t thr;      /*  8: a TSC step longer than this is a gap */
    uint64_t gaps;     /* 16 */
    uint64_t bad;      /* 24: 1 when a check failed */
    uint64_t turns;    /* 32 */
    uint64_t e0;       /* 40: %fs:0 must read this */
    int64_t off1;      /* 48: and %fs:off1 */
    uint64_t e1;       /* 56: must read this */
    uint64_t xw[16] __attribute__((aligned(16))); /* 64: xmm8..xmm15 */
};

static void spin(struct spin *s) {
    __asm__ volatile(
        "movdqa 64(%0), %%xmm8\n\t"
        "movdqa 80(%0), %%xmm9\n\t"
        "movdqa 96(%0), %%xmm10\n\t"
        "movdqa 112(%0), %%xmm11\n\t"
        "movdqa 128(%0), %%xmm12\n\t"
        "movdqa 144(%0), %%xmm13\n\t"
        "movdqa 160(%0), %%xmm14\n\t"
        "movdqa 176(%0), %%xmm15\n\t"
        "rdtsc\n\t"
        "shl $32, %%rdx\n\t"
        "or %%rdx, %%rax\n\t"
        "mov %%rax, %%r9\n"
        "1:\n\t"
        "rdtsc\n\t"
        "shl $32, %%rdx\n\t"
        "or %%rdx, %%rax\n\t"
        "mov %%rax, %%r8\n\t"
        "sub %%r9, %%r8\n\t"
        "mov %%rax, %%r9\n\t"
        "cmp 8(%0), %%r8\n\t"
        "jbe 2f\n\t"
        "incq 16(%0)\n"
        "2:\n\t"
        "movdqa %%xmm8, %%xmm0\n\t"  "pcmpeqd 64(%0), %%xmm0\n\t"  "pmovmskb %%xmm0, %%eax\n\t" "cmp $0xffff, %%eax\n\t" "jne 9f\n\t"
        "movdqa %%xmm9, %%xmm0\n\t"  "pcmpeqd 80(%0), %%xmm0\n\t"  "pmovmskb %%xmm0, %%eax\n\t" "cmp $0xffff, %%eax\n\t" "jne 9f\n\t"
        "movdqa %%xmm10, %%xmm0\n\t" "pcmpeqd 96(%0), %%xmm0\n\t"  "pmovmskb %%xmm0, %%eax\n\t" "cmp $0xffff, %%eax\n\t" "jne 9f\n\t"
        "movdqa %%xmm11, %%xmm0\n\t" "pcmpeqd 112(%0), %%xmm0\n\t" "pmovmskb %%xmm0, %%eax\n\t" "cmp $0xffff, %%eax\n\t" "jne 9f\n\t"
        "movdqa %%xmm12, %%xmm0\n\t" "pcmpeqd 128(%0), %%xmm0\n\t" "pmovmskb %%xmm0, %%eax\n\t" "cmp $0xffff, %%eax\n\t" "jne 9f\n\t"
        "movdqa %%xmm13, %%xmm0\n\t" "pcmpeqd 144(%0), %%xmm0\n\t" "pmovmskb %%xmm0, %%eax\n\t" "cmp $0xffff, %%eax\n\t" "jne 9f\n\t"
        "movdqa %%xmm14, %%xmm0\n\t" "pcmpeqd 160(%0), %%xmm0\n\t" "pmovmskb %%xmm0, %%eax\n\t" "cmp $0xffff, %%eax\n\t" "jne 9f\n\t"
        "movdqa %%xmm15, %%xmm0\n\t" "pcmpeqd 176(%0), %%xmm0\n\t" "pmovmskb %%xmm0, %%eax\n\t" "cmp $0xffff, %%eax\n\t" "jne 9f\n\t"
        "mov %%fs:0, %%rax\n\t"
        "cmp 40(%0), %%rax\n\t"
        "jne 9f\n\t"
        "mov 48(%0), %%rcx\n\t"
        "mov %%fs:(%%rcx), %%rax\n\t"
        "cmp 56(%0), %%rax\n\t"
        "jne 9f\n\t"
        "incq 32(%0)\n\t"
        "cmp 0(%0), %%r9\n\t"
        "jb 1b\n\t"
        "jmp 3f\n"
        "9:\n\t"
        "movq $1, 24(%0)\n"
        "3:\n"
        :
        : "r"(s)
        : "rax", "rcx", "rdx", "r8", "r9", "xmm0", "xmm8", "xmm9", "xmm10", "xmm11", "xmm12", "xmm13", "xmm14",
          "xmm15", "memory", "cc");
}

static uint64_t tsc(void) {
    uint32_t lo, hi;
    __asm__ volatile("rdtsc" : "=a"(lo), "=d"(hi));
    return (uint64_t)hi << 32 | lo;
}

static int64_t mono_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (int64_t)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

/* The monotonic clock in ms through a raw syscall, -1 if it fails: phase B
 * reads it while FS points at the program's own block, where no libc runs. */
static int64_t clock_ms(void) {
    struct timespec t;
    long r;
    __asm__ volatile("syscall"
                     : "=a"(r)
                     : "a"((long)SYS_clock_gettime), "D"((long)CLOCK_MONOTONIC), "S"(&t)
                     : "rcx", "r11", "memory");
    if (r != 0) return -1;
    return (int64_t)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

/* One phase: spin until <ms> have passed by the counter (per_ms from the
 * calibration) or by the monotonic clock, whichever ends first, looking at
 * the clock about once a millisecond. A host pause that the calibration did
 * not see through still cannot make the phase longer than <ms> of the clock. */
static void phase(struct spin *s, uint64_t per_ms, int ms) {
    uint64_t end = tsc() + per_ms * (uint64_t)ms;
    int64_t t0 = clock_ms();
    for (;;) {
        uint64_t next = tsc() + per_ms;
        s->deadline = next < end ? next : end;
        spin(s);
        if (s->bad || tsc() >= end) return;
        int64_t t = clock_ms();
        if (t0 >= 0 && t >= 0 && t - t0 >= ms) return;
    }
}

static int fail(const char *what) {
    printf("tls: %s\n", what);
    return 1;
}

int main(int argc, char **argv) {
    char *end;
    if (argc != 5 || strlen(argv[1]) != 16 || argv[2][0] != 't' || strlen(argv[2]) != 13)
        return fail("usage: tls <16 hex digits> t<12 hex digits> <0|1> <ms>");
    uint64_t a = strtoull(argv[1], &end, 16);
    if (*end) return fail("argv[1] is not 16 hex digits");
    uint64_t w = strtoull(argv[2] + 1, &end, 16);
    if (*end) return fail("argv[2] is not t<12 hex digits>");
    int idx = atoi(argv[3]) & 1;
    int ms = atoi(argv[4]);
    if (ms < 10 || ms > 2000) return fail("<ms> is not 10..2000");
    uint64_t seed = a ^ w;

    /* musl's block: the thread-local holds its initial value, and the
     * kernel's FS base is what %fs:0 (musl's self pointer) says. */
    if (tl_word != 0x5b007b0071500025ull) return fail("the thread-local does not hold its initial value");
    uint64_t tp, got = 0;
    __asm__ volatile("mov %%fs:0, %0" : "=r"(tp));
    if (arch_prctl(ARCH_GET_FS, (uint64_t)&got) != 0) return fail("arch_prctl(ARCH_GET_FS) failed");
    if (got != tp) return fail("arch_prctl(ARCH_GET_FS) is not musl's thread pointer");
    tl_word = splitmix64(seed ^ 0x7115);

    /* The TSC's rate, from the monotonic clock: the middle one of three
     * windows of at least 20 ms each. A host pause inside a window moves the
     * counter while the tick clock drops the ticks it missed, so that window's
     * rate comes out many times too large (and the 0.2 ms gap threshold with
     * it, which would hide every preemption); one such window is outvoted. */
    uint64_t rate[3];
    for (int k = 0; k < 3; k++) {
        int64_t m0 = mono_ms(), m1;
        uint64_t c0 = tsc(), c1;
        while ((m1 = mono_ms()) - m0 < 20) {
        }
        c1 = tsc();
        rate[k] = (c1 - c0) / (uint64_t)(m1 - m0);
    }
    uint64_t lo = rate[0] < rate[1] ? rate[0] : rate[1], hi = rate[0] < rate[1] ? rate[1] : rate[0];
    uint64_t per_ms = rate[2] < lo ? lo : rate[2] > hi ? hi : rate[2];
    if (per_ms == 0) return fail("the time-stamp counter does not move");

    static struct spin s;
    for (int i = 0; i < 16; i++) s.xw[i] = splitmix64(seed ^ (0x5e00 + (uint64_t)i));
    s.thr = per_ms / 5;

    /* Phase A: musl's block, tl_word through %fs. */
    s.e0 = tp;
    s.off1 = (int64_t)((uintptr_t)&tl_word - tp);
    s.e1 = tl_word;
    phase(&s, per_ms, ms);
    if (s.bad) return fail("phase A: %fs:0, the thread-local or an xmm register changed under the loop");
    uint64_t gaps_a = s.gaps, turns_a = s.turns;

    /* Phase B: a block of this copy's own, at an address the other copy
     * does not use. Nothing in libc runs while FS points here. */
    int slot = 2 * (int)(seed % 16) + idx;
    uint64_t *blk = &alt[8 * slot];
    uint64_t own = (uint64_t)blk;
    blk[0] = own;
    blk[1] = splitmix64(seed ^ 0xb1);
    for (int i = 0; i < 16; i++) s.xw[i] = splitmix64(seed ^ (0x5f00 + (uint64_t)i));
    s.gaps = s.turns = 0;
    s.e0 = own;
    s.off1 = 8;
    s.e1 = blk[1];
    long r = arch_prctl(ARCH_SET_FS, own);
    uint64_t back = 0;
    long r2 = arch_prctl(ARCH_GET_FS, (uint64_t)&back);
    if (r == 0 && r2 == 0 && back == own) {
        phase(&s, per_ms, ms);
    } else {
        s.bad = 2;
    }
    long r3 = arch_prctl(ARCH_SET_FS, tp);
    if (r3 != 0) return 2; /* musl's block is gone: nothing safe to print with */
    if (r != 0) return fail("phase B: arch_prctl(ARCH_SET_FS) failed");
    if (r2 != 0 || back != own) return fail("phase B: arch_prctl(ARCH_GET_FS) did not give back the new base");
    if (s.bad) return fail("phase B: %fs:0, %fs:8 or an xmm register changed under the loop");
    if (tl_word != splitmix64(seed ^ 0x7115)) return fail("the thread-local changed after FS came back");

    printf("tls %016" PRIx64 " t%012" PRIx64 " gaps %" PRIu64 " %" PRIu64 " turns %" PRIu64 " %" PRIu64 "\n",
           splitmix64(seed ^ tl_word), w, gaps_a, s.gaps, turns_a, s.turns);
    return 0;
}
