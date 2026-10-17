/*
 * user/hi/hi.c -- lab 09's program, and the course's retention device:
 * labs 12, 13, 14, 15, 17, 21 and 22 run it again (DESIGN.md).
 *
 * Protocol (CONTRACT §A.3): argv = ["hi", <nonce: 32 hex>, <index: decimal>].
 * It reads its own CS, prints the line Part C fixes,
 *     hi: cs=0x2b rpl=3 nonce=<argv[1]>
 * with the values it actually read, then evidence on '#' lines, then its one
 * KTAP result:
 *     ok <index> - ring3.cs.rpl       when cs == 0x2b and its RPL is 3
 *     not ok <index> - ring3.cs.rpl   otherwise, or when the nonce is not 32 hex
 * and exits 0. A short or malformed argv prints `not ok 0 - ring3.cs.rpl`
 * and exits 2. Each line is one write(1) call.
 */
#include "../sys.h"

#define ID      "ring3.cs.rpl"
#define USER_CS 0x2b    /* CONTRACT §A.9 S1: user code64 is 0x28, entered with RPL 3 */

static int index_valid(const char *s)
{
    int n = 0;
    for (; s[n]; n++)
        if (n >= 9 || s[n] < '0' || s[n] > '9') return 0;
    return n > 0;
}

int main(int argc, char **argv, char **envp)
{
    struct line l = { 0, {0} };
    (void)envp;

    if (argc < 3 || !index_valid(argv[2])) {
        line_str(&l, "not ok 0 - " ID);
        line_end(&l);
        return 2;
    }
    const char *nonce = argv[1];
    const char *index = argv[2];

    u64 cs  = read_cs();
    u64 rpl = cs & 3;

    line_str(&l, "hi: cs=0x");
    line_hex(&l, cs);
    line_str(&l, " rpl=");
    line_dec(&l, rpl);
    line_str(&l, " nonce=");
    line_str(&l, nonce);
    line_end(&l);

    int good_nonce = nonce_valid(nonce);
    if (good_nonce) {
        line_str(&l, "# derive=");
        line_hex64(&l, derive(nonce, ID));
    } else {
        line_str(&l, "# nonce is not 32 hex characters");
    }
    line_end(&l);

    /* The ABI once more: two more lab-09 calls, printed as they come back
     * (the kernel answers 1, and 0 with 80x25: CONTRACT §A.4). */
    line_str(&l, "# getpid=");
    line_sdec(&l, sys_getpid());
    line_end(&l);

    struct winsize ws = { 0, 0, 0, 0 };
    i64 r = sys_ioctl(STDOUT_FILENO, TIOCGWINSZ, &ws);
    line_str(&l, "# ioctl(TIOCGWINSZ)=");
    line_sdec(&l, r);
    line_str(&l, " rows=");
    line_dec(&l, ws.ws_row);
    line_str(&l, " cols=");
    line_dec(&l, ws.ws_col);
    line_end(&l);

    if (!(good_nonce && cs == USER_CS && rpl == 3)) line_str(&l, "not ");
    line_str(&l, "ok ");
    line_str(&l, index);
    line_str(&l, " - " ID);
    line_end(&l);
    return 0;
}
