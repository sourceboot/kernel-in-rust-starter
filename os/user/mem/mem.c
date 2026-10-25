/* mem -- lab 25: 32 MiB through malloc, realloc and free, plus the mmap family
 * asked directly.
 *
 *   mem <16 hex digits> t<12 hex digits>
 *
 * It first maps two pages, gives the second back, touches the first and
 * gives it back, so every run meets mmap, munmap and a first-touch fault in
 * that order.
 *
 * musl's malloc (zig 0.17.0's: page-sized and larger blocks come straight
 * from mmap, and are trimmed to their alignment with munmap and grown with
 * mremap) serves a stream of requests drawn from s = a ^ w with splitmix64:
 * 48 slots, each empty or holding one block; a draw picks a slot and either
 * fills it (a size from 8 bytes to about 1 MiB), grows or shrinks its block
 * with realloc, or frees it, until 32 MiB have been asked for. Every 8-byte
 * word j of a block with key k holds k ^ (j * 0x9e3779b97f4a7c15); each word
 * is checked before its block is freed or reallocated, and every 64th word
 * (and the last) is folded into the checksum. Then a deep call needs more
 * stack than the 64 KiB a program starts with, so the stack grows; then the
 * calls themselves: mmap with an address hint, munmap of a mapping's head and
 * tail, mremap that must move, MAP_FIXED (refused, or mapped exactly there).
 * Twice a forked child writes to memory that is no longer mapped -- the head
 * munmap gave back, and the tail an mremap shrink cut off -- and the parent
 * waits for it: it must have been killed by SIGSEGV (lab 12's kill, status
 * 11), not have lived to exit. It prints one line,
 *
 *   mem <16 hex: the checksum> t<12 hex> allocs <n> reallocs <n> frees <n>
 *
 * or `mem: <what>` on a failed check, and exits 1.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <unistd.h>

#define SLOTS 48
#define TOTAL (32u << 20)
#define K 0x9e3779b97f4a7c15ull

static uint64_t splitmix64(uint64_t x) {
    x += 0x9e3779b97f4a7c15ull;
    x = (x ^ (x >> 30)) * 0xbf58476d1ce4e5b9ull;
    x = (x ^ (x >> 27)) * 0x94d049bb133111ebull;
    return x ^ (x >> 31);
}

static uint64_t state;
static uint64_t draw(void) {
    state += 0x9e3779b97f4a7c15ull;
    return splitmix64(state);
}

struct slot {
    uint64_t *p;
    size_t words;
    uint64_t key;
};
static struct slot slots[SLOTS];
static uint64_t sum;

static int fail_at(const char *what, int i, size_t j, uint64_t got, uint64_t want) {
    printf("mem: %s: slot %d word %zu holds %016" PRIx64 ", not %016" PRIx64 "\n", what, i, j, got, want);
    return 1;
}

/* A forked child writes the byte at p, which is no longer mapped (`what`
 * says how it went): the write kills it with SIGSEGV, and the parent's
 * waitpid says so. A child that lives exits 0 without flushing stdio. */
static int gone(const char *what, volatile uint8_t *p) {
    pid_t c = fork();
    if (c < 0) return fail_at("fork failed", 0, 0, (uint64_t)errno, 0);
    if (c == 0) {
        *p = 1;
        _exit(0);
    }
    int st = 0;
    if (waitpid(c, &st, 0) != c) return fail_at("waitpid for a forked child failed", 0, 0, (uint64_t)errno, 0);
    if (WIFSIGNALED(st) && WTERMSIG(st) == SIGSEGV) return 0;
    printf("mem: a child that wrote to a page %s ended with status %#x, not killed by SIGSEGV (status 0xb)\n", what, (unsigned)st);
    return 1;
}

/* Words [from, to) of slot i: written. */
static void fill(struct slot *s, size_t from, size_t to) {
    for (size_t j = from; j < to; j++) s->p[j] = s->key ^ (j * K);
}

/* Words [0, n) of slot i: checked; 0 when right. */
static int check(int i, size_t n) {
    struct slot *s = &slots[i];
    for (size_t j = 0; j < n; j++)
        if (s->p[j] != (s->key ^ (j * K))) return fail_at("a block lost its contents", i, j, s->p[j], s->key ^ (j * K));
    return 0;
}

/* The sampled words of slot i, folded into the checksum. */
static void fold(struct slot *s) {
    for (size_t j = 0; j < s->words; j += 64) sum = splitmix64(sum ^ s->p[j]);
    sum = splitmix64(sum ^ s->p[s->words - 1] ^ s->words);
}

static size_t size_of(uint64_t r) {
    unsigned c = (r >> 16) % 16, x = (unsigned)(r >> 24);
    if (c < 10) return 1 + x % 512;         /* 8 B .. 4 KiB, in words */
    if (c < 14) return 512 + x % 16384;     /* 4 KiB .. 132 KiB */
    return 16384 + x % 131072;              /* 128 KiB .. 1.1 MiB */
}

/* More stack than a program starts with (64 KiB): 256 KiB, touched a page
 * at a time from its lowest address up, then summed. */
static __attribute__((noinline)) uint64_t deep(uint64_t s) {
    volatile uint64_t buf[32768];
    for (size_t i = 0; i < 32768; i += 512) buf[i] = s ^ (i * K);
    uint64_t t = 0;
    for (size_t i = 0; i < 32768; i += 512) t = splitmix64(t ^ buf[i]);
    return t;
}

int main(int argc, char **argv) {
    char *end;
    if (argc != 3 || strlen(argv[1]) != 16 || argv[2][0] != 't' || strlen(argv[2]) != 13)
        return fail_at("usage: mem <16 hex digits> t<12 hex digits>", 0, 0, 0, 0);
    uint64_t a = strtoull(argv[1], &end, 16);
    uint64_t w = strtoull(argv[2] + 1, &end, 16);
    state = sum = a ^ w;

    /* First, the one order every run shares: two pages from mmap, the second
     * given back, the first touched (its first-touch fault) and given back.
     * (malloc's own first calls come in an order its first request decides,
     * and that request is drawn from the nonce.) */
    size_t pg = 4096;
    uint8_t *p0 = mmap(0, 2 * pg, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (p0 == MAP_FAILED) return fail_at("mmap of two pages failed", 0, 0, (uint64_t)errno, 0);
    if (munmap(p0 + pg, pg) != 0) return fail_at("munmap of a mapping's last page failed", 0, 0, (uint64_t)errno, 0);
    p0[0] = (uint8_t)a;
    if (p0[0] != (uint8_t)a) return fail_at("a fresh page did not keep a byte", 0, 0, p0[0], (uint8_t)a);
    if (munmap(p0, pg) != 0) return fail_at("munmap of a touched page failed", 0, 0, (uint64_t)errno, 0);

    unsigned long allocs = 0, reallocs = 0, frees = 0;
    uint64_t asked = 0;
    while (asked < TOTAL) {
        uint64_t r = draw();
        int i = (int)(r % SLOTS);
        struct slot *s = &slots[i];
        if (!s->p) {
            s->words = size_of(r);
            s->key = draw();
            s->p = malloc(s->words * 8);
            if (!s->p) return fail_at("malloc returned NULL", i, s->words, 0, 0);
            fill(s, 0, s->words);
            asked += s->words * 8;
            allocs++;
            continue;
        }
        unsigned op = (unsigned)(r >> 8) % 4;
        if (op == 0 && s->words < 65536) {
            size_t n = s->words * 2 + (r >> 40) % 64;
            if (check(i, s->words)) return 1;
            uint64_t *q = realloc(s->p, n * 8);
            if (!q) return fail_at("realloc (grow) returned NULL", i, n, 0, 0);
            s->p = q;
            if (check(i, s->words)) return 1;
            fill(s, s->words, n);
            asked += (n - s->words) * 8;
            s->words = n;
            reallocs++;
        } else if (op == 1 && s->words >= 2) {
            size_t n = s->words / 2;
            if (check(i, s->words)) return 1;
            uint64_t *q = realloc(s->p, n * 8);
            if (!q) return fail_at("realloc (shrink) returned NULL", i, n, 0, 0);
            s->p = q;
            s->words = n;
            if (check(i, n)) return 1;
            reallocs++;
        } else {
            if (check(i, s->words)) return 1;
            fold(s);
            free(s->p);
            s->p = 0;
            frees++;
        }
    }
    for (int i = 0; i < SLOTS; i++) {
        if (!slots[i].p) continue;
        if (check(i, slots[i].words)) return 1;
        fold(&slots[i]);
        free(slots[i].p);
        slots[i].p = 0;
        frees++;
    }

    /* The stack grows. */
    sum = splitmix64(sum ^ deep(sum));

    /* mmap with an address hint: a hint is never a reason to refuse. */
    size_t page = 4096;
    uint8_t *h = mmap((void *)0x40000000, 24 * page, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (h == MAP_FAILED) return fail_at("mmap with an address hint failed", 0, 0, (uint64_t)errno, 0);
    for (size_t k = 0; k < 24; k++) h[k * page] = (uint8_t)(k + 1);
    /* Its head and its tail go; the middle stays, with what it held. */
    if (munmap(h, 4 * page) != 0) return fail_at("munmap of a mapping's head failed", 0, 0, (uint64_t)errno, 0);
    if (munmap(h + 20 * page, 4 * page) != 0) return fail_at("munmap of a mapping's tail failed", 0, 0, (uint64_t)errno, 0);
    for (size_t k = 4; k < 20; k++)
        if (h[k * page] != (uint8_t)(k + 1)) return fail_at("a page kept after munmap lost its contents", 0, k, h[k * page], k + 1);
    if (gone("that munmap had given back", h + page)) return 1;
    /* A hole in the middle splits it in two. */
    if (munmap(h + 10 * page, 2 * page) != 0) return fail_at("munmap of a mapping's middle failed", 0, 0, (uint64_t)errno, 0);
    for (size_t k = 4; k < 20; k++)
        if ((k < 10 || k >= 12) && h[k * page] != (uint8_t)(k + 1))
            return fail_at("a page beside a hole lost its contents", 0, k, h[k * page], k + 1);
    /* Grown past what follows it, so it must move; its pages go with it. */
    uint8_t *g = mremap(h + 12 * page, 8 * page, 64 * page, MREMAP_MAYMOVE);
    if (g == MAP_FAILED) return fail_at("mremap(MREMAP_MAYMOVE) that must move failed", 0, 0, (uint64_t)errno, 0);
    for (size_t k = 0; k < 8; k++)
        if (g[k * page] != (uint8_t)(k + 13)) return fail_at("mremap lost a page's contents", 0, k, g[k * page], k + 13);
    for (size_t k = 8; k < 64; k++) g[k * page] = (uint8_t)k;
    /* Shrunk in place. */
    uint8_t *g2 = mremap(g, 64 * page, 16 * page, 0);
    if (g2 != g) return fail_at("mremap that shrinks did not stay in place", 0, 0, (uint64_t)g2, (uint64_t)g);
    if (gone("that an mremap shrink had cut off", g + 40 * page)) return 1;
    /* MAP_FIXED at the freed head: refused with EINVAL in lab 25 (CONTRACT
     * section A.4, row 9); a kernel that takes it (a later lab may) must map
     * exactly there. */
    void *f = mmap(h, page, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0);
    if (f == MAP_FAILED ? errno != EINVAL : f != (void *)h)
        return fail_at("mmap(MAP_FIXED) neither refused with EINVAL nor mapped at its address", 0, 0, (uint64_t)f, (uint64_t)h);
    if (f != MAP_FAILED) munmap(f, page);
    uint64_t t = 0;
    for (size_t k = 0; k < 16; k++) t = splitmix64(t ^ g2[k * page]);
    sum = splitmix64(sum ^ t);
    munmap(h + 4 * page, 6 * page);
    munmap(g2, 16 * page);

    printf("mem %016" PRIx64 " t%012" PRIx64 " allocs %lu reallocs %lu frees %lu\n", sum, w, allocs, reallocs, frees);
    return 0;
}
