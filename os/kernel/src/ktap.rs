//! The cockpit: test registry, KTAP runner, expectation recorder, and the
//! run's nonce (CONTRACT §A.2, §A.3). Course-owned — the learner reads it,
//! never retypes it: the course's tests execute inside this code. It touches
//! learner code through exactly one seam, `serial::write_str` — your UART is
//! the physical layer of every report. (The vehicles in `boot/src/bin/` call
//! the published interfaces of the lab under test; this file calls none.)
//!
//! The registry is a hand-rolled link-section array (rfk lab 08) — the same
//! technique the boot shim uses for Limine requests — so the course stays on
//! stable Rust. `ktest!` places one `KtestDesc` in `.ktest_array`;
//! boot/linker.ld KEEPs that section and brackets it with
//! `__ktest_array_{start,end}`.
//!
//! A run prints, in order: `KTAP version 1`, `# sboot nonce <32 hex>` (or
//! `# sboot nonce none`), `1..N`, one `ok N - <id>` / `not ok N - <id>` per
//! test with its `#` evidence lines BEFORE it, then `# sboot end` last.
//! There is no `# SKIP` and no trailer on a result line.

use core::cell::Cell;
use core::fmt::{self, Write};

use crate::sync::SpinLock;

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
    // SAFETY: the linker script places only `KtestDesc` statics (via
    // `ktest!`) between the two symbols, 8-aligned, so the range is a valid
    // array of them for the life of the program.
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

/// Print one `# …` evidence line outside any test (a vehicle's measurement,
/// `# sboot keys-ready`).
pub fn note(args: fmt::Arguments) {
    let _ = writeln!(Out, "# {args}");
}

// ── The nonce (CONTRACT §A.2) ────────────────────────────────────────────

/// `None` = not read yet; `Some(None)` = read, absent or malformed.
static NONCE: SpinLock<Option<Option<[u8; 32]>>> = SpinLock::new(None);

fn read_nonce() -> Option<[u8; 32]> {
    let (sel, size) = crate::fwcfg::find(crate::fwcfg::NONCE_FILE)?;
    if size != 32 {
        return None;
    }
    let mut hex = [0u8; 32];
    if crate::fwcfg::read(sel, size, &mut hex) != 32 {
        return None;
    }
    if !hex.iter().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b)) {
        return None;
    }
    Some(hex)
}

/// The run's nonce: 32 lowercase hex characters from fw_cfg file
/// `opt/org.sourceboot/nonce`, read once and cached. `None` when the file is
/// absent or is not 32 lowercase hex characters.
pub fn nonce() -> Option<[u8; 32]> {
    let mut g = NONCE.lock();
    if g.is_none() {
        *g = Some(read_nonce());
    }
    g.unwrap()
}

fn hexval(c: u8) -> u64 {
    match c {
        b'0'..=b'9' => (c - b'0') as u64,
        b'a'..=b'f' => (c - b'a' + 10) as u64,
        _ => 0,
    }
}

/// FNV-1a, 64-bit.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The splitmix64 finaliser, constants as CONTRACT §A.2 gives them.
pub fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// `derive(nonce, id) = splitmix64(lo ^ hi ^ fnv1a64(id))`: the nonce's hex
/// parsed to 16 bytes, `lo` = bytes 0..8 and `hi` = bytes 8..16, each read
/// little-endian (CONTRACT §A.2). `user/sys.h derive()` computes the same.
pub fn derive(nonce: &[u8; 32], id: &str) -> u64 {
    let mut raw = [0u8; 16];
    for (i, b) in raw.iter_mut().enumerate() {
        *b = ((hexval(nonce[2 * i]) << 4) | hexval(nonce[2 * i + 1])) as u8;
    }
    let lo = u64::from_le_bytes(raw[0..8].try_into().unwrap());
    let hi = u64::from_le_bytes(raw[8..16].try_into().unwrap());
    splitmix64(lo ^ hi ^ fnv1a64(id.as_bytes()))
}

// ── The recorder ─────────────────────────────────────────────────────────

/// Per-test expectation recorder. Expectations record-and-continue (the
/// KUnit model): a failed check prints its diagnostic and the test keeps
/// running, so one bad register read reports every consequence, not just the
/// first.
pub struct TestCtx {
    failed: bool,
    nonce: Option<[u8; 32]>,
    used_nonce: Cell<bool>,
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

    /// Fail with a named reason.
    pub fn fail(&mut self, why: fmt::Arguments) {
        self.failed = true;
        let _ = writeln!(Out, "# {why}");
    }

    /// Print one `# …` evidence line inside this test (before its result).
    pub fn note(&self, args: fmt::Arguments) {
        let _ = writeln!(Out, "# {args}");
    }

    /// `derive(nonce, id)` for this run. Without a nonce the value is the
    /// all-zero nonce's, so the test still runs, and the row prints `not ok`
    /// by design (CONTRACT §A.2). By convention `id` is the row's own id, or
    /// `<row id>/<k>` for a row's k-th value.
    pub fn derived(&self, id: &str) -> u64 {
        self.used_nonce.set(true);
        derive(self.nonce.as_ref().unwrap_or(&[b'0'; 32]), id)
    }

    /// A nonce-chosen value in `0..n`.
    pub fn pick(&self, id: &str, n: u64) -> u64 {
        self.derived(id) % n
    }
}

// ── The runner ───────────────────────────────────────────────────────────

/// `KTAP version 1`, the nonce line, the plan.
pub fn begin(plan: usize) {
    let _ = writeln!(Out, "KTAP version 1");
    match nonce() {
        Some(n) => {
            // The bytes are checked lowercase hex, so this cannot fail.
            let s = core::str::from_utf8(&n).unwrap_or("none");
            let _ = writeln!(Out, "# sboot nonce {s}");
        }
        None => {
            let _ = writeln!(Out, "# sboot nonce none");
        }
    }
    let _ = writeln!(Out, "1..{plan}");
}

/// The run's last line (CONTRACT A.1-11).
pub fn end() {
    let _ = writeln!(Out, "# sboot end");
}

/// Run every registered test, emitting KTAP over the learner's serial port.
/// A test whose id is registered twice is not run: every copy prints
/// `not ok` (CONTRACT §A.3). Returns whether all tests passed.
pub fn run_all() -> bool {
    let ts = tests();
    let nonce = nonce();
    let mut all_ok = true;
    for (i, t) in ts.iter().enumerate() {
        let n = i + 1;
        let id = t.id;
        let dups = ts.iter().filter(|o| o.id == id).count();
        if dups > 1 {
            all_ok = false;
            let _ = writeln!(Out, "# {id}: registered {dups} times; a duplicate id never passes");
            let _ = writeln!(Out, "not ok {n} - {id}");
            continue;
        }
        let mut ctx = TestCtx { failed: false, nonce, used_nonce: Cell::new(false) };
        (t.run)(&mut ctx);
        if ctx.used_nonce.get() && nonce.is_none() {
            ctx.failed = true;
            let _ = writeln!(Out, "# {id}: derived from the run's nonce, and this run has none");
        }
        if ctx.failed {
            all_ok = false;
            let _ = writeln!(Out, "not ok {n} - {id}");
        } else {
            let _ = writeln!(Out, "ok {n} - {id}");
        }
    }
    all_ok
}

/// A vehicle's whole job: bring up the learner's UART, print the header and
/// the plan, run the suite, print `# sboot end`, halt. Never the exit device.
pub fn run_and_halt() -> ! {
    run_and_halt_after(|| {})
}

/// As `run_and_halt`, with a prelude that runs AFTER the plan line and before
/// the first test — where a vehicle brings up the learner's subsystems (a
/// `todo!()` there panics after the plan, so the run still scores zero with
/// a readable transcript).
pub fn run_and_halt_after(prelude: fn()) -> ! {
    crate::serial::init();
    begin(tests().len());
    prelude();
    run_all();
    end();
    crate::hcf()
}
