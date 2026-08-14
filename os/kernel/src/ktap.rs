//! Lab 02's provided cockpit: test registry, KTAP runner, expectation
//! recorder. Provided scaffold — the learner reads and extends it, never
//! retypes it: the platform's tests execute inside this code, so it stays
//! platform-authored. It touches learner code through exactly one contract,
//! `serial::write_str` — their UART is the physical layer of every report.
//!
//! The registry is a hand-rolled link-section array — the same technique the
//! boot shim uses for Limine requests — so the scaffold stays on STABLE Rust
//! (no `custom_test_frameworks`). `ktest!` places one `KtestDesc` in
//! `.ktest_array`; boot/linker.ld KEEPs that section and brackets it with
//! `__ktest_array_{start,end}`.

use core::fmt::{self, Write};

#[repr(C)]
pub struct KtestDesc {
    pub id: &'static str,
    pub run: fn(&mut TestCtx),
}

/// Register a test with the runner:
///
/// ```ignore
/// ktest!("serial.scratch", fn scratch(t: &mut TestCtx) { ... });
/// ```
///
/// The `const _` wrapper scopes the registration static, so the only name a
/// test adds to its module is the function's own.
#[macro_export]
macro_rules! ktest {
    ($id:literal, fn $name:ident($ctx:ident: &mut TestCtx) $body:block) => {
        fn $name($ctx: &mut $crate::ktap::TestCtx) $body
        const _: () = {
            #[used]
            #[link_section = ".ktest_array"]
            static REG: $crate::ktap::KtestDesc =
                $crate::ktap::KtestDesc { id: $id, run: $name };
        };
    };
}

// Defined by boot/linker.ld, delimiting .ktest_array. Typed u8 because only
// their addresses mean anything.
extern "C" {
    static __ktest_array_start: u8;
    static __ktest_array_end: u8;
}

/// Every registered test, in link order. A binary registering none gets an
/// empty slice (start == end), and the runner emits a truthful `1..0`.
pub fn tests() -> &'static [KtestDesc] {
    unsafe {
        let start = core::ptr::addr_of!(__ktest_array_start) as *const KtestDesc;
        let end = core::ptr::addr_of!(__ktest_array_end) as *const KtestDesc;
        let n = (end as usize - start as usize) / core::mem::size_of::<KtestDesc>();
        core::slice::from_raw_parts(start, n)
    }
}

// The one seam to learner code: serial::write_str, wrapped for core::fmt.
struct Out;

impl Write for Out {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        crate::serial::write_str(s);
        Ok(())
    }
}

/// Per-test expectation recorder. Expectations record-and-continue (the
/// KUnit model): a failed check prints its diagnostic and the test keeps
/// running, so one bad register read reports every consequence, not just the
/// first. Panicking stays reserved for genuinely unrecoverable states.
pub struct TestCtx {
    failed: bool,
    skip: Option<&'static str>,
}

impl TestCtx {
    pub fn expect(&mut self, cond: bool, what: &str) {
        if !cond {
            self.failed = true;
            let _ = writeln!(Out, "# {what}: observed false, expected true");
        }
    }

    pub fn expect_eq(&mut self, what: &str, observed: u64, expected: u64) {
        if observed != expected {
            self.failed = true;
            let _ = writeln!(Out, "# {what}: observed {observed:#x}, expected {expected:#x}");
        }
    }

    /// Mark the whole test skipped. Wins over any recorded failure: a check
    /// that cannot run on this machine must not fail on it either.
    pub fn skip(&mut self, reason: &'static str) {
        self.skip = Some(reason);
    }
}

/// Run every registered test, emitting KTAP over the learner's serial port.
/// Returns whether all non-SKIP tests passed.
pub fn run_all() -> bool {
    let ts = tests();
    let _ = writeln!(Out, "KTAP version 1");
    let _ = writeln!(Out, "1..{}", ts.len());
    let mut all_ok = true;
    for (i, t) in ts.iter().enumerate() {
        let mut ctx = TestCtx { failed: false, skip: None };
        (t.run)(&mut ctx);
        let n = i + 1;
        let id = t.id;
        if let Some(reason) = ctx.skip {
            let _ = writeln!(Out, "ok {n} - {id} # SKIP {reason}");
        } else if ctx.failed {
            all_ok = false;
            let _ = writeln!(Out, "not ok {n} - {id}");
        } else {
            let _ = writeln!(Out, "ok {n} - {id}");
        }
    }
    all_ok
}

/// The test binary's whole job: bring up the learner's UART, run the suite,
/// report the verdict to the exit device.
pub fn run_and_exit() -> ! {
    crate::serial::init();
    let ok = run_all();
    crate::exit::exit(if ok { crate::exit::SUCCESS } else { crate::exit::FAILURE })
}
