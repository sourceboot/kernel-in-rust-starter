/*
 * user/init/init.c -- the course's init: from lab 14 on, the runner of every lab's programs.
 *
 * Protocol (CONTRACT §A.3, the init shape in README.md):
 *     argv = [<path init was exec'd from>, <nonce>, <index>, <lab>]
 * The vehicle passes argv[0] as the path it ran init from ("/init" while the
 * boot archive is `/`, labs 14-16; "/boot/init" from 17), and init runs each
 * program from that same directory: "/init" runs "/forker". It passes its own
 * envp to every program unchanged. fds 0, 1 and 2 are the console when it starts.
 *
 *   lab 14  fork + execve forker [forker, nonce, index]; forker reports index..index+2.
 *           init reaps with wait4(-1) until -ECHILD. Any pid it reaps that it did not
 *           fork is an orphan reparented to it; one whose exit code is forker.h's
 *           orphan_code() makes
 *               ok <index+3> - fork.orphan.to_init
 *   lab 15  the pipe wiring, with pipe2, fork, dup2, close and execve:
 *           producer chain | consumer chain   (consumer reports index..index+2)
 *           producer tiny  | consumer tiny    (consumer reports index+3)
 *           A second pipe made with O_CLOEXEC is open in init across both execs of the
 *           chain; the consumer is told its two fd numbers and checks they are gone. The
 *           consumer is also forked holding the CLOEXEC_N fds of cloexec_fds(nonce)
 *           (stream.h, from 11..22), made by cloexec_make, half of them marked with F_SETFD,
 *           and checks after its execve that exactly the marked ones are gone and the others
 *           open and unmarked; argv carries only the word "keep", never a number.
 *           init closes its own copies of every pipe end before it waits, so the
 *           consumer's EOF depends only on the producer. Before either, init's own row:
 *               ok <index+4> - dup2.shared.flags
 *           when two open files hold different status flags at once, each seen through
 *           every fd that names it and nowhere else (dup2_row): O_NONBLOCK through the
 *           O_CLOEXEC pipe's read end and four dup2 chains of it, O_APPEND through the first
 *           pipe's read end and four of its own, and O_APPEND on the O_CLOEXEC pipe's write
 *           end; clearing the first leaves the others theirs. The eight dups are 20..31
 *           shuffled by the nonce (dup2_fds). F_GETFD must still differ (cloexec is per
 *           fd): FD_CLOEXEC on the first fd, 0 on every dup.
 *           A dup2 that fails in a child (the call not built yet) prints a '#' line and
 *           exits 127 before the execve, so no program writes its stream to the console.
 * Then init exits 0 and the vehicle prints `# sboot end`. An unknown lab, or a
 * malformed argv, prints a '#' line and exits 2.
 */
#include "../sys.h"
#include "../forker/forker.h"
#include "../producer/stream.h"

static const char *nonce;
static char *const *env;
static char dir[64];          /* argv[0] up to and including its last '/' ("" if none) */

static void fmt_dec(char *b, u64 v)            /* b holds at least 21 bytes */
{
    char t[20];
    int i = 20, k = 0;
    do { t[--i] = (char)('0' + v % 10); v /= 10; } while (v);
    while (i < 20) b[k++] = t[i++];
    b[k] = 0;
}

/* fork; the child moves in_fd to 0 and out_fd to 1, closes close_fds, and execve()s dir+name. Returns the pid. */
static i64 spawn(const char *name, char *const *argv, int in_fd, int out_fd, const int *close_fds, int nclose)
{
    i64 pid = sys_fork();
    if (pid != 0) return pid;
    int from = -1, to = -1;
    i64 bad = 0;
    if (in_fd >= 0 && in_fd != 0 && (bad = sys_dup2(in_fd, 0)) != 0)   { from = in_fd; to = 0; }
    else if (out_fd >= 0 && out_fd != 1 && (bad = sys_dup2(out_fd, 1)) != 1) { from = out_fd; to = 1; }
    if (from >= 0) {
        struct line l = { 0, {0} };
        line_note(&l, "init: dup2(");
        line_dec(&l, (u64)from);
        line_str(&l, ", ");
        line_dec(&l, (u64)to);
        line_str(&l, ") = ");
        line_sdec(&l, bad);
        line_str(&l, " in the child; ");
        line_str(&l, name);
        line_str(&l, " not run");
        line_end(&l);
        sys_exit(127);
    }
    for (int i = 0; i < nclose; i++)
        if (close_fds[i] > 2) sys_close(close_fds[i]);
    char path[128];
    usize n = 0;
    for (const char *s = dir; *s && n < sizeof path - 1; s++) path[n++] = *s;
    for (const char *s = name; *s && n < sizeof path - 1; s++) path[n++] = *s;
    path[n] = 0;
    i64 r = sys_execve(path, argv, env);
    struct line l = { 0, {0} };
    line_note(&l, "init: execve ");
    line_str(&l, path);
    line_str(&l, " = ");
    line_sdec(&l, r);
    line_end(&l);
    sys_exit(127);
}

static void note_reap(struct line *l, const char *what, i64 pid, int st)
{
    line_note(l, "init: reaped ");
    line_str(l, what);
    line_str(l, " pid=");
    line_sdec(l, pid);
    line_str(l, " status=0x");
    line_hex(l, (u64)(u32)st);
    line_end(l);
}

/* Reaps until -ECHILD. Children init forked are in mine[]; anything else is an orphan:
 * returns 1 if one exited with `orphan_want` (pass -1 to only reap). */
static int reap_all(const i64 *mine, int nmine, int orphan_want)
{
    struct line l = { 0, {0} };
    int found = 0;
    for (;;) {
        int st = 0;
        i64 r = sys_wait4(-1, &st, 0);
        if (r == -ECHILD) return found;
        if (r < 0) {
            line_note(&l, "init: wait4(-1) = ");
            line_sdec(&l, r);
            line_end(&l);
            return found;
        }
        int is_mine = 0;
        for (int i = 0; i < nmine; i++) if (mine[i] == r) is_mine = 1;
        note_reap(&l, is_mine ? "child" : "orphan", r, st);
        if (!is_mine && WIFEXITED(st) && WEXITSTATUS(st) == orphan_want) found = 1;
    }
}

static int lab14(u64 index)
{
    struct line l = { 0, {0} };
    char idx[24];
    fmt_dec(idx, index);
    char *argv[] = { "forker", (char *)nonce, idx, 0 };
    i64 f = spawn("forker", argv, -1, -1, 0, 0);
    int want = orphan_code(nonce);
    int found = f > 0 ? reap_all(&f, 1, want) : 0;
    line_note(&l, "fork.orphan.to_init: an orphan exiting with ");
    line_dec(&l, (u64)want);
    line_str(&l, found ? " was reaped by init" : " never reached init");
    line_end(&l);
    line_row(&l, nonce_valid(nonce) && f > 0 && found, index + 3, "fork.orphan.to_init");
    return 0;
}

/* dup2.shared.flags: dup2 shares the open file, so a status flag set through one fd is read
 * through the other; cloexec stays per fd. The flag is cleared again before any child is forked. */
/* The fds the row dup2s onto, from the nonce (fix round 2, 2026-10-19), so no fd number is
 * fixed: the twelve fds 20..31 (free in every lab 15 run: init holds 0-2 and the two pipes'
 * four ends) are shuffled by derive(nonce, "dup2.shared.flags.fds"); the first DUPS become
 * dups of the fd whose flag is set (A), the next DUPS dups of another pipe's read end (X).
 * A kernel that answers F_GETFL by fd number has to guess which four of the eight are A. */
#define DUPS 4
static void dup2_fds(int a[DUPS], int x[DUPS])
{
    int fds[12];
    for (int i = 0; i < 12; i++) fds[i] = 20 + i;
    u64 b = derive(nonce, "dup2.shared.flags.fds");
    for (int i = 11; i > 0; i--) {
        b = splitmix64(b);
        int j = (int)(b % (u64)(i + 1));
        int t = fds[i]; fds[i] = fds[j]; fds[j] = t;
    }
    for (int i = 0; i < DUPS; i++) { a[i] = fds[i]; x[i] = fds[DUPS + i]; }
}
#define NB(x) ((x) >= 0 && ((x) & O_NONBLOCK))
#define AP(x) ((x) >= 0 && ((x) & O_APPEND))
#define NONE(x) ((x) >= 0 && !((x) & (O_NONBLOCK | O_APPEND)))
#define GRP (DUPS + 1)
/* " fd=value" for each fd of a group, in decimal. */
static void line_grp(struct line *l, const int fd[GRP], const i64 v[GRP])
{
    for (int i = 0; i < GRP; i++) {
        line_putc(l, ' ');
        line_dec(l, (u64)fd[i]);
        line_putc(l, '=');
        line_sdec(l, v[i]);
    }
}
static void getfl_grp(const int fd[GRP], const int made[GRP], i64 v[GRP])
{
    for (int i = 0; i < GRP; i++) v[i] = made[i] ? sys_fcntl(fd[i], F_GETFL, 0) : -EBADF;
}
/* Fix round 3 (2026-10-19): two open files hold different flags at once. Group Q is the
 * O_CLOEXEC pipe's read end and four dups of it, group P the first pipe's read end and four
 * dups of it; each dup is made from the group's first fd or from an earlier dup, picked by
 * the nonce. O_NONBLOCK is set through one member of Q, O_APPEND through one member of P and
 * through the O_CLOEXEC pipe's write end (a third open file, on the same pipe as Q), in an
 * order the nonce picks. Every member of Q must show O_NONBLOCK and not O_APPEND, every member
 * of P and the write end O_APPEND and not O_NONBLOCK; clearing Q (through another member) must
 * leave P and the write end their O_APPEND. Flags kept anywhere but in each open file -- one
 * global, a value tagged by a pipe, an fd's dup2 history or the last file set -- cannot hold
 * all three at once. */
static void dup2_row(int fd, int other, int third, u64 index)
{
    struct line l = { 0, {0} };
    int Q[GRP], P[GRP], qsrc[GRP], psrc[GRP], qm[GRP], pm[GRP];
    i64 qd[GRP], pd[GRP], qb[GRP], pb[GRP], qs[GRP], ps[GRP], qa[GRP], pa[GRP], pz[GRP], qfd[GRP];
    int A[DUPS], X[DUPS];
    dup2_fds(A, X);
    u64 b = derive(nonce, "dup2.shared.flags.order");
    Q[0] = fd; P[0] = third; qsrc[0] = fd; psrc[0] = third; qd[0] = fd; pd[0] = third;
    for (int i = 1; i < GRP; i++) {
        Q[i] = A[i - 1]; P[i] = X[i - 1];
        qsrc[i] = Q[(b >> (4 * i)) % (u64)i];
        psrc[i] = P[(b >> (4 * i + 20)) % (u64)i];
    }
    i64 same = sys_dup2(fd, fd);
    int made = 1, nosys = 0;
    qm[0] = pm[0] = 1;
    for (int i = 1; i < GRP; i++) {
        qd[i] = qm[(b >> (4 * i)) % (u64)i] ? sys_dup2(qsrc[i], Q[i]) : -EBADF;
        pd[i] = pm[(b >> (4 * i + 20)) % (u64)i] ? sys_dup2(psrc[i], P[i]) : -EBADF;
        qm[i] = qd[i] == Q[i];
        pm[i] = pd[i] == P[i];
        made &= qm[i] && pm[i];
        nosys |= qd[i] == -ENOSYS || pd[i] == -ENOSYS;
    }
    getfl_grp(Q, qm, qb);
    getfl_grp(P, pm, pb);
    i64 ob = sys_fcntl(other, F_GETFL, 0);
    int qset = Q[(b >> 40) % GRP], pset = P[(b >> 44) % GRP], qclr = Q[(b >> 48) % GRP];
    int order = (int)((b >> 52) % 3);   /* which of the three sets goes first */
    i64 s1 = 0, s2 = 0, s3 = 0;
    for (int k = 0; k < 3; k++) {
        int which = (order + k) % 3;
        if (which == 0) s1 = sys_fcntl(qset, F_SETFL, O_NONBLOCK);
        else if (which == 1) s2 = sys_fcntl(pset, F_SETFL, O_APPEND);
        else s3 = sys_fcntl(other, F_SETFL, O_APPEND);
    }
    getfl_grp(Q, qm, qs);
    getfl_grp(P, pm, ps);
    i64 os = sys_fcntl(other, F_GETFL, 0);
    for (int i = 0; i < GRP; i++) qfd[i] = qm[i] ? sys_fcntl(Q[i], F_GETFD, 0) : -EBADF;
    i64 c1 = sys_fcntl(qclr, F_SETFL, 0);
    getfl_grp(Q, qm, qa);
    getfl_grp(P, pm, pa);
    i64 oa = sys_fcntl(other, F_GETFL, 0);
    i64 c2 = sys_fcntl(third, F_SETFL, 0);
    i64 c3 = sys_fcntl(other, F_SETFL, 0);
    getfl_grp(P, pm, pz);
    i64 oz = sys_fcntl(other, F_GETFL, 0);
    for (int i = 1; i < GRP; i++) {
        if (qm[i]) sys_close(Q[i]);
        if (pm[i]) sys_close(P[i]);
    }
    int sets = s1 == 0 && s2 == 0 && s3 == 0 && c1 == 0 && c2 == 0 && c3 == 0;
    int seen = made && sets && AP(os);
    int only = NONE(ob) && NONE(oz) && !NB(os);
    int own = AP(oa) && !NB(oa);
    int per_fd = qfd[0] == FD_CLOEXEC;
    for (int i = 0; i < GRP; i++) {
        seen &= NB(qs[i]) && AP(ps[i]);
        only &= NONE(qb[i]) && NONE(pb[i]) && !AP(qs[i]) && !NB(ps[i]) && NONE(qa[i]) && NONE(pz[i]);
        own &= AP(pa[i]) && !NB(pa[i]);
        if (i > 0) per_fd &= qfd[i] == 0;
    }

    line_note(&l, "dup2.shared.flags: dup2(from, to)=");
    for (int i = 1; i < GRP; i++) {
        line_putc(&l, ' ');
        line_dec(&l, (u64)qsrc[i]);
        line_str(&l, "->");
        line_dec(&l, (u64)Q[i]);
        line_putc(&l, '=');
        line_sdec(&l, qd[i]);
    }
    for (int i = 1; i < GRP; i++) {
        line_putc(&l, ' ');
        line_dec(&l, (u64)psrc[i]);
        line_str(&l, "->");
        line_dec(&l, (u64)P[i]);
        line_putc(&l, '=');
        line_sdec(&l, pd[i]);
    }
    line_end(&l);
    line_note(&l, "dup2.shared.flags: F_GETFL before any set:");
    line_grp(&l, Q, qb);
    line_str(&l, ";");
    line_grp(&l, P, pb);
    line_str(&l, "; ");
    line_dec(&l, (u64)other);
    line_putc(&l, '=');
    line_sdec(&l, ob);
    line_end(&l);
    line_note(&l, "dup2.shared.flags: F_SETFL(O_NONBLOCK) through ");
    line_dec(&l, (u64)qset);
    line_putc(&l, '=');
    line_sdec(&l, s1);
    line_str(&l, ", F_SETFL(O_APPEND) through ");
    line_dec(&l, (u64)pset);
    line_putc(&l, '=');
    line_sdec(&l, s2);
    line_str(&l, " and through ");
    line_dec(&l, (u64)other);
    line_putc(&l, '=');
    line_sdec(&l, s3);
    line_str(&l, order == 0 ? " (in that order)" : order == 1 ? " (O_APPEND first)" : " (the last first)");
    line_end(&l);
    line_note(&l, "dup2.shared.flags: F_GETFL while set (O_NONBLOCK 2048, O_APPEND 1024):");
    line_grp(&l, Q, qs);
    line_str(&l, ";");
    line_grp(&l, P, ps);
    line_str(&l, "; ");
    line_dec(&l, (u64)other);
    line_putc(&l, '=');
    line_sdec(&l, os);
    line_end(&l);
    line_note(&l, "dup2.shared.flags: F_SETFL(0) through ");
    line_dec(&l, (u64)qclr);
    line_putc(&l, '=');
    line_sdec(&l, c1);
    line_str(&l, ", then F_GETFL:");
    line_grp(&l, Q, qa);
    line_str(&l, ";");
    line_grp(&l, P, pa);
    line_str(&l, "; ");
    line_dec(&l, (u64)other);
    line_putc(&l, '=');
    line_sdec(&l, oa);
    line_end(&l);
    line_note(&l, "dup2.shared.flags: F_SETFL(0) through ");
    line_dec(&l, (u64)third);
    line_putc(&l, '=');
    line_sdec(&l, c2);
    line_str(&l, " and ");
    line_dec(&l, (u64)other);
    line_putc(&l, '=');
    line_sdec(&l, c3);
    line_str(&l, ", then F_GETFL:");
    line_grp(&l, P, pz);
    line_str(&l, "; ");
    line_dec(&l, (u64)other);
    line_putc(&l, '=');
    line_sdec(&l, oz);
    line_end(&l);
    line_note(&l, "dup2.shared.flags: cloexec stays per fd: F_GETFD");
    line_grp(&l, Q, qfd);
    line_str(&l, "; dup2(fd, fd)=");
    line_sdec(&l, same);
    line_end(&l);
    if (nosys || s1 == -ENOSYS) {
        line_note(&l, "dup2.shared.flags: dup2 or fcntl answered -38 (ENOSYS): the kernel has no arm for it yet");
        line_end(&l);
    } else {
        if (!seen) {
            line_note(&l, "dup2.shared.flags: a status flag set through one fd is not seen through every fd that shares "
                          "its open file: dup2 built a second OpenFile instead of sharing the first, or F_SETFL stored "
                          "the flag per fd (beside cloexec in Fd) instead of in OpenFile.flags");
            line_end(&l);
        }
        if (!only) {
            line_note(&l, "dup2.shared.flags: a status flag is read back where it was not set: F_GETFL reports a flag "
                          "the open file does not hold, or F_SETFL(0) did not clear it");
            line_end(&l);
        }
        if (!own) {
            line_note(&l, "dup2.shared.flags: clearing one open file's flags changed another's: each OpenFile keeps "
                          "its own flags, and F_SETFL writes only the flags of the file its fd names");
            line_end(&l);
        }
        if (!per_fd) {
            line_note(&l, "dup2.shared.flags: close-on-exec is not per fd: pipe2 did not mark its fds "
                          "under O_CLOEXEC, or dup2 copied the mark to the new fd (or cleared it when "
                          "both fds are one)");
            line_end(&l);
        }
    }
    line_row(&l, seen && only && own && per_fd, index, "dup2.shared.flags");
}

/* pipe.cloexec's fds (fix round 3, 2026-10-19): CLOEXEC_N fds from cloexec_fds(nonce)
 * (stream.h), each made by dup2 -- unmarked -- and then marked with F_SETFD where
 * cloexec_marked(i) says so; slots 3 and 6 are marked and then unmarked again. Their open
 * files: slots 0 and 1 the first pipe's read end (fd 0 in the consumer, unmarked); 2 and 3
 * the read end of a new pipe r, 4 and 7 its write end; 5 and 6 the two ends of a new pipe s,
 * one fd each. So each kind holds an fd that shares its open file with the other kind and an
 * fd that shares it with nothing. r's and s's own fds are closed before the consumer is
 * forked. Returns how many of the CLOEXEC_N were made as asked. */
static int cloexec_make(int p0, int fd[CLOEXEC_N])
{
    struct line l = { 0, {0} };
    int r[2] = { -1, -1 }, s[2] = { -1, -1 };
    i64 rr = sys_pipe2(r, 0), rs = sys_pipe2(s, 0);
    int src[CLOEXEC_N] = { p0, p0, r[0], r[0], r[1], s[0], s[1], r[1] };
    int made = 0;
    cloexec_fds(nonce, fd);
    line_note(&l, "init: for pipe.cloexec, dup2 then F_SETFD:");
    for (int i = 0; i < CLOEXEC_N; i++) {
        i64 d = src[i] >= 0 ? sys_dup2(src[i], fd[i]) : -EBADF;
        i64 m = 0, u = 0;
        if (d == fd[i] && cloexec_marked(i)) m = sys_fcntl(fd[i], F_SETFD, FD_CLOEXEC);
        if (d == fd[i] && (i == 3 || i == 6)) { m = sys_fcntl(fd[i], F_SETFD, FD_CLOEXEC); u = sys_fcntl(fd[i], F_SETFD, 0); }
        i64 g = d == fd[i] ? sys_fcntl(fd[i], F_GETFD, 0) : d;
        made += d == fd[i] && m == 0 && u == 0 && g == (i64)cloexec_marked(i);
        line_putc(&l, ' ');
        line_dec(&l, (u64)fd[i]);
        line_putc(&l, '=');
        line_sdec(&l, g);
    }
    line_str(&l, " (F_GETFD); pipe2 ");
    line_sdec(&l, rr);
    line_str(&l, ", ");
    line_sdec(&l, rs);
    line_end(&l);
    for (int i = 0; i < 2; i++) {
        if (r[i] >= 0) sys_close(r[i]);
        if (s[i] >= 0) sys_close(s[i]);
    }
    return made;
}

static int lab15(u64 index)
{
    struct line l = { 0, {0} };
    char idx0[24], idx3[24], qrs[24], qws[24];
    int p[2] = { -1, -1 }, q[2] = { -1, -1 }, t[2] = { -1, -1 };
    fmt_dec(idx0, index);
    fmt_dec(idx3, index + 3);

    i64 rp = sys_pipe2(p, 0);
    i64 rq = sys_pipe2(q, O_CLOEXEC);
    line_note(&l, "init: pipe2(0)=");
    line_sdec(&l, rp);
    line_str(&l, " fds ");
    line_sdec(&l, p[0]);
    line_putc(&l, ',');
    line_sdec(&l, p[1]);
    line_str(&l, "; pipe2(O_CLOEXEC)=");
    line_sdec(&l, rq);
    line_str(&l, " fds ");
    line_sdec(&l, q[0]);
    line_putc(&l, ',');
    line_sdec(&l, q[1]);
    line_end(&l);
    if (rp < 0 || rq < 0) return 1;
    dup2_row(q[0], q[1], p[0], index + 4);
    fmt_dec(qrs, (u64)q[0]);
    fmt_dec(qws, (u64)q[1]);
    int kfd[CLOEXEC_N];

    /* producer chain | consumer chain */
    int pends[2] = { p[0], p[1] };
    char *pargv[] = { "producer", (char *)nonce, idx0, "chain", 0 };
    char *cargv[] = { "consumer", (char *)nonce, idx0, "chain", qrs, qws, "keep", 0 };
    i64 kids[2];
    kids[0] = spawn("producer", pargv, -1, p[1], pends, 2);
    /* cloexec_fds(nonce): fds above 2, half marked close-on-exec, carried by fork; execve
     * must close exactly the marked ones. Made after the producer's fork, so only the
     * consumer holds them; the consumer derives the same numbers from the nonce, so argv
     * carries only the word "keep". */
    cloexec_make(p[0], kfd);
    kids[1] = spawn("consumer", cargv, p[0], -1, pends, 2);
    for (int i = 0; i < CLOEXEC_N; i++) sys_close(kfd[i]);
    sys_close(p[0]);
    sys_close(p[1]);
    sys_close(q[0]);
    sys_close(q[1]);
    reap_all(kids, 2, -1);

    /* producer tiny | consumer tiny */
    if (sys_pipe2(t, 0) < 0) return 1;
    int tends[2] = { t[0], t[1] };
    char *tpargv[] = { "producer", (char *)nonce, idx3, "tiny", 0 };
    char *tcargv[] = { "consumer", (char *)nonce, idx3, "tiny", 0 };
    kids[0] = spawn("producer", tpargv, -1, t[1], tends, 2);
    kids[1] = spawn("consumer", tcargv, t[0], -1, tends, 2);
    sys_close(t[0]);
    sys_close(t[1]);
    reap_all(kids, 2, -1);
    return 0;
}

int main(int argc, char **argv, char **envp)
{
    struct line l = { 0, {0} };
    u64 index = 0, lab = 0;
    env = envp;

    if (argc < 4 || !dec_parse(argv[2], &index) || !dec_parse(argv[3], &lab)) {
        line_note(&l, "init: usage: init <nonce> <index> <lab>");
        line_end(&l);
        return 2;
    }
    nonce = argv[1];
    usize n = 0, cut = 0;
    for (const char *s = argv[0]; *s && n < sizeof dir - 1; s++) {
        dir[n++] = *s;
        if (*s == '/') cut = n;
    }
    dir[cut] = 0;

    line_note(&l, "init: pid=");
    line_sdec(&l, sys_getpid());
    line_str(&l, " lab ");
    line_dec(&l, lab);
    line_str(&l, ", programs from \"");
    line_str(&l, dir);
    line_str(&l, "\"");
    line_end(&l);

    if (lab == 14) return lab14(index);
    if (lab == 15) return lab15(index);
    line_note(&l, "init: no programs for lab ");
    line_dec(&l, lab);
    line_end(&l);
    return 2;
}
