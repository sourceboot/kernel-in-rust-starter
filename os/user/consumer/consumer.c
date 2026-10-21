/*
 * user/consumer/consumer.c -- lab 15: the reading end of init's pipe, and its rows.
 *
 * Protocol (CONTRACT §A.3): run by init with fd 0 = the pipe's read end,
 *     argv = ["consumer", <nonce>, <index>, "chain", <qr>, <qw>, "keep"]   rows index .. index+2
 *       ("keep" is optional: lab 23's shell types the line without it, and then no fd
 *       above 2 is checked; given, the fds are cloexec_fds(nonce) (stream.h), never numbers in argv)
 *     argv = ["consumer", <nonce>, <index>, "tiny"]                row  index
 *   chain  reads fd 0 to EOF in 512-byte reads, checking every byte against stream.h's
 *          stream and summing fnv1a64; then reads once more.
 *            pipe.chain.checksum    length and fnv1a64 both equal what the nonce gives
 *            pipe.eof.last_writer   the first read to return 0 came after exactly the
 *                                   stream's length (the producer closed one fd of the write
 *                                   end before writing the tail through another), no read
 *                                   failed, and the read after EOF returned 0 again
 *            pipe.cloexec           fcntl(F_GETFD) on qr and qw -- init's pipe2(O_CLOEXEC)
 *                                   ends, open in init when it forked this process -- is
 *                                   -EBADF for both, on fd 0 (no O_CLOEXEC) is not, and
 *                                   on the CLOEXEC_N fds of cloexec_fds(nonce) -- fds above
 *                                   2 init forked this process holding -- is -EBADF on each
 *                                   cloexec_marked one and exactly 0 on each other
 *   tiny   yields TINY_YIELDS times before its first read, so the producer meets a full
 *          ring and blocks; then reads fd 0 to EOF; TINY_COUNT bytes, byte i == i % 251:
 *            pipe.stress.tiny_writes
 * Evidence on '#' lines before each row; exit 0. A malformed argv prints
 * `not ok 0 - pipe.chain.checksum` and exits 2.
 */
#include "../sys.h"
#include "../producer/stream.h"

#define TINY_YIELDS 200

static int tiny(u64 index)
{
    struct line l = { 0, {0} };
    u8 buf[64];
    u64 count = 0, reads = 0, bad = ~0ULL;
    i64 r;
    for (int i = 0; i < TINY_YIELDS; i++) sys_sched_yield();
    while ((r = sys_read(0, buf, sizeof buf)) > 0) {
        reads++;
        for (i64 k = 0; k < r; k++, count++)
            if (bad == ~0ULL && buf[k] != tiny_byte(count)) bad = count;
    }
    line_note(&l, "pipe.stress.tiny_writes: ");
    line_dec(&l, count);
    line_str(&l, " bytes in ");
    line_dec(&l, reads);
    line_str(&l, " reads, last read ");
    line_sdec(&l, r);
    if (bad != ~0ULL) { line_str(&l, ", first wrong byte at "); line_dec(&l, bad); }
    line_end(&l);
    line_row(&l, r == 0 && count == TINY_COUNT && bad == ~0ULL, index, "pipe.stress.tiny_writes");
    return 0;
}

int main(int argc, char **argv, char **envp)
{
    struct line l = { 0, {0} };
    u64 index = 0, qr = 0, qw = 0;
    (void)envp;

    if (argc < 4 || !dec_parse(argv[2], &index)) {
        line_row(&l, 0, 0, "pipe.chain.checksum");
        return 2;
    }
    const char *nonce = argv[1];
    if (streq(argv[3], "tiny")) return tiny(index);
    int good_nonce = nonce_valid(nonce);

    u64 seed = stream_seed(nonce), len = stream_len(seed);
    u64 want = 0xcbf29ce484222325ULL, got = 0xcbf29ce484222325ULL;
    for (u64 i = 0; i < len; i++) want = fnv1a64_byte(want, stream_byte(seed, i));

    u8 buf[512];
    u64 total = 0, reads = 0, bad = ~0ULL;
    i64 r;
    while ((r = sys_read(0, buf, sizeof buf)) > 0) {
        reads++;
        for (i64 k = 0; k < r; k++, total++) {
            got = fnv1a64_byte(got, buf[k]);
            if (bad == ~0ULL && (total >= len || buf[k] != stream_byte(seed, total))) bad = total;
        }
    }
    i64 again = sys_read(0, buf, sizeof buf);

    line_note(&l, "pipe.chain.checksum: ");
    line_dec(&l, total);
    line_str(&l, " of ");
    line_dec(&l, len);
    line_str(&l, " bytes in ");
    line_dec(&l, reads);
    line_str(&l, " reads, fnv1a64 ");
    line_hex64(&l, got);
    line_str(&l, " want ");
    line_hex64(&l, want);
    line_end(&l);
    if (bad != ~0ULL) {
        line_note(&l, "pipe.chain.checksum: first byte that differs from the stream at offset ");
        line_dec(&l, bad);
        line_end(&l);
    }
    line_row(&l, good_nonce && total == len && got == want, index, "pipe.chain.checksum");

    line_note(&l, "pipe.eof.last_writer: read returned ");
    line_sdec(&l, r);
    line_str(&l, " after ");
    line_dec(&l, total);
    line_str(&l, " bytes (the producer closed fd 1 at ");
    line_dec(&l, len - STREAM_TAIL);
    line_str(&l, "); read after that ");
    line_sdec(&l, again);
    line_end(&l);
    line_row(&l, good_nonce && r == 0 && total == len && again == 0, index + 1, "pipe.eof.last_writer");

    int have_q = argc >= 6 && dec_parse(argv[4], &qr) && dec_parse(argv[5], &qw);
    i64 fr = have_q ? sys_fcntl((int)qr, F_GETFD, 0) : 0;
    i64 fw = have_q ? sys_fcntl((int)qw, F_GETFD, 0) : 0;
    i64 f0 = sys_fcntl(0, F_GETFD, 0);
    int want_keep = argc >= 7;
    int have_keep = want_keep && streq(argv[6], "keep") && good_nonce;
    int kfd[CLOEXEC_N];
    i64 fk[CLOEXEC_N];
    int kept = have_keep, open_marked = 0, lost = 0;
    if (have_keep) cloexec_fds(argv[1], kfd);
    for (int i = 0; i < CLOEXEC_N; i++) {
        fk[i] = have_keep ? sys_fcntl(kfd[i], F_GETFD, 0) : -EBADF;
        if (cloexec_marked(i)) open_marked += fk[i] != -EBADF;
        else lost += fk[i] != 0;
    }
    kept &= open_marked == 0 && lost == 0;
    line_note(&l, "pipe.cloexec: F_GETFD on fd ");
    line_dec(&l, qr);
    line_str(&l, " = ");
    line_sdec(&l, fr);
    line_str(&l, ", fd ");
    line_dec(&l, qw);
    line_str(&l, " = ");
    line_sdec(&l, fw);
    line_str(&l, ", fd 0 = ");
    line_sdec(&l, f0);
    line_end(&l);
    if (want_keep) {
        line_note(&l, "pipe.cloexec: fds init marked close-on-exec before the fork:");
        for (int i = 0; i < CLOEXEC_N; i++) {
            if (!cloexec_marked(i)) continue;
            line_putc(&l, ' ');
            line_dec(&l, have_keep ? (u64)kfd[i] : 0);
            line_putc(&l, '=');
            line_sdec(&l, fk[i]);
        }
        line_str(&l, "; fds it did not:");
        for (int i = 0; i < CLOEXEC_N; i++) {
            if (cloexec_marked(i)) continue;
            line_putc(&l, ' ');
            line_dec(&l, have_keep ? (u64)kfd[i] : 0);
            line_putc(&l, '=');
            line_sdec(&l, fk[i]);
        }
        line_str(&l, " (F_GETFD after the execve)");
        line_end(&l);
    }
    if (have_keep && open_marked) {
        line_note(&l, "pipe.cloexec: an fd marked close-on-exec when this program was forked is still open after its execve");
        line_end(&l);
    }
    if (have_keep && lost) {
        line_note(&l, "pipe.cloexec: an fd open without close-on-exec when this program was forked is not open and unmarked after its execve");
        line_end(&l);
    }
    line_row(&l, have_q && fr == -EBADF && fw == -EBADF && f0 >= 0 && (!want_keep || kept), index + 2, "pipe.cloexec");
    return 0;
}
