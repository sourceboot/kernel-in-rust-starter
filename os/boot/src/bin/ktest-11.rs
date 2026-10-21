//! Lab 11's vehicle: tasks and preemption. The prelude brings up what lab
//! 10's did (the GDT, the IDT, the frame pool, the syscall MSRs, the heap),
//! turns the SSE unit on (this vehicle's own helper, `sse_on`: no ring-3
//! program runs here, and lab 09's `user::enter` is what turns it on
//! otherwise), makes the boot flow task 0 (`sched::init`), and starts the
//! PIC and the PIT (`irq::init`): from there every tick reaches the
//! learner's `sched::tick` through IRQ 0's handler.
//!
//! Every row runs in task 0 and spawns small tasks of its own, which report
//! through statics; only task 0 prints. Each task watches the ACPI PM timer
//! (it runs whether or not a tick arrives) and gives up after a bound, so a
//! kernel that never preempts, or that stalls the PIC, ends a row with its
//! own `not ok` instead of hanging the run. `task.return.dies` runs after
//! the registered rows and prints its own verdict: a task that returns into
//! nothing can take the machine down, and then it loses only that row. The
//! tasks every other row spawns end by calling `task::exit()` themselves,
//! so a kernel whose return path is broken loses `task.return.dies` alone.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../shim.rs"]
mod shim;

use core::arch::asm;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use kernel::addr::VirtAddr;
use kernel::ktap;
use kernel::ktest;
use kernel::task::{self, State, Tid};
use kernel::{bootinfo, frame, gdt, heap, idt, irq, sched, serial, syscall, time};

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 16;
const PM_HZ: u64 = 3_579_545;

// ── Time that does not depend on the learner's tick ──────────────────────

fn pm() -> u32 {
    let v: u32;
    // SAFETY: a 32-bit read of the ACPI PM timer port; no memory is touched.
    unsafe { asm!("in eax, dx", out("eax") v, in("dx") 0x608u16, options(nomem, nostack, preserves_flags)) };
    v & 0xff_ffff
}

/// Milliseconds since `new`, from the 24-bit PM timer (it wraps every
/// 4.7 s; `ms` must be called more often than that).
struct Clock {
    last: u32,
    acc: u64,
}

impl Clock {
    fn new() -> Clock {
        Clock { last: pm(), acc: 0 }
    }
    fn ms(&mut self) -> u64 {
        let now = pm();
        self.acc += (now.wrapping_sub(self.last) & 0xff_ffff) as u64;
        self.last = now;
        self.acc * 1000 / PM_HZ
    }
    fn us(&mut self) -> u64 {
        self.ms();
        self.acc * 1_000_000 / PM_HZ
    }
}

fn ticks() -> u64 {
    time::TICKS.load(Ordering::Relaxed)
}

/// Wait for `done`, at most `ms`: yielding (the waiting task is one more
/// task in the queue) or spinning with interrupts on (only a tick can then
/// take the CPU from it).
fn wait_for(done: impl Fn() -> bool, ms: u64, yielding: bool) -> bool {
    let mut c = Clock::new();
    loop {
        if done() {
            return true;
        }
        if c.ms() >= ms {
            return false;
        }
        if yielding {
            sched::yield_now();
        } else {
            core::hint::spin_loop();
        }
    }
}

/// Yield until every task in `ids` is gone from the table (at most `ms`).
fn reap(ids: &[Tid], ms: u64) -> bool {
    wait_for(|| ids.iter().all(|&id| task::state(id).is_none()), ms, true)
}

// ── This vehicle's own helpers for the machine ───────────────────────────

/// CR0.EM = 0, CR0.TS = 0, CR0.MP = 1; CR4.OSFXSR = 1, CR4.OSXMMEXCPT = 1;
/// `fninit`; MXCSR = 0x1F80 — what `user::enter` does before a program.
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

fn cr0_cr4() -> (u64, u64) {
    let (a, b): (u64, u64);
    // SAFETY: reading control registers has no side effect.
    unsafe { asm!("mov {}, cr0", "mov {}, cr4", out(reg) a, out(reg) b, options(nomem, nostack, preserves_flags)) };
    (a, b)
}

/// The master 8259's in-service register (OCW3 0x0b, then back to the IRR).
fn master_isr() -> u8 {
    serial::out8(0x20, 0x0b);
    let v = serial::in8(0x20);
    serial::out8(0x20, 0x0a);
    v
}

fn rdmsr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    // SAFETY: reading an MSR the CPU implements has no side effect.
    unsafe { asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags)) };
    (hi as u64) << 32 | lo as u64
}

fn wrmsr(msr: u32, v: u64) {
    // SAFETY: FS_BASE only, with a canonical value.
    unsafe { asm!("wrmsr", in("ecx") msr, in("eax") v as u32, in("edx") (v >> 32) as u32, options(nostack, preserves_flags)) };
}

const FS_BASE: u32 = 0xC000_0100;

/// xmm0 = (w[0], w[1]), xmm1 = (w[2], w[3]). The kernel is soft-float:
/// nothing between this and `get_xmm` in the same task touches them.
fn set_xmm(w: &[u64; 4]) {
    // SAFETY: SSE is on (`sse_on`); reads 32 bytes of `w`.
    unsafe { asm!("movdqu xmm0, [{p}]", "movdqu xmm1, [{p} + 16]", p = in(reg) w.as_ptr(), options(nostack, readonly, preserves_flags)) };
}

fn get_xmm() -> [u64; 4] {
    let mut w = [0u64; 4];
    // SAFETY: SSE is on; writes 32 bytes of `w`.
    unsafe { asm!("movdqu [{p}], xmm0", "movdqu [{p} + 16], xmm1", p = in(reg) w.as_mut_ptr(), options(nostack, preserves_flags)) };
    w
}

// ── task.preempt.counter ─────────────────────────────────────────────────

const GOAL: u64 = 3;
const STALL_MS: u64 = 500;
static CNT: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static SEEN: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static STALLED: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static CDONE: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static CTID: [AtomicU64; 2] = [AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX)];
static CSTOP: AtomicBool = AtomicBool::new(false);

/// Count, never yield; note each time the other task's count moved between
/// two of this task's own steps — which only happens if something took the
/// CPU from this task and gave it to the other one.
fn count(me: usize) -> ! {
    let o = 1 - me;
    CTID[me].store(sched::current().0, Ordering::SeqCst);
    let mut clock = Clock::new();
    let mut last = CNT[o].load(Ordering::SeqCst);
    let mut last_seen_ms = 0;
    let mut i: u64 = 0;
    loop {
        CNT[me].fetch_add(1, Ordering::SeqCst);
        let v = CNT[o].load(Ordering::SeqCst);
        if v != last {
            last = v;
            SEEN[me].fetch_add(1, Ordering::SeqCst);
            last_seen_ms = clock.ms();
        }
        if (SEEN[me].load(Ordering::SeqCst) >= GOAL && SEEN[o].load(Ordering::SeqCst) >= GOAL) || CSTOP.load(Ordering::SeqCst) {
            break;
        }
        i += 1;
        if i % 256 == 0 {
            let now = clock.ms();
            if now - last_seen_ms > STALL_MS {
                STALLED[me].store(now - last_seen_ms, Ordering::SeqCst);
                CSTOP.store(true, Ordering::SeqCst);
                break;
            }
        }
    }
    CDONE[me].store(true, Ordering::SeqCst);
    task::exit()
}

fn count_a() {
    count(0);
}
fn count_b() {
    count(1);
}

ktest!("task.preempt.counter", fn preempt_counter(t: &mut TestCtx) {
    let t0 = ticks();
    let a = task::spawn(count_a);
    let b = task::spawn(count_b);
    let both = wait_for(|| CDONE[0].load(Ordering::SeqCst) && CDONE[1].load(Ordering::SeqCst), 3000, true);
    let gone = reap(&[a, b], 1000);
    let (ca, cb) = (CNT[0].load(Ordering::SeqCst), CNT[1].load(Ordering::SeqCst));
    let (sa, sb) = (SEEN[0].load(Ordering::SeqCst), SEEN[1].load(Ordering::SeqCst));
    t.note(format_args!(
        "task.preempt.counter: two tasks that never yield ({a:?}, {b:?}); A counted {ca} and saw B's count move {sa} times, B counted {cb} and saw A's move {sb} times, over {} ticks",
        ticks() - t0
    ));
    if !both {
        t.fail(format_args!(
            "the two counting tasks had not both finished 3 s after spawn (A ran: {}, B ran: {}): a spawned task that never yields was not given the CPU back and forth",
            ca > 0, cb > 0
        ));
    }
    for (me, name, other) in [(0usize, "A", "B"), (1, "B", "A")] {
        let st = STALLED[me].load(Ordering::SeqCst);
        if st > 0 {
            t.fail(format_args!(
                "task {name} looped {st} ms without seeing task {other}'s count move: nothing took the CPU from a task that never yields — the tick did not switch tasks"
            ));
        }
    }
    if both && (sa < GOAL || sb < GOAL) {
        t.fail(format_args!("each task should see the other's count move at least {GOAL} times; A saw {sa}, B saw {sb}"));
    }
    t.expect_eq("sched::current() inside task A, against spawn's id for it", CTID[0].load(Ordering::SeqCst), a.0);
    t.expect_eq("sched::current() inside task B, against spawn's id for it", CTID[1].load(Ordering::SeqCst), b.0);
    t.expect(gone, "both counting tasks gone from the task table within 1 s of finishing");
});

// ── task.eoi_before_switch ───────────────────────────────────────────────

static P_ISR: AtomicU64 = AtomicU64::new(u64::MAX);
static P_ISR2: AtomicU64 = AtomicU64::new(u64::MAX);

/// Its first instructions read the PIC: a task the tick STARTS finds IRQ 0
/// still in service if the tick switched before its EOI. It then yields to
/// the boot task, which spins, so only a tick can bring it back: the first
/// thing it does after `yield_now` returns is read the PIC again, as a task
/// the tick RESUMES (a kernel that acknowledges IRQ 0 on the way into a new
/// task, and late everywhere else, passes the first read and not this one).
fn probe_isr() {
    P_ISR.store(master_isr() as u64, Ordering::SeqCst);
    sched::yield_now();
    P_ISR2.store(master_isr() as u64, Ordering::SeqCst);
    task::exit()
}

ktest!("task.eoi_before_switch", fn eoi_before_switch(t: &mut TestCtx) {
    let p = task::spawn(probe_isr);
    // Spin, never yield: the only way into the task, either time, is a tick.
    let ran = wait_for(|| P_ISR.load(Ordering::SeqCst) != u64::MAX, 1000, false);
    let back = ran && wait_for(|| P_ISR2.load(Ordering::SeqCst) != u64::MAX, 1000, false);
    let gone = reap(&[p], 1000);
    let (isr, isr2) = (P_ISR.load(Ordering::SeqCst), P_ISR2.load(Ordering::SeqCst));
    if !ran {
        t.fail(format_args!(
            "a spawned task never ran in 1 s while the boot task spun with interrupts on: no tick switched to it"
        ));
        return;
    }
    t.note(format_args!(
        "task.eoi_before_switch: the master PIC's in-service register, read first by a task the tick started: {isr:#04x}; read first after the tick resumed it from yield_now: {}",
        if back { alloc::format!("{isr2:#04x}") } else { String::from("never resumed") }
    ));
    if isr & 1 != 0 {
        t.fail(format_args!(
            "the tick switched tasks before eoi(0): a new task ran with IRQ 0 still in service (ISR = {isr:#04x}), so the PIC gives it no tick of its own"
        ));
    }
    if !back {
        t.fail(format_args!(
            "a task that yielded to the boot task was not given the CPU back by a tick in 1 s while the boot task spun with interrupts on"
        ));
    } else if isr2 & 1 != 0 {
        t.fail(format_args!(
            "the tick switched tasks before eoi(0): a task it resumed returned from yield_now with IRQ 0 still in service (ISR = {isr2:#04x}), so the PIC gives it no tick of its own"
        ));
    }
    t.expect(gone, "the probe task gone from the task table within 1 s of returning");
});

// ── task.1000.switches ───────────────────────────────────────────────────

static TOKEN: AtomicU64 = AtomicU64::new(0);
static HAND: AtomicU64 = AtomicU64::new(0);
static BOUND: AtomicU64 = AtomicU64::new(u64::MAX);
static PDONE: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
/// The yields each token task made, stored as it finishes.
static PYIELDS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static PSTOP: AtomicBool = AtomicBool::new(false);
static PTIMEOUT: AtomicBool = AtomicBool::new(false);
static BSTOP: AtomicBool = AtomicBool::new(false);
static BRUNS: AtomicU64 = AtomicU64::new(0);

/// Pass the token back and forth through `yield_now`: a handoff is counted
/// only by the task holding the token, and the other must run to hand it
/// back, so every count is a switch that happened.
fn pingpong(me: u64) -> ! {
    let mut clock = Clock::new();
    let mut i: u64 = 0;
    while !PSTOP.load(Ordering::SeqCst) {
        if TOKEN.load(Ordering::SeqCst) == me {
            let h = HAND.fetch_add(1, Ordering::SeqCst) + 1;
            if h >= BOUND.load(Ordering::SeqCst) {
                PSTOP.store(true, Ordering::SeqCst);
            }
            TOKEN.store(1 - me, Ordering::SeqCst);
        }
        sched::yield_now();
        i += 1;
        if i % 64 == 0 && clock.ms() > 4000 {
            PTIMEOUT.store(true, Ordering::SeqCst);
            PSTOP.store(true, Ordering::SeqCst);
        }
    }
    PYIELDS[me as usize].store(i, Ordering::SeqCst);
    PDONE[me as usize].store(true, Ordering::SeqCst);
    task::exit()
}

fn ping() {
    pingpong(0);
}
fn pong() {
    pingpong(1);
}

/// The boot task's only partner: yield straight back, every time it runs.
fn bounce() {
    while !BSTOP.load(Ordering::SeqCst) {
        BRUNS.fetch_add(1, Ordering::SeqCst);
        sched::yield_now();
    }
    task::exit()
}

static TSTOP: AtomicBool = AtomicBool::new(false);
static TRUNS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// Two more partners for the boot task, each yielding straight back and
/// counting its own runs: with them, the tick switches among three tasks.
fn trio(me: usize) {
    while !TSTOP.load(Ordering::SeqCst) {
        TRUNS[me].fetch_add(1, Ordering::SeqCst);
        sched::yield_now();
    }
    task::exit()
}
fn trio0() {
    trio(0);
}
fn trio1() {
    trio(1);
}

fn truns() -> u64 {
    TRUNS[0].load(Ordering::SeqCst) + TRUNS[1].load(Ordering::SeqCst)
}

/// The PIT's channel 0 as a rate generator with this reload value (0 = 65536).
fn pit(divisor: u16) {
    serial::out8(0x43, 0x34);
    serial::out8(0x40, (divisor & 0xff) as u8);
    serial::out8(0x40, (divisor >> 8) as u8);
}

const SLOW_HZ: u64 = 19; // 1193182 / 65536 = 18.2, rounded up
const TICK_SPIN_MS: u64 = 100;
/// Ticks the boot task waits through, spinning alone, at 1000 Hz.
const LONE_TICKS: u64 = 20;

/// Switches beyond two per partner run that an honest count may show: a
/// tick that lands while the partner runs, before it counts itself, takes
/// it out and back in without a run being counted. Measured 2026-10-19:
/// none in 24 boots of eight honest kernels (r = 64-67 each).
const TICK_SLACK: u64 = 2;

// The whole row runs with the PIT slowed to 18.2 Hz: a handoff that rides a
// tick instead of a yield then makes at most a few dozen in the time the
// row allows, far under the 500 it asks for at least, so the row needs no
// count of the learner's own to tell the two apart. The rate goes back to
// the kernel's 1000 Hz before the row returns.
ktest!("task.1000.switches", fn thousand_switches(t: &mut TestCtx) {
    let bound = t.derived("task.1000.switches") % 1001 + 500;
    BOUND.store(bound, Ordering::SeqCst);
    pit(0);
    let s0 = sched::switches();
    let mut c = Clock::new();
    let x = task::spawn(ping);
    let y = task::spawn(pong);
    // This task's own yields, as the token tasks count theirs: `live` while
    // a token task has not finished (it is Ready, so each one switches),
    // `all` in total, the reap's included.
    let (mut live, mut all) = (0u64, 0u64);
    let mut both = false;
    loop {
        if PDONE[0].load(Ordering::SeqCst) && PDONE[1].load(Ordering::SeqCst) {
            both = true;
            break;
        }
        if c.ms() >= 5000 {
            break;
        }
        sched::yield_now();
        live += 1;
        all += 1;
    }
    let us = c.us();
    PSTOP.store(true, Ordering::SeqCst);
    let r0 = c.ms();
    let gone = loop {
        if task::state(x).is_none() && task::state(y).is_none() {
            break true;
        }
        if c.ms() - r0 >= 1000 {
            break false;
        }
        sched::yield_now();
        all += 1;
    };
    let wms = c.ms();
    let h = HAND.load(Ordering::SeqCst);
    let sw = sched::switches().wrapping_sub(s0);
    let (ip, iq) = (PYIELDS[0].load(Ordering::SeqCst), PYIELDS[1].load(Ordering::SeqCst));
    // Each token task's yields switch (this task is Ready while it runs), and
    // so do its exit and this task's live yields; at most, every other yield
    // of this task switched too, and so did every tick of that time.
    let lo = ip + iq + 2 + live;
    let hi = ip + iq + 2 + all + (wms * SLOW_HZ / 1000 + 2) + TICK_SLACK;
    // (The PM timer wraps in 4.7 s, so the time is only read off it when the tasks finished.)
    let took = if both { alloc::format!("{us} us") } else { String::from("over 5 s") };
    t.note(format_args!(
        "task.1000.switches: at least {bound} handoffs asked, with the PIT at 18.2 Hz; two tasks passing a token through yield_now made {h} in {took} (sched::switches() moved {sw})"
    ));
    let mut fine = both && gone;
    if !both {
        t.fail(format_args!("the two yielding tasks had not both finished 5 s after spawn ({h} handoffs made)"));
    }
    if PTIMEOUT.load(Ordering::SeqCst) || h < bound {
        fine = false;
        t.fail(format_args!(
            "{h} handoffs in 4 s, {bound} asked: yield_now did not pass the CPU between two tasks often enough — with the clock at 18.2 ticks a second, a yield that waits for the tick makes about {}",
            4 * SLOW_HZ
        ));
    }
    t.note(format_args!(
        "task.1000.switches: the two token tasks yielded {ip} and {iq} times and this task {live} times while they ran ({all} in all, over {wms} ms)"
    ));
    if fine && !PTIMEOUT.load(Ordering::SeqCst) && (sw < lo || sw > hi) {
        fine = false;
        t.fail(format_args!(
            "{} yields by the two token tasks, {live} by this task while they ran, and their two exits are {lo} switches; with this task's other yields and every tick of those {wms} ms, at most {hi}; but sched::switches() moved {sw}: switches() does not count each switch once",
            ip + iq
        ));
    }
    t.expect(gone, "both yielding tasks gone from the task table within 1 s of finishing");
    // switches() from above: with nobody else ready a yield switches to no
    // one, so the count stays put; with one partner that yields straight
    // back, each of this task's yields is two switches, plus two for each
    // tick that lands while this task runs between yields (at 18.2 Hz).
    // Counted on the way out or on the way in, the totals are the same.
    if fine {
        let n = 64 + bound % 64;
        let others = task::with_table(|tt| tt.tasks.iter().filter(|x| x.id != tt.current && x.state == State::Ready).count());
        let a0 = sched::switches();
        for _ in 0..n {
            sched::yield_now();
        }
        let alone = sched::switches().wrapping_sub(a0);
        // Ticks with nobody else ready: back at 1000 Hz, this task spins
        // without yielding through LONE_TICKS ticks, and each one finds no
        // task to switch to, so the count stays put.
        pit(irq::PIT_DIVISOR);
        let l0 = sched::switches();
        let t0 = ticks();
        let mut lc = Clock::new();
        while ticks().wrapping_sub(t0) < LONE_TICKS && lc.ms() < 500 {
            core::hint::spin_loop();
        }
        let lone = sched::switches().wrapping_sub(l0);
        let lticks = ticks().wrapping_sub(t0);
        let lms = lc.ms();
        pit(0);
        let b = task::spawn(bounce);
        let b0 = sched::switches();
        let mut c = Clock::new();
        let mut done = 0;
        while done < n && c.ms() < 1000 {
            sched::yield_now();
            done += 1;
        }
        let ms = c.ms();
        let pair = sched::switches().wrapping_sub(b0);
        // The tick's switches too: back at the kernel's own rate, this task
        // spins without yielding while the partner is Ready, so each of the
        // partner's runs is one switch the tick made and one its yield made.
        // The partner counts its own runs, so this reads no counter of the
        // kernel's but switches().
        pit(irq::PIT_DIVISOR);
        let r0 = BRUNS.load(Ordering::SeqCst);
        let k0 = sched::switches();
        let mut sc = Clock::new();
        while sc.ms() < TICK_SPIN_MS {
            core::hint::spin_loop();
        }
        let ticked = sched::switches().wrapping_sub(k0);
        let r = BRUNS.load(Ordering::SeqCst).wrapping_sub(r0);
        // The exit's switch: told to stop, the partner ends at its next run,
        // and this task spins without yielding until it has: the tick's switch
        // in and the exit's switch out, 2, and up to TICK_SLACK more when a
        // tick lands first or the exit hands the CPU on through another task.
        let e0 = sched::switches();
        BSTOP.store(true, Ordering::SeqCst);
        let ended = wait_for(|| matches!(task::state(b), None | Some(State::Dead)), 1000, false);
        let exited = sched::switches().wrapping_sub(e0);
        let bgone = reap(&[b], 1000);
        let slack = 2 * (ms * SLOW_HZ / 1000 + 2);
        t.note(format_args!(
            "task.1000.switches: {n} yields with no other task ready moved sched::switches() by {alone}; {done} yields to one partner that yields straight back moved it by {pair} in {ms} ms ({} expected, up to {} with the ticks of that time)",
            2 * done, 2 * done + slack
        ));
        t.note(format_args!(
            "task.1000.switches: with no other task ready, {lticks} ticks at 1000 Hz in {lms} ms while this task spun moved sched::switches() by {lone}"
        ));
        if others != 0 {
            t.fail(format_args!("with every task this run spawned gone, {others} other task(s) in the table were still Ready"));
        } else if alone != 0 {
            t.fail(format_args!(
                "{n} calls to yield_now with no other task ready moved sched::switches() by {alone}: switches() counts something other than switches"
            ));
        } else if lticks < LONE_TICKS {
            t.fail(format_args!(
                "with the PIT at 1000 Hz, time::TICKS moved {lticks} in 500 ms while this task spun with interrupts on"
            ));
        } else if lone != 0 {
            t.fail(format_args!(
                "{lticks} ticks with no other task ready switched nothing, but sched::switches() moved {lone}: switches() counts ticks, not switches"
            ));
        }
        if done < n {
            t.fail(format_args!("{done} of {n} yields to a partner that yields straight back took 1 s"));
        } else if pair < 2 * done || pair > 2 * done + slack {
            t.fail(format_args!(
                "{done} yields to a partner that yields straight back are {} switches, {} with every tick of those {ms} ms, but sched::switches() moved {pair}: switches() does not count each switch once",
                2 * done, 2 * done + slack
            ));
        }
        t.note(format_args!(
            "task.1000.switches: while this task spun {TICK_SPIN_MS} ms without yielding, the partner ran {r} times (each one switch by the tick, one by its yield) and sched::switches() moved {ticked}"
        ));
        // r = 0: the tick switches nothing, which is task.preempt.counter's
        // to say; this row counts the switches a tick makes, when it makes them.
        if r > 0 && ticked < 2 * r {
            t.fail(format_args!(
                "the partner ran {r} times while this task spun without yielding, {} switches, but sched::switches() moved {ticked}: switches() does not count the switches the tick makes",
                2 * r
            ));
        } else if r > 0 && ticked > 2 * r + TICK_SLACK {
            t.fail(format_args!(
                "the partner ran {r} times while this task spun without yielding, {} switches, but sched::switches() moved {ticked}: switches() counts more than the switches",
                2 * r
            ));
        }
        t.note(format_args!(
            "task.1000.switches: from telling the partner to stop until it had ended, this task spinning, sched::switches() moved {exited}"
        ));
        if !ended {
            t.fail(format_args!("the partner had not ended 1 s after it was told to stop, while this task spun without yielding"));
        } else if exited < 2 {
            t.fail(format_args!(
                "the partner ran and ended while this task spun, at least 2 switches (the tick's in, the end's out), but sched::switches() moved {exited}: switches() does not count the switch out of a task that ends"
            ));
        } else if exited > 2 + TICK_SLACK {
            t.fail(format_args!(
                "the partner ran and ended while this task spun, 2 switches and at most {} with the ticks, but sched::switches() moved {exited}: switches() counts more than the switches",
                2 + TICK_SLACK
            ));
        }
        t.expect(bgone, "the partner task gone from the task table within 1 s of finishing");
        // The tick among three tasks: two partners that yield straight back,
        // and this task spinning without yielding at 1000 Hz. Each switch is
        // a partner's yield, which ends one of its counted runs, or a tick's:
        // so switches() moves at least once per run plus once per time this
        // task saw the partners' count move (the tick took it out each time),
        // and at most once per run plus twice per tick of that time (a tick
        // handler that runs the pick before and after its EOI switches twice;
        // task.eoi_before_switch is the row that refuses it), give or take the
        // two runs a window edge can split from their yields.
        let c1 = task::spawn(trio0);
        let c2 = task::spawn(trio1);
        let mut tc = Clock::new();
        let t0 = sched::switches();
        let q0 = truns();
        let (mut seen, mut outs) = (q0, 0u64);
        while tc.ms() < TICK_SPIN_MS {
            let q = truns();
            if q != seen {
                outs += 1;
                seen = q;
            }
        }
        let q = truns().wrapping_sub(q0);
        let tsw = sched::switches().wrapping_sub(t0);
        let tms = tc.ms();
        TSTOP.store(true, Ordering::SeqCst);
        let cgone = reap(&[c1, c2], 1000);
        let tlo = (q + outs).saturating_sub(2);
        let thi = q + 2 + 2 * (tms + 2);
        t.note(format_args!(
            "task.1000.switches: while this task spun {tms} ms without yielding beside two partners, they ran {q} times, the tick took this task out at least {outs} times, and sched::switches() moved {tsw}"
        ));
        // outs = 0: the tick switches nothing, which is task.preempt.counter's to say.
        if outs > 0 && tsw < tlo {
            t.fail(format_args!(
                "two partners ran {q} times and the tick took this task out {outs} times while it spun, at least {tlo} switches, but sched::switches() moved {tsw}: switches() does not count the switches the tick makes among three tasks"
            ));
        } else if outs > 0 && tsw > thi {
            t.fail(format_args!(
                "two partners ran {q} times in {tms} ms while this task spun, at most {thi} switches with two for every tick of that time, but sched::switches() moved {tsw}: switches() counts more than the switches"
            ));
        }
        t.expect(cgone, "the two partner tasks gone from the task table within 1 s of finishing");
    }
    pit(irq::PIT_DIVISOR);
});

// ── task.fs_fxsave.saved ─────────────────────────────────────────────────

const FX_ROUNDS: u64 = 8;
static FSV: AtomicU64 = AtomicU64::new(0);
static XMM: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
static FS_GOT: AtomicU64 = AtomicU64::new(0);
static XMM_GOT: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
static B_RUNS: AtomicU64 = AtomicU64::new(0);
static FDONE: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static FSTOP: AtomicBool = AtomicBool::new(false);
static A_SWITCHES: AtomicU64 = AtomicU64::new(0);

fn want_xmm() -> [u64; 4] {
    [0, 1, 2, 3].map(|i| XMM[i].load(Ordering::SeqCst))
}

/// Wait in place, interrupts on, until the tick has come twice (or 5 ms by
/// the PM timer): long enough for the clock to take the CPU away.
fn linger() {
    let t = ticks();
    let mut c = Clock::new();
    while ticks() < t + 2 && c.ms() < 5 {
        core::hint::spin_loop();
    }
}

/// Task A: set the FS base and xmm0/xmm1, let task B (which overwrites
/// both) run between its own steps — by yielding and by being preempted —
/// then read them back.
fn fx_a() {
    let fsv = FSV.load(Ordering::SeqCst);
    let want = want_xmm();
    wrmsr(FS_BASE, fsv);
    set_xmm(&want);
    let start = B_RUNS.load(Ordering::SeqCst);
    let s0 = sched::switches();
    let mut c = Clock::new();
    while B_RUNS.load(Ordering::SeqCst) < start + FX_ROUNDS && c.ms() < 2000 {
        sched::yield_now();
        linger();
    }
    A_SWITCHES.store(sched::switches() - s0, Ordering::SeqCst);
    FS_GOT.store(rdmsr(FS_BASE), Ordering::SeqCst);
    let got = get_xmm();
    for (s, v) in XMM_GOT.iter().zip(got) {
        s.store(v, Ordering::SeqCst);
    }
    FSTOP.store(true, Ordering::SeqCst);
    FDONE[0].store(true, Ordering::SeqCst);
    task::exit()
}

/// Task B: the other values, written again every time it runs.
fn fx_b() {
    let fsv = FSV.load(Ordering::SeqCst) ^ 0x0000_0f0f_0f0f_0f00;
    let other = want_xmm().map(|w| !w);
    let mut c = Clock::new();
    while !FSTOP.load(Ordering::SeqCst) && c.ms() < 3000 {
        wrmsr(FS_BASE, fsv);
        set_xmm(&other);
        B_RUNS.fetch_add(1, Ordering::SeqCst);
        sched::yield_now();
        linger();
    }
    FDONE[1].store(true, Ordering::SeqCst);
    task::exit()
}

ktest!("task.fs_fxsave.saved", fn fs_fxsave(t: &mut TestCtx) {
    let fsv = (t.derived("task.fs_fxsave.saved") & 0x0000_7fff_ffff_fff0) | 0x10;
    FSV.store(fsv, Ordering::SeqCst);
    for (i, s) in XMM.iter().enumerate() {
        let id = ["task.fs_fxsave.saved/xmm0.lo", "task.fs_fxsave.saved/xmm0.hi", "task.fs_fxsave.saved/xmm1.lo", "task.fs_fxsave.saved/xmm1.hi"][i];
        s.store(t.derived(id), Ordering::SeqCst);
    }
    let a = task::spawn(fx_a);
    let b = task::spawn(fx_b);
    let both = wait_for(|| FDONE[0].load(Ordering::SeqCst) && FDONE[1].load(Ordering::SeqCst), 4000, true);
    let gone = reap(&[a, b], 1000);
    let want = want_xmm();
    let got: [u64; 4] = [0, 1, 2, 3].map(|i| XMM_GOT[i].load(Ordering::SeqCst));
    let fs = FS_GOT.load(Ordering::SeqCst);
    let runs = B_RUNS.load(Ordering::SeqCst);
    t.note(format_args!(
        "task.fs_fxsave.saved: task A set fs_base = {fsv:#x}, xmm0 = {:016x}{:016x}, xmm1 = {:016x}{:016x}; task B overwrote both {runs} times; A saw {} switches",
        want[1], want[0], want[3], want[2], A_SWITCHES.load(Ordering::SeqCst)
    ));
    if !both {
        t.fail(format_args!("the two tasks had not both finished 4 s after spawn"));
        return;
    }
    if runs < FX_ROUNDS {
        t.fail(format_args!("task B ran {runs} times while task A waited, fewer than {FX_ROUNDS}: the two did not take turns"));
    }
    if fs != fsv {
        t.fail(format_args!(
            "task A wrote fs_base = {fsv:#x} and read back {fs:#x} after task B ran: the switch does not save and restore the FS base (MSR 0xC000_0100) per task"
        ));
    }
    if got != want {
        t.fail(format_args!(
            "task A loaded xmm0 = {:016x}{:016x}, xmm1 = {:016x}{:016x} and read back xmm0 = {:016x}{:016x}, xmm1 = {:016x}{:016x}: the switch does not save and restore the SSE registers per task (FXSAVE/FXRSTOR)",
            want[1], want[0], want[3], want[2], got[1], got[0], got[3], got[2]
        ));
    }
    t.expect(gone, "both tasks gone from the task table within 1 s of finishing");
});

// ── task.switch.callee_saved ─────────────────────────────────────────────

const CS_ID: &str = "task.switch.callee_saved";
const CS_NAMES: [&str; 6] = ["rbx", "rbp", "r12", "r13", "r14", "r15"];
/// What the partner leaves in each register before it switches back.
const CS_JUNK: [u64; 6] = [
    0xdead_0000_0000_0b00,
    0xdead_0000_0000_0b01,
    0xdead_0000_0000_0b02,
    0xdead_0000_0000_0b03,
    0xdead_0000_0000_0b04,
    0xdead_0000_0000_0b05,
];
/// Two places `switch` saves a stack pointer to: task 0's while the partner
/// runs, and the partner's when it leaves for good. 64 bytes each, more than
/// a `Context` needs.
static CS_PARK: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
static CS_SCRATCH: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
static CS_B_RAN: AtomicU64 = AtomicU64::new(0);

/// The partner: entered once, through the frame the learner's `spawn` built
/// and the learner's way into a new task; it puts other values in every
/// callee-saved register and switches straight back to task 0. Never resumed.
fn cs_partner() {
    CS_B_RAN.fetch_add(1, Ordering::SeqCst);
    // SAFETY: interrupts off; CS_PARK holds what `switch` saved for task 0,
    // and CS_SCRATCH is room for this task's stack pointer, never used again.
    unsafe {
        asm!("cli", options(nomem, nostack));
        asm!(
            "mov rbx, [{j}]", "mov rbp, [{j} + 8]", "mov r12, [{j} + 16]",
            "mov r13, [{j} + 24]", "mov r14, [{j} + 32]", "mov r15, [{j} + 40]",
            "call {sw}",
            "ud2",
            j = in(reg) CS_JUNK.as_ptr(),
            sw = sym task::switch,
            in("rdi") CS_SCRATCH.as_ptr(),
            in("rsi") CS_PARK.as_ptr(),
            options(noreturn),
        );
    }
}

/// Load `seed` into rbx, rbp, r12–r15, call `task::switch(from, to)`, and
/// store what those six registers hold when it returns into `got`. The
/// vehicle's own rbx and rbp are kept on its stack, so whatever `switch`
/// does to them, this function returns with its caller's.
unsafe fn canary_switch(from: *mut u64, to: *const u64, seed: &[u64; 6], got: &mut [u64; 6]) {
    // SAFETY: the caller's contract (interrupts off, `to` a frame `spawn`
    // built); the asm restores rbx and rbp itself and declares the rest.
    unsafe {
        asm!(
            "push rbx", "push rbp", "push {g}", "push {g}",
            "mov rbx, [{s}]", "mov rbp, [{s} + 8]", "mov r12, [{s} + 16]",
            "mov r13, [{s} + 24]", "mov r14, [{s} + 32]", "mov r15, [{s} + 40]",
            "call {sw}",
            "mov rax, [rsp]",
            "mov [rax], rbx", "mov [rax + 8], rbp", "mov [rax + 16], r12",
            "mov [rax + 24], r13", "mov [rax + 32], r14", "mov [rax + 40], r15",
            "add rsp, 16", "pop rbp", "pop rbx",
            s = in(reg) seed.as_ptr(),
            g = in(reg) got.as_mut_ptr(),
            sw = sym task::switch,
            in("rdi") from,
            in("rsi") to,
            out("r12") _, out("r13") _, out("r14") _, out("r15") _,
            clobber_abi("C"),
        );
    }
}

// Six nonce values go into rbx, rbp and r12–r15; task 0 calls the
// learner's `task::switch` directly — not through `yield_now`, whose own
// prologue would save them — into a fresh task that overwrites all six and
// switches straight back. Each must come back as it went in. The tick is
// masked at the PIC meanwhile, and the table's bookkeeping (who is current,
// the run queue, the states) is put back afterwards, whatever the learner's
// way into a new task did to it.
ktest!("task.switch.callee_saved", fn callee_saved(t: &mut TestCtx) {
    let mut seed = [0u64; 6];
    for (i, s) in seed.iter_mut().enumerate() {
        let id = [
            "task.switch.callee_saved/rbx",
            "task.switch.callee_saved/rbp",
            "task.switch.callee_saved/r12",
            "task.switch.callee_saved/r13",
            "task.switch.callee_saved/r14",
            "task.switch.callee_saved/r15",
        ][i];
        *s = t.derived(id) | 1;
    }
    // No tick while the two tasks trade the CPU outside the scheduler.
    let mask = serial::in8(0x21);
    serial::out8(0x21, mask | 1);
    let b = task::spawn(cs_partner);
    // Out of the run queue (only this row enters it), and what the table
    // says now, to be put back.
    let saved = task::with_table(|tt| {
        tt.ready.retain(|&id| id != b);
        let ctx = tt.get_mut(b).map(|x| &mut x.ctx as *mut task::Context as u64).unwrap_or(0);
        let states: Vec<(Tid, State)> = tt.tasks.iter().filter(|x| x.id != b).map(|x| (x.id, x.state)).collect();
        (ctx, tt.current, tt.ready.clone(), states)
    });
    let (bctx, cur, ready, states) = saved;
    let mut got = [0u64; 6];
    if bctx != 0 {
        // SAFETY: interrupts off and IRQ 0 masked; bctx is the context
        // spawn built, in the table, whose task nothing else will run.
        unsafe {
            asm!("cli", options(nomem, nostack));
            canary_switch(CS_PARK.as_ptr() as *mut u64, bctx as *const u64, &seed, &mut got);
        }
    }
    task::with_table(|tt| {
        tt.current = cur;
        tt.ready = ready;
        for (id, st) in states {
            if let Some(x) = tt.get_mut(id) {
                x.state = st;
            }
        }
        if let Some(x) = tt.get_mut(b) {
            x.state = State::Dead;
        }
    });
    serial::out8(0x21, mask);
    // SAFETY: back to interrupts on, as the row found them.
    unsafe { asm!("sti", options(nomem, nostack)) };
    let ran = CS_B_RAN.load(Ordering::SeqCst);
    if bctx == 0 {
        t.fail(format_args!("{CS_ID}: the task spawn returned ({b:?}) is not in the task table"));
        return;
    }
    let bad = (0..6).filter(|&i| got[i] != seed[i]).count();
    t.note(format_args!(
        "{CS_ID}: six nonce values in rbx, rbp, r12-r15, a direct task::switch into a task that overwrote all six and switched back; {} of 6 came back unchanged (the partner ran {ran} time(s))",
        6 - bad
    ));
    for i in 0..6 {
        if got[i] == seed[i] {
            continue;
        }
        let why = if got[i] == CS_JUNK[i] {
            String::from("the value the other task left in it: switch does not save and restore it")
        } else if let Some(j) = (0..6).find(|&j| j != i && got[i] == seed[j]) {
            alloc::format!("the value this task had put in {}: switch pops the saved registers in a different order than it pushed them", CS_NAMES[j])
        } else if got[i] == 0 {
            String::from("zero: switch clears it, or does not save and restore it")
        } else {
            String::from("neither: switch does not save and restore it")
        };
        t.fail(format_args!(
            "{CS_ID}: {} held {:#018x} before task::switch and {:#018x} when the switch came back — {why}",
            CS_NAMES[i], seed[i], got[i]
        ));
    }
});

// ── task.return.dies (after the registered rows) ─────────────────────────

const RETURN_ID: &str = "task.return.dies";
static R_RAN: AtomicBool = AtomicBool::new(false);
static S_RAN: AtomicBool = AtomicBool::new(false);

fn returns() {
    R_RAN.store(true, Ordering::SeqCst);
}

fn after() {
    S_RAN.store(true, Ordering::SeqCst);
}

/// `task.return.dies` (plain), KTAP number `n`, printed here.
fn return_dies(n: usize) {
    let mut failed = false;
    let r = task::spawn(returns);
    let ran = wait_for(|| R_RAN.load(Ordering::SeqCst), 1000, true);
    let mut c = Clock::new();
    let gone = reap(&[r], 1000);
    let ms = c.ms();
    if !ran {
        failed = true;
        ktap::note(format_args!("{RETURN_ID}: a spawned task never ran within 1 s"));
    } else if !gone {
        failed = true;
        ktap::note(format_args!(
            "{RETURN_ID}: the task returned from its function and is still in the task table 1 s later (state {:?}): returning does not end a task, or nothing frees a dead one",
            task::state(r)
        ));
    } else {
        ktap::note(format_args!("{RETURN_ID}: the task returned from its function; it was gone from the task table {ms} ms later"));
    }
    // A second task after the first: the kernel still schedules.
    let s = task::spawn(after);
    if !wait_for(|| S_RAN.load(Ordering::SeqCst), 1000, true) {
        failed = true;
        ktap::note(format_args!("{RETURN_ID}: a task spawned after that one never ran within 1 s"));
    }
    reap(&[s], 1000);
    let cur = sched::current();
    if cur != Tid(0) {
        failed = true;
        ktap::note(format_args!("{RETURN_ID}: the boot flow asked sched::current() and got {cur:?}, not Tid(0)"));
    }
    let st = task::state(Tid(0));
    if st != Some(State::Running) {
        failed = true;
        ktap::note(format_args!("{RETURN_ID}: the boot flow's own state reads {st:?}, not Running"));
    }
    if failed {
        kernel::kprintln!("not ok {n} - {RETURN_ID}");
    } else {
        kernel::kprintln!("ok {n} - {RETURN_ID}");
    }
}

fn prelude() {
    let (k, i, _) = shim::stacks();
    gdt::init(k, i);
    idt::init();
    let bi = bootinfo::get().expect("the shim published the BootInfo");
    frame::init(bi.memmap(), bi.kernel_range());
    syscall::init();
    syscall::set_kernel_stack(k);
    heap::init(VirtAddr(HEAP_START), HEAP_PAGES);
    let (cr0, cr4) = cr0_cr4();
    ktap::note(format_args!("the boot loader left CR0 = {cr0:#x}, CR4 = {cr4:#x}; the course's test turns SSE on itself"));
    sse_on();
    sched::init(k);
    // Nothing else is ready: a yield returns at once. Here, before the first
    // tick, so a kernel under construction meets `yield_now` in one place.
    sched::yield_now();
    irq::init();
    // Two ticks with no task but this one: a tick with nobody ready changes
    // nothing, and a kernel under construction meets `tick` here, before
    // the first `spawn`.
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
    // N counts the registered rows plus `task.return.dies`, which runs last.
    let n = ktap::tests().len() + 1;
    ktap::begin(n);
    prelude();
    ktap::run_all();
    return_dies(n);
    ktap::end();
    kernel::hcf()
}
