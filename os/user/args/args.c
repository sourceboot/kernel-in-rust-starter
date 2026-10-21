/*
 * user/args/args.c -- lab 13: what execve put on the initial stack (CONTRACT §A.5).
 *
 * Protocol (CONTRACT §A.3, the lab-13 shape in README.md):
 *     argv = ["args", <nonce>, <index>, <derive(nonce, "exec.argc_argv") as 16 hex>]
 *     envp = [..., "SBOOT_NONCE=<nonce>", ...]
 * It prints every argv and envp string on its own '#' line, then five rows,
 * numbered index .. index+4 in this order:
 *     exec.argc_argv             argc == 4, argv[1] is 32 hex, argv[3] == derive(argv[1], id) in hex
 *     exec.envp                  some envp entry is exactly "SBOOT_NONCE=" + argv[1]
 *     exec.rsp.aligned           the stack pointer at _start was 16-aligned
 *     exec.auxv.phdr_pagesz      past envp's NULL: AT_PAGESZ == 4096, AT_PHENT == 56, and the
 *                                AT_PHNUM headers at AT_PHDR include this program's own
 *                                executable PT_LOAD at 0x1000000
 *     exec.auxv.random.readable  AT_RANDOM is present and its 16 bytes read (printed in hex)
 * The two auxv rows come last because they dereference what the kernel handed
 * over: a bad AT_PHDR or AT_RANDOM pointer ends the process there, and the rows
 * already printed stand. Exit 0. A malformed argv[2] prints `not ok 0 -
 * exec.argc_argv` and exits 2.
 *
 * The entry stack pointer without touching crt0.c (whose bytes `hi` shares):
 * _start passes its entry rsp to _start_c, which takes argv = rsp + 8, so
 * rsp at entry = (u64)argv - 8.
 */
#include "../sys.h"

#define AT_NULL    0
#define AT_PHDR    3
#define AT_PHENT   4
#define AT_PHNUM   5
#define AT_PAGESZ  6
#define AT_RANDOM 25
#define PT_LOAD    1
#define PF_X       1

struct phdr { u32 type, flags; u64 offset, vaddr, paddr, filesz, memsz, align; };

static int has_prefix_eq(const char *s, const char *prefix, const char *rest)
{
    while (*prefix) if (*s++ != *prefix++) return 0;
    return streq(s, rest);
}

/* Prints a string as a '#' line: "# <label>[<i>]=<s>" (long strings split across writes). */
static void note_str(struct line *l, const char *label, u64 i, const char *s)
{
    line_note(l, label);
    line_putc(l, '[');
    line_dec(l, i);
    line_str(l, "]=");
    line_str(l, s);
    line_end(l);
}

int main(int argc, char **argv, char **envp)
{
    struct line l = { 0, {0} };
    u64 index = 0;

    if (argc < 3 || !dec_parse(argv[2], &index)) {
        line_row(&l, 0, 0, "exec.argc_argv");
        return 2;
    }
    const char *nonce = argv[1];
    int good_nonce = nonce_valid(nonce);

    line_note(&l, "argc=");
    line_dec(&l, (u64)argc);
    line_end(&l);
    for (int i = 0; i < argc; i++) note_str(&l, "argv", (u64)i, argv[i]);
    u64 nenv = 0;
    for (char **e = envp; *e; e++) note_str(&l, "envp", nenv++, *e);

    /* exec.argc_argv */
    char want[17];
    u64 d = derive(nonce, "exec.argc_argv");
    for (int i = 0; i < 16; i++) want[i] = "0123456789abcdef"[(d >> (60 - 4 * i)) & 0xf];
    want[16] = 0;
    int argv_ok = good_nonce && argc == 4 && streq(argv[3], want);
    line_note(&l, "exec.argc_argv: want argc=4 and argv[3]=");
    line_str(&l, good_nonce ? want : "(no nonce)");
    line_end(&l);
    line_row(&l, argv_ok, index, "exec.argc_argv");

    /* exec.envp */
    int env_ok = 0;
    for (char **e = envp; *e; e++)
        if (has_prefix_eq(*e, "SBOOT_NONCE=", nonce)) env_ok = 1;
    line_row(&l, good_nonce && env_ok, index + 1, "exec.envp");

    /* exec.rsp.aligned */
    u64 rsp = (u64)argv - 8;
    line_note(&l, "rsp at _start=");
    line_ptr(&l, rsp);
    line_end(&l);
    line_row(&l, (rsp & 15) == 0, index + 2, "exec.rsp.aligned");

    /* The auxv: past envp's NULL, (type, value) pairs to AT_NULL. */
    u64 *aux = (u64 *)(envp + nenv + 1);
    u64 phdr = 0, phent = 0, phnum = 0, pagesz = 0, rnd = 0;
    for (u64 *a = aux; a[0] != AT_NULL; a += 2) {
        line_note(&l, "auxv ");
        line_dec(&l, a[0]);
        line_str(&l, " = ");
        line_ptr(&l, a[1]);
        line_end(&l);
        if (a[0] == AT_PHDR)   phdr = a[1];
        if (a[0] == AT_PHENT)  phent = a[1];
        if (a[0] == AT_PHNUM)  phnum = a[1];
        if (a[0] == AT_PAGESZ) pagesz = a[1];
        if (a[0] == AT_RANDOM) rnd = a[1];
    }

    /* exec.auxv.phdr_pagesz */
    int own_load = 0;
    if (phdr && phent == sizeof(struct phdr) && phnum > 0 && phnum < 64) {
        for (u64 i = 0; i < phnum; i++) {
            const struct phdr *p = (const struct phdr *)(phdr + i * phent);
            if (p->type == PT_LOAD && p->vaddr == 0x1000000 && (p->flags & PF_X)) own_load = 1;
        }
    }
    line_note(&l, "exec.auxv.phdr_pagesz: AT_PAGESZ=");
    line_dec(&l, pagesz);
    line_str(&l, " AT_PHENT=");
    line_dec(&l, phent);
    line_str(&l, " AT_PHNUM=");
    line_dec(&l, phnum);
    line_str(&l, own_load ? " own PT_LOAD at 0x1000000 found" : " own PT_LOAD at 0x1000000 not found");
    line_end(&l);
    line_row(&l, pagesz == 4096 && own_load, index + 3, "exec.auxv.phdr_pagesz");

    /* exec.auxv.random.readable */
    if (rnd) {
        line_note(&l, "AT_RANDOM bytes=");
        for (int i = 0; i < 16; i++) {
            u8 b = ((const volatile u8 *)rnd)[i];
            line_putc(&l, "0123456789abcdef"[b >> 4]);
            line_putc(&l, "0123456789abcdef"[b & 15]);
        }
        line_end(&l);
    } else {
        line_note(&l, "no AT_RANDOM entry in the auxv");
        line_end(&l);
    }
    line_row(&l, rnd != 0, index + 4, "exec.auxv.random.readable");
    return 0;
}
