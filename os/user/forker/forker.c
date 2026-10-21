/*
 * user/forker/forker.c -- lab 14: fork, exit and wait4, seen from user space.
 *
 * Protocol (CONTRACT §A.3): argv = ["forker", <nonce>, <index>], run by the
 * course's init (fork + execve), whose pid it reads with getppid(). Rows,
 * numbered index .. index+2 (init reports fork.orphan.to_init as index+3):
 *   fork.wait.codes             N children (forker.h), child i exits with its derived code;
 *                               N wait4(-1) calls return N distinct pids, each with
 *                               WIFEXITED and the code that pid's child was given; then
 *                               wait4(-1, WNOHANG) is -ECHILD
 *   fork.child.write.invisible  a global holds A; a child writes ~A to it and exits 0 if it
 *                               reads ~A back and its getppid() is the pid forker read from
 *                               getpid() before the fork (PPID_WRONG if that pid is not);
 *                               after reaping the child the parent still reads A
 *   wait.echild.no_hang         wait4(-1) with no children is -ECHILD at once; wait4(pid,
 *                               WNOHANG) on a child still running (a 2000-yield loop) is 0;
 *                               then wait4(pid, 0) reaps it with code 7; and a child that
 *                               exits at once is collected by wait4(pid, WNOHANG) once
 *                               sched_yield() (which returns 0) has let it run, within
 *                               ENDED_YIELDS tries
 * Then fork.orphan.to_init's setup: a child forks a grandchild and exits at
 * once; forker reaps the child and exits. The grandchild yields until its
 * parent is init (getppid() == init's pid, at most ORPHAN_YIELDS times) and
 * exits with orphan_code() -- or with 3 if it never was. init's wait4 sees it.
 * Children print nothing except the one '#' line naming the orphan.
 */
#include "../sys.h"
#include "forker.h"

#define WNOHANG_YIELDS 2000
#define ORPHAN_YIELDS  200000
/* fork.child.write.invisible: the child exits with this when its getppid() is not forker's
 * pid. Outside forker.h's two ranges (1..100, 150..249) (class B, TPM log 2026-10-19, B9). */
#define PPID_WRONG     120
/* wait.echild.no_hang, part 3: the yields forker gives a child that exits at once before a
 * WNOHANG wait4 must have collected it. On one CPU the first yield is enough: the child was
 * queued at the fork, ahead of forker's yield (class B, TPM log 2026-10-19). */
#define ENDED_YIELDS   20
#define ENDED_CODE     9

static volatile u64 shared_word = 0x5b007b00a2c40000ULL;   /* .data: copied by fork, never shared */

static __attribute__((noreturn)) void child_exit(int code) { sys_exit(code); }

int main(int argc, char **argv, char **envp)
{
    struct line l = { 0, {0} };
    u64 index = 0;
    int st = 0;
    (void)envp;

    if (argc < 3 || !dec_parse(argv[2], &index)) {
        line_row(&l, 0, 0, "fork.wait.codes");
        return 2;
    }
    const char *nonce = argv[1];
    int good_nonce = nonce_valid(nonce);
    i64 init_pid = sys_getppid();
    line_note(&l, "forker: pid=");
    line_sdec(&l, sys_getpid());
    line_str(&l, " ppid=");
    line_sdec(&l, init_pid);
    line_end(&l);

    /* wait.echild.no_hang, part 1: no children yet. */
    i64 echild = sys_wait4(-1, &st, 0);
    line_note(&l, "wait4(-1) with no children = ");
    line_sdec(&l, echild);
    line_end(&l);

    /* fork.wait.codes */
    u64 n = wait_codes_n(nonce);
    i64 pid[5] = { 0, 0, 0, 0, 0 };
    int reaped[5] = { 0, 0, 0, 0, 0 };
    int codes_ok = good_nonce;
    for (u64 i = 0; i < n; i++) {
        i64 r = sys_fork();
        if (r == 0) child_exit(wait_code(nonce, i));
        pid[i] = r;
        if (r < 0) codes_ok = 0;
    }
    for (u64 k = 0; k < n; k++) {
        st = 0;
        i64 r = sys_wait4(-1, &st, 0);
        int match = -1;
        for (u64 i = 0; i < n; i++)
            if (r > 0 && pid[i] == r && !reaped[i]) match = (int)i;
        line_note(&l, "fork.wait.codes: wait4(-1) = ");
        line_sdec(&l, r);
        line_str(&l, " status=0x");
        line_hex(&l, (u64)(u32)st);
        if (match >= 0) {
            reaped[match] = 1;
            line_str(&l, " child ");
            line_dec(&l, (u64)match);
            line_str(&l, " given code ");
            line_dec(&l, (u64)wait_code(nonce, (u64)match));
            if (!WIFEXITED(st) || WEXITSTATUS(st) != wait_code(nonce, (u64)match)) codes_ok = 0;
        } else {
            line_str(&l, " (not one of this forker's unreaped children)");
            codes_ok = 0;
        }
        line_end(&l);
    }
    i64 after = sys_wait4(-1, &st, WNOHANG);
    line_note(&l, "fork.wait.codes: n=");
    line_dec(&l, n);
    line_str(&l, "; wait4(-1, WNOHANG) after all = ");
    line_sdec(&l, after);
    line_end(&l);
    if (after != -ECHILD) codes_ok = 0;
    line_row(&l, codes_ok, index, "fork.wait.codes");

    /* fork.child.write.invisible */
    u64 a = derive(nonce, "fork.child.write.invisible");
    shared_word = a;
    i64 me = sys_getpid();
    i64 c = sys_fork();
    if (c == 0) {
        shared_word = ~a;
        child_exit(shared_word != ~a ? 1 : sys_getppid() == me ? 0 : PPID_WRONG);
    }
    st = 0;
    i64 rc = sys_wait4(c, &st, 0);
    u64 seen = shared_word;
    line_note(&l, "fork.child.write.invisible: child ");
    line_sdec(&l, c);
    line_str(&l, " wrote ~A; wait4 = ");
    line_sdec(&l, rc);
    line_str(&l, " status=0x");
    line_hex(&l, (u64)(u32)st);
    line_str(&l, "; parent reads ");
    line_str(&l, seen == a ? "A" : (seen == ~a ? "~A (the child's write)" : "neither"));
    if (WIFEXITED(st) && WEXITSTATUS(st) == PPID_WRONG) {
        line_str(&l, "; the child's getppid() was not ");
        line_sdec(&l, me);
    }
    line_end(&l);
    line_row(&l, c > 0 && rc == c && WIFEXITED(st) && WEXITSTATUS(st) == 0 && seen == a,
             index + 1, "fork.child.write.invisible");

    /* wait.echild.no_hang, part 2: WNOHANG on a running child. */
    i64 w = sys_fork();
    if (w == 0) {
        for (int i = 0; i < WNOHANG_YIELDS; i++) sys_sched_yield();
        child_exit(7);
    }
    i64 nohang = sys_wait4(w, &st, WNOHANG);
    st = 0;
    i64 rw = sys_wait4(w, &st, 0);
    line_note(&l, "wait.echild.no_hang: wait4(child, WNOHANG) = ");
    line_sdec(&l, nohang);
    line_str(&l, "; wait4(child, 0) = ");
    line_sdec(&l, rw);
    line_str(&l, " status=0x");
    line_hex(&l, (u64)(u32)st);
    line_end(&l);
    int running_ok = echild == -ECHILD && w > 0 && nohang == 0 && rw == w && WIFEXITED(st) && WEXITSTATUS(st) == 7;

    /* wait.echild.no_hang, part 3: WNOHANG on a child that has ended. */
    i64 e = sys_fork();
    if (e == 0) child_exit(ENDED_CODE);
    i64 got = 0, yret = 0;
    int tries = 0, yields_ok = 1;
    st = 0;
    while (e > 0 && got == 0 && tries < ENDED_YIELDS) {
        yret = sys_sched_yield();
        if (yret != 0) yields_ok = 0;
        tries++;
        got = sys_wait4(e, &st, WNOHANG);
    }
    int ended_ok = e > 0 && yields_ok && got == e && WIFEXITED(st) && WEXITSTATUS(st) == ENDED_CODE;
    line_note(&l, "wait.echild.no_hang: a child that exits at once: after ");
    line_dec(&l, (u64)tries);
    line_str(&l, " sched_yield() (the last returned ");
    line_sdec(&l, yret);
    line_str(&l, "), wait4(child, WNOHANG) = ");
    line_sdec(&l, got);
    line_str(&l, " status=0x");
    line_hex(&l, (u64)(u32)st);
    line_end(&l);
    if (e > 0 && got == 0) sys_wait4(e, &st, 0);
    line_row(&l, running_ok && ended_ok, index + 2, "wait.echild.no_hang");

    /* fork.orphan.to_init: the setup; init reports. */
    i64 o = sys_fork();
    if (o == 0) {
        i64 g = sys_fork();
        if (g == 0) {
            int k = 0;
            while (sys_getppid() != init_pid && k < ORPHAN_YIELDS) { sys_sched_yield(); k++; }
            child_exit(sys_getppid() == init_pid ? orphan_code(nonce) : 3);
        }
        line_note(&l, "fork.orphan.to_init: pid ");
        line_sdec(&l, sys_getpid());
        line_str(&l, " forked ");
        line_sdec(&l, g);
        line_str(&l, " and exits first; ");
        line_sdec(&l, g);
        line_str(&l, " exits once its parent is ");
        line_sdec(&l, init_pid);
        line_end(&l);
        child_exit(0);
    }
    st = 0;
    sys_wait4(o, &st, 0);
    return 0;
}
