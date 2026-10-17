//! Lab 03 — panic and the register dump. This file is yours.
//!
//! A kernel panic prints one line, `panic: <file>:<line>: <message>`, then the
//! machine's registers one per line as `  <reg>=0x<016x>` — the sixteen
//! general registers, `rip`, `rflags`, `cr2`, `cr3` — then halts with
//! interrupts off. `bug_on!` is the kernel's assertion; the crate's
//! `#[panic_handler]` (lib.rs, course-owned) routes every Rust panic through
//! `report` too, so both print the same shape. `kprintln!` (lib.rs) formats
//! onto the wire through your `serial::write_str`.
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests call them.

use core::fmt;

/// `bug_on!(cond, "fmt", args…)`: if `cond` holds, report a kernel panic at
/// this file and line and halt.
#[macro_export]
macro_rules! bug_on {
    ($cond:expr, $($arg:tt)*) => {{
        let _ = &$cond;
        let _ = format_args!($($arg)*);
        $crate::panic::bug_on_unwritten()
    }};
}

/// The stub's stand-in for `bug_on!`'s body; delete it once `bug_on!` reports.
#[doc(hidden)]
pub fn bug_on_unwritten() {
    todo!("lab 03 step 3 — bug_on!: when the condition holds, report at this file and line")
}

/// Print the panic line and the register dump, then halt.
pub fn report(file: &str, line: u32, msg: fmt::Arguments) -> ! {
    let _ = (file, line, msg);
    todo!("lab 03 step 2 — report: mask interrupts, print the panic line, dump_state, halt")
}

/// Print every register in the dump's format, the caller's values.
pub fn dump_state() {
    todo!("lab 03 step 1 — dump_state: capture the registers, print one per line")
}

/// Stop this CPU for good: interrupts off, then `hlt` forever.
pub fn halt() -> ! {
    todo!("lab 03 step 2 — halt: cli, then a hlt loop")
}
