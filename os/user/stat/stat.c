/*
 * user/stat/stat.c -- lab 17's `stat`: the fields of x86-64 Linux's 144-byte struct stat
 * that the course's rows read, one fixed line each.
 *
 *     stat <path>...
 *
 * For each path, from one stat(2) call:
 *     file=<path>
 *     st_dev=<decimal>
 *     st_ino=<decimal>
 *     st_mode=0<octal>          (type and permission bits, e.g. 0100644, 040755)
 *     st_nlink=<decimal>
 *     st_size=<decimal>
 *     st_blocks=<decimal>       (512-byte units, as Linux counts them)
 * An error is `stat: <path>: <errno name or -N>` on fd 2; stat goes on and exits 1 if any
 * failed; no path is a usage error, exit 2.
 * Syscalls: stat, write, exit (CONTRACT §A.4, lab 17).
 */
#include "../sys.h"

static struct obuf out;          /* .bss; main sets out.fd = 1 */

static void ob_oct(struct obuf *o, u64 v)
{
    char t[24];
    int i = 24;
    do { t[--i] = (char)('0' + (v & 7)); v >>= 3; } while (v);
    ob_putc(o, '0');
    while (i < 24) ob_putc(o, t[i++]);
}

static void field(const char *name, u64 v)
{
    ob_str(&out, name);
    ob_putc(&out, '=');
    ob_dec(&out, v);
    ob_putc(&out, '\n');
}

int main(int argc, char **argv, char **envp)
{
    (void)envp;
    out.fd = STDOUT_FILENO;
    if (argc < 2) { err_msg("stat", "usage: stat <path>..."); return 2; }
    int status = 0;
    for (int i = 1; i < argc; i++) {
        struct stat st;
        i64 r = sys_stat(argv[i], &st);
        if (r < 0) { ob_flush(&out); err_line("stat", argv[i], r); status = 1; continue; }
        ob_str(&out, "file=");
        ob_str(&out, argv[i]);
        ob_putc(&out, '\n');
        field("st_dev", st.st_dev);
        field("st_ino", st.st_ino);
        ob_str(&out, "st_mode=");
        ob_oct(&out, st.st_mode);
        ob_putc(&out, '\n');
        field("st_nlink", st.st_nlink);
        field("st_size", (u64)st.st_size);
        field("st_blocks", (u64)st.st_blocks);
    }
    ob_flush(&out);
    if (out.err) { err_line("stat", "stdout", out.err); return 1; }
    return status;
}
