/*
 * user/mkfile/mkfile.c -- lab 18's `mkfile`: create (or truncate) a file and write a line.
 *
 *     mkfile <path> [word...]
 *
 * open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644), then the words joined by one space and a
 * '\n' -- in ONE write call when the line fits 4096 bytes (a longer one takes one call per
 * 4096). No words: the file is left empty (0 bytes). An error is
 * `mkfile: <path>: <errno name or -N>` on fd 2 and exit 1; no path is a usage error, exit 2.
 * Syscalls: open, write, close, exit (CONTRACT §A.4, lab 18: open's write flags).
 */
#include "../sys.h"

static struct obuf out;          /* .bss; the file's fd is set once it is open */

int main(int argc, char **argv, char **envp)
{
    (void)envp;
    if (argc < 2) { err_msg("mkfile", "usage: mkfile <path> [word...]"); return 2; }
    i64 fd = sys_open(argv[1], O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
    if (fd < 0) { err_line("mkfile", argv[1], fd); return 1; }
    out.fd = (int)fd;
    for (int i = 2; i < argc; i++) {
        if (i > 2) ob_putc(&out, ' ');
        ob_str(&out, argv[i]);
    }
    if (argc > 2) ob_putc(&out, '\n');
    ob_flush(&out);
    if (out.err) { err_line("mkfile", argv[1], out.err); return 1; }
    i64 r = sys_close((int)fd);
    if (r < 0) { err_line("mkfile", argv[1], r); return 1; }
    return 0;
}
