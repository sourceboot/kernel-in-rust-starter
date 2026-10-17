/*
 * user/crt0.c -- the entry point every pre-23 course program links with.
 *
 * At entry (CONTRACT §A.5, the psABI): rsp points at argc, then argv[0..argc],
 * a NULL, envp[...], a NULL, the auxv. rsp % 16 == 0 at entry, but this file
 * does not rely on it: it re-aligns before the call, so a kernel that gets
 * the alignment wrong still reaches main() (a later lab's row checks the
 * alignment itself; this program is not that test).
 *
 * _start clears rbp (the end of a frame-pointer walk), passes the stack
 * pointer to _start_c, which calls main(argc, argv, envp) and then exit()s
 * with its return value. No SSE, no TLS, no libc.
 */
#include "sys.h"

int main(int argc, char **argv, char **envp);

__attribute__((noreturn, used)) void _start_c(long *sp)
{
    int argc = (int)sp[0];
    char **argv = (char **)(sp + 1);
    char **envp = argv + argc + 1;
    sys_exit(main(argc, argv, envp));
}

__asm__(
    ".text\n"
    ".globl _start\n"
    ".type _start, @function\n"
    "_start:\n"
    "    xor  %ebp, %ebp\n"        /* outermost frame */
    "    mov  %rsp, %rdi\n"        /* sp -> argc */
    "    and  $-16, %rsp\n"        /* 16-byte aligned at the call (psABI) */
    "    call _start_c\n"
    "    ud2\n"                    /* _start_c never returns */
    ".size _start, . - _start\n"
);

/*
 * The four functions a freestanding C compiler may emit calls to on its own
 * (struct copies, zeroing). -fno-builtin keeps these loops from being turned
 * back into calls to themselves.
 */
void *memset(void *d, int c, usize n)
{
    u8 *p = d;
    while (n--) *p++ = (u8)c;
    return d;
}
void *memcpy(void *d, const void *s, usize n)
{
    u8 *p = d;
    const u8 *q = s;
    while (n--) *p++ = *q++;
    return d;
}
void *memmove(void *d, const void *s, usize n)
{
    u8 *p = d;
    const u8 *q = s;
    if (p < q) { while (n--) *p++ = *q++; }
    else       { p += n; q += n; while (n--) *--p = *--q; }
    return d;
}
int memcmp(const void *a, const void *b, usize n)
{
    const u8 *p = a, *q = b;
    for (; n; n--, p++, q++)
        if (*p != *q) return *p - *q;
    return 0;
}
