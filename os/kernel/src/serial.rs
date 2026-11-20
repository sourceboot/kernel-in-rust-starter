//! Lab 02 — the report channel. This file is yours: the 16550 UART driver that
//! every test in this course reports through. The course's tests call
//! `write_str`; get these right and they light up green. See this lab's page.
//!
//! Fill in the bodies. The signatures are the contract the rest of the kernel
//! depends on — keep them. `todo!()` compiles but panics, so the suite prints nothing until
//! you implement write_byte/write_str: `sboot hint` will point you at
//! `serial.no_output` until then.

/// COM1 I/O base port.
pub const COM1: u16 = 0x3F8;

/// Write one byte to an I/O port (`out dx, al`).
pub fn out8(port: u16, val: u8) {
    let _ = (port, val);
    todo!("lab 02 task 1 — out8: write `val` to I/O `port` via inline asm")
}

/// Read one byte from an I/O port (`in al, dx`).
pub fn in8(port: u16) -> u8 {
    let _ = port;
    todo!("lab 02 task 1 — in8: read a byte from I/O `port` via inline asm")
}

/// Initialise COM1: 8N1, FIFOs on, interrupts off.
pub fn init() {
    todo!("lab 02 task 2 — init: IER off, divisor through DLAB, 8N1, FIFOs on")
}

/// Send one byte — poll LSR bit 5 (THRE) until set, then write the data register.
pub fn write_byte(b: u8) {
    let _ = b;
    todo!("lab 02 task 3 — write_byte: poll THRE, then out8(COM1, b)")
}

/// Send a string, translating `\n` to `\r\n`.
pub fn write_str(s: &str) {
    let _ = s;
    todo!("lab 02 task 4 — write_str: write_byte each byte; \\n -> \\r\\n")
}
