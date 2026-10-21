/*
 * user/hostile-badptr/hostile-badptr.c -- lab 12: write(2) with pointers the process may not read.
 *
 * Protocol (CONTRACT §A.3): argv = ["hostile-badptr", <nonce>, <index>].
 * Calls write(1, p, 16) for three p, each of which must come back -EFAULT (14),
 * a refusal, never a kill (§A.6: copy_from_user's range check):
 *   0                       the NULL page: nothing maps it (course binaries start at 0x1000000)
 *   0x0000100000000000      user half, unmapped (no VMA, no PTE)
 *   0xffff800000001000      kernel half (the HHDM: present, U=0)
 * One '#' line per call, then
 *     ok <index> - sys.badptr.efault     when all three returned -EFAULT
 * and exit 0. A malformed argv prints `not ok 0 - sys.badptr.efault` and exits 2.
 */
#include "../sys.h"

#define ID "sys.badptr.efault"

int main(int argc, char **argv, char **envp)
{
    struct line l = { 0, {0} };
    u64 index = 0;
    (void)envp;

    if (argc < 3 || !dec_parse(argv[2], &index)) {
        line_row(&l, 0, 0, ID);
        return 2;
    }

    static const u64 ptr[3] = { 0, 0x0000100000000000ULL, 0xffff800000001000ULL };
    i64 r[3];
    for (int i = 0; i < 3; i++)
        r[i] = sys_write(STDOUT_FILENO, (const void *)ptr[i], 16);

    int good = 1;
    for (int i = 0; i < 3; i++) {
        line_note(&l, "write(1, ");
        line_ptr(&l, ptr[i]);
        line_str(&l, ", 16) = ");
        line_sdec(&l, r[i]);
        line_str(&l, r[i] == -EFAULT ? " (-EFAULT)" : " (expected -EFAULT = -14)");
        line_end(&l);
        if (r[i] != -EFAULT) good = 0;
    }
    line_row(&l, good, index, ID);
    return 0;
}
