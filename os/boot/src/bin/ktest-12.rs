//! Lab 12's vehicle: processes. The prelude is lab 11's (the GDT, the IDT,
//! the frame pool, the syscall MSRs, the heap, `sse_on`, task 0, the PIC
//! and the PIT) plus the learner's `proc::init`, which routes #UD and #GP
//! from ring 3 to `proc::kill` as a #PF goes to `mm::fault` (CONTRACT §A.9
//! S2). `kill.ud_gp.routed` observes both: a `ud2` probe and a `hlt` probe,
//! each kill line read off COM1 through an I/O breakpoint (below).
//!
//! Every program runs as a process the learner's `proc::spawn_user` built,
//! preempted by the learner's tick like any task. The vehicle (task 0) only
//! starts them and watches: `proc::each` for how each one ended, the task
//! table for its address space, the ACPI PM timer (never the learner's tick)
//! for every bound. The programs report their own rows through the
//! learner's `write(1)`, one line per call (a syscall runs with interrupts
//! off, so a line is never cut); the vehicle prints its own with interrupts
//! off for the same reason. Rows are numbered in the order they print:
//!
//!   1, 2  `ring3.cs.rpl` from the two `hi` instances (in either order)
//!   3     `proc.two.aspaces.order` (this vehicle)
//!   4     `sys.badnum.enosys` (hostile-badnum)
//!   5     `sys.badptr.efault` (hostile-badptr)
//!   6     `proc.exit.frees` (this vehicle: frames in use across programs
//!         that exit and programs that are killed, after a warm-up)
//!   7     `vma.covers.prot` (this vehicle: the learner's `VmaList` on a list
//!         the vehicle builds)
//!   8     `regs.survive.preempt` (hostile-spin, the reporter)
//!   9     `kill.spin.preempted` (this vehicle)
//!   10    `kill.kwrite` (this vehicle, after the spinners: the kernel that
//!         killed hostile-kwrite went on serving)
//!   11    `sys.badptr.no_vma` (this vehicle: a probe's `write(1, p, 16)` on
//!         a page the vehicle maps USER into its address space outside
//!         every VMA must be refused: S5, the VMA list asked before the
//!         page tables — *added 2026-10-19, s12 widening round*)
//!   12    `vma.insert.sorted` (this vehicle: the learner's
//!         `VmaList::insert` on ranges handed out of order, then an overlap,
//!         an unaligned and an empty range — *added 2026-10-19, s12
//!         widening round*)
//!   13    `kill.ud_gp.routed` (this vehicle, last: a `proc::init` that
//!         routes nothing ends the run in lab 05's default, and only this
//!         row is lost)
//!
//! `ring3.cs.rpl` appears twice. The judge looks only at the rubric's ids
//! and passes one only when exactly one result line names it, so the pair
//! touches no lab 12 grade — and `ring3.cs.rpl` must never be a lab 12 row.
//! Both lines count in the plan.
//! Before the first program runs, a kernel task the vehicle spawns reads
//! the TSS's `rsp0` off the CPU (`sgdt`, the TSS descriptor's base, as
//! ktest-04 does) and compares it with its own kernel stack's top. A
//! mismatch prints one `#` line naming lab 11's cause — the switch does
//! not point `rsp0` and the syscall entry at the next task's stack — and
//! changes no row: without it the first ring-3 trap lands on another task's
//! stack and the run ends in a #PF that names nothing (class B, TPM log
//! 2026-10-19, B12).
//! The two spinners never exit: they still run while row 11's probes do, and
//! after the last row the vehicle prints `# sboot end` and halts with
//! interrupts off, so nothing runs after it.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../shim.rs"]
mod shim;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::arch::asm;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};

use kernel::addr::{PhysAddr, VirtAddr};
use kernel::idt::{self, TrapFrame};
use kernel::ktap;
use kernel::mm::{self, KillReason};
use kernel::paging;
use kernel::proc::{self, KillLine, Pid, Vma, VmaKind, VmaList};
use kernel::task::{self, Tid};
use kernel::{bootinfo, frame, gdt, heap, irq, sched, serial, syscall, time};

static HI: &[u8] = include_bytes!("../../../user/bin/hi");
static BADNUM: &[u8] = include_bytes!("../../../user/bin/hostile-badnum");
static BADPTR: &[u8] = include_bytes!("../../../user/bin/hostile-badptr");
static KWRITE: &[u8] = include_bytes!("../../../user/bin/hostile-kwrite");
static SPIN: &[u8] = include_bytes!("../../../user/bin/hostile-spin");

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 16;
const PM_HZ: u64 = 3_579_545;

const PLAN: usize = 13;
const ORDER_ID: &str = "proc.two.aspaces.order";
const N_ORDER: usize = 3;
const N_BADNUM: &str = "4";
const N_BADPTR: &str = "5";
const FREES_ID: &str = "proc.exit.frees";
const N_FREES: usize = 6;
/// Programs per path after the warm-up; frames in use must grow by fewer.
const FREES_RUNS: usize = 8;
const VMA_ID: &str = "vma.covers.prot";
const N_VMA: usize = 7;
const REGS_ID: &str = "regs.survive.preempt";
const N_REGS: usize = 8;
const SPIN_ID: &str = "kill.spin.preempted";
/// The most the median wait for a turn may be beside the two spinners. An
/// honest kernel's is two ticks (one slice each): 3.02-3.07 ms in 127 boots
/// of the reference and eight honest variants at host loads 7.6-29.6
/// (2026-10-19, s12 fix round 4), while the longest single wait reached
/// 226 ms. Unlike the reference's, a longer quantum's median grows with
/// the host's load: a kernel that switches away from ring 3 every 4th tick
/// waits 12.1-18.4 ms at loads 13-20, and every 10th 30-48 ms (fix round 5,
/// which raised this from 15 ms); every 25th, 77 ms with the host idle.
const SPIN_MEDIAN_US: u64 = 50_000;
/// The least the median time inside one `yield_now` call may be. Lab 11's
/// scheduler is round robin with a one-tick quantum, so a call holds the
/// noise instance's whole slice, tick to tick: one PIT period
/// (`PIT_PERIOD_NS`), the floor. Timed around the call alone, because the
/// wait from turn to turn also holds this task's own look for the
/// reporter's row, which a `yield_now` that never switches turns into the
/// whole wait: 200 us at host load 4.5, 568 us at 26.
/// Added 2026-10-19, s12 fix round 5 (TPM B18 condition 4).
const SPIN_MEDIAN_MIN_US: u64 = 1_000;
/// One period of the PIT at lab 06's divisor 1193: 1193 / 1,193,182 Hz.
/// The PIT and the PM timer run on the same machine clock.
const PIT_PERIOD_NS: u64 = 999_847;
/// The ceiling's allowance over the learner's tick count: the two
/// spinners' increments, if `on_tick` counts after `sched::tick()` and each
/// was preempted there and has not yet resumed to count. Both counters are
/// read with interrupts off, so no tick falls between the two reads.
const SPIN_TICK_SLACK: u64 = 2;
const N_SPIN: usize = 9;
const KWRITE_ID: &str = "kill.kwrite";
const N_KWRITE: usize = 10;
const NOVMA_ID: &str = "sys.badptr.no_vma";
const N_NOVMA: usize = 11;
const INSERT_ID: &str = "vma.insert.sorted";
const N_INSERT: usize = 12;
const ROUTED_ID: &str = "kill.ud_gp.routed";
const N_ROUTED: usize = 13;
/// What hostile-kwrite stores to: HHDM + 0x1000, present, U = 0.
const KADDR: u64 = 0xffff_8000_0000_1000;

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

// ── Watching processes ───────────────────────────────────────────────────

/// What `proc::each` says about one process.
struct Seen {
    task: Tid,
    ppid: Pid,
    root: PhysAddr,
    argv: Vec<String>,
    text: Option<(u64, u64, u8)>,
    stack: Option<(u64, u64, u8)>,
}

fn seen(pid: Pid) -> Option<Seen> {
    let mut out = None;
    proc::each(|p| {
        if p.pid == pid {
            let mut text = None;
            let mut stack = None;
            for v in p.vmas.iter() {
                let r = Some((v.start.0, v.end.0, v.prot));
                match v.kind {
                    VmaKind::Text => text = r,
                    VmaKind::Stack => stack = r,
                    // Data, and every kind a later lab adds (Anon at 25).
                    _ => {}
                }
            }
            out = Some(Seen {
                task: p.task,
                ppid: p.ppid,
                root: p.root,
                argv: p.argv.clone(),
                text,
                stack,
            });
        }
    });
    out
}

fn exit_of(pid: Pid) -> Option<i32> {
    let mut e = None;
    proc::each(|p| {
        if p.pid == pid {
            e = p.exit;
        }
    });
    e
}

/// Yield until every process in `pids` has ended (at most `ms`).
fn wait_exit(pids: &[Pid], ms: u64) -> bool {
    let mut c = Clock::new();
    loop {
        if pids.iter().all(|&p| exit_of(p).is_some()) {
            return true;
        }
        if c.ms() >= ms {
            return false;
        }
        sched::yield_now();
    }
}

/// The task's address space: its root, and where `va` lands in it.
fn space_of(tid: Tid, va: u64) -> Option<(PhysAddr, Option<u64>)> {
    task::with_table(|t| {
        t.get(tid).and_then(|t| t.aspace.as_ref()).map(|a| (a.root(), a.translate(VirtAddr(va)).map(|(pa, _)| pa.0)))
    })
}

fn nonce_str() -> String {
    match ktap::nonce() {
        Some(n) => String::from(core::str::from_utf8(&n).unwrap_or("none")),
        None => String::from("none"),
    }
}

fn spawn(img: &[u8], argv: &[&str]) -> Option<Pid> {
    match proc::spawn_user(img, argv) {
        Ok(p) => Some(p),
        Err(e) => {
            note(format_args!("proc::spawn_user({}) returned -{e}", argv[0]));
            None
        }
    }
}

fn status_text(s: Option<i32>) -> String {
    match s {
        None => String::from("still running"),
        Some(v) if v & 0x7f == 0 => format!("exit({}) (status {v:#x})", (v >> 8) & 0xff),
        Some(v) => format!("killed by signal {} (status {v:#x})", v & 0x7f),
    }
}

// ── proc.two.aspaces.order ───────────────────────────────────────────────

/// Spawn the two `hi` instances and check what can be seen before either
/// runs. Called with interrupts off.
fn spawn_and_look(nonce: &str, first: [&str; 2]) -> (Option<(Pid, Pid)>, bool) {
    let mut ok = true;
    let mut fail = |why: core::fmt::Arguments| {
        ok = false;
        note(format_args!("{ORDER_ID}: {why}"));
    };
    let (pa, pb, before) = {
        let a = spawn(HI, &["hi", nonce, first[0]]);
        let b = spawn(HI, &["hi", nonce, first[1]]);
        let look = |p: Option<Pid>| p.and_then(|p| seen(p).map(|s| (p, s)));
        let la = look(a);
        let lb = look(b);
        let spaces = [&la, &lb].map(|l| l.as_ref().and_then(|(_, s)| space_of(s.task, proc::BASE)));
        (a, b, (la, lb, spaces))
    };
    let (Some(a), Some(b)) = (pa, pb) else {
        fail(format_args!("the two hi processes could not both be spawned"));
        return (None, false);
    };
    let (Some((_, sa)), Some((_, sb))) = (&before.0, &before.1) else {
        fail(format_args!("proc::each does not list the two processes spawn_user returned (pids {} and {})", a.0, b.0));
        return (None, false);
    };
    let mut order = Vec::new();
    proc::each(|p| {
        if p.pid == a || p.pid == b {
            order.push(p.pid);
        }
    });
    if order != [a, b] {
        fail(format_args!("proc::each lists them as {:?}, not in spawn order (pid {} then pid {})", order, a.0, b.0));
    }
    if b.0 <= a.0 {
        fail(format_args!("the second process's pid {} is not above the first's {}", b.0, a.0));
    }
    for (p, s, idx) in [(a, sa, first[0]), (b, sb, first[1])] {
        let want = ["hi", nonce, idx];
        if s.argv.iter().map(|x| x.as_str()).ne(want.iter().copied()) {
            fail(format_args!("pid {}'s argv reads {:?}, not the one passed", p.0, s.argv));
        }
        if s.ppid != Pid(0) {
            fail(format_args!("pid {}'s ppid is {}; the kernel spawned it (0)", p.0, s.ppid.0));
        }
        match s.text {
            Some((lo, _, prot)) if lo == proc::BASE && prot & proc::prot::WRITE == 0 && prot & proc::prot::EXEC != 0 => {}
            other => fail(format_args!("pid {}'s Text VMA is {:x?}; expected one at {:#x}, executable, not writable", p.0, other, proc::BASE)),
        }
        match s.stack {
            Some((_, hi, prot)) if hi == proc::STACK_TOP && prot & proc::prot::WRITE != 0 => {}
            other => fail(format_args!("pid {}'s Stack VMA is {:x?}; expected one ending at {:#x}, writable", p.0, other, proc::STACK_TOP)),
        }
    }
    let [ra, rb] = before.2;
    match (ra, rb) {
        (Some((ra, Some(fa))), Some((rb, Some(fb)))) => {
            note(format_args!(
                "{ORDER_ID}: pid {} (index {}) runs on CR3 {:#x}, its {:#x} in frame {:#x}; pid {} (index {}) on CR3 {:#x}, frame {:#x}",
                a.0, first[0], ra.0, proc::BASE, fa, b.0, first[1], rb.0, fb
            ));
            if ra == rb {
                fail(format_args!("both processes' tasks hold the same address space ({:#x})", ra.0));
            }
            if fa == fb {
                fail(format_args!("both processes' {:#x} lands in the same frame ({fa:#x})", proc::BASE));
            }
            if ra != sa.root || rb != sb.root {
                fail(format_args!("a process's recorded root differs from its task's address space"));
            }
            for (pid, f) in [(a, fa), (b, fb)] {
                let page = &HI[..HI.len().min(4096)];
                // SAFETY: the frame behind a mapped user page, read through the HHDM.
                let have = unsafe { core::slice::from_raw_parts(paging::hhdm(PhysAddr(f)).as_ptr::<u8>(), page.len()) };
                if have != page {
                    fail(format_args!("pid {}'s first text page does not hold hi's first {} bytes", pid.0, page.len()));
                }
            }
        }
        _ => fail(format_args!("a process's task has no address space with {:#x} mapped", proc::BASE)),
    }
    (Some((a, b)), ok)
}

/// Two `hi` instances, both spawned before either runs. Which argv index
/// (KTAP number 1 or 2) the first one gets is the nonce's:
/// `derive(nonce, "proc.two.aspaces.order") & 1` = 0 → the first spawned is
/// handed "1" and the second "2"; = 1 → the first "2", the second "1".
/// Prints its own row.
fn two_hi(nonce: &str) {
    let ok = two_hi_run(nonce);
    if ok {
        row(true, N_ORDER, ORDER_ID);
    }
}

fn two_hi_run(nonce: &str) -> bool {
    let d = ktap::nonce().map_or(0, |n| ktap::derive(&n, ORDER_ID));
    let first: [&str; 2] = if d & 1 == 0 { ["1", "2"] } else { ["2", "1"] };
    note(format_args!(
        "{ORDER_ID}: derive(nonce, \"{ORDER_ID}\") = {d:016x}; the first hi spawned is handed index {}, the second {}",
        first[0], first[1]
    ));
    // Both spawned before either runs, and looked at before either runs:
    // interrupts stay off from the first spawn to the last check.
    // A failed look prints the row before interrupts come back on: tables
    // two processes share may not survive the first one's exit.
    let (pids, mut ok) = quiet(|| {
        let r = spawn_and_look(nonce, first);
        if !r.1 {
            row(false, N_ORDER, ORDER_ID);
        }
        r
    });
    let Some((a, b)) = pids else { return false };
    if !ok {
        return false;
    }
    let mut fail = |why: core::fmt::Arguments| {
        ok = false;
        note(format_args!("{ORDER_ID}: {why}"));
    };
    if !wait_exit(&[a, b], 5000) {
        fail(format_args!("the two hi processes did not both end within 5 s"));
    }
    for p in [a, b] {
        let st = exit_of(p);
        if st != Some(0) {
            fail(format_args!("pid {} ended with {}, not exit(0)", p.0, status_text(st)));
        }
    }
    if !ok {
        row(false, N_ORDER, ORDER_ID);
    }
    // `false` here means the row is already out.
    ok
}

// ── The refusals: two programs that report their own rows ────────────────

fn refusal(img: &[u8], name: &str, index: &str, nonce: &str) {
    let before = syscall::enosys_count();
    let Some(p) = spawn(img, &[name, nonce, index]) else { return };
    let ended = wait_exit(&[p], 5000);
    note(format_args!(
        "{name}: pid {} {}; enosys_count moved by {}",
        p.0,
        if ended { status_text(exit_of(p)) } else { String::from("did not end within 5 s") },
        syscall::enosys_count() - before
    ));
}

// ── The kill ─────────────────────────────────────────────────────────────

/// Run hostile-kwrite; true when it was killed the way §A.6 says. The
/// expected line itself never goes on the wire from here: only the kernel
/// prints `kill: …`.
fn kwrite(nonce: &str) -> bool {
    let mut ok = true;
    let mut fail = |why: core::fmt::Arguments| {
        ok = false;
        note(format_args!("{KWRITE_ID}: {why}"));
    };
    let idx = format!("{N_KWRITE}");
    let Some(p) = spawn(KWRITE, &["hostile-kwrite", nonce, &idx]) else {
        fail(format_args!("hostile-kwrite could not be spawned"));
        return false;
    };
    if !wait_exit(&[p], 5000) {
        fail(format_args!("pid {} did not end within 5 s of a store to {KADDR:#x}", p.0));
        return false;
    }
    let st = exit_of(p);
    if st != Some(11) {
        fail(format_args!("pid {} ended with {}, not killed by signal 11 (SIGSEGV)", p.0, status_text(st)));
    }
    let Some(k) = quiet(mm::last_kill) else {
        fail(format_args!("mm::last_kill() is None after the store"));
        return false;
    };
    let text_end = proc::BASE + KWRITE.len() as u64;
    note(format_args!(
        "{KWRITE_ID}: pid {} ended with {}; mm::last_kill() = {} at rip {:#x}, address {:#x}",
        p.0, status_text(st), k.reason.as_str(), k.rip, k.addr
    ));
    if k.reason != KillReason::PfKernelAddr {
        fail(format_args!("the fault was classified {}, not pf-kernel-addr", k.reason.as_str()));
    }
    if k.addr != KADDR {
        fail(format_args!("the fault's address is {:#x}, not {KADDR:#x}", k.addr));
    }
    if k.rip < proc::BASE || k.rip >= text_end {
        fail(format_args!("the fault's rip {:#x} is outside the program's text [{:#x}, {text_end:#x})", k.rip, proc::BASE));
    }
    // The line `proc::kill` prints, as `KillLine` formats it: field by field
    // against §A.6, never echoed whole.
    let line = format!("{}", KillLine { pid: p, kill: k });
    let fields: Vec<&str> = line.split(' ').collect();
    let want = [
        format!("pid={}", p.0),
        String::from("reason=pf-kernel-addr"),
        format!("rip={:#x}", k.rip),
        format!("addr={KADDR:#x}"),
    ];
    if fields.len() != 5 || fields[0] != "kill:" {
        fail(format_args!("KillLine formats as {} space-separated fields, not the kill line's five", fields.len()));
    } else {
        for (i, w) in want.iter().enumerate() {
            if fields[i + 1] != w.as_str() {
                fail(format_args!("KillLine's field {} differs from the kill line's (expected the {} field)", i + 2, w.split('=').next().unwrap_or("")));
            }
        }
    }
    ok
}

/// A program of a few instructions: one PT_LOAD at `proc::BASE`, the
/// 120-byte ELF and program headers, then `code`, which is the entry.
fn probe_image(code: &[u8]) -> Vec<u8> {
    let n = 120 + code.len();
    let mut b = alloc::vec![0u8; n];
    let mut put = |off: usize, v: u64, k: usize| b[off..off + k].copy_from_slice(&v.to_le_bytes()[..k]);
    put(0, 0x0101_0246_4c45_7f, 7); // \x7fELF, 64-bit, little-endian, version 1
    put(16, 2, 2); // ET_EXEC
    put(18, 0x3e, 2); // x86-64
    put(20, 1, 4);
    put(24, proc::BASE + 120, 8); // e_entry: the code
    put(32, 64, 8); // e_phoff
    put(52, 64, 2); // e_ehsize
    put(54, 56, 2); // e_phentsize
    put(56, 1, 2); // e_phnum
    put(64, 1, 4); // PT_LOAD
    put(68, 5, 4); // R X
    put(80, proc::BASE, 8); // p_vaddr
    put(88, proc::BASE, 8); // p_paddr
    put(96, n as u64, 8); // p_filesz
    put(104, n as u64, 8); // p_memsz
    put(112, 0x1000, 8);
    b[120..].copy_from_slice(code);
    b
}

/// `mov eax, 60; xor edi, edi; syscall`: exit(0).
const EXIT_CODE: &[u8] = &[0xb8, 0x3c, 0, 0, 0, 0x31, 0xff, 0x0f, 0x05];
/// `xor eax, eax; mov [rax], al`: a store to address 0, which no VMA holds.
const FAULT_CODE: &[u8] = &[0x31, 0xc0, 0x88, 0x00];

/// Every frame of the user half of the space rooted at `root`: the pages
/// mapped below the kernel half, the page tables that map them, and the
/// PML4 itself, read through the HHDM.
fn user_frames(root: PhysAddr) -> Vec<u64> {
    const M: u64 = 0x000f_ffff_ffff_f000;
    fn walk(t: u64, level: u32, out: &mut Vec<u64>) {
        let p = paging::hhdm(PhysAddr(t)).as_ptr::<u64>();
        for i in 0..512 {
            // SAFETY: a page table of a live process, through the HHDM.
            let e = unsafe { p.add(i).read_volatile() };
            if e & 1 == 0 {
                continue;
            }
            if level == 1 || e & (1 << 7) != 0 {
                out.push(e & M);
            } else {
                walk(e & M, level - 1, out);
            }
        }
        out.push(t);
    }
    let mut out = Vec::new();
    let p = paging::hhdm(root).as_ptr::<u64>();
    for i in 0..256 {
        // SAFETY: as above.
        let e = unsafe { p.add(i).read_volatile() };
        if e & 1 != 0 {
            walk(e & M, 3, &mut out);
        }
    }
    out.push(root.0);
    out
}

/// Spawn `img` with interrupts off and list its user half's frames before
/// it runs; `wait` says whether to wait for it to end. Its pid, how it
/// ended (if it did), and those frames.
fn run_probe_held(img: &[u8], name: &str, nonce: &str, wait: bool) -> Option<(Pid, Option<i32>, Vec<u64>)> {
    let (p, held) = quiet(|| {
        let p = spawn(img, &[name, nonce])?;
        let tid = seen(p)?.task;
        let (root, _) = space_of(tid, proc::BASE)?;
        Some((p, user_frames(root)))
    })?;
    if wait {
        wait_exit(&[p], 2000);
    }
    Some((p, exit_of(p), held))
}

/// Spawn `img` and wait for it to end; its pid and how it ended.
fn run_probe(img: &[u8], name: &str, nonce: &str) -> Option<(Pid, Option<i32>)> {
    let p = spawn(img, &[name, nonce])?;
    wait_exit(&[p], 2000);
    Some((p, exit_of(p)))
}

/// Frames in use once whatever an ended task left for later has had its
/// chance to go: a few turns first (a dead task's kernel stack is freed by
/// the next task that yields). The count is `frame::stats().used`, held to
/// a drain (the cross-lab frame-count check, `coord/frames-spec.md`): with
/// interrupts off, `frame::alloc` until `None`, each frame linked through
/// its first word, then every one freed. `stats().free` must equal what the
/// drain found, `stats().used` must be `total` less that, and `stats()`
/// must read after the refill what it read
/// before. On a mismatch, the second value names both numbers.
fn frames_settled() -> (usize, Option<String>) {
    for _ in 0..4 {
        sched::yield_now();
    }
    quiet(|| {
        const END: u64 = u64::MAX;
        let before = frame::stats();
        let mut head = END;
        let mut drained = 0usize;
        while let Some(f) = frame::alloc() {
            // SAFETY: a frame just taken from the pool, reached through the HHDM.
            unsafe { paging::hhdm(f.0).as_mut_ptr::<u64>().write_volatile(head) };
            head = f.0 .0;
            drained += 1;
        }
        while head != END {
            // SAFETY: a frame of this drain, linked above.
            let next = unsafe { paging::hhdm(PhysAddr(head)).as_ptr::<u64>().read_volatile() };
            frame::free(frame::Frame(PhysAddr(head)));
            head = next;
        }
        let after = frame::stats();
        let why = if before.free != drained || before.used != before.total.wrapping_sub(drained) {
            Some(format!(
                "frame::stats() says {} in use and {} free; a drain of frame::alloc finds {drained} free, so {} in use",
                before.used,
                before.free,
                before.total.saturating_sub(drained)
            ))
        } else if after != before {
            Some(format!(
                "frame::stats() says {} in use and {} free before a drain of frame::alloc and {} in use and {} free once all {drained} frames went back",
                before.used, before.free, after.used, after.free
            ))
        } else {
            None
        };
        (before.used, why)
    })
}

// ── proc.exit.frees ──────────────────────────────────────────────────────

/// Two paths a process ends by, `exit` and a fault's kill: for each, one
/// warm-up program, then FREES_RUNS more one after another. Each holds a
/// page of text, a stack, its page tables and a PML4 while it runs; frames
/// in use may grow a little (the heap, the records nobody reaps), but by
/// fewer than one frame per program. The last of each run has its user
/// half's frames listed before it runs; once it has ended and one more
/// program has been spawned, each of them is free or that program's own,
/// so frames handed back from elsewhere cannot stand in for its teardown.
fn frees(nonce: &str) -> bool {
    let mut ok = true;
    let mut fail = |why: core::fmt::Arguments| {
        ok = false;
        note(format_args!("{FREES_ID}: {why}"));
    };
    for (name, code, what, want) in [
        ("exit-probe", EXIT_CODE, "call exit(0)", 0),
        ("fault-probe", FAULT_CODE, "store to address 0 and are killed", 11),
    ] {
        let img = probe_image(code);
        let mut ran = 0;
        let mut u0 = 0;
        let mut held = Vec::new();
        for i in 0..=FREES_RUNS {
            let r = if i == FREES_RUNS {
                run_probe_held(&img, name, nonce, true)
            } else {
                run_probe(&img, name, nonce).map(|(p, s)| (p, s, Vec::new()))
            };
            match r {
                Some((_, Some(st), h)) if st == want => held = h,
                Some((p, st, _)) => {
                    fail(format_args!("{name}: pid {} ended with {}, not {}", p.0, status_text(st), status_text(Some(want))));
                    break;
                }
                None => {
                    fail(format_args!("{name} could not be spawned"));
                    break;
                }
            }
            if i == 0 {
                let (u, why) = frames_settled();
                u0 = u;
                if let Some(why) = why {
                    fail(format_args!("before {FREES_RUNS} programs that {what}: {why}"));
                }
            } else {
                ran += 1;
            }
        }
        if ran < FREES_RUNS {
            continue;
        }
        let (u1, why) = frames_settled();
        if let Some(why) = why {
            fail(format_args!("after {FREES_RUNS} programs that {what}: {why}"));
        }
        // One more program, spawned with interrupts off, so a kernel that
        // frees an ended space as late as the next spawn has done so; a frame
        // that program's own user half now holds is in use honestly.
        let next = run_probe_held(&img, name, nonce, false);
        let (kept, reused) = quiet(|| {
            let mine = next.as_ref().map_or(&[][..], |(_, _, h)| &h[..]);
            let kept = held.iter().filter(|&&f| !mine.contains(&f) && frame::refs(frame::Frame(PhysAddr(f))) != 0).count();
            (kept, held.iter().filter(|&&f| mine.contains(&f)).count())
        });
        if let Some((p, _, _)) = next {
            wait_exit(&[p], 2000);
        }
        note(format_args!(
            "{FREES_ID}: the last program's address space held {} frames; once it had ended and one more program was spawned, {reused} of them were that program's and {kept} were still in use",
            held.len()
        ));
        if held.is_empty() || kept != 0 {
            fail(format_args!(
                "{kept} of the {} frames the last program's address space held were still in use after it ended and the next program was spawned: the user half of an ended process was not given back",
                held.len()
            ));
        }
        note(format_args!(
            "{FREES_ID}: {FREES_RUNS} programs that {what}, one after another, after one warm-up: frames in use {u0} before, {u1} after"
        ));
        if u1 >= u0 + FREES_RUNS {
            fail(format_args!(
                "frames in use grew by {} over {FREES_RUNS} programs that {what}: when a process ends, the frames of its user half (its pages, its page tables, its PML4) do not all go back to the pool",
                u1 - u0
            ));
        }
    }
    ok
}

// ── vma.covers.prot ──────────────────────────────────────────────────────

/// The learner's `VmaList` on a list this vehicle builds and owns: Text
/// [0x400000, 0x402000) read+exec, Data [0x402000, 0x404000) and
/// [0x404000, 0x405000) read+write, a one-page gap, Data [0x406000,
/// 0x407000) read+write. `covers` must answer for every byte of the range
/// and every bit of `need`.
fn vma_covers() -> bool {
    use proc::prot::{EXEC, READ, WRITE};
    let mut ok = true;
    let mut fail = |why: core::fmt::Arguments| {
        ok = false;
        note(format_args!("{VMA_ID}: {why}"));
    };
    let mut l = VmaList::new();
    for (lo, hi, prot, kind) in [
        (0x40_0000u64, 0x40_2000u64, READ | EXEC, VmaKind::Text),
        (0x40_2000, 0x40_4000, READ | WRITE, VmaKind::Data),
        (0x40_4000, 0x40_5000, READ | WRITE, VmaKind::Data),
        (0x40_6000, 0x40_7000, READ | WRITE, VmaKind::Data),
    ] {
        if let Err(e) = l.insert(Vma { start: VirtAddr(lo), end: VirtAddr(hi), prot, kind }) {
            fail(format_args!("VmaList::insert refused [{lo:#x}, {hi:#x}), page-aligned and overlapping nothing, with -{e}"));
            return false;
        }
    }
    let cases: [(u64, usize, u8, bool, &str); 10] = [
        (0x40_0000, 0x2000, READ, true, "a read of all of Text"),
        (0x40_0100, 0x10, WRITE, false, "a write into Text, which is read+exec: covers must ask each VMA for the bits of need"),
        (0x40_0100, 0x10, EXEC, true, "an exec inside Text"),
        (0x40_1ff0, 0x20, READ, true, "a read that runs from Text into Data (both readable)"),
        (0x40_1ff0, 0x20, EXEC, false, "an exec that runs from Text into Data, which is not executable: covers must ask every VMA the range crosses for every bit of need"),
        (0x40_2000, 0x3000, WRITE, true, "a write across the two adjacent Data VMAs"),
        (0x40_4ff0, 0x20, READ, false, "a read that runs off the end of Data into the gap: every byte counts, not only the first"),
        (0x40_5800, 0x10, READ, false, "a read inside the gap, which no VMA holds"),
        (0x40_0000, 0x7000, READ, false, "a read of the whole extent, gap included"),
        (0x40_6ff8, 0x10, READ, false, "a read past the last VMA's end"),
    ];
    let mut right = 0;
    for (start, len, need, want, what) in cases {
        let got = l.covers(VirtAddr(start), len, need);
        if got == want {
            right += 1;
        } else {
            fail(format_args!("covers({start:#x}, {len:#x}, need {need}) answered {got} for {what}"));
        }
    }
    note(format_args!("{VMA_ID}: VmaList::covers on four VMAs the test built (Text, two adjacent Data, a gap, Data): {right} of {} answers right", cases.len()));
    ok
}

// ── sys.badptr.no_vma ────────────────────────────────────────────────────

/// Where the vehicle maps the probe's page: the page right after its one
/// page of text, below its stack, inside no VMA `spawn_user` records.
const NOVMA_VA: u64 = proc::BASE + 0x1000;
/// What that page holds: a whole `#` line, so a `write` that lets it out
/// does not cut into the plan.
const NOVMA_MSG: &[u8; 16] = b"\n# not in a VMA\n";
/// What the vehicle puts in the last 16 bytes of the probe's text page, for
/// the same reason: the second `write` starts there.
const TEXT_TAIL: &[u8; 16] = b"\n# end of Text \n";
/// Three calls, each of which must return -EFAULT (-14): `write(1, NOVMA_VA,
/// 16)`; `write(1, BASE+0xff0, 32)`, which starts in the text VMA and runs
/// into `NOVMA_VA`; `ioctl(1, TIOCGWINSZ, BASE)`, a store into the text VMA,
/// which is readable and executable but not writable. `exit(0)` when all
/// three are refused, else `exit(2)`, `exit(3)` or `exit(4)` for the first
/// that was not (`WORK/w3/asm/novma.asm`, nasm -f bin).
const NOVMA_CODE: &[u8] = &[
    0xb8, 0x01, 0x00, 0x00, 0x00, 0xbf, 0x01, 0x00, 0x00, 0x00, 0xbe, 0x00, 0x10, 0x00, 0x01, 0xba, 0x10, 0x00,
    0x00, 0x00, 0x0f, 0x05, 0x48, 0x83, 0xf8, 0xf2, 0x75, 0x3c, 0xb8, 0x01, 0x00, 0x00, 0x00, 0xbf, 0x01, 0x00,
    0x00, 0x00, 0xbe, 0xf0, 0x0f, 0x00, 0x01, 0xba, 0x20, 0x00, 0x00, 0x00, 0x0f, 0x05, 0x48, 0x83, 0xf8, 0xf2,
    0x75, 0x27, 0xb8, 0x10, 0x00, 0x00, 0x00, 0xbf, 0x01, 0x00, 0x00, 0x00, 0xbe, 0x13, 0x54, 0x00, 0x00, 0xba,
    0x00, 0x00, 0x00, 0x01, 0x0f, 0x05, 0x48, 0x83, 0xf8, 0xf2, 0x75, 0x12, 0x31, 0xff, 0xeb, 0x13, 0xbf, 0x02,
    0x00, 0x00, 0x00, 0xeb, 0x0c, 0xbf, 0x03, 0x00, 0x00, 0x00, 0xeb, 0x05, 0xbf, 0x04, 0x00, 0x00, 0x00, 0xb8,
    0x3c, 0x00, 0x00, 0x00, 0x0f, 0x05,
];

/// Set the writable bit on every level of the walk to `va` in the space
/// rooted at `root`, and return the 4 KiB leaf's frame. The probe's text
/// page then is writable in the page tables while its VMA says it is not,
/// so a kernel that lets the `ioctl` through stores 8 bytes into it rather
/// than faulting: the row goes red and the rows after it still run.
fn text_writable(root: PhysAddr, va: u64) -> Option<u64> {
    const M: u64 = 0x000f_ffff_ffff_f000;
    let mut t = root.0;
    for shift in [39u32, 30, 21, 12] {
        let p = paging::hhdm(PhysAddr(t)).as_mut_ptr::<u64>();
        let i = ((va >> shift) & 511) as usize;
        // SAFETY: a page table of a process that has not run yet, through the HHDM.
        let e = unsafe { p.add(i).read_volatile() };
        if e & 1 == 0 || (shift != 12 && e & (1 << 7) != 0) {
            return None;
        }
        // SAFETY: as above; the space is not loaded in any CR3 yet.
        unsafe { p.add(i).write_volatile(e | 2) };
        if shift == 12 {
            return Some(e & M);
        }
        t = e & M;
    }
    None
}

/// A process whose page tables say more than its VMA list: the vehicle maps
/// one page USER, readable, at `NOVMA_VA` (in no VMA), and makes the text
/// page writable in the page tables (its VMA stays read and execute), with
/// interrupts off between `spawn_user` and both changes so the probe cannot
/// run before them. From this lab the VMA list decides what a pointer may
/// touch, for every byte of the range and for the access asked, so all three
/// of the probe's calls are refusals (-EFAULT). The page's frame is the
/// pool's: whoever tears the space down may free it.
fn no_vma(nonce: &str) -> bool {
    let mut ok = true;
    let mut fail = |why: core::fmt::Arguments| {
        ok = false;
        note(format_args!("{NOVMA_ID}: {why}"));
    };
    let img = probe_image(NOVMA_CODE);
    let made: Result<Pid, String> = quiet(|| {
        let p = proc::spawn_user(&img, &["novma-probe", nonce]).map_err(|e| format!("proc::spawn_user(novma-probe) returned -{e}"))?;
        let Some(s) = seen(p) else {
            return Err(format!("proc::each does not list pid {}, which spawn_user returned", p.0));
        };
        if s.text.map(|(lo, hi, _)| (lo, hi)) != Some((proc::BASE, proc::BASE + 0x1000)) {
            return Err(format!(
                "pid {}'s text VMA is {:x?}, not [{:#x}, {:#x}) for a program of {} bytes",
                p.0,
                s.text,
                proc::BASE,
                proc::BASE + 0x1000,
                img.len()
            ));
        }
        let Some((root, _)) = space_of(s.task, proc::BASE) else {
            return Err(format!("pid {}'s task has no address space", p.0));
        };
        let Some(leaf) = text_writable(root, proc::BASE) else {
            return Err(format!("pid {}'s text page at {:#x} is not mapped by a 4 KiB page", p.0, proc::BASE));
        };
        // SAFETY: the probe's text frame, through the HHDM; its last 16 bytes are past the code.
        unsafe { core::ptr::copy_nonoverlapping(TEXT_TAIL.as_ptr(), paging::hhdm(PhysAddr(leaf + 0xff0)).as_mut_ptr::<u8>(), 16) };
        let Some(f) = frame::alloc() else {
            return Err(String::from("the frame pool had no frame for the probe's page"));
        };
        // SAFETY: a frame just taken from the pool, reached through the HHDM.
        unsafe { core::ptr::copy_nonoverlapping(NOVMA_MSG.as_ptr(), paging::hhdm(f.0).as_mut_ptr::<u8>(), NOVMA_MSG.len()) };
        let mapped = task::with_table(|t| {
            t.get_mut(s.task).and_then(|t| t.aspace.as_mut()).map(|a| a.map(VirtAddr(NOVMA_VA), f, paging::flags::USER).is_ok())
        });
        if mapped != Some(true) {
            frame::free(f);
            return Err(format!("the test could not map a page at {NOVMA_VA:#x} in pid {}'s address space", p.0));
        }
        Ok(p)
    });
    let p = match made {
        Ok(p) => p,
        Err(why) => {
            fail(format_args!("{why}"));
            return false;
        }
    };
    let ended = wait_exit(&[p], 2000);
    let st = exit_of(p);
    note(format_args!(
        "{NOVMA_ID}: a program calls write(1, {NOVMA_VA:#x}, 16) on a page mapped for it that no VMA holds, write(1, {:#x}, 32) from its text into that page, and ioctl(1, TIOCGWINSZ, {:#x}) into its text, writable in the page tables but not in its VMA: pid {} {}",
        proc::BASE + 0xff0,
        proc::BASE,
        p.0,
        if ended { status_text(st) } else { String::from("did not end within 2 s") }
    ));
    if !ended {
        fail(format_args!("pid {} did not end within 2 s", p.0));
    } else if st != Some(0) {
        // exit(n) is status n << 8; a kernel that records n itself still gets the right reason.
        let which = match st.map(|v| if v & 0xff == 0 { (v >> 8) & 0xff } else { v }) {
            Some(2) => "write from a page in no VMA was not refused with -EFAULT: the pointer check answered from the page tables, not from the VMA list",
            Some(3) => "write of 32 bytes that start in the text VMA and end in a page no VMA holds was not refused with -EFAULT: the pointer check looked at the first byte, not at the whole range",
            Some(4) => "ioctl(TIOCGWINSZ) storing into the text VMA, which is not writable, was not refused with -EFAULT: the pointer check did not ask for the access the call makes",
            _ => "the program did not reach its exit",
        };
        fail(format_args!("pid {} ended with {}: its {which}", p.0, status_text(st)));
    }
    ok
}

// ── vma.insert.sorted ────────────────────────────────────────────────────

/// The learner's `VmaList::insert` on a list this vehicle owns: four VMAs
/// handed in an order that is not their address order, which `iter` must
/// give back lowest first; then seven ranges it must refuse and leave the list
/// as it was (four overlaps, one of them caught only by the VMA below it:
/// -EEXIST; an unaligned start, an unaligned end
/// and an empty range, each in the gap where nothing overlaps: -EINVAL);
/// then the gap itself, which touches both neighbours and overlaps neither.
fn vma_insert() -> bool {
    use proc::prot::{EXEC, READ, WRITE};
    use syscall::errno::{EEXIST, EINVAL};
    let mut ok = true;
    let mut fail = |why: core::fmt::Arguments| {
        ok = false;
        note(format_args!("{INSERT_ID}: {why}"));
    };
    let vma = |lo: u64, hi: u64, prot: u8, kind: VmaKind| Vma { start: VirtAddr(lo), end: VirtAddr(hi), prot, kind };
    let starts = |l: &VmaList| l.iter().map(|v| v.start.0).collect::<Vec<u64>>();
    let mut l = VmaList::new();
    for (lo, hi, prot, kind) in [
        (0x40_6000u64, 0x40_7000u64, READ | WRITE, VmaKind::Data),
        (0x40_0000, 0x40_2000, READ | EXEC, VmaKind::Text),
        (0x40_4000, 0x40_5000, READ | WRITE, VmaKind::Data),
        (0x40_2000, 0x40_4000, READ | WRITE, VmaKind::Data),
    ] {
        if let Err(e) = l.insert(vma(lo, hi, prot, kind)) {
            fail(format_args!("VmaList::insert refused [{lo:#x}, {hi:#x}), page-aligned and overlapping nothing, with -{e}"));
            return false;
        }
    }
    let want4 = [0x40_0000u64, 0x40_2000, 0x40_4000, 0x40_6000];
    let got = starts(&l);
    if got != want4 {
        fail(format_args!("after inserting VMAs starting at 0x406000, 0x400000, 0x404000, 0x402000 in that order, iter gives starts {got:x?}, not lowest first"));
    }
    let refusals: [(u64, u64, i64, &str); 7] = [
        (0x40_1000, 0x40_3000, EEXIST, "a range that overlaps the end of one VMA and the start of the next"),
        (0x40_3000, 0x40_4000, EEXIST, "a range that starts inside a VMA and ends where the next one starts"),
        (0x40_6000, 0x40_7000, EEXIST, "a range equal to a VMA already there"),
        (0x40_5000, 0x40_8000, EEXIST, "a range that starts in a gap and runs over the next VMA"),
        (0x40_5800, 0x40_6000, EINVAL, "a range in a gap whose start is not page-aligned"),
        (0x40_5000, 0x40_5800, EINVAL, "a range in a gap whose end is not page-aligned"),
        (0x40_5000, 0x40_5000, EINVAL, "an empty range in a gap"),
    ];
    let mut right = 0;
    for (lo, hi, want, what) in refusals {
        let n = l.iter().count();
        match l.insert(vma(lo, hi, READ, VmaKind::Data)) {
            Err(e) if e == want => right += 1,
            Err(e) => fail(format_args!("insert of [{lo:#x}, {hi:#x}), {what}, returned -{e}, not -{want}")),
            Ok(()) => fail(format_args!("insert of [{lo:#x}, {hi:#x}), {what}, was accepted; it must be refused with -{want}")),
        }
        if l.iter().count() != n {
            fail(format_args!("insert of [{lo:#x}, {hi:#x}) changed the number of VMAs from {n} to {}", l.iter().count()));
            return false;
        }
    }
    if let Err(e) = l.insert(vma(0x40_5000, 0x40_6000, READ | WRITE, VmaKind::Data)) {
        fail(format_args!("insert of [0x405000, 0x406000), which ends where the next VMA starts and starts where the last one ends, returned -{e}: touching is not overlapping"));
    }
    let want5 = [0x40_0000u64, 0x40_2000, 0x40_4000, 0x40_5000, 0x40_6000];
    let got = starts(&l);
    if got != want5 {
        fail(format_args!("after filling the gap at 0x405000, iter gives starts {got:x?}, not lowest first"));
    }
    note(format_args!(
        "{INSERT_ID}: VmaList::insert on ranges the test chose: {right} of {} refusals right; iter lowest first: {}",
        refusals.len(),
        if got == want5 { "yes" } else { "no" }
    ));
    ok
}

// ── rsp0 follows the running task (class B, TPM log 2026-10-19, B12) ─────

#[repr(C, packed)]
struct Dtr {
    limit: u16,
    base: u64,
}

// Bracket the whole image (boot/linker.ld); only their addresses matter.
extern "C" {
    static __kernel_start: u8;
    static __kernel_end: u8;
}

/// The TSS's `rsp0` as the CPU will use it: the GDTR's table, the 16-byte
/// TSS descriptor at selector 0x30, its base, and the field at offset 4.
/// `None` when that base is not a TSS inside the kernel image (lab 04's
/// `gdt.layout` names that case; reading it here could fault).
fn tss_rsp0() -> Option<u64> {
    let mut d = Dtr { limit: 0, base: 0 };
    // SAFETY: `sgdt` stores 10 bytes into `d`.
    unsafe { asm!("sgdt [{}]", in(reg) &mut d, options(nostack, preserves_flags)) };
    let (gdt, limit) = (d.base, d.limit);
    if (limit as u64) < 0x3f {
        return None;
    }
    // SAFETY: slots 6 and 7 lie below the limit of the table the CPU uses.
    let (lo, hi) = unsafe { (core::ptr::read_volatile((gdt as *const u64).add(6)), core::ptr::read_volatile((gdt as *const u64).add(7))) };
    let base = ((lo >> 16) & 0xff_ffff) | (((lo >> 56) & 0xff) << 24) | ((hi & 0xffff_ffff) << 32);
    let (start, end) = (core::ptr::addr_of!(__kernel_start) as u64, core::ptr::addr_of!(__kernel_end) as u64);
    if base < start || base.checked_add(104).is_none_or(|e| e > end) {
        return None;
    }
    // SAFETY: a TSS inside the kernel image; rsp0 is the u64 at offset 4.
    Some(unsafe { core::ptr::read_unaligned((base + 4) as *const u64) })
}

static RSP0_DONE: AtomicBool = AtomicBool::new(false);
static RSP0_SEEN: AtomicU64 = AtomicU64::new(0);
static RSP0_TOP: AtomicU64 = AtomicU64::new(0);

/// A kernel task: what `rsp0` holds while it runs, and its own stack's top.
fn rsp0_probe() {
    let (rsp0, top) = quiet(|| (tss_rsp0(), task::with_current(|t| t.kstack.top()).map(|v| v.0)));
    RSP0_SEEN.store(rsp0.unwrap_or(0), Ordering::SeqCst);
    RSP0_TOP.store(top.unwrap_or(0), Ordering::SeqCst);
    RSP0_DONE.store(rsp0.is_some() && top.is_some(), Ordering::SeqCst);
}

/// Lab 11's switch must leave `rsp0` (and the syscall entry's stack) at the
/// stack of the task it switches to: a trap from ring 3 lands there. Read it
/// from a kernel task of the vehicle's, before any program runs, and name
/// the cause on a `#` line when it is not that task's stack. No row changes:
/// the line is the only effect, so a kernel that is right here (or one that
/// reaches the task's stack another way) loses nothing by it.
fn rsp0_follows() {
    task::spawn(rsp0_probe);
    let mut c = Clock::new();
    while !RSP0_DONE.load(Ordering::SeqCst) && c.ms() < 500 {
        sched::yield_now();
    }
    if !RSP0_DONE.load(Ordering::SeqCst) {
        return;
    }
    let (rsp0, top) = (RSP0_SEEN.load(Ordering::SeqCst), RSP0_TOP.load(Ordering::SeqCst));
    if rsp0 != top {
        note(format_args!(
            "before the first program runs: a kernel task whose stack ends at {top:#x} runs with the TSS's rsp0 at {rsp0:#x}. Your lab 11 sched.rs does not call gdt::set_rsp0 and syscall::set_kernel_stack with the next task's stack top before task::switch, so a program's interrupts and system calls land on another task's stack"
        ));
    }
}

// ── kill.ud_gp.routed ────────────────────────────────────────────────────

/// What came out of COM1's data port while the tap was armed: an x86 I/O
/// breakpoint (CR4.DE, DR0 = 0x3F8, DR7 R/W0 = I/O) traps after every byte
/// the kernel sends there, whoever sends it, and `on_db` keeps the byte.
mod tap {
    use super::*;
    pub static BUF: [AtomicU8; 1024] = [const { AtomicU8::new(0) }; 1024];
    pub static LEN: AtomicUsize = AtomicUsize::new(0);
    pub static ON: AtomicBool = AtomicBool::new(false);
    pub static CR4: AtomicUsize = AtomicUsize::new(0);
    const DR7_IO: u64 = 1 | (1 << 8) | (1 << 9) | (1 << 10) | (0b10 << 16);

    pub fn arm() {
        LEN.store(0, Ordering::SeqCst);
        ON.store(true, Ordering::SeqCst);
        // SAFETY: CR4.DE turns DR7's R/W = 10 into an I/O breakpoint; DR0
        // names the port; nothing else the kernel relies on changes.
        unsafe {
            let cr4: u64;
            asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
            CR4.store(cr4 as usize, Ordering::SeqCst);
            asm!(
                "mov cr4, {c}", "mov dr0, {p}", "xor {t:e}, {t:e}", "mov dr6, {t}", "mov dr7, {d}",
                c = in(reg) cr4 | (1 << 3), p = in(reg) 0x3f8u64, d = in(reg) DR7_IO, t = out(reg) _,
                options(nomem, nostack, preserves_flags)
            );
        }
    }

    /// Off again; the bytes caught, in order.
    pub fn disarm() -> Vec<u8> {
        // SAFETY: DR7 = 0 turns the breakpoint off; CR4 goes back as it was.
        unsafe {
            asm!(
                "xor {t:e}, {t:e}", "mov dr7, {t}", "mov dr6, {t}", "mov cr4, {c}",
                t = out(reg) _, c = in(reg) CR4.load(Ordering::SeqCst) as u64,
                options(nomem, nostack, preserves_flags)
            );
        }
        ON.store(false, Ordering::SeqCst);
        let n = LEN.load(Ordering::SeqCst).min(BUF.len());
        BUF[..n].iter().map(|b| b.load(Ordering::Relaxed)).collect()
    }
}

/// #DB: the tap's breakpoint. The trap comes after the instruction, so the
/// byte is still where `out dx, al` (or `outsb`) took it from.
fn on_db(tf: &mut TrapFrame) {
    let dr6: u64;
    // SAFETY: reading DR6 has no side effect.
    unsafe { asm!("mov {}, dr6", out(reg) dr6, options(nomem, nostack, preserves_flags)) };
    if dr6 & 1 != 0 && tf.cs & 3 == 0 && tap::ON.load(Ordering::SeqCst) {
        // SAFETY: the last byte of the kernel instruction that just ran.
        let op = unsafe { ((tf.rip - 1) as *const u8).read_volatile() };
        let byte = match op {
            0xee => Some(tf.regs[14] as u8),
            // SAFETY: outsb read the byte at rsi and moved rsi past it.
            0x6e => Some(unsafe { ((tf.regs[10] - 1) as *const u8).read_volatile() }),
            _ => None,
        };
        if let Some(b) = byte {
            let i = tap::LEN.fetch_add(1, Ordering::SeqCst);
            if i < tap::BUF.len() {
                tap::BUF[i].store(b, Ordering::Relaxed);
            }
        }
    }
    // SAFETY: clearing DR6's sticky bits touches nothing else.
    unsafe { asm!("xor {t:e}, {t:e}", "mov dr6, {t}", t = out(reg) _, options(nomem, nostack, preserves_flags)) };
}

/// Two programs of one instruction each, run while the tap is armed: `ud2`
/// (#UD, vector 6) and `hlt` (privileged at CPL 3: #GP, vector 13). Each
/// must end killed with §A.6's line — its pid, the reason its vector names,
/// rip at the instruction, addr 0 — and the signal `wait4` will report
/// (4 for ud, 11 for gp). The expected line never goes on the wire from
/// here; a field that differs is named, never shown.
fn routed(nonce: &str) -> bool {
    let mut ok = true;
    let mut fail = |why: core::fmt::Arguments| {
        ok = false;
        note(format_args!("{ROUTED_ID}: {why}"));
    };
    idt::set_handler(1, on_db, 0);
    let rip = proc::BASE + 120;
    for (name, code, insn, vec, reason, sig) in [
        ("ud-probe", &[0x0f, 0x0b][..], "ud2", "#UD (vector 6)", "ud", 4),
        ("gp-probe", &[0xf4][..], "hlt", "#GP (vector 13)", "gp", 11),
    ] {
        let img = probe_image(code);
        let p = quiet(|| {
            tap::arm();
            let p = proc::spawn_user(&img, &[name, nonce]);
            if p.is_err() {
                tap::disarm();
            }
            p
        });
        let p = match p {
            Ok(p) => p,
            Err(e) => {
                fail(format_args!("proc::spawn_user({name}) returned -{e}"));
                continue;
            }
        };
        let ended = wait_exit(&[p], 2000);
        let cap = quiet(tap::disarm);
        let st = exit_of(p);
        note(format_args!(
            "{ROUTED_ID}: a program whose first instruction is {insn}: pid {} {}",
            p.0,
            if ended { status_text(st) } else { String::from("did not end within 2 s") }
        ));
        if !ended {
            fail(format_args!("pid {} did not end within 2 s of a {vec} in ring 3: the handler proc::init installs did not end the process", p.0));
            continue;
        }
        if st != Some(sig) {
            fail(format_args!(
                "pid {} ended with {}, not killed by signal {sig} ({}): a {vec} kill reports {sig}",
                p.0,
                status_text(st),
                if sig == 4 { "SIGILL" } else { "SIGSEGV" }
            ));
        }
        let text = String::from_utf8_lossy(&cap);
        let Some(line) = text.split('\n').map(|l| l.trim_end_matches('\r')).find(|l| l.starts_with("kill:")) else {
            fail(format_args!("no kill line came out on the serial port while pid {} ended: the {vec} did not reach proc::kill", p.0));
            continue;
        };
        let fields: Vec<&str> = line.split(' ').collect();
        let want = [format!("pid={}", p.0), format!("reason={reason}"), format!("rip={rip:#x}"), String::from("addr=0x0")];
        let why = [
            "the pid of the process that ran the instruction",
            "the reason the vector names (vector 6 is ud, 13 is gp)",
            "the rip of the instruction that trapped, from the trap frame",
            "0: a #UD or #GP has no faulting address",
        ];
        if fields.len() != 5 || fields[0] != "kill:" {
            fail(format_args!("after the {vec}, the kill line has {} space-separated fields, not the kill line's five", fields.len()));
        } else {
            for i in 0..4 {
                if fields[i + 1] != want[i].as_str() {
                    let key = want[i].split('=').next().unwrap_or("");
                    fail(format_args!("after the {vec}, the kill line's {key} field is not {}", why[i]));
                }
            }
        }
    }
    ok
}

// ── The spinners ─────────────────────────────────────────────────────────

/// What the reporter's own stack says about its row: its line buffer holds
/// the row it last wrote. `Some(true)` = `ok`, `Some(false)` = `not ok`.
fn reporter_row(tid: Tid) -> Option<bool> {
    let needle = format!("ok {N_REGS} - {REGS_ID}");
    let mut found = None;
    for k in 1..=2u64 {
        let Some((_, Some(pa))) = space_of(tid, proc::STACK_TOP - k * 4096) else { continue };
        // SAFETY: a stack frame of a live process (it never exits), read
        // through the HHDM; it may change under us, which only delays the find.
        let page = unsafe { core::slice::from_raw_parts(paging::hhdm(PhysAddr(pa & !0xfff)).as_ptr::<u8>(), 4096) };
        if let Some(at) = page.windows(needle.len()).position(|w| w == needle.as_bytes()) {
            found = Some(!(at >= 4 && &page[at - 4..at] == b"not "));
        }
    }
    found
}

/// Spawn the reporter and the noise instance; keep taking turns (yielding)
/// while they spin, until the reporter has written its row and had a few
/// more slices to send it. Returns the verdict of `kill.spin.preempted` and
/// whether the reporter's row was seen.
fn spinners(nonce: &str) -> (bool, bool) {
    let mut ok = true;
    let fail = |ok: &mut bool, why: core::fmt::Arguments| {
        *ok = false;
        note(format_args!("{SPIN_ID}: {why}"));
    };
    let idx = format!("{N_REGS}");
    let (r, n) = quiet(|| (spawn(SPIN, &["hostile-spin", nonce, &idx]), spawn(SPIN, &["hostile-spin", nonce, "0", "noise"])));
    let (Some(r), Some(n)) = (r, n) else {
        fail(&mut ok, format_args!("the two hostile-spin processes could not both be spawned"));
        return (false, false);
    };
    let Some(rt) = seen(r).map(|s| s.task) else {
        fail(&mut ok, format_args!("proc::each does not list the reporter (pid {})", r.0));
        return (false, false);
    };
    let mut c = Clock::new();
    let (sw0, t0) = quiet(|| (sched::switches(), ticks()));
    let mut last = 0u64;
    let (mut turns, mut max_gap) = (0u64, 0u64);
    let mut row_at = None;
    // Every wait for a turn, the first 512: the verdict is on their median,
    // which a host that stalls this machine now and then does not move (a
    // stall is one long wait among many short ones); a kernel that lets a
    // spinner keep the CPU past its tick makes most waits long.
    let mut gaps: Vec<u64> = Vec::with_capacity(512);
    // And the time inside each `yield_now` call alone, for the floor: the
    // look for the reporter's row below is this task's own work, and the
    // host's load stretches it.
    let mut away: Vec<u64> = Vec::with_capacity(512);
    loop {
        let before = c.us();
        sched::yield_now();
        let now = c.us();
        if away.len() < away.capacity() {
            away.push(now - before);
        }
        turns += 1;
        max_gap = max_gap.max(now - last);
        if gaps.len() < gaps.capacity() {
            gaps.push(now - last);
        }
        last = now;
        if row_at.is_none() && reporter_row(rt).is_some() {
            row_at = Some(now);
        }
        // 50 ms after the row is in its buffer (it has had its slices to send
        // it), and not before this task has had its 20 turns: a reporter that
        // finishes early must not cut kill.spin.preempted's window short.
        if (turns >= 20 && row_at.is_some_and(|t| now - t >= 50_000)) || now >= 20_000_000 {
            break;
        }
    }
    let (sw, tk) = quiet(|| (sched::switches() - sw0, ticks() - t0));
    let span = c.us();
    let alive = [r, n].map(exit_of);
    gaps.sort_unstable();
    let median = gaps.get(gaps.len() / 2).copied().unwrap_or(u64::MAX);
    away.sort_unstable();
    let median_away = away.get(away.len() / 2).copied().unwrap_or(0);
    // `switches()` against what this task counted itself. Only this task
    // yields in the window (the spinners make no syscall, and neither ends),
    // so every switch is one of its `turns` yields or a tick's. Each turn is
    // three: this task out by its yield, then, round robin, the reporter and
    // the noise instance each out by a tick. A tick switches at most once,
    // and the PIT cannot have fired more often than the window's periods
    // allow, plus one at each end.
    let periods = span * 1000 / PIT_PERIOD_NS + 2;
    let (floor, ceiling) = (3 * turns, turns + (tk + SPIN_TICK_SLACK).min(periods));
    note(format_args!(
        "{SPIN_ID}: over {} ms beside two processes that never yield: {turns} turns for this task, the median wait {median} us (inside yield_now {median_away} us), the longest {max_gap} us; {sw} switches, {tk} ticks, {periods} PIT periods at most; the reporter's row was in its buffer {}; both spinners {}",
        last / 1000,
        match row_at {
            Some(t) => format!("after {} ms", t / 1000),
            None => String::from("never"),
        },
        if alive == [None, None] { "still running" } else { "did not both stay running" }
    ));
    if row_at.is_none() {
        fail(&mut ok, format_args!("the reporter never got far enough to write its row within 20 s"));
    }
    if alive != [None, None] {
        fail(&mut ok, format_args!("a spinner ended: reporter {}, noise {}", status_text(alive[0]), status_text(alive[1])));
    }
    if turns < 20 {
        fail(&mut ok, format_args!("this task had only {turns} turns while the spinners ran"));
    }
    if median > SPIN_MEDIAN_US {
        fail(&mut ok, format_args!("this task's median wait for a turn was {} ms: the spinners kept the CPU past their ticks", median / 1000));
    }
    if median_away < SPIN_MEDIAN_MIN_US {
        fail(&mut ok, format_args!("this task's median time inside yield_now was {median_away} us, less than one tick: yield_now came back before either spinner had its slice"));
    }
    if sw < floor {
        fail(&mut ok, format_args!("sched::switches() counted {sw} switches over {turns} turns; each turn is three switches (this task out, then each spinner out by a tick), so at least {floor}"));
    }
    if sw > ceiling {
        fail(&mut ok, format_args!("sched::switches() counted {sw} switches; {turns} yields and {} ticks can make at most {ceiling}", ceiling - turns));
    }
    (ok, row_at.is_some())
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

/// The usable memory at or above 1 MiB that this vehicle hands
/// `frame::init`: far more than any row here takes, far less than the
/// machine has (128 MiB). Each frame-count verdict drains the pool through
/// `frame::alloc`, and an allocator that searches from the bottom every time
/// takes time that grows with the square of the pool for each drain.
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
    let frames0 = quiet(|| frame::stats().used);
    rsp0_follows();

    two_hi(&nonce);

    refusal(BADNUM, "hostile-badnum", N_BADNUM, &nonce);
    refusal(BADPTR, "hostile-badptr", N_BADPTR, &nonce);

    let killed = kwrite(&nonce);
    row(frees(&nonce), N_FREES, FREES_ID);
    row(vma_covers(), N_VMA, VMA_ID);
    note(format_args!(
        "every process so far has ended; frames in use: {} before the first spawn, {} now",
        frames0,
        quiet(|| frame::stats().used)
    ));

    let (spun, regs_seen) = spinners(&nonce);
    if !regs_seen {
        note(format_args!("{REGS_ID}: the reporter wrote no row within the bound"));
        row(false, N_REGS, REGS_ID);
    }
    row(spun, N_SPIN, SPIN_ID);
    if killed && !regs_seen {
        note(format_args!("{KWRITE_ID}: no process ran to its row after the kill"));
    }
    row(killed && regs_seen, N_KWRITE, KWRITE_ID);
    row(no_vma(&nonce), N_NOVMA, NOVMA_ID);
    row(vma_insert(), N_INSERT, INSERT_ID);
    // Last: a #UD or #GP no handler takes ends the run in lab 05's default.
    row(routed(&nonce), N_ROUTED, ROUTED_ID);
    quiet(ktap::end);
    halt()
}
