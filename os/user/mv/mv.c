/*
 * user/mv/mv.c -- lab 18's `mv`: rename one path to another.
 *
 *     mv <from> <to>
 *
 * One rename(2): replaces an existing <to> the way the kernel's rename does; across file
 * systems it is EXDEV (mv copies nothing). An error is `mv: <from>: <errno name or -N>` on
 * fd 2 and exit 1; any other argument count is a usage error, exit 2.
 * Syscalls: rename, write, exit (CONTRACT §A.4, lab 18).
 */
#include "../sys.h"

int main(int argc, char **argv, char **envp)
{
    (void)envp;
    if (argc != 3) { err_msg("mv", "usage: mv <from> <to>"); return 2; }
    i64 r = sys_rename(argv[1], argv[2]);
    if (r < 0) { err_line("mv", argv[1], r); return 1; }
    return 0;
}
