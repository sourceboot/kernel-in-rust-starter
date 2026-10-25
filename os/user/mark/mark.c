/*
 * user/mark/mark.c -- lab 29's group marker: the line that says one group of typed commands
 * has finished, and where the shell that ran it stands.
 *
 *     mark <N>                   prints `mark <N> <cwd>` on fd 1, in one write; exit 0
 *
 * <N> is a decimal number, printed back as typed digits' value; <cwd> is what getcwd
 * answers. Lab 23's script ends each group with `echo $? "q"N`; lab 29's shell has no
 * quoting, so its script ends each group with this program instead, and the cwd it
 * prints is how the script sees `cd` work (the grammar has no `pwd`). A missing or
 * malformed <N> is `mark: usage: mark <N>` on fd 2, exit 2; a refused getcwd is
 * `mark: getcwd: <errno>`, exit 1.
 * Syscalls: getcwd, write, exit (CONTRACT §A.4, labs 09 and 23).
 */
#include "../sys.h"

static struct obuf out;          /* .bss; main sets out.fd = 1 */

int main(int argc, char **argv, char **envp)
{
    (void)envp;
    u64 n;
    if (argc != 2 || !dec_parse(argv[1], &n)) {
        err_msg("mark", "usage: mark <N>");
        return 2;
    }
    char cwd[1024];
    i64 r = sys_getcwd(cwd, sizeof cwd);
    if (r < 0) {
        err_line("mark", "getcwd", r);
        return 1;
    }
    out.fd = STDOUT_FILENO;
    ob_str(&out, "mark ");
    ob_dec(&out, n);
    ob_putc(&out, ' ');
    ob_str(&out, cwd);
    ob_putc(&out, '\n');
    ob_flush(&out);
    return out.err ? 1 : 0;
}
