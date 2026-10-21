/*
 * user/cat/cat.c -- lab 17's `cat`: each file's bytes to fd 1, in order.
 *
 *     cat [path...]              (no path, or "-": fd 0)
 *
 * Reads in 4096-byte calls (a multi-block file takes one read per block) and writes every
 * byte it read, retrying a short write. An error is `cat: <path>: <errno name or -N>` on fd 2
 * (`<stdin>` for fd 0, `stdout` for a refused write, which ends the run); cat goes on to the
 * next path and exits 1 if any failed. A directory is EISDIR from read.
 * Syscalls: open, read, write, close, exit (CONTRACT §A.4, lab 17).
 */
#include "../sys.h"

static u8 buf[4096];

/* Copies fd to fd 1. Returns 0, a read refusal (negative), or 1 for a write refusal (printed). */
static i64 copy(int fd)
{
    for (;;) {
        i64 n = sys_read(fd, buf, sizeof buf);
        if (n <= 0) return n;
        for (i64 at = 0; at < n;) {
            i64 w = sys_write(STDOUT_FILENO, buf + at, (usize)(n - at));
            if (w <= 0) { err_line("cat", "stdout", w < 0 ? w : -EIO); return 1; }
            at += w;
        }
    }
}

int main(int argc, char **argv, char **envp)
{
    (void)envp;
    char *dash[] = { "-", 0 };
    char **paths = argc > 1 ? argv + 1 : dash;
    int n = argc > 1 ? argc - 1 : 1, status = 0;
    for (int i = 0; i < n; i++) {
        int is_stdin = streq(paths[i], "-");
        const char *name = is_stdin ? "<stdin>" : paths[i];
        i64 fd = is_stdin ? 0 : sys_open(paths[i], O_RDONLY | O_CLOEXEC, 0);
        if (fd < 0) { err_line("cat", name, fd); status = 1; continue; }
        i64 r = copy((int)fd);
        if (!is_stdin) sys_close((int)fd);
        if (r == 1) return 1;
        if (r < 0) { err_line("cat", name, r); status = 1; }
    }
    return status;
}
