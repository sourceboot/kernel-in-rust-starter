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
/// `chan()`. Lab 26: hand the scancode to `devfs::feed` first; when that
/// returns `true` the input device keeps it.
pub fn push(sc: u8) {
    let _ = sc;
    todo!("lab 06 task 4 — push: store the scancode if the ring has room")
}

/// The oldest scancode not yet popped.
pub fn pop() -> Option<u8> {
    todo!("lab 06 task 4 — pop: the oldest scancode, or None")
}

/// The ASCII byte a set-1 make code types, if any; a break code types nothing.
pub fn to_ascii(sc: u8) -> Option<u8> {
    let _ = sc;
    todo!("lab 06 task 5 — to_ascii: the set-1 table")
}

/// Lab 23: the wait channel a reader of the keyboard sleeps on. `push` wakes
/// it (`sched::wakeup`) after each scancode it stores.
pub fn chan() -> usize {
    todo!("lab 23 task 2 — chan: one address this file owns; push wakes it")
}

/// Lab 23: the byte one scancode types with the shift and control keys held
/// now. Each call is the next scancode in order: a shift key's make code
/// (0x2a, 0x36) sets what its break code (0xaa, 0xb6) clears, and the same for
/// control (0x1d, 0x9d). Shifted, a key types the US layout's upper symbol;
/// with control, a letter types `c & 0x1f` (ctrl+d is 0x04).
pub fn translate(sc: u8) -> Option<u8> {
    let _ = sc;
    todo!("lab 23 task 1 — translate: follow shift and control; the shifted symbols, ctrl+letter's control code")
}

/// Lab 26: the `KEY_*` number (Linux's input-event-codes.h) of a set-1
/// scancode, make or break alike; `None` for a scancode that names no key
/// and for the 0xe0 prefix. Lab 26 also grows `push`: every scancode goes to
/// `devfs::feed` first, and a key the input device keeps (a program holds
/// it grabbed) goes no further.
pub fn to_keycode(sc: u8) -> Option<u16> {
    let _ = sc;
    todo!("lab 26 step 1 — to_keycode: a set-1 scancode to Linux's KEY_* number")
}

/// Lab 33 step 4: the bytes one scancode sends to a raw-mode reader, as the
/// Linux console sends them, into `out`; the count (0 for a prefix, a break
/// code, a modifier). Esc 0x1b, Enter CR, Backspace DEL; after the 0xe0
/// prefix up, down, right, left `ESC [ A`/`B`/`C`/`D`, Home `ESC [ 1 ~`,
/// End `ESC [ 4 ~`; any other key what `translate` types (ctrl+c 0x03).
pub fn vt100(sc: u8, out: &mut [u8; 4]) -> usize {
    let _ = (sc, out);
    todo!("lab 33 step 4 — vt100: what the Linux console sends for this key: ESC, CR, DEL, ESC [ A..D, ESC [ 1 ~, ESC [ 4 ~")
}
