/*
 * user/ls/ls.c -- lab 17's `ls`: a directory's names, one per line, sorted bytewise.
 *
 *     ls [-a] [path...]          (no path: ".")
 *
 * A directory is read with open(O_RDONLY | O_DIRECTORY) and getdents64 into a 512-byte
 * buffer, so any directory of more than a dozen entries takes several calls -- the kernel's
 * resume offset is what keeps them from repeating or losing a name. Names starting with '.'
 * (".", "..", dot-files) are left out unless -a is given. A path that is not a directory
 * (open says ENOTDIR and stat finds it) is printed as given. With more than one path, each
 * directory's names follow a `<path>:` line and a blank line separates the outputs, in the
 * order the paths were given. An error is `ls: <path>: <errno name or -N>` on fd 2 and makes
 * the exit status 1; a usage error is 2. A record whose d_reclen is under 20 or runs past
 * what getdents64 returned is reported as EIO (the kernel built it wrong); more than
 * MAX_NAMES names or NAME_BYTES bytes of names in one directory is ENOMEM.
 * Syscalls: open, getdents64, stat, close, write, exit (CONTRACT §A.4, lab 17).
 */
#include "../sys.h"

#define DENTS_BUF  512
#define MAX_NAMES  1024
#define NAME_BYTES 16384

static char names[NAME_BYTES];
static char *sorted[MAX_NAMES];
static struct obuf out;          /* .bss; main sets out.fd = 1 */

static int bytecmp(const char *a, const char *b)
{
    while (*a && *a == *b) { a++; b++; }
    return (int)(u8)*a - (int)(u8)*b;
}

/* Lists one directory; returns 0, or the refusal (negative) after printing it. */
static i64 list_dir(const char *path, int fd, int all)
{
    u8 buf[DENTS_BUF] __attribute__((aligned(8)));
    usize used = 0;
    int n = 0;
    for (;;) {
        i64 got = sys_getdents64(fd, buf, sizeof buf);
        if (got == 0) break;
        if (got < 0) return got;
        for (i64 at = 0; at < got;) {
            u16 reclen = (u16)(buf[at + 16] | buf[at + 17] << 8);
            if (reclen < 20 || at + reclen > got) return -EIO;
            const char *name = (const char *)buf + at + 19;
            usize len = 0;
            while (19 + len < reclen && name[len]) len++;
            if (19 + len == reclen) return -EIO;            /* no NUL inside the record */
            at += reclen;
            if (name[0] == '.' && !all) continue;
            if (n == MAX_NAMES || used + len + 1 > NAME_BYTES) return -ENOMEM;
            sorted[n++] = names + used;
            for (usize i = 0; i <= len; i++) names[used++] = name[i];
        }
    }
    /* insertion sort, bytewise (C locale) */
    for (int i = 1; i < n; i++) {
        char *k = sorted[i];
        int j = i - 1;
        while (j >= 0 && bytecmp(sorted[j], k) > 0) { sorted[j + 1] = sorted[j]; j--; }
        sorted[j + 1] = k;
    }
    for (int i = 0; i < n; i++) { ob_str(&out, sorted[i]); ob_putc(&out, '\n'); }
    (void)path;
    return 0;
}

int main(int argc, char **argv, char **envp)
{
    (void)envp;
    out.fd = STDOUT_FILENO;
    int all = 0, first = 1;
    while (first < argc && argv[first][0] == '-' && argv[first][1]) {
        if (streq(argv[first], "-a")) { all = 1; first++; continue; }
        if (streq(argv[first], "--")) { first++; break; }
        err_msg("ls", "usage: ls [-a] [path...]");
        return 2;
    }
    char *dot[] = { ".", 0 };
    char **paths = first < argc ? argv + first : dot;
    int npaths = first < argc ? argc - first : 1;
    int status = 0;
    for (int i = 0; i < npaths; i++) {
        const char *p = paths[i];
        if (i > 0) ob_putc(&out, '\n');
        i64 fd = sys_open(p, O_RDONLY | O_DIRECTORY | O_CLOEXEC, 0);
        if (fd == -ENOTDIR) {
            struct stat st;
            i64 r = sys_stat(p, &st);
            if (r == 0 && !S_ISDIR(st.st_mode)) { ob_str(&out, p); ob_putc(&out, '\n'); continue; }
            fd = r < 0 ? r : -ENOTDIR;
        }
        if (fd < 0) { ob_flush(&out); err_line("ls", p, fd); status = 1; continue; }
        if (npaths > 1) { ob_str(&out, p); ob_str(&out, ":\n"); }
        i64 r = list_dir(p, (int)fd, all);
        sys_close((int)fd);
        if (r < 0) { ob_flush(&out); err_line("ls", p, r); status = 1; }
    }
    ob_flush(&out);
    if (out.err) { err_line("ls", "stdout", out.err); return 1; }
    return status;
}
