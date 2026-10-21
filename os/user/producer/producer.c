/*
 * user/producer/producer.c -- lab 15: the writing end of init's pipe.
 *
 * Protocol (CONTRACT §A.3): argv = ["producer", <nonce>, <index>, "chain" | "tiny"],
 * run by init with fd 1 = the pipe's write end. It reports no row (consumer
 * does); its '#' evidence goes to fd 2, the console.
 *   chain  writes stream.h's stream: all but the last STREAM_TAIL bytes to fd 1 in
 *          stream_chunk()-sized write calls with a sched_yield() after each (so the
 *          pipe runs empty while a writer is still open); then dup(1), close(1),
 *          20 yields, the tail through the dup, close it, exit 0. A kernel that
 *          signals EOF when one fd of the write end closes, rather than the last,
 *          cuts the consumer's stream short by the tail. If dup fails the tail is
 *          not written at all (the stream is then short, on purpose).
 *   tiny   TINY_COUNT write(1, &b, 1) calls, no yields.
 */
#include "../sys.h"
#include "stream.h"

/* Ends an evidence line on fd 2. Every line here is far under LINE_MAX, so line_putc never
 * flushes early -- an early flush would go to fd 1, the pipe. */
static void note(struct line *l) { l->b[l->n++] = '\n'; write_buf_fd(2, l->b, l->n); l->n = 0; }

int main(int argc, char **argv, char **envp)
{
    (void)envp;
    struct line l = { 0, {0} };
    if (argc < 4) return 2;
    const char *nonce = argv[1];
    int fd = 1;

    if (streq(argv[3], "tiny")) {
        u64 i = 0;
        for (; i < TINY_COUNT; i++) {
            u8 b = tiny_byte(i);
            if (sys_write(1, &b, 1) != 1) break;
        }
        line_note(&l, "producer tiny: ");
        line_dec(&l, i);
        line_str(&l, " one-byte writes");
        note(&l);
        return i == TINY_COUNT ? 0 : 1;
    }

    u64 seed = stream_seed(nonce), len = stream_len(seed), body = len - STREAM_TAIL;
    u64 pos = 0, calls = 0;
    u8 buf[1024];
    for (u64 j = 0; pos < len; j++) {
        if (pos == body) {
            i64 x = sys_dup(1);
            if (x < 0) {
                line_note(&l, "producer chain: dup(1) = ");
                line_sdec(&l, x);
                line_str(&l, "; the tail is not written");
                note(&l);
                return 1;
            }
            sys_close(1);
            for (int k = 0; k < 20; k++) sys_sched_yield();
            fd = (int)x;
        }
        u64 n = stream_chunk(seed, j);
        u64 stop = pos < body ? body : len;
        if (n > stop - pos) n = stop - pos;
        for (u64 k = 0; k < n; k++) buf[k] = stream_byte(seed, pos + k);
        u64 off = 0;
        while (off < n) {
            i64 r = sys_write(fd, buf + off, n - off);
            calls++;
            if (r <= 0) {
                line_note(&l, "producer chain: write = ");
                line_sdec(&l, r);
                line_str(&l, " at byte ");
                line_dec(&l, pos + off);
                note(&l);
                return 1;
            }
            off += (u64)r;
        }
        pos += n;
        sys_sched_yield();
    }
    sys_close(fd);
    line_note(&l, "producer chain: ");
    line_dec(&l, len);
    line_str(&l, " bytes in ");
    line_dec(&l, calls);
    line_str(&l, " write calls; the last ");
    line_dec(&l, STREAM_TAIL);
    line_str(&l, " through fd ");
    line_dec(&l, (u64)fd);
    line_str(&l, " after close(1)");
    note(&l);
    return 0;
}
