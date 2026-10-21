/*
 * user/sys.h -- the course's freestanding user-space header (CONTRACT §A.4, §A.8).
 *
 * Course-owned. No libc, no SSE, no TLS: every line here compiles under
 * -ffreestanding -nostdlib -mgeneral-regs-only. Everything is static inline,
 * so a program that includes it pulls in only what it calls.
 *
 * Contents: the raw x86-64 Linux syscall stubs, the SYS_* numbers the
 * pre-25 programs use (Linux's numbering, which is §A.4's), the errno
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
#define SYS_dup            32   /* 15 */
#define SYS_dup2           33   /* 15 */
#define SYS_fcntl          72   /* 15 */
#define SYS_getppid       110   /* 14 */
#define SYS_pipe2         293   /* 15 */
#define SYS_open            2   /* 17: O_RDONLY, O_DIRECTORY, O_CLOEXEC; 18 adds the write flags */
#define SYS_stat            4   /* 17 */
#define SYS_lseek           8   /* 17 */
#define SYS_getdents64    217   /* 17 */
#define SYS_rename         82   /* 18 */
#define SYS_mkdir          83   /* 18 */
#define SYS_rmdir          84   /* 18 */
#define SYS_unlink         87   /* 18 */
#define SYS_chdir          80   /* 21 */
#define SYS_getcwd         79   /* 21 */
/* Later programs add their numbers here, with §A.4's lab, never renumbered. */

/* ---- flags the lab 13-15 calls take (Linux's values) ---- */
#define WNOHANG        1          /* wait4 */
#define O_NONBLOCK     0x800      /* pipe2, fcntl F_SETFL */
#define O_CLOEXEC      0x80000    /* pipe2 */
#define F_GETFD        1          /* fcntl */
#define F_SETFD        2
#define F_GETFL        3
#define F_SETFL        4
#define FD_CLOEXEC     1
/* open's flags (Linux's values): 17 accepts the read-side ones, 18 adds the write side (§A.4) */
#define O_RDONLY       0
#define O_WRONLY       1
#define O_RDWR         2
#define O_CREAT        0x40       /* 18 */
#define O_EXCL         0x80       /* 18 */
#define O_TRUNC        0x200      /* 18 */
#define O_APPEND       0x400      /* 18 */
#define O_DIRECTORY    0x10000    /* 17 */
#define SEEK_SET       0
#define SEEK_CUR       1
#define SEEK_END       2
/* wait4's status is Linux's encoding (§A.6): an exit is code << 8, a kill is the signal in the low 7 bits. */
#define WIFEXITED(s)   (((s) & 0x7f) == 0)
#define WEXITSTATUS(s) (((s) >> 8) & 0xff)
#define WTERMSIG(s)    ((s) & 0x7f)

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
#define ENOEXEC  8
#define ESPIPE  29
#define EPIPE   32
#define EIO      5
#define E2BIG    7
#define EACCES  13
#define EBUSY   16
#define EEXIST  17
#define EXDEV   18
#define ENOTDIR 20
#define EISDIR  21
#define EMFILE  24
#define ENOSPC  28
#define EROFS   30
#define ENAMETOOLONG 36
#define ENOTEMPTY 39
#define ELOOP   40

/* The name of an errno the course's refusals use (§A.6), or 0 for any other: a program then
 * prints -N. The W3 programs' error lines are `<prog>: <path>: <name or -N>` (README.md). */
static inline const char *errno_name(long e)
{
    switch (e) {
    case EPERM: return "EPERM";     case ENOENT: return "ENOENT";   case EIO: return "EIO";
    case E2BIG: return "E2BIG";     case ENOEXEC: return "ENOEXEC"; case EBADF: return "EBADF";
    case ECHILD: return "ECHILD";   case EAGAIN: return "EAGAIN";   case ENOMEM: return "ENOMEM";
    case EACCES: return "EACCES";   case EFAULT: return "EFAULT";   case EBUSY: return "EBUSY";
    case EEXIST: return "EEXIST";   case EXDEV: return "EXDEV";     case ENOTDIR: return "ENOTDIR";
    case EISDIR: return "EISDIR";   case EINVAL: return "EINVAL";   case EMFILE: return "EMFILE";
    case ENOTTY: return "ENOTTY";   case ENOSPC: return "ENOSPC";   case ESPIPE: return "ESPIPE";
    case EROFS: return "EROFS";     case EPIPE: return "EPIPE";     case ENAMETOOLONG: return "ENAMETOOLONG";
    case ENOSYS: return "ENOSYS";   case ENOTEMPTY: return "ENOTEMPTY"; case ELOOP: return "ELOOP";
    default: return 0;
    }
}

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

/* ---- typed wrappers for the lab 11-15 calls ---- */
static inline i64 sys_sched_yield(void)                       { return syscall0(SYS_sched_yield); }
static inline i64 sys_getppid(void)                           { return syscall0(SYS_getppid); }
static inline i64 sys_fork(void)                              { return syscall0(SYS_fork); }
static inline i64 sys_execve(const char *path, char *const argv[], char *const envp[])
{
    return syscall3(SYS_execve, (i64)path, (i64)argv, (i64)envp);
}
/* status may be 0 (NULL); rusage is always NULL (no course kernel fills one). */
static inline i64 sys_wait4(i64 pid, int *status, int options)
{
    return syscall4(SYS_wait4, pid, (i64)status, options, 0);
}
static inline i64 sys_pipe2(int fds[2], int flags)            { return syscall2(SYS_pipe2, (i64)fds, flags); }
static inline i64 sys_dup(int fd)                             { return syscall1(SYS_dup, fd); }
static inline i64 sys_dup2(int oldfd, int newfd)              { return syscall2(SYS_dup2, oldfd, newfd); }
static inline i64 sys_fcntl(int fd, int cmd, i64 arg)         { return syscall3(SYS_fcntl, fd, cmd, arg); }

/* ---- the lab 17-23 calls (the W3 programs: ls, cat, stat, mkfile, rm, mv, mkdir, rmdir, mounts, sh) ---- */
/* struct stat, x86-64 Linux's layout (§A.4: 144 bytes). */
struct stat {
    u64 st_dev, st_ino, st_nlink;
    u32 st_mode, st_uid, st_gid, __pad0;
    u64 st_rdev;
    i64 st_size, st_blksize, st_blocks;
    u64 st_atime, st_atime_nsec, st_mtime, st_mtime_nsec, st_ctime, st_ctime_nsec;
    i64 __unused[3];
};
_Static_assert(sizeof(struct stat) == 144, "x86-64 struct stat is 144 bytes");
#define S_IFMT   0170000
#define S_IFDIR  0040000
#define S_ISDIR(m) (((m) & S_IFMT) == S_IFDIR)
static inline i64 sys_open(const char *path, int flags, int mode) { return syscall3(SYS_open, (i64)path, flags, mode); }
static inline i64 sys_stat(const char *path, struct stat *st)    { return syscall2(SYS_stat, (i64)path, (i64)st); }
static inline i64 sys_lseek(int fd, i64 off, int whence)          { return syscall3(SYS_lseek, fd, off, whence); }
/* linux_dirent64 records: u64 d_ino, i64 d_off, u16 d_reclen at 16, u8 d_type at 18, name at 19. */
static inline i64 sys_getdents64(int fd, void *buf, usize n)      { return syscall3(SYS_getdents64, fd, (i64)buf, (i64)n); }
static inline i64 sys_unlink(const char *path)                    { return syscall1(SYS_unlink, (i64)path); }
static inline i64 sys_mkdir(const char *path, int mode)           { return syscall2(SYS_mkdir, (i64)path, mode); }
static inline i64 sys_rmdir(const char *path)                     { return syscall1(SYS_rmdir, (i64)path); }
static inline i64 sys_rename(const char *from, const char *to)    { return syscall2(SYS_rename, (i64)from, (i64)to); }
static inline i64 sys_chdir(const char *path)                     { return syscall1(SYS_chdir, (i64)path); }
static inline i64 sys_getcwd(char *buf, usize n)                  { return syscall2(SYS_getcwd, (i64)buf, (i64)n); }

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

/* write_buf to any fd (a program whose fd 1 is a pipe puts its evidence on fd 2). */
static inline void write_buf_fd(int fd, const char *s, usize n)
{
    while (n) {
        i64 r = sys_write(fd, s, n);
        if (r <= 0) return;
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

/* ---- the protocol every program shares (CONTRACT §A.3; README.md "The protocol") ---- */
/* Parses a decimal of 1-9 digits (an index, a count, an fd number). 1 on success, else 0. */
static inline int dec_parse(const char *s, u64 *out)
{
    u64 v = 0;
    int n = 0;
    for (; s[n]; n++) {
        if (n >= 9 || s[n] < '0' || s[n] > '9') return 0;
        v = v * 10 + (u64)(s[n] - '0');
    }
    if (n == 0) return 0;
    *out = v;
    return 1;
}
/* One KTAP result line, in one write(1): `ok <index> - <id>` or `not ok <index> - <id>`. */
static inline void line_row(struct line *l, int ok, u64 index, const char *id)
{
    if (!ok) line_str(l, "not ");
    line_str(l, "ok ");
    line_dec(l, index);
    line_str(l, " - ");
    line_str(l, id);
    line_end(l);
}
/* A `# ` evidence line in one write(1): prefix, then the line is the caller's to finish. */
static inline void line_note(struct line *l, const char *what)
{
    line_str(l, "# ");
    line_str(l, what);
}
/* A pointer as 0x + 16 hex digits. */
static inline void line_ptr(struct line *l, u64 v)
{
    line_str(l, "0x");
    line_hex64(l, v);
}

/* ---- the W3 programs' shared output (labs 17-23) ----
 * err_line: `<prog>: <what>: <errno name or -N>` on fd 2, in one write. */
static inline void err_line(const char *prog, const char *what, i64 r)
{
    char b[256];
    usize n = 0;
    const char *parts[3] = { prog, ": ", what };
    for (int i = 0; i < 3; i++)
        for (const char *s = parts[i]; *s && n < 200; s++) b[n++] = *s;
    b[n++] = ':'; b[n++] = ' ';
    const char *name = errno_name(-r);
    if (name) { while (*name) b[n++] = *name++; }
    else {
        char t[20];
        int i = 20;
        u64 v = (u64)0 - (u64)r;
        do { t[--i] = (char)('0' + v % 10); v /= 10; } while (v);
        b[n++] = '-';
        while (i < 20) b[n++] = t[i++];
    }
    b[n++] = '\n';
    write_buf_fd(STDERR_FILENO, b, n);
}
/* A message line on fd 2: `<prog>: <msg>`, one write. */
static inline void err_msg(const char *prog, const char *msg)
{
    char b[256];
    usize n = 0;
    for (const char *s = prog; *s && n < 120; s++) b[n++] = *s;
    b[n++] = ':'; b[n++] = ' ';
    for (const char *s = msg; *s && n < 250; s++) b[n++] = *s;
    b[n++] = '\n';
    write_buf_fd(STDERR_FILENO, b, n);
}
/* A buffered writer: whole lines leave in as few write calls as fit 4096 bytes. ob_flush
 * returns the first refusal (negative), else 0; a refused write ends the output. */
struct obuf { int fd; i64 err; usize n; char b[4096]; };
static inline void ob_flush(struct obuf *o)
{
    usize at = 0;
    while (at < o->n && o->err == 0) {
        i64 r = sys_write(o->fd, o->b + at, o->n - at);
        if (r <= 0) { o->err = r < 0 ? r : -EIO; break; }
        at += (usize)r;
    }
    o->n = 0;
}
static inline void ob_putc(struct obuf *o, char c)
{
    if (o->n == sizeof o->b) ob_flush(o);
    o->b[o->n++] = c;
}
static inline void ob_str(struct obuf *o, const char *s) { while (*s) ob_putc(o, *s++); }
static inline void ob_dec(struct obuf *o, u64 v)
{
    char t[20];
    int i = 20;
    do { t[--i] = (char)('0' + v % 10); v /= 10; } while (v);
    while (i < 20) ob_putc(o, t[i++]);
}
static inline void ob_sdec(struct obuf *o, i64 v)
{
    if (v < 0) { ob_putc(o, '-'); ob_dec(o, (u64)0 - (u64)v); } else ob_dec(o, (u64)v);
}

#endif /* SBOOT_USER_SYS_H */
