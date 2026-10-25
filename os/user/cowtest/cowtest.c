/*
 * user/cowtest/cowtest.c -- lab 24: copy-on-write fork, seen from user space.
 *
 * Protocol (CONTRACT §B.2-24): argv = ["cowtest", <nonce>, "t" <16 hex>], the
 * second word unpublished (CONTRACT §A.1-5). fd 0 is the control pipe the
 * kernel side writes one command into at a time; fds 1 and 2 are the pipe it
 * reads. Every line below goes out in one write(1). The program holds 48 MiB
 * in .bss (BIG_PAGES pages); once touched, word w of page p holds pre(p, w):
 * base(p) at word 0, base(p) ^ (w * step) after it (step drawn from the
 * nonce), except X_PAGE's words 1..FORKS, which are 0.
 *
 *   touched <pages> <addr>              every word of every page written (addr: big's first byte); waits for "1"
 *   "1": three forks. Before fork k, word k of page X_PAGE is vpre(k); right
 *        after it, in the parent, vpost(k). Child k checks its view (words
 *        j < k: vpost(j); word k: vpre(k); later words 0; sampled base
 *        words), then:
 *   ready <k> <mismatches>              (child k) and waits on the go pipe
 *   forked <r1> <r2> <r3>               (parent) fork's three returns; waits
 *   "2" <16 hex u>: read straight into page Y_PAGE (the kernel's write into a
 *        page the parent shares with its children). Every process i (0 = the
 *        parent, k = child k) writes pat(i, p, j) at word 1 + 64j of each of
 *        S_COUNT pages chosen from the seed, waits until the others have
 *        written theirs, and reads them back; the children report and exit 0:
 *   torn <i> <page> <byte> <n>          (any process, right before its private line, only
 *                                       when n > 0) n words it never wrote, in the pages it
 *                                       reads back (X_PAGE, Y_PAGE, Z_PAGE and row 2's, every
 *                                       word but its own), differ from pre(p, w); the first
 *                                       in <page>, at <byte> of it. Not in <mismatches>.
 *   private <i> <H> <mismatches>        H folds the 256 words read with splitmix64
 *   reaped <pid> <status>               (parent) its wait4 status written into page Z_PAGE
 *   private 0 <H> <mismatches> <u>      (parent) after reaping, with u read back from Y_PAGE
 *   "3": a child writes a byte of the program's text and exits 77 if it lives:
 *   text <pid> <status> <addr> <byte before> <byte after>
 *   "4": a child Q (the parent waits for it) writes nothing until told:
 *   ready4 <K>                          (Q) K = 64 + derive(nonce, "cow.meminfo.deltas") % 193
 *   "w": Q writes one word into each of K pages, then:  wrote <K>
 *   "x": Q exits 0;  reaped4 <pid> <status>   (parent)
 *   "p": the parent writes the same K pages:  rewrote <K> <mismatches>
 *   "e" (at any wait): exit 0.
 */
#include "../sys.h"

#define BIG_PAGES 12288u                 /* 48 MiB */
#define X_PAGE    1u
#define Y_PAGE    2u
#define Z_PAGE    3u
#define S_FIRST   16u                    /* row 2's pages: [S_FIRST, S4_FIRST) */
#define S_COUNT   32u
#define S_WORDS   8u
#define S4_FIRST  4096u                  /* row 4's pages: [S4_FIRST, BIG_PAGES) */
#define SAMPLES   64u
#define FORKS     3

static u8 big[BIG_PAGES * 4096u] __attribute__((aligned(4096)));
static u64 base_seed, seed, step;

static volatile u64 *word(u64 page, u64 w) { return (volatile u64 *)(big + page * 4096u) + w; }
static u64 base(u64 p) { return splitmix64(base_seed ^ (p << 16) ^ 0xb5); }
static u64 vpre(u64 k) { return splitmix64(base_seed ^ 0xa000 ^ k); }
static u64 vpost(u64 k) { return splitmix64(base_seed ^ 0xb000 ^ k); }
static u64 spage(u64 s) { return S_FIRST + (seed % (S4_FIRST - S_FIRST) + s * 113u) % (S4_FIRST - S_FIRST); }
static u64 pat(u64 i, u64 p, u64 j) { return splitmix64(seed ^ (i << 56) ^ (p << 8) ^ j); }
/* What word w of page p held at the forks. */
static u64 pre(u64 p, u64 w)
{
    if (p == X_PAGE && w >= 1 && w <= FORKS) return 0;
    return base(p) ^ (w * step);
}

/* The function the text write aims at: its first byte is program text. */
__attribute__((noinline)) static u64 text_target(u64 x) { return splitmix64(x) ^ 0x7e; }

static __attribute__((noreturn)) void die(const char *what, i64 r)
{
    err_line("cowtest", what, r);
    sys_exit(3);
}

/* Write to every stack page this process will use from here on, and to `buf`:
 * after a fork, a page written first by user code is copied then, not later
 * inside a syscall or a measured window. Each byte is written with its own value. */
static __attribute__((noinline)) void pretouch(volatile u8 *buf, usize n)
{
    u64 sp;
    __asm__ volatile("mov %%rsp, %0" : "=r"(sp));
    for (u64 a = (sp & ~0xfffull) - 4 * 4096u; a <= (sp & ~0xfffull) + 4096u && a < 0x7ffffffff000ull; a += 4096u) {
        volatile u8 *b = (volatile u8 *)a;
        *b = *b;
    }
    for (usize i = 0; i < n; i += 64) buf[i] = buf[i];
}

/* One byte from `fd` into `c`, or the program ends ("e", end of file). */
static u8 ctl(int fd, volatile u8 *c)
{
    i64 r = sys_read(fd, (void *)c, 1);
    if (r != 1) die("read", r < 0 ? r : -EIO);
    if (*c == 'e') sys_exit(0);
    return *c;
}

/* The sampled base words still hold base(p) (row 4's range, untouched until then). */
static u64 base_bad(void)
{
    u64 bad = 0;
    for (u64 s = 0; s < SAMPLES; s++) {
        u64 p = S4_FIRST + s * ((BIG_PAGES - S4_FIRST) / SAMPLES);
        if (*word(p, 0) != base(p)) bad++;
    }
    return bad;
}

/* Words of X_PAGE as process `who` should see them after `forks` forks. */
static u64 x_bad(u64 who, u64 forks)
{
    u64 bad = 0;
    for (u64 j = 1; j <= FORKS; j++) {
        u64 want = 0;
        if (who == 0) want = j <= forks ? vpost(j) : 0;
        else if (j < who) want = vpost(j);
        else if (j == who) want = vpre(j);
        if (*word(X_PAGE, j) != want) bad++;
    }
    return bad;
}

/* Every word of page p from word `from` on that this process never wrote
 * (row 2's pattern words in row 2's pages, X_PAGE's words 1..FORKS) still
 * holds pre(p, w): a copy made on a write, the process's own or the kernel's,
 * carries the whole page. Differences are counted in torn_n; the first is
 * kept for the `torn` line. */
static u64 torn_n, torn_page, torn_byte;
static void whole(u64 p, u64 from, int spg)
{
    for (u64 w = from; w < 512; w++) {
        if (spg && w % 64 == 1 && w / 64 < S_WORDS) continue;
        if (p == X_PAGE && w >= 1 && w <= FORKS) continue;
        if (*word(p, w) != pre(p, w) && torn_n++ == 0) {
            torn_page = p;
            torn_byte = w * 8;
        }
    }
}

static void torn_line(u64 i)
{
    if (!torn_n) return;
    struct line l = { 0, {0} };
    line_str(&l, "torn ");
    line_dec(&l, i);
    line_str(&l, " ");
    line_dec(&l, torn_page);
    line_str(&l, " ");
    line_dec(&l, torn_byte);
    line_str(&l, " ");
    line_dec(&l, torn_n);
    line_end(&l);
}

static void write_pattern(u64 i)
{
    for (u64 s = 0; s < S_COUNT; s++)
        for (u64 j = 0; j < S_WORDS; j++) *word(spage(s), 1 + 64 * j) = pat(i, spage(s), j);
}

/* Reads the pattern back: H, and the mismatches (pattern, base words of those pages). */
static u64 read_pattern(u64 i, u64 *bad)
{
    u64 h = 0;
    for (u64 s = 0; s < S_COUNT; s++) {
        u64 p = spage(s);
        if (*word(p, 0) != base(p)) (*bad)++;
        whole(p, 0, 1);
        for (u64 j = 0; j < S_WORDS; j++) {
            u64 v = *word(p, 1 + 64 * j);
            if (v != pat(i, p, j)) (*bad)++;
            h = splitmix64(h ^ v);
        }
    }
    return h;
}

static __attribute__((noreturn)) void child(u64 k, int go, int done, int go2)
{
    struct line l = { 0, {0} };
    volatile u8 c[64];
    pretouch(c, sizeof c);
    u64 bad = x_bad(k, k) + base_bad();
    line_str(&l, "ready ");
    line_dec(&l, k);
    line_str(&l, " ");
    line_dec(&l, bad);
    line_end(&l);
    if (sys_read(go, (void *)c, 1) != 1) sys_exit(4);
    write_pattern(k);
    if (sys_write(done, (const void *)c, 1) != 1) sys_exit(5);
    if (sys_read(go2, (void *)c, 1) != 1) sys_exit(6);
    u64 pbad = x_bad(k, k) + base_bad() + (*word(Y_PAGE, 0) != base(Y_PAGE));
    whole(X_PAGE, 0, 0);
    whole(Y_PAGE, 0, 0);
    u64 h = read_pattern(k, &pbad);
    torn_line(k);
    line_str(&l, "private ");
    line_dec(&l, k);
    line_str(&l, " ");
    line_hex64(&l, h);
    line_str(&l, " ");
    line_dec(&l, pbad);
    line_end(&l);
    sys_exit(0);
}

/* Row 4's child: nothing of its own is written between "ready4" and "w", nor
 * between "w" and "x", except the K pages. */
static __attribute__((noreturn)) void q_child(u64 k4, u64 first)
{
    struct line l = { 0, {0} };
    volatile u8 c[64];
    pretouch(c, sizeof c);
    pretouch((volatile u8 *)&l, sizeof l);
    line_str(&l, "ready4 ");
    line_dec(&l, k4);
    line_end(&l);
    if (ctl(0, c) != 'w') sys_exit(8);
    for (u64 i = 0; i < k4; i++) {
        u64 p = S4_FIRST + (first + i) % (BIG_PAGES - S4_FIRST);
        *word(p, 2) = pat(9, p, 2);
    }
    line_str(&l, "wrote ");
    line_dec(&l, k4);
    line_end(&l);
    ctl(0, c);
    sys_exit(0);
}

static int hexword(const volatile u8 *s, u64 *out)
{
    u64 v = 0;
    for (int i = 0; i < 16; i++) {
        int h = hexval((char)s[i]);
        if (h < 0) return 0;
        v = v << 4 | (u64)h;
    }
    *out = v;
    return 1;
}

int main(int argc, char **argv, char **envp)
{
    struct line l = { 0, {0} };
    volatile u8 c[64];
    u64 t = 0;
    (void)envp;
    if (argc < 3 || !nonce_valid(argv[1]) || argv[2][0] != 't' || !hexword((const volatile u8 *)argv[2] + 1, &t)) {
        const char *u = "cowtest: usage: cowtest <nonce> t<16 hex>\n";
        write_buf_fd(2, u, strlen(u));
        return 2;
    }
    const char *nonce = argv[1];
    base_seed = derive(nonce, "cow.48m.3forks.fits");
    seed = derive(nonce, "cow.private.pages") ^ t;
    step = splitmix64(base_seed ^ 0xc0) | 1;

    for (u64 p = 0; p < BIG_PAGES; p++)
        for (u64 w = 0; w < 512; w++) *word(p, w) = pre(p, w);
    line_str(&l, "touched ");
    line_dec(&l, BIG_PAGES);
    line_str(&l, " ");
    line_hex(&l, (u64)(usize)big);
    line_end(&l);
    pretouch(c, sizeof c);
    if (ctl(0, c) != '1') return 7;

    /* Row 1: three forks. */
    int go[2], done[2], go2[2];
    i64 r;
    if ((r = sys_pipe2(go, 0)) < 0 || (r = sys_pipe2(done, 0)) < 0 || (r = sys_pipe2(go2, 0)) < 0) die("pipe2", r);
    i64 pid[FORKS];
    u64 kids = 0;
    for (u64 k = 1; k <= FORKS; k++) {
        *word(X_PAGE, k) = vpre(k);
        i64 f = sys_fork();
        if (f == 0) child(k, go[0], done[1], go2[0]);
        *word(X_PAGE, k) = vpost(k);
        pid[k - 1] = f;
        if (f > 0) kids++;
    }
    line_str(&l, "forked");
    for (int k = 0; k < FORKS; k++) {
        line_str(&l, " ");
        line_sdec(&l, pid[k]);
    }
    line_end(&l);
    pretouch(c, sizeof c);

    /* Row 2: the control message lands in a page shared with every child. */
    volatile u8 *y = big + Y_PAGE * 4096u;
    r = sys_read(0, (void *)y, 17);
    if (r >= 1 && y[0] == 'e') return 0;
    u64 u = 0;
    if (r != 17 || y[0] != '2' || !hexword(y + 1, &u)) die("read", r < 0 ? r : -EINVAL);
    for (u64 k = 0; k < kids; k++)
        if (sys_write(go[1], (const void *)c, 1) != 1) die("write", -EIO);
    write_pattern(0);
    for (u64 k = 0; k < kids; k++)
        if (sys_read(done[0], (void *)c, 1) != 1) die("read", -EIO);
    for (u64 k = 0; k < kids; k++)
        if (sys_write(go2[1], (const void *)c, 1) != 1) die("write", -EIO);
    for (u64 k = 0; k < kids; k++) {
        int *st = (int *)(big + Z_PAGE * 4096u) + 2 * k;
        i64 w = sys_wait4(-1, st, 0);
        line_str(&l, "reaped ");
        line_sdec(&l, w);
        line_str(&l, " ");
        line_hex(&l, (u64)(u32)*st);
        line_end(&l);
    }
    u64 pbad = x_bad(0, FORKS) + base_bad();
    whole(X_PAGE, 0, 0);
    whole(Y_PAGE, 3, 0);
    whole(Z_PAGE, FORKS, 0);
    u64 h = read_pattern(0, &pbad);
    torn_line(0);
    u64 back = 0;
    if (!hexword(y + 1, &back) || back != u) pbad++;
    line_str(&l, "private 0 ");
    line_hex64(&l, h);
    line_str(&l, " ");
    line_dec(&l, pbad);
    line_str(&l, " ");
    line_hex64(&l, back);
    line_end(&l);
    pretouch(c, sizeof c);
    if (ctl(0, c) != '3') return 7;

    /* Row 3: a write into the program's text. */
    volatile u8 *tp = (volatile u8 *)(usize)&text_target;
    u8 before = *tp;
    i64 d = sys_fork();
    if (d == 0) {
        *tp = (u8)(before ^ 0xff);
        sys_exit(77);
    }
    int st3 = 0;
    i64 w3 = d > 0 ? sys_wait4(d, &st3, 0) : d;
    line_str(&l, "text ");
    line_sdec(&l, w3);
    line_str(&l, " ");
    line_hex(&l, (u64)(u32)st3);
    line_str(&l, " ");
    line_hex(&l, (u64)(usize)tp);
    line_str(&l, " ");
    line_hex(&l, before);
    line_str(&l, " ");
    line_hex(&l, *tp);
    line_end(&l);
    (void)text_target(t);
    pretouch(c, sizeof c);
    if (ctl(0, c) != '4') return 7;

    /* Row 4: K pages written by a child, then by the parent alone. */
    u64 dv = derive(nonce, "cow.meminfo.deltas");
    u64 k4 = 64 + dv % 193;
    u64 first = splitmix64(dv) % (BIG_PAGES - S4_FIRST);
    int st4 = 0;
    i64 q = sys_fork();
    if (q == 0) q_child(k4, first);
    i64 w4 = q > 0 ? sys_wait4(q, &st4, 0) : q;
    line_str(&l, "reaped4 ");
    line_sdec(&l, w4);
    line_str(&l, " ");
    line_hex(&l, (u64)(u32)st4);
    line_end(&l);
    pretouch(c, sizeof c);
    if (ctl(0, c) != 'p') return 7;
    u64 bad4 = 0;
    for (u64 i = 0; i < k4; i++) {
        u64 p = S4_FIRST + (first + i) % (BIG_PAGES - S4_FIRST);
        if (*word(p, 0) != base(p) || *word(p, 2) != pre(p, 2)) bad4++;
        *word(p, 2) = pat(0, p, 2);
    }
    for (u64 i = 0; i < k4; i++) {
        u64 p = S4_FIRST + (first + i) % (BIG_PAGES - S4_FIRST);
        if (*word(p, 2) != pat(0, p, 2)) bad4++;
    }
    line_str(&l, "rewrote ");
    line_dec(&l, k4);
    line_str(&l, " ");
    line_dec(&l, bad4);
    line_end(&l);
    ctl(0, c);
    return 0;
}
