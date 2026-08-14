//! QEMU's isa-debug-exit device: the run's verdict, reported from inside the
//! guest. Provided scaffold, with its OWN port write — the verdict channel
//! must not ride the learner's port helpers.
//!
//! Needs `-device isa-debug-exit,iobase=0xf4,iosize=0x04` on the QEMU line;
//! without the device the write is ignored and we halt instead (which is
//! what a plain `cargo xtask run` gets). QEMU transforms the written value
//! into host exit code `(value << 1) | 1`, so SUCCESS lands as 33 and
//! FAILURE as 35 — the harness decodes that.

const EXIT_PORT: u16 = 0xf4;

pub const SUCCESS: u8 = 0x10;
pub const FAILURE: u8 = 0x11;

pub fn exit(code: u8) -> ! {
    unsafe {
        core::arch::asm!("out dx, al", in("dx") EXIT_PORT, in("al") code,
            options(nomem, nostack, preserves_flags));
    }
    crate::hcf()
}
