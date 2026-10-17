//! Lab 06 — the keyboard ring. This file is yours.
//!
//! IRQ 1 reads one set-1 scancode from port 0x60 and pushes it here; the
//! kernel pops scancodes in order. One producer (the interrupt handler) and
//! one consumer (the kernel) share the ring, and neither side may take a
//! lock an interrupt could be waiting on.
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests call them.

/// Producer side (IRQ 1).
pub fn push(sc: u8) {
    let _ = sc;
    todo!("lab 06 step 4 — push: store the scancode if the ring has room")
}

/// The oldest scancode not yet popped.
pub fn pop() -> Option<u8> {
    todo!("lab 06 step 4 — pop: the oldest scancode, or None")
}

/// The ASCII byte a set-1 make code types, if any; a break code types nothing.
pub fn to_ascii(sc: u8) -> Option<u8> {
    let _ = sc;
    todo!("lab 06 step 5 — to_ascii: the set-1 table")
}
