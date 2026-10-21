/*
 * user/mounts/mounts.c -- lab 22's `mounts`: every mount point the mount table lists, with
 * the st_dev a stat of it returns.
 *
 *     mounts [file]              (default /proc/mounts)
 *
 * Reads the table in 512-byte calls; each line is Linux's /proc/mounts shape, `<source>
 * <mount point> <fstype> ...` separated by spaces, the mount point with `\NNN` octal escapes
 * (decoded before the stat). For each line it prints
 *     <mount point> <fstype> st_dev=<decimal>
 * so one run shows both lab 22 facts the table and stat must agree on: every mounted path is
 * listed, and each mount answers its own st_dev. A line with fewer than three fields, or
 * longer than 1023 bytes, is `mounts: <file>: EINVAL`; a mount point stat refuses is
 * `mounts: <mount point>: <errno name or -N>`; both on fd 2, and the exit status is then 1.
 * A table that cannot be opened or read: the same line for <file>, exit 1.
 * Syscalls: open, read, stat, close, write, exit (CONTRACT §A.4, all lab 17's; it is lab 22's
 * program because /proc/mounts is).
 */
#include "../sys.h"

static struct obuf out;          /* .bss; main sets out.fd = 1 */
static char line[1024];
static const char *table;        /* argv[1], or "/proc/mounts" */
static int status;

/* Splits off the next space-separated field (in place); 0 when there is none. */
static char *field(char **at)
{
    char *s = *at;
    while (*s == ' ' || *s == '\t') s++;
    if (!*s) return 0;
    char *f = s;
    while (*s && *s != ' ' && *s != '\t') s++;
    if (*s) *s++ = 0;
    *at = s;
    return f;
}

static void one_line(char *l)
{
    char *at = l;
    field(&at);                                   /* source */
    char *mp = field(&at), *fs = field(&at);
    if (!fs) { if (mp) { ob_flush(&out); err_line("mounts", table, -EINVAL); status = 1; } return; }
    char *w = mp;                                 /* decode \NNN in place */
    for (char *r = mp; *r;) {
        if (r[0] == '\\' && r[1] >= '0' && r[1] <= '3' && r[2] >= '0' && r[2] <= '7' && r[3] >= '0' && r[3] <= '7') {
            *w++ = (char)((r[1] - '0') << 6 | (r[2] - '0') << 3 | (r[3] - '0'));
            r += 4;
        } else *w++ = *r++;
    }
    *w = 0;
    struct stat st;
    i64 r = sys_stat(mp, &st);
    if (r < 0) { ob_flush(&out); err_line("mounts", mp, r); status = 1; return; }
    ob_str(&out, mp);
    ob_putc(&out, ' ');
    ob_str(&out, fs);
    ob_str(&out, " st_dev=");
    ob_dec(&out, st.st_dev);
    ob_putc(&out, '\n');
}

int main(int argc, char **argv, char **envp)
{
    (void)envp;
    out.fd = STDOUT_FILENO;
    if (argc > 2) { err_msg("mounts", "usage: mounts [file]"); return 2; }
    table = argc == 2 ? argv[1] : "/proc/mounts";
    i64 fd = sys_open(table, O_RDONLY | O_CLOEXEC, 0);
    if (fd < 0) { err_line("mounts", table, fd); return 1; }
    char buf[512];
    usize n = 0;
    int overlong = 0;
    for (;;) {
        i64 got = sys_read((int)fd, buf, sizeof buf);
        if (got < 0) { ob_flush(&out); err_line("mounts", table, got); status = 1; break; }
        for (i64 i = 0; i < got; i++) {
            if (buf[i] != '\n') {
                if (n < sizeof line - 1) line[n++] = buf[i]; else overlong = 1;
                continue;
            }
            line[n] = 0;
            if (overlong) { ob_flush(&out); err_line("mounts", table, -EINVAL); status = 1; }
            else one_line(line);
            n = 0; overlong = 0;
        }
        if (got == 0) {
            line[n] = 0;                              /* a last line without '\n' */
            if (overlong) { ob_flush(&out); err_line("mounts", table, -EINVAL); status = 1; }
            else if (n) one_line(line);
            break;
        }
    }
    sys_close((int)fd);
    ob_flush(&out);
    if (out.err) { err_line("mounts", "stdout", out.err); return 1; }
    return status;
}
