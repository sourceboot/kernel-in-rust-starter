/*
 * user/forker/forker.h -- the lab 14 values forker and init both derive (CONTRACT §A.2 derive).
 * One copy, so the two programs cannot disagree. The ranges do not overlap: an
 * orphan init reaps with a fork.wait.codes code is a child forker failed to
 * reap, never the grandchild.
 */
#ifndef SBOOT_USER_FORKER_H
#define SBOOT_USER_FORKER_H
#include "../sys.h"

/* fork.wait.codes: N = derive % 4 + 2 children (2..5); child i exits with 1..100. */
static inline u64 wait_codes_n(const char *nonce) { return derive(nonce, "fork.wait.codes") % 4 + 2; }
static inline int wait_code(const char *nonce, u64 i)
{
    return 1 + (int)(((derive(nonce, "fork.wait.codes") >> (8 * (i + 1))) & 0xff) % 100);
}
/* fork.orphan.to_init: the grandchild exits with 150..249 once it is init's child. */
static inline int orphan_code(const char *nonce) { return 150 + (int)(derive(nonce, "fork.orphan.to_init") % 100); }

#endif
