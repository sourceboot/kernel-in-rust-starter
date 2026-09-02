//! The scaffold's boot handshake — lab 00's evidence channel. Provided.
//!
//! Two fixed lines on COM1, printed by `kernel_main` before your code runs:
//! one when the provided kernel comes up, one when the bootloader's framebuffer
//! handoff checks out. Lab 00 ("Welcome to the machine") grades by looking for
//! them: if they arrive, your toolchain built a bootable ISO and QEMU ran it —
//! the whole build→boot loop proven before you write a line of code.
//!
//! Like exit.rs, this has its OWN port I/O and does not touch serial.rs. That
//! file is yours — lab 02 has you write the real 16550 driver there, and the
//! handshake must work before that driver exists and keep working however you
//! change it. Leave these lines in place: `sboot test 00-welcome` is how any
//! machine (including a new one, months from now) proves itself.

const COM1: u16 = 0x3F8;

/// Line Status Register offset; bit 5 (THRE) = transmit holding register empty.
const LSR: u16 = COM1 + 5;

fn out8(port: u16, val: u8) {
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") val,
            options(nomem, nostack, preserves_flags));
    }
}

fn in8(port: u16) -> u8 {
    let v: u8;
    unsafe {
        core::arch::asm!("in al, dx", out("al") v, in("dx") port,
            options(nomem, nostack, preserves_flags));
    }
    v
}

fn put(b: u8) {
    // Poll THRE, but bounded: on QEMU (this course's one target) the UART is
    // ready almost immediately, and a handshake must never be able to hang a
    // boot on hardware that answers strangely.
    let mut spins: u32 = 0;
    while in8(LSR) & 0x20 == 0 {
        spins += 1;
        if spins > 100_000 {
            return;
        }
    }
    out8(COM1, b);
}

/// Print one handshake line, `\r\n`-terminated. No UART init on purpose: QEMU's
/// COM1 transmits without it, and initialising the device here would pre-chew
/// the setup lab 02 grades you on building.
pub fn line(s: &str) {
    for &b in s.as_bytes() {
        put(b);
    }
    put(b'\r');
    put(b'\n');
}
