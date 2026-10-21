//! Lab 06 — the keyboard ring. This file is yours.
//!
//! IRQ 1 reads one set-1 scancode from port 0x60 and pushes it here; the
//! kernel pops scancodes in order. One producer (the interrupt handler) and
//! one consumer (the kernel) share the ring, and neither side may take a
//! lock an interrupt could be waiting on.
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests call them.

/// Producer side (IRQ 1). Lab 23: after storing, wake whoever sleeps on
/// `chan()`.
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

/// Lab 23: the wait channel a reader of the keyboard sleeps on. `push` wakes
/// it (`sched::wakeup`) after each scancode it stores.
pub fn chan() -> usize {
    todo!("lab 23 step 2 — chan: one address this file owns; push wakes it")
}

/// Lab 23: the byte one scancode types with the shift and control keys held
/// now. Each call is the next scancode in order: a shift key's make code
/// (0x2a, 0x36) sets what its break code (0xaa, 0xb6) clears, and the same for
/// control (0x1d, 0x9d). Shifted, a key types the US layout's upper symbol;
/// with control, a letter types `c & 0x1f` (ctrl+d is 0x04).
pub fn translate(sc: u8) -> Option<u8> {
    let _ = sc;
    todo!("lab 23 step 1 — translate: follow shift and control; the shifted symbols, ctrl+letter's control code")
}
