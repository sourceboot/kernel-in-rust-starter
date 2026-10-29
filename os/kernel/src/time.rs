//! Lab 06 — the monotonic clock. This file is yours.
//!
//! IRQ 0 counts `TICKS`; `now_ns` turns the count into nanoseconds since
//! `irq::init`, at tick resolution.
//!
//! Fill in the body; keep the signature and the static — later labs and the
//! course's tests use them.

use core::sync::atomic::AtomicU64;

/// PIT interrupts since `irq::init`.
pub static TICKS: AtomicU64 = AtomicU64::new(0);

/// Nanoseconds since the first tick; never goes back.
pub fn now_ns() -> u64 {
    todo!("lab 06 task 3 — now_ns: TICKS times the PIT period, in nanoseconds")
}
