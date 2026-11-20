/*
 * user/mark/mark.c -- lab 29's group marker: the line that says one group of typed commands
 * has finished, and where the shell that ran it stands. And two things lab 29's vehicle
 * reads about the kernel and the shell that ran it.
 *
 *     mark <N>                   prints `mark <N> <cwd>` and then `# ppid=<P>` on fd 1, in one
 *                                write; exit 0
 *     mark -x <name>             the bare-name probe: execve(<name>, {"mark", "-x"}, {}) with
 *                                <name> exactly as given, no `/bin/` put in front
 *     mark -x                    what that execve starts if it runs: `# mark -x: started by
 *                                execve of a name with no /` on fd 1; exit 3
 *
 * <N> is a decimal number, printed back as typed digits' value; <cwd> is what getcwd
 * answers; <P> is what getppid answers, the pid of the process that started this one. Lab
 * 23's script ends each group with `echo $? "q"N`; lab 29's shell has no quoting, so its
 * script ends each group with this program instead, and the cwd it prints is how the
 * script sees `cd` work (the grammar has no `pwd`). The `# ` line is a note: lab 29's
 * session leaves notes out of every comparison, and reads `mark 2`'s to see that the shell
 * itself started the command, not a process the shell started.
 * A missing or malformed <N> is `mark: usage: mark <N>` on fd 2, exit 2; a refused getcwd is
 * `mark: getcwd: <errno>`, exit 1.
 *
 * The probe: lab 29's vehicle starts `/bin/mark -x mark` itself, at cwd `/`. A kernel
 * resolves a path with no `/` against the cwd (lab 23), where there is no `mark`,
 * so the execve returns: the probe prints `# execve(<name>) = -<errno> (<NAME>)` on fd 1 and
 * exits 0. A kernel that looks a bare name up under /bin itself runs /bin/mark as `mark -x`,
 * which exits 3, and the vehicle reads that status.
 * Syscalls: getcwd, getppid, execve, write, exit (labs 09, 13, 14 and 23).
 */
#include "../sys.h"

static struct obuf out;          /* .bss; main sets out.fd = 1 */

int main(int argc, char **argv, char **envp)
{
    (void)envp;
    out.fd = STDOUT_FILENO;
    if (argc >= 2 && streq(argv[1], "-x")) {
        if (argc == 2) {
            ob_str(&out, "# mark -x: started by execve of a name with no /\n");
            ob_flush(&out);
            return 3;
        }
        if (argc != 3) {
            err_msg("mark", "usage: mark -x <name>");
            return 2;
        }
        char *const args[] = { "mark", "-x", 0 };
        char *const env[] = { 0 };
        i64 r = sys_execve(argv[2], args, env);
        const char *name = errno_name(-r);
        ob_str(&out, "# execve(");
        ob_str(&out, argv[2]);
        ob_str(&out, ") = ");
        ob_sdec(&out, r);
        if (name) {
            ob_str(&out, " (");
            ob_str(&out, name);
            ob_putc(&out, ')');
        }
        ob_putc(&out, '\n');
        ob_flush(&out);
        return 0;
    }
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
    i64 ppid = sys_getppid();
    ob_str(&out, "mark ");
    ob_dec(&out, n);
    ob_putc(&out, ' ');
    ob_str(&out, cwd);
    ob_str(&out, "\n# ppid=");
    ob_sdec(&out, ppid);
    ob_putc(&out, '\n');
    ob_flush(&out);
    return out.err ? 1 : 0;
}
