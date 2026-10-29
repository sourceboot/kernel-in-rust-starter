//! Lab 06 — the 8259 PIC and the 8254 PIT. This file is yours.
//!
//! The two PICs are remapped so IRQ 0–15 arrive on vectors 32–47; every line
//! is masked except IRQ 0 (the PIT) and IRQ 1 (the keyboard); the PIT's
//! channel 0 runs at 1000 Hz. A handler acknowledges its IRQ with an EOI
//! before it returns, or the PIC never delivers that line again.
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests call them.

/// The vector IRQ 0 arrives on; IRQ n arrives on `IRQ_BASE + n`.
pub const IRQ_BASE: u8 = 32;
/// The PIT's input clock, in Hz (8254 datasheet).
pub const PIT_HZ: u64 = 1_193_182;
/// The reload value for 1000 Hz: 1193182 / 1000, rounded down.
pub const PIT_DIVISOR: u16 = 1193;

/// Remap and mask the PICs, start the PIT, install the two handlers, `sti`.
pub fn init() {
    todo!("lab 06 task 1 — init: remap the PICs to 32–47, unmask IRQ 0 and 1, PIT at 1000 Hz, handlers, sti")
}

/// Acknowledge `irq` (0–15) at the PIC that raised it.
pub fn eoi(irq: u8) {
    let _ = irq;
    todo!("lab 06 task 2 — eoi: end-of-interrupt to the right PIC (both, for 8–15)")
}
