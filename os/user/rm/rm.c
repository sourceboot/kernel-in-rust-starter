/*
 * user/rm/rm.c -- lab 18's `rm`: unlink each path.
 *
 *     rm <path>...
 *
 * One unlink(2) per path; a directory is the kernel's EISDIR (use rmdir). An error is
 * `rm: <path>: <errno name or -N>` on fd 2; rm goes on and exits 1 if any failed; no path
 * is a usage error, exit 2. Syscalls: unlink, write, exit (CONTRACT §A.4, lab 18).
 */
#include "../sys.h"

int main(int argc, char **argv, char **envp)
{
    (void)envp;
    if (argc < 2) { err_msg("rm", "usage: rm <path>..."); return 2; }
    int status = 0;
    for (int i = 1; i < argc; i++) {
        i64 r = sys_unlink(argv[i]);
        if (r < 0) { err_line("rm", argv[i], r); status = 1; }
    }
    return status;
}
