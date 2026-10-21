//! Lab 14's vehicle: fork, exit and wait4. The prelude is lab 13's (the
//! GDT, the IDT, the frame pool, the syscall MSRs, the heap, `sse_on`,
//! `proc::init`, task 0, the PIC and the PIT), and so is the boot archive at
//! `/` (CONTRACT §A.7), served by the learner's `fs::archive::Archive` and
//! mounted through the learner's `vfs::mount`.
//!
//! The vehicle starts one program, the course's `init`, through the
//! learner's `exec::spawn` — the first process of the run, so pid 1 —
//! with `argv = ["/init", <nonce>, "1", "14"]` and `SBOOT_NONCE` in its
//! environment. Everything after that is the learner's syscalls: init forks
//! and execs `/forker`, and reaps with `wait4(-1)` until `-ECHILD`; forker
//! forks its children and reaps them. Rows, in the order they print:
//!
//!   1–3  `fork.wait.codes`, `fork.child.write.invisible`,
//!        `wait.echild.no_hang` — printed by forker
//!   4    `fork.orphan.to_init` — printed by init, when it reaps forker's
//!        grandchild, orphaned and handed to it, with the derived code
//!   5    `wait.sleeps.slices` (this vehicle)
//!   6    `fork.exit.frees` (this vehicle)
//!
//! Row 5 watches init while forker runs. From the moment forker's record
//! exists to the moment forker has ended, init has nothing to do but wait
//! in `wait4` for its one child; the learner's `sched::slices` says how many
//! times the CPU was given to it in that window. A `wait4` that sleeps is
//! given a handful (going to sleep, waking for forker's exit); one that
//! polls — yields in a loop — is given one per turn of the run queue, which
//! forker's thousands of `sched_yield`s keep turning. The vehicle (task 0)
//! never sleeps itself: it yields and reads `proc::each`, bounded by the
//! ACPI PM timer — never the learner's tick — so a wakeup the learner loses
//! leaves rows missing and a `#` line naming who is still running, not a
//! silent hang.
//!
//! Init may be given `SLICE_BOUND` slices plus one per tick the window
//! spans, the ticks taken from the window's length by the vehicle's PM
//! timer at the course's PIT rate (never the learner's tick count): a
//! kernel whose tick also wakes every sleeper, beside a working `wakeup`,
//! gives init one slice per tick, and that design is accepted (class B,
//! TPM log 2026-10-19, B15/B17).
//!
//! The count of slices alone needs a run queue that keeps turning: when
//! nothing in the window yields, the queue turns once per tick, and a
//! polling init is given about as few slices as a sleeping one. So row 5
//! also looks where init IS at each of the vehicle's looks inside the
//! window: a task asleep in `wait4` is off the run queue (the same test
//! lab 15's vehicle uses for "asleep"), and one that polls is on it at
//! nearly every look, since it is runnable and the vehicle is the one on
//! the CPU. Init may be on the queue at no more than `QUEUED_PCT` percent
//! of those looks (class B, TPM log 2026-10-19, fix round 1 of slice 4).
//! A window in which the vehicle never looked shows nothing, and fails.
//!
//! Neither count asks WHAT woke init: a kernel whose `wakeup` moves nobody
//! and whose tick puts every sleeper back in the run queue now and then
//! keeps init asleep between sweeps. So row 5 also counts, in the vehicle's
//! own yields, how long a wakeup takes to reach its sleeper: once forker
//! has ended (its record holds a status), init must have collected it
//! within `WAKE_BOUND` of the vehicle's looks, and the probe's sleeper must
//! be running again within `PROBE_YIELDS` yields of the probe's `wakeup`,
//! in each of `PROBE_ROUNDS` rounds. A wakeup puts the sleeper in the run
//! queue, so it runs within one turn; a sweep waits for its tick, thousands
//! of the vehicle's yields, and a sweep every few turns of the run queue
//! lands within two yields of a wakeup by chance, never sixteen times in a
//! row. A sleeper woken early by something else is legal: the probe's
//! sleeper sleeps in a loop on its condition, the way `wait4` does (class
//! A, slice 4, s14 fix rounds 2 and 3; the rounds are the gate B17's
//! reversal rule asks for, since a kernel that slows itself stretches the
//! window and buys slices).
//!
//! Row 5 trusts `sched::slices` only after a positive control, run before
//! init: a kernel task of the vehicle's is driven through
//! `CONTROL_TURNS` turns of the run queue (a `yield_now` and back, each),
//! and `slices` must say it was given the CPU at least that many times. A
//! count that misses the turns a yield gives (or returns 0) would read a
//! polling `wait4` as asleep, so it turns row 5 red by itself.
//!
//! Row 6 counts frames around forks that end: a program of the vehicle's,
//! `/ktest/forkloop`, forks a child that exits at once and reaps it with
//! `wait4`, `FORKS` times, then exits. After a warm-up run of the same loop
//! (`WARM_FORKS` times), the frames in use must grow by less than `FORKS`
//! across the measured run: a child that keeps even one frame after it is
//! reaped (its user half, its page tables, its PML4) grows the count by at
//! least one per fork. The bound is not equality, so a kernel that frees a
//! process's memory at the reap instead of at the exit (the loop program,
//! started by the kernel, is never reaped) stays inside it. Both counts
//! the row compares are the learner's `frame::stats()`, cross-checked by a
//! drain of `frame::alloc` (`frames_checked`): a `stats` that misreports
//! fails the row by itself (class A, slice 4, s14 fix round 3).
//!
//! Before any of that, a probe of `sched::sleep`/`sched::wakeup` on a kernel
//! task of the vehicle's own: asleep it is given no slices and its lock is
//! free (`#` lines only), and `wakeup` brings it back within `PROBE_YIELDS`
//! of the vehicle's yields, `PROBE_ROUNDS` times (row 5).

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../shim.rs"]
mod shim;

use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

use kernel::addr::{PhysAddr, VirtAddr};
use kernel::exec;
use kernel::fs::archive::Archive;
use kernel::fs::vfs::{self, dt, mode, DirEntry, Inode, InodeRef, Stat};
use kernel::idt;
use kernel::ktap;
use kernel::proc::{self, Pid};
use kernel::sync::SpinLock;
use kernel::syscall::errno::{EISDIR, ENOENT, ENOTDIR};
use kernel::syscall::Errno;
use kernel::task::{self, Tid};
use kernel::{bootinfo, frame, gdt, heap, irq, sched, serial, syscall, time};

const ARCHIVE_LEN: usize = include_bytes!("../../../user/bin/boot.archive").len();
/// The boot archive, in the kernel image as a section of its own (§A.7).
#[link_section = ".sboot_archive"]
static ARCHIVE: [u8; ARCHIVE_LEN] = *include_bytes!("../../../user/bin/boot.archive");

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 16;
const PM_HZ: u64 = 3_579_545;

const PLAN: usize = 6;
const CODES_ID: &str = "fork.wait.codes";
const PROGRAM_IDS: [&str; 4] = [CODES_ID, "fork.child.write.invisible", "wait.echild.no_hang", "fork.orphan.to_init"];
const SLICES_ID: &str = "wait.sleeps.slices";
const SLICES_ROW: usize = 5;
/// The most slices init may be given while it waits for forker, beyond one
/// per tick the window spans (measured: CONTRACT §B.2-14).
const SLICE_BOUND: u64 = 16;
/// The course's PIT: its input clock and the reload `irq::init` programs
/// (CONTRACT §B.2-06, 1000 Hz). The vehicle's own constants, never the
/// learner's: the window's ticks are its PM-timer length at this rate.
const PIT_INPUT_HZ: u64 = 1_193_182;
const PIT_RELOAD: u64 = 1193;
/// The most of the window's looks at which init may be on the run queue,
/// in percent (measured: CONTRACT §B.2-14).
const QUEUED_PCT: u64 = 50;
/// The most of the vehicle's looks from forker's end until init has
/// collected it (measured: CONTRACT §B.2-14).
const WAKE_BOUND: u64 = 32;
/// Times the probe's sleeper is put to sleep and woken.
const PROBE_ROUNDS: u64 = 16;
/// The most of the vehicle's yields a `wakeup` may take to reach the
/// probe's sleeper, in every round (measured: CONTRACT §B.2-14).
const PROBE_YIELDS: u64 = 2;
/// How long the whole run may take before the vehicle stops waiting.
const RUN_MS: u64 = 20_000;
const INIT: &str = "/init";
/// Row 5's positive control: the turns its kernel task is driven through.
const CONTROL_TURNS: u64 = 50;
const FREES_ID: &str = "fork.exit.frees";
const FREES_ROW: usize = 6;
/// Forks (each child reaped) in the warm-up run and in the measured run.
const WARM_FORKS: u32 = 4;
const FORKS: u32 = 48;

/// The usable memory at or above 1 MiB that this vehicle hands
/// `frame::init`: far more than any row here takes, far less than the
/// machine has (128 MiB). Row 6 drains the pool through `frame::alloc` at
/// both measure points, and an allocator that searches from the bottom
/// every time takes time that grows with the square of the pool for each
/// drain (the same clip as lab 10's vehicle).
const POOL_BYTES: u64 = 16 << 20;
const ONE_MIB: u64 = 1 << 20;

/// `map` with its usable memory at or above 1 MiB cut to `POOL_BYTES`, in
/// address order: a region past the budget is cut short or dropped. The rest
/// of the map is passed as it is.
fn clipped(map: &[bootinfo::MemRegion], out: &mut [bootinfo::MemRegion; bootinfo::MEMMAP_MAX]) -> usize {
    let mut left = POOL_BYTES;
    let mut n = 0;
    for r in map {
        let mut r = *r;
        if r.kind == 0 && r.base + r.len > ONE_MIB {
            let low = ONE_MIB.saturating_sub(r.base).min(r.len);
            let high = (r.len - low).min(left);
            left -= high;
            r.len = low + high;
            if r.len == 0 {
                continue;
            }
        }
        if n < out.len() {
            out[n] = r;
            n += 1;
        }
    }
    n
}

// ── Time that does not depend on the learner's tick ──────────────────────

fn pm() -> u32 {
    let v: u32;
    // SAFETY: a 32-bit read of the ACPI PM timer port; no memory is touched.
    unsafe { asm!("in eax, dx", out("eax") v, in("dx") 0x608u16, options(nomem, nostack, preserves_flags)) };
    v & 0xff_ffff
}

/// Microseconds since `new`, from the 24-bit PM timer (it wraps every
/// 4.7 s; `us` must be called more often than that).
struct Clock {
    last: u32,
    acc: u64,
}

impl Clock {
    fn new() -> Clock {
        Clock { last: pm(), acc: 0 }
    }
    fn us(&mut self) -> u64 {
        let now = pm();
        self.acc += (now.wrapping_sub(self.last) & 0xff_ffff) as u64;
        self.last = now;
        self.acc * 1_000_000 / PM_HZ
    }
    fn ms(&mut self) -> u64 {
        self.us() / 1000
    }
}

fn ticks() -> u64 {
    time::TICKS.load(Ordering::Relaxed)
}

// ── Interrupts off around what must not be cut ───────────────────────────

fn irq_off() -> bool {
    let rflags: u64;
    // SAFETY: reads RFLAGS through the stack and clears IF.
    unsafe { asm!("pushfq", "pop {}", "cli", out(reg) rflags) };
    rflags & (1 << 9) != 0
}

fn irq_restore(on: bool) {
    if on {
        // SAFETY: setting IF touches no memory.
        unsafe { asm!("sti", options(nomem, nostack)) };
    }
}

fn quiet<R>(f: impl FnOnce() -> R) -> R {
    let on = irq_off();
    let r = f();
    irq_restore(on);
    r
}

/// One `#` line, whole: a tick between two of its bytes would let a
/// program's line land inside it.
fn note(args: core::fmt::Arguments) {
    quiet(|| ktap::note(args));
}

fn row(ok: bool, n: usize, id: &str) {
    quiet(|| {
        if ok {
            kernel::kprintln!("ok {n} - {id}");
        } else {
            kernel::kprintln!("not ok {n} - {id}");
        }
    });
}

fn halt() -> ! {
    loop {
        // SAFETY: interrupts off, then wait: nothing runs after this.
        unsafe { asm!("cli", "hlt", options(nomem, nostack)) };
    }
}

fn status_text(s: Option<i32>) -> String {
    match s {
        None => String::from("still running"),
        Some(v) if v & 0x7f == 0 => format!("exit({}) (status {v:#x})", (v >> 8) & 0xff),
        Some(v) => format!("killed by signal {} (status {v:#x})", v & 0x7f),
    }
}

fn errno_text(e: Errno) -> String {
    let name = match e {
        2 => "ENOENT",
        8 => "ENOEXEC",
        10 => "ECHILD",
        12 => "ENOMEM",
        14 => "EFAULT",
        22 => "EINVAL",
        _ => "?",
    };
    format!("-{e} ({name})")
}

fn nonce_str() -> String {
    match ktap::nonce() {
        Some(n) => String::from(core::str::from_utf8(&n).unwrap_or("none")),
        None => String::from("none"),
    }
}

fn frames_used() -> usize {
    quiet(|| frame::stats().used)
}

/// Frames in use, as `frame::stats()` says and as a drain confirms: with
/// interrupts off, take every free frame with `frame::alloc` (each linked to
/// the last through its first word, by the HHDM), count them and give them
/// all back. `stats` must say exactly that many are free, the rest in use,
/// and read the same after the refill. An `alloc` that runs dry early only
/// makes more frames look used, so a row that bounds the growth fails,
/// never passes (CONTRACT §B.2-14).
fn frames_checked() -> Result<usize, String> {
    quiet(|| {
        let s0 = frame::stats();
        let off = bootinfo::hhdm_offset();
        let cap = 2 * s0.total + 64;
        let mut head: u64 = 0;
        let mut n = 0usize;
        while n < cap {
            let Some(f) = frame::alloc() else { break };
            let pa = f.0 .0;
            // SAFETY: the frame was just handed out to the vehicle alone; the
            // HHDM maps all RAM writable.
            unsafe { *((pa + off) as *mut u64) = head };
            head = pa;
            n += 1;
        }
        for _ in 0..n {
            // SAFETY: `head` is a frame this drain took and wrote above.
            let next = unsafe { *((head + off) as *const u64) };
            frame::free(frame::Frame(PhysAddr(head)));
            head = next;
        }
        let s1 = frame::stats();
        if n >= cap {
            return Err(format!("frame::alloc never ran out: a drain took {n} frames of a pool frame::stats() says holds {}", s0.total));
        }
        if s0.free != n || s0.total.checked_sub(n) != Some(s0.used) {
            return Err(format!(
                "frame::stats() says {} in use and {} free of {}; a drain of frame::alloc finds {n} free, so {} in use",
                s0.used,
                s0.free,
                s0.total,
                s0.total.saturating_sub(n)
            ));
        }
        if s1 != s0 {
            return Err(format!(
                "frame::stats() said {} in use before a drain of frame::alloc and {} after every frame went back",
                s0.used, s1.used
            ));
        }
        Ok(s0.used)
    })
}

// ── Watching processes ───────────────────────────────────────────────────

/// One process as `proc::each` shows it.
#[derive(Clone, Copy)]
struct Seen {
    pid: Pid,
    ppid: Pid,
    task: Tid,
    exit: Option<i32>,
}

fn snapshot() -> Vec<Seen> {
    let mut v = Vec::new();
    quiet(|| proc::each(|p| v.push(Seen { pid: p.pid, ppid: p.ppid, task: p.task, exit: p.exit })));
    v
}

fn find(all: &[Seen], pid: Pid) -> Option<Seen> {
    all.iter().copied().find(|s| s.pid == pid)
}

// ── The archive at / ─────────────────────────────────────────────────────

fn mount_archive() -> bool {
    let a = match Archive::new(&ARCHIVE) {
        Ok(a) => a,
        Err(e) => {
            note(format_args!("vfs: Archive::new over the boot archive ({ARCHIVE_LEN} bytes) returned {}", errno_text(e)));
            return false;
        }
    };
    let root: InodeRef = Arc::new(a);
    if let Err(e) = vfs::mount("/", root.clone()) {
        note(format_args!("vfs: mount(\"/\", the archive) returned {}", errno_text(e)));
        return false;
    }
    let present: Vec<&str> = ["init", "forker"].into_iter().filter(|n| root.lookup(n).is_ok()).collect();
    note(format_args!("vfs: / is the boot archive ({ARCHIVE_LEN} bytes); lookup finds {present:?} of [\"init\", \"forker\"]"));
    true
}

// ── The probe: sleep and wakeup on a kernel task (# lines only) ──────────

static PROBE_LOCK: SpinLock<u64> = SpinLock::new(0);
/// The round the probe's sleeper is going to sleep for (0: not yet).
static ASLEEP: AtomicU64 = AtomicU64::new(0);
static WOKEN: AtomicU64 = AtomicU64::new(0);
/// The probe's condition: the round the vehicle has released, set just
/// before that round's `wakeup`.
static PROBE_GO: AtomicU64 = AtomicU64::new(0);
/// Times the probe's sleeper came back from `sleep` before its condition held.
static EARLY: AtomicU64 = AtomicU64::new(0);

fn probe_chan() -> usize {
    &ASLEEP as *const AtomicU64 as usize
}

/// The probe's task: take the lock and, `PROBE_ROUNDS` times, say which
/// round it is going to sleep for, sleep on the probe's channel until the
/// vehicle has released that round (a wakeup from elsewhere sends it back
/// to sleep), and record that it holds the lock again.
fn sleeper() {
    let on = irq_off();
    let mut g = PROBE_LOCK.lock();
    for round in 1..=PROBE_ROUNDS {
        ASLEEP.store(round, Ordering::SeqCst);
        while PROBE_GO.load(Ordering::SeqCst) < round {
            g = sched::sleep(probe_chan(), &PROBE_LOCK, g);
            if PROBE_GO.load(Ordering::SeqCst) < round {
                EARLY.fetch_add(1, Ordering::SeqCst);
            }
        }
        *g += 1;
        WOKEN.store(round, Ordering::SeqCst);
    }
    drop(g);
    irq_restore(on);
}

fn yield_until(ms: u64, mut done: impl FnMut() -> bool) -> bool {
    let mut c = Clock::new();
    loop {
        if done() {
            return true;
        }
        if c.ms() >= ms {
            return false;
        }
        sched::yield_now();
    }
}

/// The probe; `true` when, in every one of `PROBE_ROUNDS` rounds, its
/// `wakeup` brought the sleeper back within `PROBE_YIELDS` of the vehicle's
/// yields. One round shows a wakeup that works; a sweep that puts every
/// sleeper back now and then answers one round in a few yields by chance,
/// and not sixteen in a row.
fn probe_sleep() -> bool {
    let t = task::spawn(sleeper);
    if !yield_until(1000, || ASLEEP.load(Ordering::SeqCst) == 1) {
        note(format_args!("sleep probe: task {} never reached sched::sleep within 1 s", t.0));
        return false;
    }
    // It reached `sleep`; let it settle and count what it is given.
    sched::yield_now();
    let s0 = sched::slices(t);
    for _ in 0..50 {
        sched::yield_now();
    }
    let s1 = sched::slices(t);
    let free = quiet(|| PROBE_LOCK.try_lock().map(drop).is_some());
    let early = EARLY.load(Ordering::SeqCst);
    let mut took: Vec<u64> = Vec::new();
    let mut back = true;
    for round in 1..=PROBE_ROUNDS {
        // From round 2 on: wait until the sleeper is asleep for this round
        // (it says so holding the lock, and `sleep` lets the lock go).
        if round > 1
            && !yield_until(1000, || {
                ASLEEP.load(Ordering::SeqCst) == round && quiet(|| PROBE_LOCK.try_lock().map(drop).is_some())
            })
        {
            back = false;
            break;
        }
        PROBE_GO.store(round, Ordering::SeqCst);
        sched::wakeup(probe_chan());
        let mut c = Clock::new();
        let mut yields = 0u64;
        while WOKEN.load(Ordering::SeqCst) < round && c.ms() < 1000 {
            sched::yield_now();
            yields += 1;
        }
        took.push(yields);
        if WOKEN.load(Ordering::SeqCst) < round {
            back = false;
            break;
        }
    }
    let held = quiet(|| PROBE_LOCK.try_lock().map(|g| *g));
    let most = took.iter().copied().max().unwrap_or(0);
    note(format_args!(
        "sleep probe: task {} asleep: slices {s0} -> {s1} over 50 yields of the test; its lock {} while it slept; it came back from sleep {early} times before its first wakeup; after each wakeup: {} (lock count {:?})",
        t.0,
        if free { "was free" } else { "was held" },
        if back {
            format!("it ran again with the lock after {took:?} of the test's yields, {PROBE_ROUNDS} rounds")
        } else {
            format!("it did not run again within 1 s in round {} ({took:?} of the test's yields)", took.len().max(1))
        },
        held
    ));
    let ok = back && most <= PROBE_YIELDS;
    if !ok {
        note(format_args!(
            "{SLICES_ID}: sched::wakeup on the channel a task sleeps on did not put it back to run: wakeup moves every sleeper on that channel to the run queue, so it runs on the next turn"
        ));
    }
    ok
}

// ── Row 5's positive control: slices counts every turn ────────────────────

static TURN_STAGE: AtomicU64 = AtomicU64::new(0);
static TURN_GO: AtomicU64 = AtomicU64::new(0);
static TURN_STOP: AtomicU64 = AtomicU64::new(0);

/// The control's task: once told to go, `CONTROL_TURNS` yields — each one
/// gives the CPU away and waits to be given it back — then it waits to be
/// told to stop, and returns.
fn turner() {
    TURN_STAGE.store(1, Ordering::SeqCst);
    while TURN_GO.load(Ordering::SeqCst) == 0 {
        sched::yield_now();
    }
    for _ in 0..CONTROL_TURNS {
        sched::yield_now();
    }
    TURN_STAGE.store(2, Ordering::SeqCst);
    while TURN_STOP.load(Ordering::SeqCst) == 0 {
        sched::yield_now();
    }
}

/// `true` when `sched::slices` grew by at least `CONTROL_TURNS` for a task
/// that was put back on the CPU at least that many times.
fn slices_control() -> bool {
    let t = task::spawn(turner);
    if !yield_until(1000, || TURN_STAGE.load(Ordering::SeqCst) == 1) {
        note(format_args!("{SLICES_ID}: control: task {} never ran within 1 s", t.0));
        return false;
    }
    let s0 = sched::slices(t);
    TURN_GO.store(1, Ordering::SeqCst);
    let done = yield_until(1000, || TURN_STAGE.load(Ordering::SeqCst) == 2);
    let s1 = sched::slices(t);
    TURN_STOP.store(1, Ordering::SeqCst);
    if !done {
        note(format_args!("{SLICES_ID}: control: task {} did not finish {CONTROL_TURNS} yields within 1 s", t.0));
        return false;
    }
    let n = s1.saturating_sub(s0);
    note(format_args!(
        "{SLICES_ID}: control: a kernel task given the CPU back after each of {CONTROL_TURNS} yields: sched::slices {s0} -> {s1} ({n})"
    ));
    if n < CONTROL_TURNS {
        note(format_args!(
            "{SLICES_ID}: control: sched::slices counted {n} of at least {CONTROL_TURNS} times that task was given the CPU: it must count every time the switch puts a task on the CPU — after a yield as well as after a tick — or a wait4 that polls looks asleep"
        ));
    }
    n >= CONTROL_TURNS
}

// ── Row 6: the vehicle's directory, /ktest, and the fork loop ────────────

/// A directory of this vehicle's, served through the learner's VFS.
struct KtestDir {
    files: Vec<(&'static str, InodeRef)>,
}

struct MemFile {
    bytes: Vec<u8>,
    ino: u64,
}

impl Inode for MemFile {
    fn lookup(&self, _name: &str) -> Result<InodeRef, Errno> {
        Err(ENOTDIR)
    }
    fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<usize, Errno> {
        let len = self.bytes.len() as u64;
        if off >= len {
            return Ok(0);
        }
        let n = buf.len().min((len - off) as usize);
        buf[..n].copy_from_slice(&self.bytes[off as usize..off as usize + n]);
        Ok(n)
    }
    fn getdents(&self, _index: u64) -> Result<Option<DirEntry>, Errno> {
        Err(ENOTDIR)
    }
    fn stat(&self) -> Stat {
        Stat { ino: self.ino, mode: mode::S_IFREG | 0o555, nlink: 1, size: self.bytes.len() as u64, ..Stat::default() }
    }
}

impl Inode for KtestDir {
    fn lookup(&self, name: &str) -> Result<InodeRef, Errno> {
        self.files.iter().find(|(n, _)| *n == name).map(|(_, f)| f.clone()).ok_or(ENOENT)
    }
    fn read_at(&self, _off: u64, _buf: &mut [u8]) -> Result<usize, Errno> {
        Err(EISDIR)
    }
    fn getdents(&self, index: u64) -> Result<Option<DirEntry>, Errno> {
        Ok(self.files.get(index as usize).map(|(n, f)| DirEntry { ino: f.stat().ino, kind: dt::DT_REG, name: String::from(*n) }))
    }
    fn stat(&self) -> Stat {
        Stat { ino: 1, mode: mode::S_IFDIR | 0o555, nlink: 2, ..Stat::default() }
    }
}

/// `forkloop`, `forks` times: `fork`; the child `exit(0)`s; the parent
/// `wait4(-1, NULL, 0, NULL)`s. Then `exit(0)`; a failed `fork` or `wait4`
/// is `exit(100 + errno)`. A static ELF of one PT_LOAD at `proc::BASE`.
fn forkloop(forks: u32) -> Vec<u8> {
    let n = forks.to_le_bytes();
    let code: [u8; 85] = [
        0x41, 0xbc, n[0], n[1], n[2], n[3], //    0: mov r12d, forks
        0xb8, 57, 0, 0, 0, //                     6: mov eax, 57 (fork)
        0x0f, 0x05, //                           11: syscall
        0x48, 0x85, 0xc0, //                     13: test rax, rax
        0x78, 0x33, //                           16: js fail (69)
        0x74, 0x28, //                           18: jz child (60)
        0x48, 0xc7, 0xc7, 0xff, 0xff, 0xff, 0xff, // 20: mov rdi, -1
        0x31, 0xf6, //                           27: xor esi, esi
        0x31, 0xd2, //                           29: xor edx, edx
        0x45, 0x31, 0xd2, //                     31: xor r10d, r10d
        0xb8, 61, 0, 0, 0, //                    34: mov eax, 61 (wait4)
        0x0f, 0x05, //                           39: syscall
        0x48, 0x85, 0xc0, //                     41: test rax, rax
        0x78, 0x17, //                           44: js fail (69)
        0x41, 0xff, 0xcc, //                     46: dec r12d
        0x75, 0xd3, //                           49: jnz 6
        0x31, 0xff, //                           51: xor edi, edi
        0xb8, 60, 0, 0, 0, //                    53: mov eax, 60 (exit)
        0x0f, 0x05, //                           58: syscall
        0x31, 0xff, //                           60: child: xor edi, edi
        0xb8, 60, 0, 0, 0, //                    62: mov eax, 60 (exit)
        0x0f, 0x05, //                           67: syscall
        0xf7, 0xd8, //                           69: fail: neg eax
        0x83, 0xc0, 100, //                      71: add eax, 100
        0x89, 0xc7, //                           74: mov edi, eax
        0xb8, 60, 0, 0, 0, //                    76: mov eax, 60 (exit)
        0x0f, 0x05, //                           81: syscall
        0x0f, 0x0b, //                           83: ud2
    ];
    let len = 120 + code.len();
    let mut b = alloc::vec![0u8; len];
    let mut put = |off: usize, v: u64, n: usize| b[off..off + n].copy_from_slice(&v.to_le_bytes()[..n]);
    put(0, 0x0101_0246_4c45_7f, 7); // \x7fELF, 64-bit, little-endian, version 1
    put(16, 2, 2); // ET_EXEC
    put(18, 0x3e, 2); // x86-64
    put(20, 1, 4);
    put(24, proc::BASE + 120, 8); // e_entry
    put(32, 64, 8); // e_phoff
    put(52, 64, 2); // e_ehsize
    put(54, 56, 2); // e_phentsize
    put(56, 1, 2); // e_phnum
    put(64, 1, 4); // PT_LOAD
    put(68, 5, 4); // R X
    put(80, proc::BASE, 8); // p_vaddr
    put(88, proc::BASE, 8); // p_paddr
    put(96, len as u64, 8); // p_filesz
    put(104, len as u64, 8); // p_memsz
    put(112, 0x1000, 8);
    b[120..].copy_from_slice(&code);
    b
}

fn mount_ktest() -> bool {
    let files: Vec<(&'static str, InodeRef)> = alloc::vec![
        ("forkloop-warm", Arc::new(MemFile { bytes: forkloop(WARM_FORKS), ino: 2 }) as InodeRef),
        ("forkloop", Arc::new(MemFile { bytes: forkloop(FORKS), ino: 3 }) as InodeRef),
    ];
    match vfs::mount("/ktest", Arc::new(KtestDir { files })) {
        Ok(()) => true,
        Err(e) => {
            note(format_args!("{FREES_ID}: vfs: mount(\"/ktest\", the test's directory) returned {}", errno_text(e)));
            false
        }
    }
}

/// Start `path` and wait for it to end (at most `ms`): its pid, its status,
/// and the frames its image took when it was started.
fn run_loop(path: &str, ms: u64) -> Result<(Pid, Option<i32>, i64), String> {
    let f0 = frames_used();
    let pid = exec::spawn(path, &["forkloop"], &[]).map_err(|e| format!("exec::spawn(\"{path}\") returned {}", errno_text(e)))?;
    let image = frames_used() as i64 - f0 as i64;
    yield_until(ms, || find(&snapshot(), pid).is_none_or(|s| s.exit.is_some()));
    // The dead tasks go on a yield.
    sched::yield_now();
    Ok((pid, find(&snapshot(), pid).and_then(|s| s.exit), image))
}

fn fork_frees() {
    if !quiet(mount_ktest) {
        return row(false, FREES_ROW, FREES_ID);
    }
    let warm = match run_loop("/ktest/forkloop-warm", 2000) {
        Ok(w) => w,
        Err(why) => {
            note(format_args!("{FREES_ID}: {why}; nothing ran"));
            return row(false, FREES_ROW, FREES_ID);
        }
    };
    if warm.1 != Some(0) {
        note(format_args!(
            "{FREES_ID}: the warm-up loop (pid {}, {WARM_FORKS} forks, each child reaped) {}, where it exits 0; a status of 100 + n is a fork or wait4 that returned -n",
            warm.0 .0,
            status_text(warm.1)
        ));
        return row(false, FREES_ROW, FREES_ID);
    }
    let f0 = match frames_checked() {
        Ok(n) => n,
        Err(why) => {
            note(format_args!("{FREES_ID}: before the measured loop, {why}"));
            return row(false, FREES_ROW, FREES_ID);
        }
    };
    let run = match run_loop("/ktest/forkloop", 4000) {
        Ok(r) => r,
        Err(why) => {
            note(format_args!("{FREES_ID}: {why}; nothing ran"));
            return row(false, FREES_ROW, FREES_ID);
        }
    };
    let f1 = match frames_checked() {
        Ok(n) => n,
        Err(why) => {
            note(format_args!("{FREES_ID}: after the measured loop, {why}"));
            return row(false, FREES_ROW, FREES_ID);
        }
    };
    let grown = f1 as i64 - f0 as i64;
    note(format_args!(
        "{FREES_ID}: pid {} forked {FORKS} children, each exiting at once and reaped, then {}: frames in use {f0} before, {f1} after ({grown:+}; its own image took {})",
        run.0 .0,
        status_text(run.1),
        run.2
    ));
    let mut ok = true;
    if run.1 != Some(0) {
        ok = false;
        note(format_args!("{FREES_ID}: the loop must exit 0; a status of 100 + n is a fork or wait4 that returned -n"));
    }
    if grown >= FORKS as i64 {
        ok = false;
        note(format_args!(
            "{FREES_ID}: the frames in use grew by {grown} over {FORKS} forks whose children all ended and were reaped: a process that ends gives back its whole user half — every page its VMAs map, its page tables and its PML4"
        ));
    }
    row(ok, FREES_ROW, FREES_ID);
}

// ── The run ──────────────────────────────────────────────────────────────

/// What the run measured.
struct Run {
    init: Pid,
    init_task: Tid,
    forker: Option<Pid>,
    /// init's slices when forker's record first appeared, and once forker had ended.
    window: Option<(u64, u64)>,
    window_us: u64,
    /// The learner's tick count across the window: printed, never judged.
    window_ticks: u64,
    /// The vehicle's looks strictly inside the window, and how many of
    /// them found init on the run queue.
    looks: u64,
    queued: u64,
    /// The vehicle's looks from the first that found forker ended to the
    /// first that found its record gone (collected).
    reap_looks: Option<u64>,
    init_exit: Option<i32>,
    left: Vec<Seen>,
    /// The most frames in use, and the most processes, at any look.
    peak_frames: usize,
    peak_procs: usize,
}

fn run(init: Pid) -> Run {
    let all = snapshot();
    let init_task = find(&all, init).map_or(Tid(u64::MAX), |s| s.task);
    let mut r = Run { init, init_task, forker: None, window: None, window_us: 0, window_ticks: 0, looks: 0, queued: 0, reap_looks: None, init_exit: None, left: Vec::new(), peak_frames: 0, peak_procs: 0 };
    let mut c = Clock::new();
    let mut open: Option<(u64, u64)> = None;
    let mut ticks0 = 0;
    let mut since_end: u64 = 0;
    loop {
        let all = snapshot();
        let now = c.us();
        r.peak_frames = r.peak_frames.max(frames_used());
        r.peak_procs = r.peak_procs.max(all.len());
        if r.forker.is_none() {
            if let Some(f) = all.iter().find(|s| s.ppid == init && s.pid != init) {
                r.forker = Some(f.pid);
                open = Some((sched::slices(init_task), now));
                ticks0 = ticks();
            }
        }
        if let (Some(f), Some((s0, t0)), None) = (r.forker, open, r.window) {
            if find(&all, f).is_none_or(|s| s.exit.is_some()) {
                r.window = Some((s0, sched::slices(init_task)));
                r.window_us = now - t0;
                r.window_ticks = ticks().wrapping_sub(ticks0);
            } else if now != t0 {
                r.looks += 1;
                if task::with_table(|tt| tt.ready.contains(&init_task)) {
                    r.queued += 1;
                }
            }
        }
        if let (Some(f), Some(_), None) = (r.forker, r.window, r.reap_looks) {
            if find(&all, f).is_none() {
                r.reap_looks = Some(since_end);
            } else {
                since_end += 1;
            }
        }
        r.init_exit = find(&all, init).and_then(|s| s.exit);
        if r.init_exit.is_some() || now / 1000 >= RUN_MS {
            r.left = all.into_iter().filter(|s| s.exit.is_none()).collect();
            return r;
        }
        sched::yield_now();
    }
}

fn programs_never_ran() {
    for (i, id) in PROGRAM_IDS.iter().enumerate() {
        row(false, i + 1, id);
    }
}

fn report(r: &Run, control: bool, probe: bool) {
    note(format_args!(
        "init: pid {} {}",
        r.init.0,
        match r.init_exit {
            Some(_) => status_text(r.init_exit),
            None => format!("did not end within {} s", RUN_MS / 1000),
        }
    ));
    if !r.left.is_empty() {
        let who: Vec<String> = r.left.iter().map(|s| format!("pid {} (parent {})", s.pid.0, s.ppid.0)).collect();
        note(format_args!("still running at the end: {}; any row they had not printed is missing", who.join(", ")));
    }
    let ok = match (r.forker, r.window) {
        (None, _) => {
            note(format_args!("{SLICES_ID}: init (pid {}) never forked a child the test saw", r.init.0));
            false
        }
        (Some(f), None) => {
            note(format_args!("{SLICES_ID}: forker (pid {}) did not end while the test watched", f.0));
            false
        }
        (Some(f), Some((s0, s1))) => {
            let n = s1.saturating_sub(s0);
            let spans = r.window_us * PIT_INPUT_HZ / PIT_RELOAD / 1_000_000;
            let bound = SLICE_BOUND + spans;
            note(format_args!(
                "{SLICES_ID}: while init (pid {}, task {}) waited for forker (pid {}), {} ms ({spans} ticks at the PIT's rate; the kernel counted {}), init was given {n} slices (sched::slices {s0} -> {s1}; at most {bound})",
                r.init.0,
                r.init_task.0,
                f.0,
                r.window_us / 1000,
                r.window_ticks
            ));
            note(format_args!(
                "{SLICES_ID}: init was on the run queue at {} of the test's {} looks in that window",
                r.queued, r.looks
            ));
            if n > bound {
                note(format_args!(
                    "{SLICES_ID}: init kept getting the CPU while its only child ran: its wait4 looks again on every turn instead of sleeping until a child's exit wakes it"
                ));
            }
            if r.looks == 0 {
                note(format_args!(
                    "{SLICES_ID}: forker ended before the test looked again, so nothing showed how init waited: forker runs thousands of turns when its children run"
                ));
            }
            let parked = r.looks > 0 && r.queued * 100 <= r.looks * QUEUED_PCT;
            if r.looks > 0 && !parked {
                note(format_args!(
                    "{SLICES_ID}: init stayed on the run queue while its only child ran: a task asleep in wait4 is off the run queue until a child's exit wakes it"
                ));
            }
            let woken = match r.reap_looks {
                Some(k) => {
                    note(format_args!(
                        "{SLICES_ID}: init collected forker {k} of the test's looks after forker ended"
                    ));
                    if k > WAKE_BOUND {
                        note(format_args!(
                            "{SLICES_ID}: init went on sleeping after forker ended: exit wakes the parent asleep in wait4 on its record, so it runs and collects the child on the next turn"
                        ));
                    }
                    k <= WAKE_BOUND
                }
                None => {
                    note(format_args!(
                        "{SLICES_ID}: forker's record was still there when the test stopped watching: init never collected it"
                    ));
                    false
                }
            };
            n <= bound && parked && woken
        }
    };
    if ok && !control {
        note(format_args!("{SLICES_ID}: the count above is not trusted: the control before init found sched::slices missing turns"));
    }
    row(ok && control && probe, SLICES_ROW, SLICES_ID);
}

// ── The machine ──────────────────────────────────────────────────────────

/// CR0.EM = 0, CR0.TS = 0, CR0.MP = 1; CR4.OSFXSR = 1, CR4.OSXMMEXCPT = 1;
/// `fninit`; MXCSR = 0x1F80 — so the switch's FXSAVE saves xmm before the
/// first program's `user::enter` turns SSE on.
fn sse_on() {
    let mxcsr: u32 = 0x1F80;
    // SAFETY: these bits change only how x87/SSE instructions behave; the
    // kernel is soft-float and uses neither.
    unsafe {
        asm!(
            "mov {t}, cr0",
            "and {t}, {clear0}",
            "or {t}, 2",
            "mov cr0, {t}",
            "mov {t}, cr4",
            "or {t}, {set4}",
            "mov cr4, {t}",
            "fninit",
            "ldmxcsr [{m}]",
            t = out(reg) _,
            clear0 = in(reg) !((1u64 << 2) | (1 << 3)),
            set4 = const (1u64 << 9) | (1 << 10),
            m = in(reg) &mxcsr,
            options(nostack),
        )
    };
}

fn prelude() {
    let (k, i, _) = shim::stacks();
    gdt::init(k, i);
    idt::init();
    let bi = bootinfo::get().expect("the shim published the BootInfo");
    let mut map = [bi.memmap()[0]; bootinfo::MEMMAP_MAX];
    let n = clipped(bi.memmap(), &mut map);
    frame::init(&map[..n], bi.kernel_range());
    syscall::init();
    syscall::set_kernel_stack(k);
    heap::init(VirtAddr(HEAP_START), HEAP_PAGES);
    sse_on();
    proc::init();
    sched::init(k);
    sched::yield_now();
    irq::init();
    let t0 = ticks();
    let mut c = Clock::new();
    while ticks() < t0 + 2 && c.ms() < 100 {
        core::hint::spin_loop();
    }
}

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    serial::init();
    ktap::begin(PLAN);
    prelude();
    let nonce = nonce_str();

    let probe = probe_sleep();
    let control = slices_control();

    let d = ktap::nonce().map_or(0, |n| ktap::derive(&n, CODES_ID));
    let n = d % 4 + 2;
    let codes: Vec<u64> = (0..n).map(|i| 1 + ((d >> (8 * (i + 1))) & 0xff) % 100).collect();
    note(format_args!("{CODES_ID}: derive(nonce, \"{CODES_ID}\") = {d:016x}: forker forks {n} children, exiting with {codes:?}"));

    let mounted = mount_archive();
    // Grow the heap once, before counting, for the kernel stacks the run
    // will need (the heap never gives frames back): what the count then
    // shows is what the processes themselves kept.
    drop(core::hint::black_box(alloc::vec![0u8; 256 * 1024]));
    let frames0 = frames_used();
    let envp = format!("SBOOT_NONCE={nonce}");
    let spawned = if mounted {
        exec::spawn(INIT, &[INIT, nonce.as_str(), "1", "14"], &[envp.as_str()])
    } else {
        note(format_args!("init: not run, / is not mounted"));
        Err(0)
    };
    match spawned {
        Ok(init) => {
            if init != Pid(1) {
                note(format_args!("init: exec::spawn gave it pid {}; the first process is pid 1, and orphans go to pid 1", init.0));
            }
            let r = run(init);
            report(&r, control, probe);
            note(format_args!(
                "at the busiest look: {} processes alive or not yet collected, {} frames in use (each fork copies every page)",
                r.peak_procs, r.peak_frames
            ));
        }
        Err(e) => {
            if mounted {
                note(format_args!("init: exec::spawn(\"{INIT}\", …) returned {}; nothing ran", errno_text(e)));
            }
            programs_never_ran();
            row(false, SLICES_ROW, SLICES_ID);
        }
    }
    // The dead tasks' stacks go back on a yield; then count frames.
    sched::yield_now();
    note(format_args!("frames in use: {frames0} before init, {} after the run", frames_used()));
    if mounted {
        fork_frees();
    } else {
        note(format_args!("{FREES_ID}: not run, / is not mounted"));
        row(false, FREES_ROW, FREES_ID);
    }
    quiet(ktap::end);
    halt()
}
