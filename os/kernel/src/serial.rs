//! Lab 02 — the report channel. This file is yours: the 16550 UART driver that
//! every test in this course reports through. The provided cockpit (ktap.rs)
//! calls write_str; get these right and it lights up green. See project.md.
//!
//! Fill in the bodies. The signatures are the contract the cockpit depends on —
//! keep them. `todo!()` compiles but panics, so the suite prints nothing until
//! you implement write_byte/write_str: `sboot hint` will point you at
//! `serial.no_output` until then.

/// COM1 I/O base port.
pub const COM1: u16 = 0x3F8;

/// Write one byte to an I/O port (`out dx, al`).
pub fn out8(port: u16, val: u8) {
    let _ = (port, val);
    todo!("out8: write `val` to I/O `port` via inline asm")
}

/// Read one byte from an I/O port (`in al, dx`).
pub fn in8(port: u16) -> u8 {
    let _ = port;
    todo!("in8: read a byte from I/O `port` via inline asm")
}

/// Initialise COM1: 8N1, FIFOs on, interrupts off.
pub fn init() {
    todo!("init: configure the 16550 at COM1 (line control, FIFO, IER)")
}

/// Send one byte — poll LSR bit 5 (THRE) until set, then write the data register.
pub fn write_byte(b: u8) {
    let _ = b;
    todo!("write_byte: poll THRE, then out8(COM1, b)")
}

/// Send a string, translating `\n` to `\r\n`.
pub fn write_str(s: &str) {
    let _ = s;
    todo!("write_str: write_byte each byte; \\n -> \\r\\n")
}
