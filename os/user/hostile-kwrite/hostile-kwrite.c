/*
 * user/hostile-kwrite/hostile-kwrite.c -- lab 12: a program that writes a kernel address.
 *
 * Protocol (CONTRACT §A.3): argv = ["hostile-kwrite", <nonce>, <index>].
 * It names what it is about to do on one '#' line, then stores one byte at
 * KADDR, an address in the kernel half (the HHDM's second page: present, U=0).
 * A correct kernel kills it there with §A.6's line,
 *     kill: pid=<n> reason=pf-kernel-addr rip=0x<hex> addr=0xffff800000001000
 * and keeps serving; the row kill.kwrite is the vehicle's to print (K), from
 * that line. This program prints nothing after the store. If the store
 * returns, it says so and prints `not ok <index> - kill.kwrite`, which the
 * judge's duplicate-id rule (§A.2 (d)) turns into a failed row whatever the
 * vehicle prints.
 */
#include "../sys.h"

#define ID    "kill.kwrite"
#define KADDR 0xffff800000001000ULL   /* 0xffff_8000_0000_0000 + 0x1000 */

int main(int argc, char **argv, char **envp)
{
    struct line l = { 0, {0} };
    u64 index = 0;
    (void)envp;

    if (argc < 3 || !dec_parse(argv[2], &index)) {
        line_row(&l, 0, 0, ID);
        return 2;
    }
    line_note(&l, "hostile-kwrite: writing one byte to ");
    line_ptr(&l, KADDR);
    line_end(&l);

    *(volatile u8 *)KADDR = 0x5b;

    line_note(&l, "hostile-kwrite: the write to a kernel address returned");
    line_end(&l);
    line_row(&l, 0, index, ID);
    return 1;
}
