/* Limine executable layout: higher-half, requests kept in their own PT_LOAD.
   Adapted from the official Limine C template. */
OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)

PHDRS
{
    limine_requests PT_LOAD;
    text            PT_LOAD;
    rodata          PT_LOAD;
    data            PT_LOAD;
}

SECTIONS
{
    . = 0xffffffff80000000;
    __kernel_start = .;

    .limine_requests : {
        KEEP(*(.limine_requests_start))
        KEEP(*(.limine_requests))
        KEEP(*(.limine_requests_end))
    } :limine_requests

    . = ALIGN(CONSTANT(MAXPAGESIZE));
    /* Lab 03's `panic.dump.rip` (2026-10-18): the code of the learner's
       `kernel::panic` module (report, dump_state and whatever helpers they
       write there) is placed first, and the rest of the text starts at the
       next 64 KiB boundary. So a `rip` taken anywhere in the panic code reads
       0xffffffff8000xxxx on every build, and one taken anywhere else (another
       function, a stack slot, zero) does not. The pattern matches both
       manglings (v0 `_RNvNtCs<hash>_6kernel5panic…`, legacy `_ZN6kernel5panic…`). */
    .text : {
        *(.text.*6kernel5panic*)
        __panic_text_end = .;
        . = ALIGN(0x10000);
        __text_rest = .;
        *(.text .text.*)
    } :text
    /* The window above is only a window while the panic code fits in it and the
       rest of the text starts at 0xffffffff80010000 (a panic module over ~60 KiB,
       or a MAXPAGESIZE that moves .text, would move every address and refuse every
       learner): fail the build by name instead (the TPM's condition, 2026-10-18). */
    ASSERT(__text_rest == 0xffffffff80010000, "kernel-in-rust: the text after kernel::panic no longer starts at 0xffffffff80010000 - lab 03 panic.dump.rip window moved (boot/linker.ld)")
    ASSERT(__panic_text_end <= 0xffffffff80010000, "kernel-in-rust: kernel::panic code is over its 60 KiB window (boot/linker.ld, lab 03 panic.dump.rip)")

    . = ALIGN(CONSTANT(MAXPAGESIZE));
    .rodata : { *(.rodata .rodata.*) } :rodata

    /* Lab 02's test registry: ktest! statics land here and the runner walks
       start..end. KEEP because nothing references them by symbol name. */
    .ktest_array : {
        . = ALIGN(8);
        __ktest_array_start = .;
        KEEP(*(.ktest_array))
        __ktest_array_end = .;
    } :rodata

    . = ALIGN(CONSTANT(MAXPAGESIZE));
    .data : { *(.data .data.*) } :data
    .bss : { *(.bss .bss.*) *(COMMON) } :data
    . = ALIGN(4096);
    __kernel_end = .;

    /DISCARD/ : { *(.eh_frame*) *(.note .note.*) }
}
