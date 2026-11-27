//! The ACPI PM timer's elapsed counts between two reads, for every test
//! binary that keeps time on it (`#[path]`-included, as `shim.rs` is).
//!
//! The PM timer counts at 3,579,545 Hz in 24 bits, so it wraps every 2^24
//! counts, 4687 ms. A read lower than the one before it is either a wrap (the
//! count went past 0xff_ffff and on from 0) or a read the host returned
//! behind the one before. Taken as a wrap, a read that came back even one
//! count behind adds a whole 4687 ms that never passed. A read that is behind
//! the one before by at most `PM_BACK_MAX` is read as the same instant: no
//! time passed. Further behind than that, it is a wrap, as before.
//!
//! `PM_BACK_MAX` is 100 ms of counts. The host can hold a guest's read back
//! by milliseconds, never by seconds, so 100 ms is far more than any read
//! comes back behind. And a real wait between two reads only falls in that
//! band when it lasted between 4587 and 4687 ms, beside the 4687 ms and more
//! that the 24-bit count already cannot tell from its remainder: a clock must
//! be read more often than every 4.5 s.

// A binary that measures only spans uses `pm_ahead` alone.
#![allow(dead_code)]

/// The furthest a PM timer read may come behind the one before it and still
/// be read as the same instant, in counts: 100 ms at 3,579,545 Hz.
pub const PM_BACK_MAX: u32 = 357_954;

/// PM counts from the read `from` to the later read `to` (both 24 bits): 0
/// when `to` came back behind `from` by at most `PM_BACK_MAX`.
pub fn pm_ahead(from: u32, to: u32) -> u32 {
    let d = to.wrapping_sub(from) & 0xff_ffff;
    if d != 0 && 0x100_0000 - d <= PM_BACK_MAX {
        0
    } else {
        d
    }
}

/// A clock's step to the read `now` from its last one, `*last`: the counts
/// `pm_ahead` gives, and `*last` moves to `now` only when `now` is ahead, so
/// a read that came back behind never counts the same counts twice.
pub fn pm_step(last: &mut u32, now: u32) -> u32 {
    let d = pm_ahead(*last, now);
    if d != 0 {
        *last = now;
    }
    d
}
