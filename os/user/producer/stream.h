/*
 * user/producer/stream.h -- lab 15's nonce-seeded stream, which producer writes and
 * consumer checks (CONTRACT §B.2-15 pipe.chain.checksum). One copy for both.
 *
 *   seed      = derive(nonce, "pipe.chain.checksum")
 *   length    = 4096 + seed % 8192                       (4096..12287 bytes: always > PIPE_BUF)
 *   byte i    = bits 8*(i%8) .. 8*(i%8)+7 of splitmix64(seed ^ (i / 8))
 *   checksum  = fnv1a64 over the length bytes            (offset 0xcbf29ce484222325, prime 0x100000001b3)
 *   chunk j   = 1 + splitmix64(seed + 0x100000000 + j) % 1024 bytes per write(1) call
 *   tail      = the last STREAM_TAIL bytes, written through a dup of fd 1 after fd 1 is closed
 *
 * The tiny stream (pipe.stress.tiny_writes): TINY_COUNT one-byte writes, byte i = i % 251.
 *
 * cloexec_fds (pipe.cloexec, fix round 3, 2026-10-19): the CLOEXEC_N fds above 2 that init
 * hands the chain consumer, half of them marked close-on-exec. Both programs derive the
 * numbers from the nonce, so none reaches the kernel in argv: fds 11..22 (above init's six
 * pipe ends) shuffled by derive(nonce, "pipe.cloexec.fds"), the first CLOEXEC_N taken. Slot
 * i's kind is fixed (init.c's cloexec_make); cloexec_marked(i) says whether slot i carries the
 * mark when the consumer is forked. The marked and the unmarked each include an fd that shares
 * its open file with an fd of the other kind and an fd that shares it with nothing, so neither
 * an fd's number nor its sharing tells them apart: only the close-on-exec bit does.
 */
#ifndef SBOOT_USER_STREAM_H
#define SBOOT_USER_STREAM_H
#include "../sys.h"

#define STREAM_TAIL 64
#define TINY_COUNT  10000

static inline u64 stream_seed(const char *nonce)  { return derive(nonce, "pipe.chain.checksum"); }
static inline u64 stream_len(u64 seed)            { return 4096 + seed % 8192; }
static inline u8  stream_byte(u64 seed, u64 i)    { return (u8)(splitmix64(seed ^ (i >> 3)) >> (8 * (i & 7))); }
static inline u64 stream_chunk(u64 seed, u64 j)   { return 1 + splitmix64(seed + 0x100000000ULL + j) % 1024; }
static inline u64 fnv1a64_byte(u64 h, u8 b)       { return (h ^ b) * 0x100000001b3ULL; }
static inline u8  tiny_byte(u64 i)                { return (u8)(i % 251); }
#define CLOEXEC_N 8
static inline int cloexec_marked(int i)           { return (0xa6 >> i) & 1; }
static inline void cloexec_fds(const char *nonce, int fd[CLOEXEC_N])
{
    int fds[12];
    for (int i = 0; i < 12; i++) fds[i] = 11 + i;
    u64 b = derive(nonce, "pipe.cloexec.fds");
    for (int i = 11; i > 0; i--) {
        b = splitmix64(b);
        int j = (int)(b % (u64)(i + 1));
        int t = fds[i]; fds[i] = fds[j]; fds[j] = t;
    }
    for (int i = 0; i < CLOEXEC_N; i++) fd[i] = fds[i];
}

#endif
