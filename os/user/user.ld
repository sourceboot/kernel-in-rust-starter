/*
 * user/user.ld -- the link for every pre-23 course program (CONTRACT §A.5, §A.8).
 *
 * ET_EXEC at 0x1000000, entry _start. The ELF and program headers ride the
 * first page of the text segment, so the first PT_LOAD starts at file offset
 * 0 = vaddr 0x1000000: a program with no writable data is ONE PT_LOAD whose
 * file bytes equal its memory image, and labs 09-12's vehicle can place the
 * file flat at 0x1000000 and enter at e_entry. Writable data, when a later
 * program has any, gets its own RW segment on the next page (build.sh reports
 * which programs stay flat). Unwind tables, notes and the compiler's comment
 * are discarded: nothing unwinds, and they would vary the bytes.
 */
ENTRY(_start)

PHDRS
{
    text  PT_LOAD FILEHDR PHDRS FLAGS(5);   /* R E */
    data  PT_LOAD FLAGS(6);                 /* RW  */
    stack PT_GNU_STACK FLAGS(6);            /* RW, not executable; memsz 0 = the kernel's default */
}

SECTIONS
{
    . = 0x1000000 + SIZEOF_HEADERS;
    .text   : { *(.text .text.*) } :text
    .rodata : { *(.rodata .rodata.*) } :text

    . = ALIGN(0x1000);
    .data   : { *(.data .data.*) } :data
    .bss    : { *(.bss .bss.*) *(COMMON) } :data

    /DISCARD/ : { *(.eh_frame*) *(.note*) *(.comment) }
}
