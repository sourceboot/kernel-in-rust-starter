/*
 * user/hostile-badnum/hostile-badnum.c -- lab 12: syscall numbers the kernel does not serve.
 *
 * Protocol (CONTRACT §A.3): argv = ["hostile-badnum", <nonce>, <index>].
 * Issues three numbers and expects exactly -ENOSYS (38) from each, a refusal,
 * never a kill (§A.4, §A.6):
 *   1000           above every number in §A.4's table (and above Linux's)
 *   (u64)-1        the all-ones number: a dispatch that indexes a table by it faults
 *   158            arch_prctl(ARCH_GET_FS, &v): in §A.4's table, but its arm is lab 25's,
 *                  so lab 12's kernel answers -ENOSYS. (Linux serves it: run natively, this
 *                  call returns 0 and the row prints `not ok` -- by design, see README.md.)
 * One '#' line per call with what came back, then
 *     ok <index> - sys.badnum.enosys     when all three returned -ENOSYS
 * and exit 0. A malformed argv prints `not ok 0 - sys.badnum.enosys` and exits 2.
 */
#include "../sys.h"

#define ID "sys.badnum.enosys"
#define SYS_arch_prctl 158      /* §A.4: lab 25; -ENOSYS before it */
#define ARCH_GET_FS    0x1003

int main(int argc, char **argv, char **envp)
{
    struct line l = { 0, {0} };
    u64 index = 0;
    (void)envp;

    if (argc < 3 || !dec_parse(argv[2], &index)) {
        line_row(&l, 0, 0, ID);
        return 2;
    }

    u64 fs = 0;
    static const char *const what[3] = { "syscall(1000)", "syscall(-1)", "syscall(158 arch_prctl, ARCH_GET_FS)" };
    i64 r[3];
    r[0] = syscall0(1000);
    r[1] = syscall0(-1);
    r[2] = syscall2(SYS_arch_prctl, ARCH_GET_FS, (i64)&fs);

    int good = 1;
    for (int i = 0; i < 3; i++) {
        line_note(&l, what[i]);
        line_str(&l, " = ");
        line_sdec(&l, r[i]);
        line_str(&l, r[i] == -ENOSYS ? " (-ENOSYS)" : " (expected -ENOSYS = -38)");
        line_end(&l);
        if (r[i] != -ENOSYS) good = 0;
    }
    line_row(&l, good, index, ID);
    return 0;
}
