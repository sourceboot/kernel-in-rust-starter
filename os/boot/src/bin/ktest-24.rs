//! Lab 24's vehicle (`24-cow-fork`): `fork` shares the caller's pages
//! copy-on-write (the learner's `cow::share` from `proc::fork`), and a write
//! to a shared page copies that page alone (`cow::write_fault` from the arm
//! `mm::fault` gains, for user and kernel writes alike — CR0.WP stays set).
//!
//! The prelude is lab 23's (the GDT, the IDT, the frame pool, the syscall
//! MSRs, the heap, `sse_on`, `proc::init`, task 0, the PIC and the PIT).
//! Then two probes of the learner's `cow.rs`, before any process runs:
//!
//!   A (step 1, the first call into lab 24's code): two fresh spaces; a
//!     writable page and a read-only page mapped in the first, one page left
//!     unmapped; `cow::share` of each into the second. The writable page must
//!     be read-only and `COW` in both, on the same frame, which now has two
//!     references; the read-only page read-only in both and never `COW`, two
//!     references; nothing mapped for the unmapped one. Both spaces are then
//!     torn down (this file's `teardown`, which frees what `proc::discard`
//!     frees) and every frame comes back.
//!   B (step 2), interrupts off: two fresh spaces, the first made the
//!     running one and both of its pages written once through it (the TLB
//!     holds them writable), then shared. Each fault below is taken inside a
//!     process: the first space is the learner's own process's — the
//!     learner's `proc::spawn_user` starts it from an image of one readable,
//!     writable segment over the two pages (one Data VMA) — whose task never
//!     runs; it is made the running task for the fault, with the faulting
//!     space the one CR3 names, its task's `Task::aspace` and its record's
//!     `root` (task 0 is put back after each fault, and the process ends
//!     after the last): the arm may reach the entry any of those ways. `mm::fault` called directly for a
//!     kernel write (error code PRESENT|WRITE) to the first page must come
//!     back `Resolved` with that page now writable on a new frame holding the
//!     old contents, the old frame down to one reference. A real write to the
//!     second page must fault and land in a copy, never in the shared frame
//!     (a `share` that does not `invlpg` the running space leaves the
//!     writable entry in the TLB, and the entry still says COW; an arm that
//!     makes the frame writable in place leaves it writable on that frame).
//!     Then, in the second space, a write to the
//!     first page — whose frame no one else maps any more — must make it
//!     writable in place: same frame, no frame taken. Both spaces torn down,
//!     every frame back.
//!
//! A probe answer that would make running processes unsafe (a share that
//! takes no reference, a kernel write the arm does not resolve) is FATAL:
//! row 1 still runs, rows 2–4 print `not ok` with that answer.
//!
//! Then ext2 is mounted at `/` from `/dev/hda2` (the learner's `Ata`,
//! `partition2` and `Ext2::mount`, as lab 22's) and `/bin/cowtest`
//! (user/cowtest/cowtest.c; its header is the protocol) starts through the
//! learner's exec with `argv = ["cowtest", <nonce>, "t" <16 hex>]`, the last
//! word unpublished (CONTRACT §A.1-5), fd 0 a control pipe this file writes,
//! fds 1 and 2 a pipe this file drains and forwards as `# cowtest: ` lines.
//! It touches 48 MiB of `.bss` (12,288 pages) and waits. The rows, in the
//! order they print:
//!
//!   1 `cow.48m.3forks.fits` (plain) — "1": three forks all return a pid,
//!     the three children report, all four processes are alive, and frames
//!     in use (`frame::stats`) grew by less than 12,288 across the forks —
//!     one eager copy of 48 MiB; in 128 MiB a second one cannot fit.
//!   2 `cow.private.pages` (U, derived) — each child saw its parent's
//!     memory as it was at its own fork (the parent's writes right after
//!     each fork invisible to it); "2" and an unpublished word u, which the
//!     parent `read`s straight into a page all four share (a kernel write);
//!     then every process writes its own pattern (seeded by the nonce and
//!     the unpublished `t`) into the same 32 nonce-chosen pages, waits for
//!     the others, and reads it back: each fold equals the one this file
//!     computes, no word differs, every other word of those pages and of
//!     the pages written around the forks (all 512 words of every page
//!     were filled from the nonce before them) still holds what it held at
//!     the forks, in every process (a `torn` line names the first byte that
//!     does not; for page 2 it says whether the parent's entry is still on
//!     the frame it shared with its children when its `read` wrote there,
//!     the one cause this file can see, fx24d 2026-10-22), the parent's three `wait4` statuses
//!     (written by the kernel into another shared page) are exit(0), and u
//!     comes back. Probe B's copy and TLB answers are part of this row.
//!   3 `cow.text.write.kills` (plain) — "3": a child writes a byte of the
//!     program's text: it is killed by SIGSEGV with `pf-write-ro` at that
//!     address (`mm::last_kill`), the parent's byte is unchanged, and the
//!     parent's text page is read-only and not `COW`. Probe A's read-only
//!     answer is part of this row.
//!   4 `cow.meminfo.deltas` (derived) — "4": a child Q of the parent alone,
//!     both asleep; K = 64 + derive(nonce, "cow.meminfo.deltas") % 193. "w":
//!     Q writes one word into each of K pages it shares: free frames
//!     (`frame::stats`, what `/proc/meminfo`'s MemFree reports) drop by
//!     exactly K. "x": Q exits: they rise by at least K. "p": the parent
//!     writes the same K pages, now its alone: free frames do not move and
//!     not one of those pages changes frame. Probe B's reference, in-place
//!     and teardown answers are part of this row.
//!
//! Every wait is bounded by the ACPI PM timer (never the learner's tick); a
//! line that never comes fails the row that waited for it, and the rows
//! after it say they could not run.

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
use core::sync::atomic::Ordering;

use kernel::addr::{PhysAddr, VirtAddr};
use kernel::blk::ata::{self, Ata, BlockDevice};
use kernel::console::{Fd, FileRef};
use kernel::exec;
use kernel::frame::{self, Frame};
use kernel::fs::ext2::Ext2;
use kernel::fs::mount;
use kernel::idt::TrapFrame;
use kernel::ktap;
use kernel::mm::{self, pf, FaultOutcome, KillReason};
use kernel::paging::{self, flags, AddressSpace};
use kernel::pipe;
use kernel::proc::{self, Pid};
use kernel::syscall::Errno;
use kernel::task::{self, Tid};
use kernel::{bootinfo, cow, gdt, heap, idt, irq, sched, serial, syscall, time};

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 64;
const PM_HZ: u64 = 3_579_545;

const PLAN: usize = 4;
const FITS_ID: &str = "cow.48m.3forks.fits";
const PRIVATE_ID: &str = "cow.private.pages";
const TEXT_ID: &str = "cow.text.write.kills";
const DELTAS_ID: &str = "cow.meminfo.deltas";

const O_NONBLOCK: u32 = 0x800;
const ADDR_MASK: u64 = 0x000f_ffff_ffff_f000;

/// cowtest's 48 MiB, in pages, and its layout (user/cowtest/cowtest.c).
const BIG_PAGES: u64 = 12_288;
const Y_PAGE: u64 = 2;
const S_FIRST: u64 = 16;
const S4_FIRST: u64 = 4096;
const S_COUNT: u64 = 32;
const S_WORDS: u64 = 8;
const FORKS: usize = 3;

/// How long any one line of cowtest's may take, by the PM timer.
const WAIT_MS: u64 = 10_000;
/// Consecutive looks at which the processes a window needs are all asleep
/// and free frames have not moved, before the window's count is taken.
const STILL_LOOKS: u32 = 8;

/// Probe addresses: user-half pages of two spaces no process runs on.
const PA_W: u64 = 0x4000_0000;
const PA_R: u64 = 0x4000_1000;
const PA_N: u64 = 0x4000_2000;
const PB_1: u64 = 0x4100_0000;
const PB_2: u64 = 0x4100_1000;

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

/// A number no one knows before this moment: the time-stamp counter and
/// the PM timer, mixed (ktest-22's). Each is printed only after the row
/// that carries it has read it back.
fn unpublished() -> u64 {
    // SAFETY: rdtsc reads a counter; no memory is touched.
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
    ktap::splitmix64(tsc ^ (pm() as u64) << 40)
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

fn note(args: core::fmt::Arguments) {
    quiet(|| ktap::note(args));
}

fn halt() -> ! {
    loop {
        // SAFETY: interrupts off, then wait: nothing runs after this.
        unsafe { asm!("cli", "hlt", options(nomem, nostack)) };
    }
}

fn errno_text(e: Errno) -> String {
    let name = match e {
        2 => "ENOENT",
        5 => "EIO",
        8 => "ENOEXEC",
        12 => "ENOMEM",
        13 => "EACCES",
        14 => "EFAULT",
        22 => "EINVAL",
        23 => "ENFILE",
        24 => "EMFILE",
        _ => "?",
    };
    format!("-{e} ({name})")
}

fn read_cr3() -> u64 {
    let v: u64;
    // SAFETY: reading CR3 has no side effect.
    unsafe { asm!("mov {}, cr3", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

fn write_cr3(v: u64) {
    // SAFETY: `v` is a CR3 value read earlier in this run (the boot tables),
    // whose kernel half maps this code and stack.
    unsafe { asm!("mov cr3, {}", in(reg) v, options(nostack, preserves_flags)) };
}

// ── One row's recorder ───────────────────────────────────────────────────

struct Row {
    id: &'static str,
    failed: bool,
    used_nonce: bool,
}

impl Row {
    fn new(id: &'static str) -> Row {
        Row { id, failed: false, used_nonce: false }
    }
    fn fail(&mut self, why: core::fmt::Arguments) {
        self.failed = true;
        note(format_args!("{}: {why}", self.id));
    }
    fn nonce(&mut self) -> [u8; 32] {
        self.used_nonce = true;
        ktap::nonce().unwrap_or([b'0'; 32])
    }
    fn finish(mut self, n: usize) {
        if self.used_nonce && ktap::nonce().is_none() {
            self.failed = true;
            note(format_args!("{}: derived from the run's nonce, and this run has none", self.id));
        }
        quiet(|| {
            if self.failed {
                kernel::kprintln!("not ok {n} - {}", self.id);
            } else {
                kernel::kprintln!("ok {n} - {}", self.id);
            }
        });
    }
}

fn not_run(n: usize, id: &'static str, why: &str) {
    let mut r = Row::new(id);
    r.fail(format_args!("not run: {why}"));
    r.finish(n);
}

// ── Page tables, read through the HHDM ───────────────────────────────────

/// The 4 KiB leaf entry for `va` under the PML4 at `root`, if every table on
/// the way is present.
fn pte_at(root: PhysAddr, va: u64) -> Option<u64> {
    let mut table = root.0 & ADDR_MASK;
    for shift in [39u32, 30, 21, 12] {
        let i = ((va >> shift) & 0x1ff) as usize;
        // SAFETY: a page-table frame reached through the HHDM; i < 512.
        let e = unsafe { paging::hhdm(PhysAddr(table)).as_ptr::<u64>().add(i).read() };
        if shift == 12 {
            return Some(e);
        }
        if e & flags::PRESENT == 0 || e & flags::HUGE != 0 {
            return None;
        }
        table = e & ADDR_MASK;
    }
    None
}

/// Throw a space away as `proc::discard` (lab 13) does — every frame its user
/// half maps, each through `frame::free`, then its tables and its PML4 —
/// without naming it, so this file builds against every cut of the course
/// and the learner's template, where `discard` is not there yet (2026-10-22).
/// The spaces here are never the running one when they go: step A never
/// switches to them and step B restores the boot tables first.
fn teardown(a: AddressSpace) {
    fn walk(table: u64, level: u32) {
        for i in 0..512 {
            // SAFETY: a page-table frame of this space, through the HHDM; i < 512.
            let e = unsafe { paging::hhdm(PhysAddr(table)).as_ptr::<u64>().add(i).read() };
            if e & flags::PRESENT == 0 {
                continue;
            }
            if level == 1 {
                frame::free(Frame(PhysAddr(e & ADDR_MASK)));
            } else if e & flags::HUGE == 0 {
                walk(e & ADDR_MASK, level - 1);
            }
        }
        frame::free(Frame(PhysAddr(table)));
    }
    let root = a.root().0 & ADDR_MASK;
    assert!(read_cr3() & ADDR_MASK != root, "ktest-24: teardown of the running space");
    for i in 0..256 {
        // SAFETY: as in `walk`; entries 0–255 are this space's own user half.
        let e = unsafe { paging::hhdm(PhysAddr(root)).as_ptr::<u64>().add(i).read() };
        if e & flags::PRESENT != 0 {
            walk(e & ADDR_MASK, 3);
        }
    }
    frame::free(Frame(PhysAddr(root)));
}

fn pte_of(a: &mut AddressSpace, va: u64) -> u64 {
    a.entry_mut(VirtAddr(va)).map_or(0, |e| e.0)
}

fn word_of(fr: u64, w: usize) -> u64 {
    // SAFETY: a whole frame of the pool, reached through the HHDM.
    unsafe { paging::hhdm(PhysAddr(fr & ADDR_MASK)).as_ptr::<u64>().add(w).read() }
}

/// Word `w` of probe B's first page: the seed itself at word 0.
fn page_word(s: u64, w: usize) -> u64 {
    if w == 0 {
        s
    } else {
        ktap::splitmix64(s ^ (w as u64).rotate_left(32))
    }
}

fn set_word(fr: Frame, w: usize, v: u64) {
    // SAFETY: as `word_of`; the frame is this file's alone.
    unsafe { paging::hhdm(fr.0).as_mut_ptr::<u64>().add(w).write(v) }
}

fn bits(e: u64) -> String {
    let mut s = String::new();
    for (b, n) in [(flags::PRESENT, "P"), (flags::WRITE, "W"), (flags::USER, "U"), (flags::COW, "COW"), (flags::NX, "NX")] {
        if e & b != 0 {
            if !s.is_empty() {
                s.push('|');
            }
            s.push_str(n);
        }
    }
    if s.is_empty() {
        s.push('0');
    }
    format!("{e:#x} ({s})")
}

// ── The probes ───────────────────────────────────────────────────────────

/// What the probes found: a fatal answer, and each row's failure lines.
#[derive(Default)]
struct Probes {
    fatal: Option<String>,
    row2: Vec<String>,
    row3: Vec<String>,
    row4: Vec<String>,
}

/// Probe A: `cow::share` on a writable, a read-only and an unmapped page.
fn probe_share(p: &mut Probes, word: u64) {
    let free0 = frame::stats().free;
    let (Some(w), Some(r)) = (frame::alloc(), frame::alloc()) else {
        p.fatal = Some(String::from("frame::alloc() returned None before any process ran"));
        return;
    };
    set_word(w, 0, word);
    set_word(r, 0, !word);
    let mut a = AddressSpace::new_kernel();
    let mut b = AddressSpace::new_kernel();
    if a.map(VirtAddr(PA_W), w, flags::USER | flags::WRITE | flags::NX).is_err() || a.map(VirtAddr(PA_R), r, flags::USER).is_err() {
        p.fatal = Some(String::from("paging could not map two pages into a fresh space"));
        return;
    }
    let s_w = cow::share(&mut a, &mut b, VirtAddr(PA_W));
    let s_r = cow::share(&mut a, &mut b, VirtAddr(PA_R));
    let s_n = cow::share(&mut a, &mut b, VirtAddr(PA_N));
    for (what, s) in [("a writable page", s_w), ("a read-only page", s_r), ("an address with nothing mapped", s_n)] {
        if let Err(e) = s {
            p.fatal = Some(format!("cow::share of {what} mapped in its source space returned {}", errno_text(e)));
        }
    }
    let (aw, bw, ar, br) = (pte_of(&mut a, PA_W), pte_of(&mut b, PA_W), pte_of(&mut a, PA_R), pte_of(&mut b, PA_R));
    let bn = b.translate(VirtAddr(PA_N));
    let (rw, rr) = (frame::refs(w), frame::refs(r));
    note(format_args!(
        "cow::share: writable page: source {} child {} refs {rw}; read-only page: source {} child {} refs {rr}",
        bits(aw),
        bits(bw),
        bits(ar),
        bits(br)
    ));
    if p.fatal.is_none() {
        if rw != 2 || rr != 2 {
            p.fatal = Some(format!(
                "after cow::share mapped one frame in two spaces, its reference count is {} (writable page) and {} (read-only page), not 2: the first of the two to exit would free a frame the other still maps",
                rw, rr
            ));
        }
        let ww = w.0 .0;
        if aw & flags::PRESENT == 0 || aw & ADDR_MASK != ww || aw & flags::WRITE != 0 || aw & flags::COW == 0 {
            p.row2.push(format!(
                "after cow::share, the source's entry for a writable page is {}: it must stay on frame {ww:#x}, read-only and COW, or the parent's writes reach the child's page",
                bits(aw)
            ));
        }
        if bw & flags::PRESENT == 0 || bw & ADDR_MASK != ww || bw & flags::WRITE != 0 || bw & flags::COW == 0 || bw & flags::USER == 0 {
            p.row2.push(format!(
                "after cow::share, the child's entry for a writable page is {}: it must be frame {ww:#x}, USER, read-only and COW",
                bits(bw)
            ));
        }
        if aw & flags::NX == 0 || bw & flags::NX == 0 {
            p.row2.push(format!("cow::share dropped NX from a data page (source {}, child {})", bits(aw), bits(bw)));
        }
        let rr_ = r.0 .0;
        if ar & flags::COW != 0 || br & flags::COW != 0 {
            p.row3.push(format!(
                "cow::share marked a read-only page COW (source {}, child {}): a write to the program's text would get a copy of its own instead of killing the writer",
                bits(ar),
                bits(br)
            ));
        }
        if ar & flags::WRITE != 0 || br & flags::WRITE != 0 || br & flags::PRESENT == 0 || br & ADDR_MASK != rr_ {
            p.row3.push(format!(
                "after cow::share, a read-only page's entries are source {} and child {}: both read-only, on frame {rr_:#x}",
                bits(ar),
                bits(br)
            ));
        }
        if bn.is_some() {
            p.row2.push(String::from("cow::share of an address with nothing mapped in the source mapped something in the child"));
        }
    }
    // Teardown drops one reference per mapping: give a share that took none
    // its missing references first, so the frames go back exactly once.
    if rw == 0 || rr == 0 {
        p.fatal.get_or_insert_with(|| String::from("cow::share left a shared frame with no reference"));
        return;
    }
    for (fr, n) in [(w, rw), (r, rr)] {
        for _ in n..2 {
            frame::inc(fr);
        }
    }
    teardown(a);
    teardown(b);
    let free1 = frame::stats().free;
    if free1 != free0 && rw <= 2 && rr <= 2 {
        p.row4.push(format!(
            "two spaces that shared two frames were torn down and {} frames stayed in use",
            free0.abs_diff(free1)
        ));
    }
}

/// Probe B's image: a static ELF of one PT_LOAD, readable and writable, at
/// `PB_1`, two pages of memory (its file bytes are these 120 bytes of
/// headers; the rest is zero), entry at its start. It never runs.
fn probe_image() -> Vec<u8> {
    let mut b = alloc::vec![0u8; 120];
    let mut put = |off: usize, v: u64, n: usize| b[off..off + n].copy_from_slice(&v.to_le_bytes()[..n]);
    put(0, 0x0101_0246_4c45_7f, 7); // \x7fELF, 64-bit, little-endian, version 1
    put(16, 2, 2); // ET_EXEC
    put(18, 0x3e, 2); // x86-64
    put(20, 1, 4);
    put(24, PB_1, 8); // e_entry
    put(32, 64, 8); // e_phoff
    put(52, 64, 2); // e_ehsize
    put(54, 56, 2); // e_phentsize
    put(56, 1, 2); // e_phnum
    put(64, 1, 4); // PT_LOAD
    put(68, 6, 4); // R W
    put(80, PB_1, 8); // p_vaddr
    put(88, PB_1, 8); // p_paddr
    put(96, 120, 8); // p_filesz
    put(104, PB_2 + 0x1000 - PB_1, 8); // p_memsz: both pages
    put(112, 0x1000, 8);
    b
}

/// Probe B's process: the learner's `proc::spawn_user` starts it from
/// `probe_image` — a record in the learner's own table, its `root` the
/// space the loader built, one readable, writable Data VMA over the two
/// pages (as a forked process's data: a COW page lies in a writable VMA, so
/// a fault arm that asks the running process's VMAs first, lab 12's
/// `vmas_allow`, finds the write allowed) — and a task for it, which leaves
/// the run queue at once and gives the space back, so it never runs.
/// `retire` ends it. Every cut of the course has `spawn_user` (lab 12's),
/// so this file builds against the learner's template, where lab 13's
/// `proc::start` is not there yet (2026-10-22); since lab 13 `spawn_user`
/// is `exec::build` and `start`, the learner's own. Interrupts are off.
fn probe_process() -> Result<(Tid, AddressSpace), String> {
    let img = probe_image();
    let pid = proc::spawn_user(&img, &["cowprobe"])
        .map_err(|e| format!("starting a process from an image of one readable, writable segment over two pages returned {}", errno_text(e)))?;
    let Some(t) = task_of(pid) else {
        return Err(format!("starting a process returned pid {}, and no process record has that pid", pid.0));
    };
    let sp = task::with_table(|tt| {
        tt.ready.retain(|&x| x != t);
        tt.get_mut(t).and_then(|x| x.aspace.take())
    });
    match sp {
        Some(sp) => Ok((t, sp)),
        None => {
            retire(t);
            Err(format!("starting a process made pid {} a task with no address space", pid.0))
        }
    }
}

/// End probe B's process without running it: its record says it exited 0,
/// and its task, never run, is taken out of the table (its fds dropped after
/// the table's lock is let go).
fn retire(t: Tid) {
    let was = task::with_table(|tt| core::mem::replace(&mut tt.current, t));
    proc::with_current(|p| p.exit = Some(0));
    let dead = task::with_table(|tt| {
        tt.current = was;
        tt.ready.retain(|&x| x != t);
        if let Some(x) = tt.get_mut(t) {
            x.state = task::State::Dead;
        }
        tt.take_dead()
    });
    drop(dead);
}

/// While `f` runs, probe B's process (task `t`) is the running one and `sp`
/// is its space: the space CR3 names (the caller loads it), its task's
/// `Task::aspace` and its record's `root`. Task 0 is put back after. A fault
/// arm that reaches the faulting space through CR3, the running task's
/// record or the running process's record finds the same tables, as inside a
/// process. `None` when the space was not there to take back.
fn as_running<R>(t: Tid, sp: AddressSpace, f: impl FnOnce() -> R) -> Option<(AddressSpace, R)> {
    let root = sp.root();
    let was = task::with_table(|tt| core::mem::replace(&mut tt.current, t));
    proc::with_current(|p| p.root = root);
    let before = task::with_current(|x| x.aspace.replace(sp));
    let r = f();
    let sp = task::with_current(|x| core::mem::replace(&mut x.aspace, before));
    task::with_table(|tt| tt.current = was);
    sp.map(|sp| (sp, r))
}

const GONE: &str = "the running process's address space, installed for a fault probe B took, was gone from its task after the fault: the fault arm must leave the running task's record as it found it";

/// Probe B: the fault arm, from a direct call and from real kernel writes.
fn probe_fault(p: &mut Probes, seed: u64) {
    quiet(|| probe_fault_quiet(p, seed));
}

fn probe_fault_quiet(p: &mut Probes, seed: u64) {
    let free0 = frame::stats().free;
    let (s1, s2, x) = (ktap::splitmix64(seed ^ 1), ktap::splitmix64(seed ^ 2), ktap::splitmix64(seed ^ 3));
    let (t, mut a) = match probe_process() {
        Ok(x) => x,
        Err(e) => {
            p.fatal = Some(e);
            return;
        }
    };
    // The loader mapped the segment's two pages, writable, on frames of its
    // own: those are the frames the probe shares.
    let (e1, e2) = (pte_of(&mut a, PB_1), pte_of(&mut a, PB_2));
    let rw = flags::PRESENT | flags::WRITE | flags::USER;
    if e1 & rw != rw || e2 & rw != rw {
        retire(t);
        teardown(a);
        p.fatal = Some(format!(
            "a process started from an image of one readable, writable segment over two pages at {PB_1:#x} has them mapped {} and {}, not both present, writable and user",
            bits(e1),
            bits(e2)
        ));
        return;
    }
    let (f1, f2) = (Frame(PhysAddr(e1 & ADDR_MASK)), Frame(PhysAddr(e2 & ADDR_MASK)));
    // Every word of the first page is drawn from the seed, so a copy that
    // carries only part of the page is seen.
    for w in 0..512 {
        set_word(f1, w, page_word(s1, w));
    }
    set_word(f2, 0, s2);
    let b = AddressSpace::new_kernel();
    let spaces = probe_faults(p, t, a, b, (f1, f2), (s1, s2, x));
    retire(t);
    let Some((a, b)) = spaces else { return };
    teardown(a);
    teardown(b);
    let free1 = frame::stats().free;
    if free1 != free0 {
        p.row4.push(format!(
            "after two spaces that shared two pages took their copy-on-write faults and were torn down, {} frames stayed in use",
            free0.abs_diff(free1)
        ));
    }
}

/// Probe B's faults, in its process `t`: the spaces back when every step ran.
fn probe_faults(
    p: &mut Probes,
    t: Tid,
    a: AddressSpace,
    mut b: AddressSpace,
    (f1, f2): (Frame, Frame),
    (s1, s2, x): (u64, u64, u64),
) -> Option<(AddressSpace, AddressSpace)> {
    let boot = read_cr3();
    a.activate();
    // SAFETY: both pages are mapped writable in the running space, and CR4.SMAP is 0.
    unsafe {
        core::ptr::write_volatile(PB_2 as *mut u64, s2);
        let _ = core::ptr::read_volatile(PB_1 as *const u64);
    }
    // `share` runs with the source as the running process's own space, as
    // in fork: CR3, the running task's record and the running process's
    // record all name it, so a `share` that asks any of them whether the
    // source is live gets the same answer.
    let Some((mut a, Some(shared))) = as_running(t, a, || {
        let sp = task::with_current(|x| x.aspace.as_mut().map(|s| s as *mut AddressSpace))?;
        // SAFETY: the running task's own space, which `as_running` installed and
        // takes back after; nothing else holds it while `share` runs.
        let src = unsafe { &mut *sp };
        Some(cow::share(src, &mut b, VirtAddr(PB_1)).and_then(|_| cow::share(src, &mut b, VirtAddr(PB_2))))
    }) else {
        write_cr3(boot);
        p.fatal = Some(String::from(GONE));
        return None;
    };
    if let Err(e) = shared {
        write_cr3(boot);
        p.fatal = Some(format!("cow::share of a writable page of the running space returned {}", errno_text(e)));
        return None;
    }
    let tf = TrapFrame { regs: [0; 15], vector: 14, error: pf::PRESENT | pf::WRITE, rip: 0, cs: 0x08, rflags: 0x2, rsp: 0, ss: 0x10 };
    let a0 = pte_of(&mut a, PB_1);
    let Some((mut a, out)) = as_running(t, a, || mm::fault(VirtAddr(PB_1 + 8), pf::PRESENT | pf::WRITE, &tf)) else {
        write_cr3(boot);
        p.fatal = Some(String::from(GONE));
        return None;
    };
    let a1 = pte_of(&mut a, PB_1);
    if let FaultOutcome::Kill(k) = out {
        write_cr3(boot);
        p.fatal = Some(format!(
            "mm::fault for a kernel write to a COW page at user address {:#x} (error code 0x3: present, write, not user) returned Kill (reason {}) instead of resolving it: the kernel's own write into a page a process shares after fork (wait4's status, read's buffer) would stop the machine. When the fault was taken, the page's entry was {} in the space CR3 named, which was also the running task's and the running process's own",
            PB_1 + 8,
            k.reason.as_str(),
            bits(a0)
        ));
        return None;
    }
    if a1 & flags::WRITE == 0 || a1 & flags::PRESENT == 0 {
        write_cr3(boot);
        p.fatal = Some(format!(
            "mm::fault answered Resolved for a write to a COW page, but the page's entry is {}, still not writable: the write would fault again forever",
            bits(a1)
        ));
        return None;
    }
    let f1a = f1.0 .0;
    if a1 & ADDR_MASK == f1a {
        p.row2.push(format!(
            "a write fault on a COW page whose frame two spaces map made that frame writable in place (entry {}): the other space sees the write — with a second reference the page is copied",
            bits(a1)
        ));
    } else if let Some(w) = (0..512).find(|&w| word_of(a1, w) != page_word(s1, w)) {
        p.row2.push(format!(
            "a write fault on a COW page mapped a new frame holding {:#x} at byte {} where the page held {:#x}: the copy must carry all 4,096 bytes of the page",
            word_of(a1, w),
            w * 8,
            page_word(s1, w)
        ));
    }
    if a1 & flags::COW != 0 {
        p.row2.push(format!("after a write fault was resolved by a copy, the entry still says COW: {}", bits(a1)));
    }
    let r1 = frame::refs(f1);
    if a1 & ADDR_MASK != f1a && r1 != 1 {
        p.row4.push(format!(
            "after a write fault copied a page two spaces shared, the old frame's reference count is {r1}, not 1: the faulting space's reference was never dropped"
        ));
    }
    // A real kernel write to the second page: the entry `share` changed is
    // in the running space, so the TLB must not still say writable.
    // SAFETY: the page is mapped in the running space; the write faults and
    // the arm resolves it (as it just did for the first page).
    let Some((mut a, ())) = as_running(t, a, || unsafe { core::ptr::write_volatile((PB_2 + 8) as *mut u64, x) }) else {
        write_cr3(boot);
        p.fatal = Some(String::from(GONE));
        return None;
    };
    let a2 = pte_of(&mut a, PB_2);
    if word_of(f2.0 .0, 1) == x && a2 & ADDR_MASK == f2.0 .0 && a2 & flags::WRITE != 0 {
        p.row2.push(format!(
            "a kernel write to a COW page whose frame two spaces map landed in that shared frame, and the page's entry is now {}, writable on that frame: the other space sees the write — with a second reference the page is copied",
            bits(a2)
        ));
    } else if word_of(f2.0 .0, 1) == x {
        p.row2.push(String::from(
            "a write through the running space to a page cow::share had just made COW landed in the shared frame, with no fault: the TLB still held the writable entry — share must flush the page (invlpg) when the source is the running space",
        ));
    } else if a2 & ADDR_MASK == f2.0 .0 || word_of(a2, 1) != x {
        p.row2.push(format!("a kernel write to a COW page did not land in a copy of its own (entry {})", bits(a2)));
    }
    // The first page in the second space: its frame has one reference now.
    b.activate();
    let before = frame::stats().free;
    // SAFETY: mapped (COW) in the running space; the arm makes it writable.
    let Some((mut b, ())) = as_running(t, b, || unsafe { core::ptr::write_volatile((PB_1 + 16) as *mut u64, x) }) else {
        write_cr3(boot);
        p.fatal = Some(String::from(GONE));
        return None;
    };
    let after = frame::stats().free;
    let b1 = pte_of(&mut b, PB_1);
    write_cr3(boot);
    if a1 & ADDR_MASK != f1a && (b1 & ADDR_MASK != f1a || after != before || b1 & flags::WRITE == 0 || b1 & flags::COW != 0) {
        p.row4.push(format!(
            "a write to a COW page whose frame no other space maps any more took {} frame(s) and left the entry {} (was frame {f1a:#x}): a frame with one reference is made writable in place, never copied",
            before.saturating_sub(after),
            bits(b1)
        ));
    }
    Some((a, b))
}

// ── Processes ────────────────────────────────────────────────────────────

fn task_of(pid: Pid) -> Option<Tid> {
    let mut tid = None;
    quiet(|| {
        proc::each(|p| {
            if p.pid == pid {
                tid = Some(p.task);
            }
        })
    });
    tid
}

fn exit_of(pid: Pid) -> Option<i32> {
    let mut out = None;
    quiet(|| {
        proc::each(|p| {
            if p.pid == pid {
                out = p.exit;
            }
        })
    });
    out
}

fn root_of(pid: Pid) -> Option<PhysAddr> {
    let mut out = None;
    quiet(|| {
        proc::each(|p| {
            if p.pid == pid && p.exit.is_none() {
                out = Some(p.root);
            }
        })
    });
    out
}

/// The live child of `parent`, if it has exactly one.
fn only_child(parent: Pid) -> Option<Pid> {
    let mut out = (None, 0);
    quiet(|| {
        proc::each(|p| {
            if p.ppid == parent && p.exit.is_none() {
                out = (Some(p.pid), out.1 + 1);
            }
        })
    });
    if out.1 == 1 { out.0 } else { None }
}

/// Off the CPU, out of the run queue, not ended: asleep (ktest-15's test).
fn asleep(pid: Pid) -> bool {
    let Some(t) = task_of(pid) else { return false };
    quiet(|| task::with_table(|tt| tt.current != t && !tt.ready.contains(&t) && tt.get(t).is_some_and(|x| x.state != task::State::Dead)))
}

fn status_text(s: Option<i32>) -> String {
    match s {
        None => String::from("still running"),
        Some(v) if v & 0x7f == 0 => format!("exit({})", (v >> 8) & 0xff),
        Some(v) => format!("killed by signal {} (status {v:#x})", v & 0x7f),
    }
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

/// `start`, then `fds` installed in the new process's table before it runs.
fn start_with(what: &str, start: impl FnOnce() -> Result<Pid, Errno>, fds: &[(usize, &FileRef)]) -> Result<Pid, String> {
    let ends: Vec<(usize, Fd)> = fds.iter().map(|&(n, f)| (n, Fd { file: f.clone(), cloexec: false })).collect();
    let started = quiet(|| -> Result<(Pid, Vec<Fd>), (String, Vec<Fd>)> {
        let pid = start().map_err(|e| (format!("{what}: starting it returned {}", errno_text(e)), Vec::new()))?;
        let mut tid = None;
        proc::each(|p| {
            if p.pid == pid {
                tid = Some(p.task);
            }
        });
        let Some(tid) = tid else {
            return Err((format!("{what}: pid {} has no task", pid.0), Vec::new()));
        };
        let back: Vec<Fd> = task::with_table(move |tt| {
            let mut back = Vec::new();
            match tt.get_mut(tid) {
                Some(t) => {
                    for (n, d) in ends {
                        if let Ok(Some(old)) = t.fds.install(n, d) {
                            back.push(old);
                        }
                    }
                }
                None => back.extend(ends.into_iter().map(|(_, d)| d)),
            }
            back
        });
        Ok((pid, back))
    });
    match started {
        Ok((pid, back)) => {
            drop(back);
            Ok(pid)
        }
        Err((why, back)) => {
            drop(back);
            Err(why)
        }
    }
}

fn mount_root() -> Result<(), String> {
    let drive: Arc<dyn BlockDevice> = Arc::new(quiet(Ata::probe).map_err(|e| format!("Ata::probe() returned {}", errno_text(e)))?);
    let fs = quiet(|| ata::partition2(drive))
        .and_then(Ext2::mount)
        .map_err(|e| format!("partition2 then Ext2::mount returned {}", errno_text(e)))?;
    let fs = Arc::new(fs);
    let root = quiet(|| fs.root()).map_err(|e| format!("Ext2::root() returned {}", errno_text(e)))?;
    mount::mount("/dev/hda2", "/", "ext2", root).map_err(|e| format!("mount::mount(\"/dev/hda2\", \"/\", \"ext2\", its root) returned {}", errno_text(e)))
}

// ── cowtest's output: kept whole, never grown inside a counted window ────

struct Tee {
    r: FileRef,
    buf: Vec<u8>,
    fwd: usize,
}

impl Tee {
    /// Everything readable now; the buffer was sized up front, so this
    /// allocates nothing (a counted window must take no frame of its own).
    fn pump(&mut self) {
        let mut chunk = [0u8; 256];
        loop {
            match self.r.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(n) => {
                    let room = self.buf.capacity() - self.buf.len();
                    self.buf.extend_from_slice(&chunk[..n.min(room)]);
                }
            }
        }
    }
    fn lines(&self) -> impl Iterator<Item = &str> {
        let end = self.buf.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        core::str::from_utf8(&self.buf[..end]).unwrap_or("").lines()
    }
    /// The fields after `tag` of the first line that starts with `tag `.
    fn find(&self, tag: &str) -> Option<Fields<'_>> {
        self.lines().find_map(|l| l.strip_prefix(tag).and_then(|r| r.strip_prefix(' ')).map(|r| Fields(r.split(' '))))
    }
    fn count(&self, tag: &str) -> usize {
        self.lines().filter(|l| l.strip_prefix(tag).is_some_and(|r| r.starts_with(' '))).count()
    }
    /// Print what came in since the last call, one `# cowtest: ` line each.
    fn forward(&mut self) {
        let end = self.buf.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        if end > self.fwd {
            let text = String::from_utf8_lossy(&self.buf[self.fwd..end]).into_owned();
            for l in text.lines() {
                note(format_args!("cowtest: {l}"));
            }
            self.fwd = end;
        }
    }
}

struct Fields<'a>(core::str::Split<'a, char>);

impl Fields<'_> {
    fn dec(&mut self) -> Option<i64> {
        self.0.next()?.parse().ok()
    }
    fn hex(&mut self) -> Option<u64> {
        u64::from_str_radix(self.0.next()?, 16).ok()
    }
}

// ── The run ──────────────────────────────────────────────────────────────

struct Run {
    tee: Tee,
    ctl: FileRef,
    pid: Pid,
}

impl Run {
    fn send(&self, bytes: &[u8]) {
        let _ = quiet(|| self.ctl.write(bytes));
    }
    /// Wait until `done` holds of what cowtest wrote; `false` after WAIT_MS.
    fn wait(&mut self, mut done: impl FnMut(&Tee) -> bool) -> bool {
        let tee = &mut self.tee;
        let ok = yield_until(WAIT_MS, || {
            tee.pump();
            done(tee)
        });
        tee.forward();
        ok
    }
    /// `done`, then `who` all asleep and free frames unchanged over
    /// STILL_LOOKS looks in a row: the moment a window's count is taken.
    fn settle(&mut self, who: &[Pid], mut done: impl FnMut(&Tee) -> bool) -> Option<u64> {
        let tee = &mut self.tee;
        let mut last = u64::MAX;
        let mut still = 0;
        let ok = yield_until(WAIT_MS, || {
            tee.pump();
            if !done(tee) || !who.iter().all(|&p| asleep(p)) {
                still = 0;
                return false;
            }
            let f = frame::stats().free as u64;
            if f == last {
                still += 1;
            } else {
                still = 0;
                last = f;
            }
            still >= STILL_LOOKS
        });
        ok.then_some(last)
    }
    fn missing(&self, what: &str) -> String {
        format!("cowtest never printed its `{what}` line (cowtest: {})", status_text(exit_of(self.pid)))
    }
}

fn pat(seed: u64, i: u64, p: u64, j: u64) -> u64 {
    ktap::splitmix64(seed ^ (i << 56) ^ (p << 8) ^ j)
}

fn spage(seed: u64, s: u64) -> u64 {
    S_FIRST + (seed % (S4_FIRST - S_FIRST) + s * 113) % (S4_FIRST - S_FIRST)
}

/// The fold process `i` reports when every word it wrote reads back.
fn fold(seed: u64, i: u64) -> u64 {
    let mut h = 0;
    for s in 0..S_COUNT {
        let p = spage(seed, s);
        for j in 0..S_WORDS {
            h = ktap::splitmix64(h ^ pat(seed, i, p, j));
        }
    }
    h
}

fn s4_page(first: u64, i: u64) -> u64 {
    S4_FIRST + (first + i) % (BIG_PAGES - S4_FIRST)
}

fn rows(pr: Probes) {
    let fatal = pr.fatal.clone();
    // The disk and the program.
    if let Err(why) = mount_root() {
        let why = format!("no cowtest ran: {why}");
        for (n, id) in [FITS_ID, PRIVATE_ID, TEXT_ID, DELTAS_ID].into_iter().enumerate() {
            not_run(n + 1, id, &why);
        }
        return;
    }
    let pipes = (pipe::open(0), pipe::open(0));
    let ((or, ow), (cr, cw)) = match pipes {
        (Ok(o), Ok(c)) => (o, c),
        (Err(e), _) | (_, Err(e)) => {
            let why = format!("no cowtest ran: pipe::open(0) returned {}", errno_text(e));
            for (n, id) in [FITS_ID, PRIVATE_ID, TEXT_ID, DELTAS_ID].into_iter().enumerate() {
                not_run(n + 1, id, &why);
            }
            return;
        }
    };
    or.flags.store(or.flags.load(Ordering::SeqCst) | O_NONBLOCK, Ordering::SeqCst);
    let n = ktap::nonce().unwrap_or([b'0'; 32]);
    let nonce_str = String::from_utf8_lossy(&n).into_owned();
    let t = unpublished();
    let targ = format!("t{t:016x}");
    let argv = ["cowtest", nonce_str.as_str(), targ.as_str()];
    let started = start_with("cowtest", || exec::spawn("/bin/cowtest", &argv, &[]), &[(0, &cr), (1, &ow), (2, &ow)]);
    drop(cr);
    drop(ow);
    let pid = match started {
        Ok(pid) => pid,
        Err(why) => {
            let why = format!("no cowtest ran: {why}");
            for (n, id) in [FITS_ID, PRIVATE_ID, TEXT_ID, DELTAS_ID].into_iter().enumerate() {
                not_run(n + 1, id, &why);
            }
            return;
        }
    };
    let mut run = Run { tee: Tee { r: or, buf: Vec::with_capacity(64 * 1024), fwd: 0 }, ctl: cw, pid };
    let mut kids: Vec<Pid> = Vec::with_capacity(FORKS);
    let mut s4_frames: Vec<u64> = Vec::with_capacity(256);

    // ── Row 1 ──
    let mut r1 = Row::new(FITS_ID);
    let mut big_va = 0u64;
    let mut later: Option<String> = None;
    match run.settle(&[pid], |t| t.find("touched").is_some()) {
        None => {
            let why = run.missing("touched");
            r1.fail(format_args!("{why}"));
            later = Some(why);
        }
        Some(_) => {
            let (touched, va) = run.tee.find("touched").map_or((0, 0), |mut f| (f.dec().unwrap_or(0), f.hex().unwrap_or(0)));
            big_va = va;
            let s0 = frame::stats();
            run.send(b"1");
            let got = run.wait(|t| {
                t.find("forked").is_some_and(|mut f| {
                    let want = (0..FORKS).filter(|_| f.dec().is_some_and(|r| r > 0)).count();
                    t.count("ready") >= want
                })
            });
            if !got {
                let why = run.missing(if run.tee.find("forked").is_none() { "forked" } else { "ready" });
                r1.fail(format_args!("{why}"));
                later = Some(why);
            } else {
                let mut f = run.tee.find("forked").map(|f| f.0.clone());
                let rets: Vec<i64> = (0..FORKS).map(|_| f.as_mut().and_then(|s| s.next()).and_then(|v| v.parse().ok()).unwrap_or(0)).collect();
                for &r in &rets {
                    if r > 0 {
                        kids.push(Pid(r as u64));
                    }
                }
                let mut all: Vec<Pid> = alloc::vec![pid];
                all.extend_from_slice(&kids);
                let s1 = run.settle(&all, |_| true).map(|_| frame::stats());
                let s1 = s1.unwrap_or_else(frame::stats);
                let grew = s1.used as i64 - s0.used as i64;
                note(format_args!(
                    "{FITS_ID}: frames in use {} before the forks, {} after ({grew:+}); {} of {} free; fork returned {:?}",
                    s0.used, s1.used, s1.free, s1.total, rets
                ));
                if touched != BIG_PAGES as i64 {
                    r1.fail(format_args!("cowtest reported touching {touched} pages, not {BIG_PAGES}"));
                }
                for (k, &r) in rets.iter().enumerate() {
                    if r <= 0 {
                        r1.fail(format_args!(
                            "fork {} of {FORKS}, by a process holding 48 MiB, returned {}: with frames shared rather than copied, three forks fit in this machine's memory ({} frames)",
                            k + 1,
                            if r < 0 { errno_text(-r) } else { String::from("0 in the parent") },
                            s1.total
                        ));
                    }
                }
                for &k in &kids {
                    if exit_of(k).is_some() {
                        r1.fail(format_args!("child pid {} ended right after the fork: {}", k.0, status_text(exit_of(k))));
                    }
                }
                if exit_of(pid).is_some() {
                    r1.fail(format_args!("the forking process ended: {}", status_text(exit_of(pid))));
                }
                if grew >= BIG_PAGES as i64 {
                    r1.fail(format_args!(
                        "frames in use grew by {grew} across the forks: a fork that shares its parent's pages takes page tables and a kernel stack, not a copy of the 12,288 pages"
                    ));
                }
                if kids.is_empty() {
                    later = Some(String::from("no fork of the 48 MiB process succeeded"));
                }
            }
        }
    }
    r1.finish(1);
    if let Some(why) = fatal.or(later) {
        not_run(2, PRIVATE_ID, &why);
        not_run(3, TEXT_ID, &why);
        not_run(4, DELTAS_ID, &why);
        return;
    }

    // ── Row 2 ──
    let mut r2 = Row::new(PRIVATE_ID);
    let nn = r2.nonce();
    let seed = ktap::derive(&nn, PRIVATE_ID) ^ t;
    let u = unpublished();
    let mut later: Option<String> = None;
    for line in &pr.row2 {
        r2.fail(format_args!("{line}"));
    }
    let ready: Vec<(i64, i64)> = run
        .tee
        .lines()
        .filter_map(|l| l.strip_prefix("ready "))
        .filter_map(|r| {
            let mut f = Fields(r.split(' '));
            Some((f.dec()?, f.dec()?))
        })
        .collect();
    for (k, bad) in &ready {
        if *bad != 0 {
            r2.fail(format_args!(
                "child {k} found {bad} word(s) of its memory different from its parent's at its fork: the parent's writes right after fork reached the child's pages"
            ));
        }
    }
    // The frame page 2 is on in the parent while every child still shares it:
    // the parent's `read` writes there before any child is let go.
    let y_va = big_va + Y_PAGE * 4096;
    let y_shared = root_of(pid).and_then(|r| pte_at(r, y_va)).map(|e| e & ADDR_MASK).filter(|&f| f != 0);
    run.send(format!("2{u:016x}").as_bytes());
    if !run.wait(|t| t.find("private 0").is_some()) {
        let why = run.missing("private 0");
        r2.fail(format_args!("{why}"));
        later = Some(why);
    } else {
        let mut seen = alloc::vec![false; kids.len() + 1];
        let lines: Vec<String> = run.tee.lines().filter(|l| l.starts_with("private ")).map(String::from).collect();
        for l in &lines {
            let mut f = Fields(l["private ".len()..].split(' '));
            let (Some(i), Some(h), Some(bad)) = (f.dec(), f.hex(), f.dec()) else {
                r2.fail(format_args!("cowtest printed a malformed line: {l}"));
                continue;
            };
            if i < 0 || i as usize > kids.len() {
                r2.fail(format_args!("cowtest reported for process {i}, which it never forked"));
                continue;
            }
            seen[i as usize] = true;
            let want = fold(seed, i as u64);
            if bad != 0 {
                r2.fail(format_args!(
                    "process {i} ({}) read back {bad} word(s) different from what it wrote or from what the page held before the fork: another process's write reached a page it maps",
                    if i == 0 { "the parent" } else { "a child" }
                ));
            } else if h != want {
                r2.fail(format_args!("process {i} folded what it read back to {h:016x}, but its pattern folds to {want:016x}"));
            }
            if i == 0 {
                match f.hex() {
                    Some(back) if back == u => {}
                    back => r2.fail(format_args!(
                        "the parent read {:?} back from the page it had read its control message into, not the word the message carried",
                        back.map(|b| format!("{b:016x}"))
                    )),
                }
            }
        }
        let torn: Vec<String> = run.tee.lines().filter_map(|l| l.strip_prefix("torn ")).map(String::from).collect();
        // A child's torn page 2 whose parent entry is still on the shared frame:
        // the parent's `read` wrote into it in place (fx24c-n1). Any other torn
        // page names no cause: a short copy and an in-place write look the same.
        let y_now = root_of(pid).and_then(|r| pte_at(r, y_va));
        let y_in_place = match (y_shared, y_now) {
            (Some(f), Some(e)) if e & flags::PRESENT != 0 && e & ADDR_MASK == f => Some(e),
            _ => None,
        };
        for l in &torn {
            let mut f = Fields(l.split(' '));
            let (Some(i), Some(page), Some(byte), Some(n)) = (f.dec(), f.dec(), f.dec(), f.dec()) else {
                r2.fail(format_args!("cowtest printed a malformed line: torn {l}"));
                continue;
            };
            let who = if i == 0 { "the parent" } else { "a child" };
            match y_in_place {
                Some(e) if i != 0 && page == Y_PAGE as i64 => r2.fail(format_args!(
                    "process {i} ({who}) read {n} word(s) of page {page} of its 48 MiB, words it never wrote, different from what the page held at the forks, the first at byte {byte} of the page: the parent's read put its control message into that page while its children shared it, and the parent's entry for the page is still on the frame they shared, now {}: the kernel's write went into the shared page itself, not into a copy",
                    bits(e)
                )),
                _ => r2.fail(format_args!(
                    "process {i} ({who}) read {n} word(s) of page {page} of its 48 MiB, words it never wrote, different from what the page held at the forks, the first at byte {byte} of the page"
                )),
            }
        }
        for (i, s) in seen.iter().enumerate() {
            if !s {
                r2.fail(format_args!("process {i} never reported its pattern"));
            }
        }
        let reaped: Vec<(i64, u64)> = run
            .tee
            .lines()
            .filter_map(|l| l.strip_prefix("reaped "))
            .filter_map(|r| {
                let mut f = Fields(r.split(' '));
                Some((f.dec()?, f.hex()?))
            })
            .collect();
        if reaped.len() != kids.len() {
            r2.fail(format_args!("the parent reaped {} children, not {}", reaped.len(), kids.len()));
        }
        for (w, st) in reaped {
            if !kids.iter().any(|k| k.0 as i64 == w) || st != 0 {
                r2.fail(format_args!(
                    "the parent's wait4, writing into a page its children shared, returned {w} with status {st:#x}: each child exits 0 (status 0)"
                ));
            }
        }
    }
    r2.finish(2);
    note(format_args!("{PRIVATE_ID}: the unpublished words were t={t:016x} u={u:016x}"));
    if let Some(why) = later {
        not_run(3, TEXT_ID, &why);
        not_run(4, DELTAS_ID, &why);
        return;
    }

    // ── Row 3 ──
    let mut r3 = Row::new(TEXT_ID);
    for line in &pr.row3 {
        r3.fail(format_args!("{line}"));
    }
    run.send(b"3");
    if !run.wait(|t| t.find("text").is_some()) {
        let why = run.missing("text");
        r3.fail(format_args!("{why}"));
        r3.finish(3);
        not_run(4, DELTAS_ID, &why);
        return;
    }
    let (d, st, addr, before, after) = match run.tee.find("text") {
        Some(mut f) => (f.dec().unwrap_or(0), f.hex().unwrap_or(0) as i32, f.hex().unwrap_or(0), f.hex().unwrap_or(0), f.hex().unwrap_or(1)),
        None => (0, 0, 0, 0, 1),
    };
    if d <= 0 {
        r3.fail(format_args!("the fork for the text write returned {}", errno_text(-d)));
    } else if st & 0x7f != 11 {
        r3.fail(format_args!(
            "a child that wrote a byte of its program's text at {addr:#x} ended {}: a write to read-only text kills the writer with SIGSEGV",
            status_text(Some(st))
        ));
    } else {
        match mm::last_kill() {
            Some(k) if k.reason == KillReason::PfWriteRo && k.addr == addr => {}
            k => r3.fail(format_args!(
                "the child's text write at {addr:#x} was killed, but the last kill mm::fault reported was {:?}, not pf-write-ro at that address",
                k.map(|k| (k.reason.as_str(), format!("{:#x}", k.addr)))
            )),
        }
    }
    if before != after {
        r3.fail(format_args!("the parent's text byte at {addr:#x} was {before:#x} before its child's write and {after:#x} after: the child wrote the parent's text"));
    }
    match root_of(pid).and_then(|root| pte_at(root, addr)) {
        Some(e) if e & flags::PRESENT != 0 && e & flags::WRITE == 0 && e & flags::COW == 0 => {}
        e => r3.fail(format_args!(
            "the parent's entry for its text page at {addr:#x} is {}: text stays read-only and is never COW",
            e.map_or(String::from("missing"), bits)
        )),
    }
    r3.finish(3);

    // ── Row 4 ──
    let mut r4 = Row::new(DELTAS_ID);
    for line in &pr.row4 {
        r4.fail(format_args!("{line}"));
    }
    let nn = r4.nonce();
    let dv = ktap::derive(&nn, DELTAS_ID);
    let k = 64 + dv % 193;
    let first = ktap::splitmix64(dv) % (BIG_PAGES - S4_FIRST);
    run.send(b"4");
    let mut q = None;
    let f0 = run.settle(&[pid], |t| t.find("ready4").is_some()).and_then(|_| {
        q = only_child(pid);
        let q = q?;
        run.settle(&[pid, q], |_| true)
    });
    let proot = root_of(pid);
    let frames_of = |out: &mut Vec<u64>| {
        out.clear();
        for i in 0..k {
            let e = proot.and_then(|r| pte_at(r, big_va + s4_page(first, i) * 4096)).unwrap_or(0);
            out.push(e & ADDR_MASK);
        }
    };
    let Some(f0) = f0 else {
        let why = run.missing("ready4");
        r4.fail(format_args!("{why}"));
        r4.finish(4);
        return;
    };
    let q = q.unwrap_or(Pid(0));
    let got_k = run.tee.find("ready4").and_then(|mut f| f.dec()).unwrap_or(-1);
    if got_k != k as i64 {
        r4.fail(format_args!("cowtest's child chose {got_k} pages to write, not K={k}"));
    }
    run.send(b"w");
    let f1 = run.settle(&[pid, q], |t| t.find("wrote").is_some());
    run.send(b"x");
    let f2 = f1.and_then(|_| run.settle(&[pid], |t| t.find("reaped4").is_some()));
    let mut f3 = None;
    let mut moved = 0;
    if f2.is_some() {
        frames_of(&mut s4_frames);
        run.send(b"p");
        f3 = run.settle(&[pid], |t| t.find("rewrote").is_some());
        let before_frames = s4_frames.clone();
        frames_of(&mut s4_frames);
        moved = before_frames.iter().zip(&s4_frames).filter(|(a, b)| a != b).count();
    }
    run.send(b"e");
    let _ = run.wait(|_| exit_of(pid).is_some());
    let show = |f: Option<u64>| f.map_or(String::from("(never settled)"), |v| format!("{v}"));
    note(format_args!(
        "{DELTAS_ID}: K={k}; free frames {f0} with the child ready, {} after its writes, {} after its exit, {} after the parent's writes",
        show(f1),
        show(f2),
        show(f3)
    ));
    match (f1, f2, f3) {
        (None, _, _) => r4.fail(format_args!("{}", run.missing("wrote"))),
        (_, None, _) => r4.fail(format_args!("{}", run.missing("reaped4"))),
        (_, _, None) => r4.fail(format_args!("{}", run.missing("rewrote"))),
        (Some(f1), Some(f2), Some(f3)) => {
            if f0 as i64 - f1 as i64 != k as i64 {
                r4.fail(format_args!(
                    "while a child wrote one word into each of K={k} pages it shared with its parent, free frames went from {f0} to {f1} ({:+}): each first write to a shared page copies that page, one frame each, and nothing else",
                    f1 as i64 - f0 as i64
                ));
            }
            if (f2 as i64 - f1 as i64) < k as i64 {
                r4.fail(format_args!(
                    "when that child exited, free frames rose by {} (from {f1} to {f2}), fewer than the K={k} pages it had copied: its frames did not all come back",
                    f2 as i64 - f1 as i64
                ));
            }
            if f3 != f2 {
                r4.fail(format_args!(
                    "when the parent then wrote the same K={k} pages, which no other process mapped any more, free frames went from {f2} to {f3}: a page whose frame has one reference is made writable in place"
                ));
            }
            if moved != 0 {
                r4.fail(format_args!(
                    "{moved} of the parent's K={k} pages moved to new frames when it wrote them after its child had exited: a page whose frame has one reference is made writable in place, never copied"
                ));
            }
        }
    }
    let reaped4 = run.tee.find("reaped4").map(|f| Fields(f.0.clone()));
    if let Some(mut f) = reaped4 {
        let (w, st) = (f.dec().unwrap_or(0), f.hex().unwrap_or(1));
        if w as u64 != q.0 || st != 0 {
            r4.fail(format_args!("the parent's wait4 for its child returned {w} with status {st:#x}, not {} with 0", q.0));
        }
    }
    if let Some(mut f) = run.tee.find("rewrote") {
        let _ = f.dec();
        let bad = f.dec().unwrap_or(-1);
        if bad != 0 {
            r4.fail(format_args!(
                "the parent found {bad} of the K={k} pages different from before its child's writes, or not holding its own write after it: the child's writes reached the parent's pages"
            ));
        }
    }
    if exit_of(pid) != Some(0) {
        r4.fail(format_args!("cowtest ended {}, not exit(0)", status_text(exit_of(pid))));
    }
    r4.finish(4);
}

// ── The machine ──────────────────────────────────────────────────────────

/// CR0.EM = 0, CR0.TS = 0, CR0.MP = 1; CR4.OSFXSR = 1, CR4.OSXMMEXCPT = 1;
/// `fninit`; MXCSR = 0x1F80 — lab 15's prelude, unchanged.
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
    frame::init(bi.memmap(), bi.kernel_range());
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
    // Grow the heap once, before any frame is counted (ktest-14's).
    drop(core::hint::black_box(alloc::vec![0u8; 256 * 1024]));
    let n = ktap::nonce().unwrap_or([b'0'; 32]);
    let mut pr = Probes::default();
    probe_share(&mut pr, ktap::derive(&n, "cow.probe") ^ unpublished());
    if pr.fatal.is_none() {
        probe_fault(&mut pr, ktap::derive(&n, "cow.probe.fault") ^ unpublished());
    }
    if let Some(why) = &pr.fatal {
        note(format_args!("cow: {why}"));
    }
    rows(pr);
    sched::yield_now();
    quiet(ktap::end);
    halt()
}
