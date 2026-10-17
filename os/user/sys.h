/*
 * user/sys.h -- the course's freestanding user-space header (CONTRACT §A.4, §A.8).
 *
 * Course-owned. No libc, no SSE, no TLS: every line here compiles under
 * -ffreestanding -nostdlib -mgeneral-regs-only. Everything is static inline,
 * so a program that includes it pulls in only what it calls.
 *
 * Contents: the raw x86-64 Linux syscall stubs, the SYS_* numbers the
 * pre-23 programs use (Linux's numbering, which is §A.4's), the errno
 * helpers, derive() (§A.2), and write(1) helpers that print without printf.
 */
#ifndef SBOOT_USER_SYS_H
#define SBOOT_USER_SYS_H

typedef unsigned char      u8;
typedef unsigned short     u16;
typedef unsigned int       u32;
typedef unsigned long long u64;
typedef long long          i64;
typedef unsigned long      usize;

/* ---- syscall numbers: x86-64 Linux, = CONTRACT §A.4 (lab that adds the arm) ---- */
#define SYS_read            0   /* 09: console returns 0 (EOF) until 21 */
#define SYS_write           1   /* 09 */
#define SYS_close           3   /* 09 */
#define SYS_ioctl          16   /* 09: TIOCGWINSZ on the console */
#define SYS_sched_yield    24   /* 11 */
#define SYS_getpid         39   /* 09: 1 until 14 */
#define SYS_fork           57   /* 14 */
#define SYS_execve         59   /* 13 */
#define SYS_exit           60   /* 09 */
#define SYS_wait4          61   /* 14 */
#define SYS_exit_group    231   /* 13 */
/* Later programs add their numbers here, with §A.4's lab, never renumbered. */

#define TIOCGWINSZ     0x5413
#define STDIN_FILENO   0
#define STDOUT_FILENO  1
#define STDERR_FILENO  2

/* ---- errno: Linux's numbers; a syscall returns -errno on failure (§A.6) ---- */
#define EPERM    1
#define ENOENT   2
#define EBADF    9
#define ECHILD  10
#define EAGAIN  11
#define ENOMEM  12
#define EFAULT  14
#define EINVAL  22
#define ENOTTY  25
#define ENOSYS  38

static inline int  sys_is_err(i64 r)  { return r < 0 && r >= -4095; }
static inline long sys_errno(i64 r)   { return sys_is_err(r) ? (long)-r : 0; }

/* ---- raw syscall stubs ----
 * Number in rax; arguments in rdi rsi rdx r10 r8 r9; result in rax;
 * the `syscall` instruction clobbers rcx (return rip) and r11 (rflags).
 * SBOOT_HOST_TEST is the one test seam: a host-side harness (never shipped,
 * never in bin/) defines it and supplies syscall0..6 and read_cs itself, so
 * derive() and the formatting can be run on the build machine. */
#ifndef SBOOT_HOST_TEST
static inline i64 syscall0(i64 n)
{
    i64 r;
    __asm__ volatile ("syscall" : "=a"(r) : "a"(n) : "rcx", "r11", "memory");
    return r;
}
static inline i64 syscall1(i64 n, i64 a1)
{
    i64 r;
    __asm__ volatile ("syscall" : "=a"(r) : "a"(n), "D"(a1) : "rcx", "r11", "memory");
    return r;
}
static inline i64 syscall2(i64 n, i64 a1, i64 a2)
{
    i64 r;
    __asm__ volatile ("syscall" : "=a"(r) : "a"(n), "D"(a1), "S"(a2) : "rcx", "r11", "memory");
    return r;
}
static inline i64 syscall3(i64 n, i64 a1, i64 a2, i64 a3)
{
    i64 r;
    __asm__ volatile ("syscall" : "=a"(r) : "a"(n), "D"(a1), "S"(a2), "d"(a3)
                      : "rcx", "r11", "memory");
    return r;
}
static inline i64 syscall4(i64 n, i64 a1, i64 a2, i64 a3, i64 a4)
{
    i64 r;
    register i64 r10 __asm__("r10") = a4;
    __asm__ volatile ("syscall" : "=a"(r) : "a"(n), "D"(a1), "S"(a2), "d"(a3), "r"(r10)
                      : "rcx", "r11", "memory");
    return r;
}
static inline i64 syscall5(i64 n, i64 a1, i64 a2, i64 a3, i64 a4, i64 a5)
{
    i64 r;
    register i64 r10 __asm__("r10") = a4;
    register i64 r8  __asm__("r8")  = a5;
    __asm__ volatile ("syscall" : "=a"(r) : "a"(n), "D"(a1), "S"(a2), "d"(a3), "r"(r10), "r"(r8)
                      : "rcx", "r11", "memory");
    return r;
}
static inline i64 syscall6(i64 n, i64 a1, i64 a2, i64 a3, i64 a4, i64 a5, i64 a6)
{
    i64 r;
    register i64 r10 __asm__("r10") = a4;
    register i64 r8  __asm__("r8")  = a5;
    register i64 r9  __asm__("r9")  = a6;
    __asm__ volatile ("syscall" : "=a"(r)
                      : "a"(n), "D"(a1), "S"(a2), "d"(a3), "r"(r10), "r"(r8), "r"(r9)
                      : "rcx", "r11", "memory");
    return r;
}

/* The code segment selector this program runs under; the low two bits are the RPL. */
static inline u64 read_cs(void)
{
    u64 cs;
    __asm__ volatile ("mov %%cs, %0" : "=r"(cs));
    return cs & 0xffff;   /* a selector is 16 bits, whatever operand size the assembler picks */
}
#else
i64 syscall0(i64 n);
i64 syscall1(i64 n, i64 a1);
i64 syscall2(i64 n, i64 a1, i64 a2);
i64 syscall3(i64 n, i64 a1, i64 a2, i64 a3);
i64 syscall4(i64 n, i64 a1, i64 a2, i64 a3, i64 a4);
i64 syscall5(i64 n, i64 a1, i64 a2, i64 a3, i64 a4, i64 a5);
i64 syscall6(i64 n, i64 a1, i64 a2, i64 a3, i64 a4, i64 a5, i64 a6);
u64 read_cs(void);
#endif

/* ---- typed wrappers for the lab-09 calls ---- */
static inline i64 sys_read(int fd, void *buf, usize n)        { return syscall3(SYS_read, fd, (i64)buf, (i64)n); }
static inline i64 sys_write(int fd, const void *buf, usize n) { return syscall3(SYS_write, fd, (i64)buf, (i64)n); }
static inline i64 sys_close(int fd)                           { return syscall1(SYS_close, fd); }
static inline i64 sys_ioctl(int fd, u64 req, void *arg)       { return syscall3(SYS_ioctl, fd, (i64)req, (i64)arg); }
static inline i64 sys_getpid(void)                            { return syscall0(SYS_getpid); }
static inline __attribute__((noreturn)) void sys_exit(int code)
{
    for (;;) syscall1(SYS_exit, code);   /* exit never returns; the loop says so to the compiler */
}

struct winsize { u16 ws_row, ws_col, ws_xpixel, ws_ypixel; };

/* ---- strings ---- */
static inline usize strlen(const char *s)
{
    usize n = 0;
    while (s[n]) n++;
    return n;
}
static inline int streq(const char *a, const char *b)
{
    while (*a && *a == *b) { a++; b++; }
    return *a == *b;
}

/* ---- output over write(1), without printf ----
 * A program builds each line in a `struct line` and sends it with ONE write(1)
 * call (line_end), so a line reaches the console whole even when another
 * process writes between two of this one's calls (labs 12 onward run two at
 * once). The write_* helpers below are the unbuffered convenience: one call each.
 * Every number is formatted by shifts or by division by the CONSTANT 10, which
 * the compiler turns into a multiply; nothing calls a libgcc helper. */
static inline void write_buf(const char *s, usize n)
{
    while (n) {
        i64 r = sys_write(STDOUT_FILENO, s, n);
        if (r <= 0) return;             /* a refusal ends the output; never a loop */
        s += r; n -= (usize)r;
    }
}

#define LINE_MAX 128
struct line { usize n; char b[LINE_MAX]; };

/* Sends what the line holds and empties it. Out of line on purpose: one copy of
 * the write loop per program instead of one per character appended. */
static __attribute__((noinline, unused)) void line_flush(struct line *l)
{
    write_buf(l->b, l->n);
    l->n = 0;
}
static inline void line_putc(struct line *l, char c)
{
    if (l->n == LINE_MAX) line_flush(l);   /* only an overlong line splits */
    l->b[l->n++] = c;
}
static inline void line_str(struct line *l, const char *s) { while (*s) line_putc(l, *s++); }
/* 16 lowercase hex digits, zero-padded, no prefix. */
static inline void line_hex64(struct line *l, u64 v)
{
    for (int i = 60; i >= 0; i -= 4) line_putc(l, "0123456789abcdef"[(v >> i) & 0xf]);
}
/* printf's %x: lowercase, no padding, no prefix. */
static inline void line_hex(struct line *l, u64 v)
{
    int i = 60;
    while (i > 0 && !((v >> i) & 0xf)) i -= 4;
    for (; i >= 0; i -= 4) line_putc(l, "0123456789abcdef"[(v >> i) & 0xf]);
}
static inline void line_dec(struct line *l, u64 v)
{
    char b[20];
    int i = 20;
    do { b[--i] = (char)('0' + v % 10); v /= 10; } while (v);
    while (i < 20) line_putc(l, b[i++]);
}
static inline void line_sdec(struct line *l, i64 v)
{
    if (v < 0) { line_putc(l, '-'); line_dec(l, (u64)0 - (u64)v); }
    else line_dec(l, (u64)v);
}
/* Appends the newline and sends the line in one write(1) call. */
static inline void line_end(struct line *l)
{
    line_putc(l, '\n');
    line_flush(l);
}

static inline void write_str(const char *s) { write_buf(s, strlen(s)); }
static inline void write_hex64(u64 v) { struct line l = { 0, {0} }; line_hex64(&l, v); write_buf(l.b, l.n); }
static inline void write_hex(u64 v)   { struct line l = { 0, {0} }; line_hex(&l, v);   write_buf(l.b, l.n); }
static inline void write_dec(u64 v)   { struct line l = { 0, {0} }; line_dec(&l, v);   write_buf(l.b, l.n); }
static inline void write_sdec(i64 v)  { struct line l = { 0, {0} }; line_sdec(&l, v);  write_buf(l.b, l.n); }

/* ---- derive (CONTRACT §A.2) ----
 * derive(nonce, id) = splitmix64(lo ^ hi ^ fnv1a64(id)), where the nonce is 32
 * hex characters = 16 bytes, lo/hi are bytes 0..8 and 8..16 read little-endian,
 * and fnv1a64 is FNV-1a 64 over the id's bytes. The cockpit's ktap::derive is
 * the same function in Rust; README.md carries the shared test vectors. */
static inline int hexval(char c)
{
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}
/* 1 if s is exactly 32 hex characters (the nonce's only shape), else 0. */
static inline int nonce_valid(const char *s)
{
    int i = 0;
    for (; s[i]; i++)
        if (i >= 32 || hexval(s[i]) < 0) return 0;
    return i == 32;
}
static inline u64 fnv1a64(const char *s)
{
    u64 h = 0xcbf29ce484222325ULL;
    for (; *s; s++) { h ^= (u8)*s; h *= 0x100000001b3ULL; }
    return h;
}
static inline u64 splitmix64(u64 x)
{
    u64 z = x + 0x9E3779B97F4A7C15ULL;
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
    return z ^ (z >> 31);
}
/* nonce: 32 hex characters as text. Call nonce_valid() first: a bad digit, or
 * a string that ends early, counts as 0 here rather than reading past its end. */
static inline u64 derive(const char *nonce, const char *id)
{
    u64 lo = 0, hi = 0;
    int k = 0;                          /* index into the text; stops at its NUL */
    for (int i = 0; i < 16; i++) {
        int h = nonce[k] ? hexval(nonce[k++]) : 0;
        int l = nonce[k] ? hexval(nonce[k++]) : 0;
        u64 byte = (u64)(((h < 0 ? 0 : h) << 4) | (l < 0 ? 0 : l));
        if (i < 8) lo |= byte << (8 * i);
        else       hi |= byte << (8 * (i - 8));
    }
    return splitmix64(lo ^ hi ^ fnv1a64(id));
}

#endif /* SBOOT_USER_SYS_H */
