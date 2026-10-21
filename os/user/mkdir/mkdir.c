/*
 * user/mkdir/mkdir.c -- lab 18's `mkdir`: make each directory (mode 0755).
 *
 *     mkdir <path>...
 *
 * One mkdir(2) per path, no -p. An error is `mkdir: <path>: <errno name or -N>` on fd 2;
 * mkdir goes on and exits 1 if any failed; no path is a usage error, exit 2.
 * Syscalls: mkdir, write, exit (CONTRACT §A.4, lab 18).
 */
#include "../sys.h"

int main(int argc, char **argv, char **envp)
{
    (void)envp;
    if (argc < 2) { err_msg("mkdir", "usage: mkdir <path>..."); return 2; }
    int status = 0;
    for (int i = 1; i < argc; i++) {
        i64 r = sys_mkdir(argv[i], 0755);
        if (r < 0) { err_line("mkdir", argv[i], r); status = 1; }
    }
    return status;
}
