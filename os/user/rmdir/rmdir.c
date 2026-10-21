/*
 * user/rmdir/rmdir.c -- lab 18's `rmdir`: remove each empty directory.
 *
 *     rmdir <path>...
 *
 * One rmdir(2) per path; a directory with entries is the kernel's ENOTEMPTY. An error is
 * `rmdir: <path>: <errno name or -N>` on fd 2; rmdir goes on and exits 1 if any failed; no
 * path is a usage error, exit 2. Syscalls: rmdir, write, exit (CONTRACT §A.4, lab 18).
 */
#include "../sys.h"

int main(int argc, char **argv, char **envp)
{
    (void)envp;
    if (argc < 2) { err_msg("rmdir", "usage: rmdir <path>..."); return 2; }
    int status = 0;
    for (int i = 1; i < argc; i++) {
        i64 r = sys_rmdir(argv[i]);
        if (r < 0) { err_line("rmdir", argv[i], r); status = 1; }
    }
    return status;
}
