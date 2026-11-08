//! Lab 15's vehicle: pipes, and the fd calls that share an open file. The
//! prelude is lab 14's (the GDT, the IDT, the frame pool, the syscall MSRs,
//! the heap, `sse_on`, `proc::init`, task 0, the PIC and the PIT), and so is
//! the boot archive at `/` (CONTRACT §A.7), served by the learner's
//! `fs::archive::Archive` and mounted through the learner's `vfs::mount`.
//!
//! The vehicle starts the course's `init` through the learner's
//! `exec::spawn` — pid 1 — with `argv = ["/init", <nonce>, "1", "15"]` and
//! `SBOOT_NONCE` in its environment. Everything after that is the learner's
//! syscalls: init makes its pipes with `pipe2`, wires `producer | consumer`
//! with `fork`, `dup2`, `close` and `execve`, and reaps with `wait4`. Every
//! row but the first two and the last is a program's, in the order they print:
//!
//!   7    `pipe.full.writer_sleeps` — this vehicle's, first (the probe below)
//!   8    `fcntl.dupfd.cloexec` — this vehicle's, second, before init (below)
//!   5    `dup2.shared.flags` — init's: `O_NONBLOCK` set with `F_SETFL`
//!        through one fd is read back with `F_GETFL` through its `dup2`
//!   1–3  `pipe.chain.checksum`, `pipe.eof.last_writer`, `pipe.cloexec` —
//!        consumer's, reading the nonce-seeded stream (more than `PIPE_BUF`
//!        bytes) that producer writes
//!   4    `pipe.stress.tiny_writes` — consumer's, reading 10,000 one-byte
//!        writes under the tick
//!   6    `pipe.release.outside_lock` — this vehicle's, last (below)
//!
//! The vehicle never sleeps: it yields and reads `proc::each`, bounded by
//! the ACPI PM timer — never the learner's tick — so a wakeup the learner
//! loses leaves rows missing and a `#` line naming who is still running.
//!
//! Before any of that, row 7, a probe of the learner's `pipe::open` on
//! kernel tasks (fix round 1, 2026-10-19): a writer task pushes more than
//! three rings' worth through one write call and must sleep on the full ring
//! — seen off the CPU and out of the run queue (`off_cpu`, row 6's test)
//! while its write is unreturned, so a writer that yields and retries fails
//! it; the vehicle reads it all back in order through a non-blocking read
//! end, the write returns its whole length (so it went in parts), and the
//! writer's last end gone is end of file. Each time the writer is asleep on
//! the full ring, the vehicle reads once (512 bytes of room) and counts its
//! own yields until the writer runs again: within `WAKE_BOUND`, so the read
//! that made room woke it, not a sweep at the next tick (fix round 2); the
//! writer then refills a ring that still holds bytes, so a copy that ignores
//! them shows as a wrong byte. A read in which the writer ran meanwhile is
//! counted apart and not timed. Then reader tasks asleep on an empty pipe, a
//! fresh pipe and reader each round: each must run again within `WAKE_BOUND`
//! of the vehicle writing it a byte, and again of the vehicle dropping its
//! only write end, and then read end of file. Every one of those calls is
//! bracketed by channel 0 of the PIT, latched by the vehicle itself with
//! interrupts off (G1983, probe 2, 2026-11-04): a call the count shows a
//! reload inside, that lasted a reload interval by the PM timer, or that
//! began with IRQ 0 pending or in service at the PIC or within
//! `RELOAD_SLACK` counts of a reload, is not judged and is retried, and the
//! row needs `MIN_CLEAN_READS`, `MIN_CLEAN_BYTES` and `MIN_CLEAN_DROPS` calls
//! no tick touched (at most `READER_MAX` reader rounds). So a call that waits
//! for the tick to wake its sleeper (`hlt` inside the read) never counts,
//! and a sleeper that ran during a clean call is credited to the call.
//! Channel 0 not as lab 06 leaves it (mode 2 or 3, low byte then high,
//! peaking in 1100..=1193), before the rounds or after them, or any latch
//! above 1193, fails the row: it cannot tell then. Last, on a pipe of its
//! own with no task asleep on it, the vehicle has helper tasks fill it —
//! 4096 bytes, then 512 at a time, as many as a first pipe of its own took
//! before a write waited, whatever the ring's length (G2058, 2026-11-06) —
//! reads 512, and has another write 512: lab 15's ring takes that write at
//! once (G1972, 2026-11-04), and one that reuses its room only once it is
//! empty puts the writer to sleep (asleep: still unreturned after
//! `ASLEEP_MS` and out of the run queue). The
//! bound counts the vehicle's own yields, so it is paired with quiet windows
//! (fix round 3): whenever a sleeper has nothing to wake it — the writer on a
//! full ring the vehicle is not reading, a reader on an empty pipe no one
//! writes — the vehicle yields `QUIET` times, and the sleeper may be given at
//! most one slice plus one per PIT tick the window spans by the PM timer
//! (G1166: no cap). A sweep that puts sleepers back on yields often enough
//! to meet `WAKE_BOUND` runs the sleeper at least eight times in a window; a
//! sweep on the tick, beside real wakeups, runs it at most once per tick and
//! is accepted (B15). And right after each of those wake events (the read, the
//! byte, the drop) the vehicle looks once, interrupts off, at the sleeper: one
//! that has not run yet must already be in the run queue (`tt.ready`), since
//! lab 14's `wakeup` puts it there at once (G1166, B19 (i')); in a clean call a
//! sleeper that ran during it is credited to the call. Then, on `#` lines only, `-EPIPE`
//! with no reader and `-EBADF` on a closed fd. The row prints before init
//! runs, so it stands when a later step deadlocks the run.
//!
//! After init, row 6, the drop case (CONTRACT §A.9 S4, §B.2-15; 2026-10-17):
//! the two places a process's last write end of a pipe goes away inside the
//! fd calls — exec's close-on-exec and a `dup2` over an open fd — each while
//! a reader is asleep on that pipe. A kernel task reads the pipe's read end
//! and sleeps (the vehicle sees it off the CPU and out of the run queue);
//! the vehicle starts a program of its own through the learner's
//! `exec::spawn`, puts the write end in its fd table as fd 3 — the only
//! reference left — and lets it run. `exec-probe` execs `/ktest/parker`
//! (fd 3 close-on-exec); `dup2-probe` calls `dup2(0, 3)`. Both then park in
//! a `sched_yield` loop and never exit, so only the exec's or the dup2's
//! drop can end the reader's sleep: its read must return 0 (end of file).
//! A drop made while the task table's lock is held hangs the machine in
//! `wakeup`, where `sync.rs`'s check prints `# deadlock: …`; row 6 is then
//! missing, which is why it runs last. The programs live in this vehicle's
//! own directory, `/ktest`, mounted through the learner's `vfs::mount` after
//! init's run.
//!
//! Row 8, `fcntl.dupfd.cloexec`, one look at the fd table (2026-10-23, the
//! port ladder's lab 15 amendment, first inside row 6; its own row since the
//! slice 8 fix round, the same day): on the vehicle's own table, fd 4 holds a
//! console open file with close-on-exec clear and fds 9 and 10 hold another;
//! `fcntl(4, F_DUPFD_CLOEXEC, 9)` must return 11, the lowest free fd from 9
//! up, naming fd 4's open file and marked close-on-exec, with fd 4's own mark
//! still clear. The vehicle reads both marks off the `Fd`s themselves, never
//! through `F_GETFD`. Everything it takes out of the table is dropped after
//! `with_fds` returns. It prints right after row 7, before init runs, so
//! neither a program's deadlock nor the drop case's can leave it missing;
//! the table it looks at is the vehicle's own, which no program inherits
//! (`exec::spawn` starts each on a table of its own).

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
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use kernel::addr::VirtAddr;
use kernel::console::{self, Fd, FdTable, FileObj, FileRef};
use kernel::exec;
use kernel::fs::archive::Archive;
use kernel::fs::vfs::{self, dt, mode, DirEntry, Inode, InodeRef, Stat};
use kernel::idt;
use kernel::ktap;
use kernel::pipe;
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

const PLAN: usize = 8;
const CHAIN_ID: &str = "pipe.chain.checksum";
/// Every row, by number (init's index is 1; consumer reports 1..4, init 5).
const ROW_IDS: [&str; 5] = [CHAIN_ID, "pipe.eof.last_writer", "pipe.cloexec", "pipe.stress.tiny_writes", "dup2.shared.flags"];
/// Row 6, the drop case: this vehicle's own, printed last.
const DROP_ROW: usize = 6;
const DROP_ID: &str = "pipe.release.outside_lock";
/// The probe's row, printed before init runs.
const SLEEP_ROW: usize = 7;
const SLEEP_ID: &str = "pipe.full.writer_sleeps";
/// The `F_DUPFD_CLOEXEC` look's row, printed second, before init runs.
const DUPFD_ROW: usize = 8;
const DUPFD_ID: &str = "fcntl.dupfd.cloexec";
/// The drop case's bounds: for the reader to be seen asleep, and for its
/// read to return once the write end is gone (each sub-case).
const ASLEEP_MS: u64 = 200;
const EOF_MS: u64 = 500;
/// The fd the drop case's programs get the write end on.
const DROP_FD: usize = 3;
/// Row 8's `F_DUPFD_CLOEXEC` look: Linux's command number
/// (include/uapi/linux/fcntl.h), the fd it copies, the minimum it is given,
/// and the fds from the minimum up that are already taken, so the answer is
/// `DUPFD_MIN + DUPFD_TAKEN` (11).
const DUPFD_CLOEXEC: u32 = 1030;
const DUPFD_SRC: usize = 4;
const DUPFD_MIN: usize = 9;
const DUPFD_TAKEN: usize = 2;
/// `O_RDWR` (Linux's value), for the console files the look opens.
const O_RDWR: u32 = 2;
const EXEC_PROBE: &str = "/ktest/exec-probe";
const DUP2_PROBE: &str = "/ktest/dup2-probe";
const PARKER: &str = "/ktest/parker";
/// What `exec-probe` and `dup2-probe` add to the errno of a call that failed.
const PROBE_FAILED: i32 = 100;
/// How long the whole run may take before the vehicle stops waiting.
const RUN_MS: u64 = 20_000;
/// Row 7's probe reads until the write's every byte and then the 0 are back,
/// and stops waiting when `PROBE_IDLE_MS` pass with no byte read, or after
/// `PROBE_CAP_MS` in all (PM timer; Part 2 of sprint 7 #82, 2026-11-06,
/// class B (d), the blind pick agreeing). The bound only ends the wait:
/// every round is judged by the wake tally, so a longer wait cannot turn a
/// refusal into a pass. Until then the budget was a flat 2 s, and an honest
/// kernel whose yield is slow (tick-wakes-all-slow-yield-200us: 264 quiet
/// yields of 200 µs each a round) read 17 of its 26 rounds in it on a loaded
/// host and was refused (8 of 15 boots at load 21-31 on the mini).
const PROBE_IDLE_MS: u64 = 2000;
const PROBE_CAP_MS: u64 = 5000;
const INIT: &str = "/init";
/// The probe's one write: three rings and a bit.
const PROBE_BYTES: usize = 3 * 4096 + 1000;
/// `O_NONBLOCK` (Linux's value), for the probe's read end.
const O_NONBLOCK: u32 = 0x800;
/// The most of the vehicle's yields a writer asleep on a full ring may take
/// to run again once the vehicle has emptied the ring (lab 14's bound): a
/// reader's wakeup puts it in the run queue, so it runs on the next turn; a
/// sweep that only the tick makes waits for that tick, hundreds of yields.
const WAKE_BOUND: u64 = 32;
/// The fewest reads making room that row 7 must judge with no PIT tick
/// inside the call (clean; G1983, probe 2, 2026-11-04): a round the tick
/// touched is retried, never judged, since the tick's sweep may be what ran
/// the writer, so a read that wakes no one and waits for the tick inside the
/// call has no clean round at all. A clean round in which the writer ran
/// during the call is credited to the call (a kernel whose read gives the CPU
/// up right after its wakeup, G1981): no tick can have woken it.
const MIN_CLEAN_READS: u32 = 6;
/// The same floors for the reader half: one-byte writes and last-end drops.
const MIN_CLEAN_BYTES: u32 = 3;
const MIN_CLEAN_DROPS: u32 = 3;
/// The most pipes the reader half uses (one reader task each) to meet them.
const READER_MAX: u32 = 12;
/// A round must start with channel 0's count more than this many counts
/// below its reload value of 1193 (about 250 µs in mode 2, where the count
/// drops 1 per 1.193 MHz input clock; about 125 µs in mode 3, where it drops
/// 2): QEMU raises IRQ 0 from a
/// host timer callback that may run after the count has already reloaded,
/// so a call that starts just after a reload may still have the tick's
/// handler land inside it.
const RELOAD_SLACK: u16 = 300;
/// The vehicle's yields in a quiet window (fix round 3): eight times
/// `WAKE_BOUND + 1`, so a sweep that meets the bound runs a sleeper at least 8
/// times in it, whatever its phase.
const QUIET: u64 = 8 * (WAKE_BOUND + 1);
/// The course's PIT rate (lab 06: 1,193,182 Hz input, reload 1193), the
/// vehicle's own copy: a window's ticks come from its PM-timer length.
const PIT_INPUT_HZ: u64 = 1_193_182;
const PIT_RELOAD: u64 = 1193;

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

// ── The PIT's channel 0, read by the test itself (G1983, probe 2) ────────
//
// A round of row 7 is one pipe call that must wake a sleeper (a read making
// room, a one-byte write, the last write end's drop) and the test's look at
// the sleeper right after it. The tick's sweep (B15's, accepted beside the
// calls' own wakeups) may run the sleeper too, so a round is judged only if
// no PIT tick touched it: channel 0's count, latched before the call and
// again before the look, did not reload in between, the call lasted less
// than one reload interval by the PM timer, IRQ 0 was neither pending nor in
// service at the start, and the count was more than RELOAD_SLACK below its
// reload value at the start (QEMU raises IRQ 0 from a host timer callback
// that may run after the count has reloaded). Never the learner's TICKS.

fn outb(port: u16, v: u8) {
    // SAFETY: an 8-bit write to an I/O port; no memory is touched.
    unsafe { asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack, preserves_flags)) };
}

fn inb(port: u16) -> u8 {
    let v: u8;
    // SAFETY: an 8-bit read of an I/O port; no memory is touched.
    unsafe { asm!("in al, dx", out("al") v, in("dx") port, options(nomem, nostack, preserves_flags)) };
    v
}

/// Channel 0's count, latched (command 0x00 at 0x43, low byte then high
/// from 0x40). Called with interrupts off, so nothing else on the CPU
/// touches the PIT between the latch and its two reads.
fn pit_latch() -> u16 {
    outb(0x43, 0x00);
    let lo = inb(0x40) as u16;
    let hi = inb(0x40) as u16;
    hi << 8 | lo
}

/// IRQ 0 in service (ISR, OCW3 0x0b) or pending (IRR, OCW3 0x0a) at the
/// master PIC, read with interrupts off. IRR is read last, so the PIC is
/// left reading IRR, as it starts.
fn irq0_waiting() -> bool {
    outb(0x20, 0x0b);
    let isr = inb(0x20) & 1;
    outb(0x20, 0x0a);
    let irr = inb(0x20) & 1;
    isr | irr != 0
}

/// One PIT period (1193 input clocks) in PM-timer counts.
const PIT_PM: u32 = (PM_HZ * PIT_RELOAD / PIT_INPUT_HZ) as u32;

/// Channel 0 as lab 06 leaves it, or what it reads instead: mode 2 or 3 by
/// the read-back (command 0xe2), low byte then high, and a latched count
/// that moves and peaks in 1100..=1193: sampled for at least 2 ms, and on
/// until a sample lands in that top band or 50 ms have passed (a loaded
/// host deschedules the guest between latches, so 2 ms can hold only a
/// handful of samples; a reload below 1100 never reaches the band and
/// fails at 50 ms, one above 1193 reads above it). Returns how long one
/// reload interval lasts in PM counts: a period in mode 2, half of one in
/// mode 3 (whose count reloads twice a period).
fn pit_check() -> Result<u32, String> {
    let status = quiet(|| {
        outb(0x43, 0xe2);
        inb(0x40)
    });
    let mode = (status >> 1) & 7;
    let access = (status >> 4) & 3;
    let (mut low, mut peak, mut above) = (u16::MAX, 0u16, false);
    let p0 = pm();
    loop {
        let el = pm().wrapping_sub(p0) & 0xff_ffff;
        if el >= 50 * PIT_PM || (el >= 2 * PIT_PM && peak >= 1100) {
            break;
        }
        let c = quiet(pit_latch);
        low = low.min(c);
        peak = peak.max(c);
        above |= c > 1193;
    }
    let ms = (pm().wrapping_sub(p0) & 0xff_ffff) / PIT_PM;
    if !(mode & 3 == 2 || mode & 3 == 3) || access != 3 || above || peak < 1100 || low == peak {
        Err(format!("mode {mode}, access {access}, its latched count {low}..{peak} over {ms} ms"))
    } else if mode & 3 == 3 {
        Ok(PIT_PM / 2)
    } else {
        Ok(PIT_PM)
    }
}

/// Set when a latch reads above 1193: channel 0 was reprogrammed mid-probe.
static PIT_ABOVE: AtomicBool = AtomicBool::new(false);

/// What the test reads right before a round's call, interrupts off.
struct Before {
    waiting: bool,
    count: u16,
    pm: u32,
}

fn before() -> Before {
    quiet(|| Before { waiting: irq0_waiting(), count: pit_latch(), pm: pm() })
}

/// Why a round is not judged: a PIT tick may have touched it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Taint {
    /// The count reloaded between the two latches, or the call lasted a
    /// reload interval or more by the PM timer (or read above 1193).
    Reload,
    /// IRQ 0 was pending or in service at the start, or the count was
    /// within RELOAD_SLACK of its reload: a late tick may land in the call.
    Near,
}

/// Right after a round's call: the count latched again and the sleeper
/// looked at (`after_wake`), in one stretch with interrupts off, so no tick
/// falls between the latch and the look.
fn after(t: Tid, s: u64, b: &Before, span: u32) -> (Woke, Option<Taint>) {
    quiet(|| {
        let count = pit_latch();
        let el = pm().wrapping_sub(b.pm) & 0xff_ffff;
        let woke = after_wake(t, s);
        if count > 1193 || b.count > 1193 {
            PIT_ABOVE.store(true, Ordering::SeqCst);
        }
        let taint = if count > b.count || el >= span || count > 1193 {
            Some(Taint::Reload)
        } else if b.waiting || b.count > 1193 - RELOAD_SLACK {
            Some(Taint::Near)
        } else {
            None
        };
        (woke, taint)
    })
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
        7 => "E2BIG",
        8 => "ENOEXEC",
        9 => "EBADF",
        11 => "EAGAIN",
        10 => "ECHILD",
        12 => "ENOMEM",
        14 => "EFAULT",
        22 => "EINVAL",
        20 => "ENOTDIR",
        24 => "EMFILE",
        32 => "EPIPE",
        38 => "ENOSYS",
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

// ── Watching processes ───────────────────────────────────────────────────

/// One process as `proc::each` shows it.
#[derive(Clone, Copy)]
struct Seen {
    pid: Pid,
    ppid: Pid,
    exit: Option<i32>,
}

fn snapshot() -> Vec<Seen> {
    let mut v = Vec::new();
    quiet(|| proc::each(|p| v.push(Seen { pid: p.pid, ppid: p.ppid, exit: p.exit })));
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
    let present: Vec<&str> = ["init", "producer", "consumer"].into_iter().filter(|n| root.lookup(n).is_ok()).collect();
    note(format_args!("vfs: / is the boot archive ({ARCHIVE_LEN} bytes); lookup finds {present:?} of [\"init\", \"producer\", \"consumer\"]"));
    true
}

// ── The probe: a pipe between two kernel tasks (# lines only) ────────────

static PROBE_W: SpinLock<Option<FileRef>> = SpinLock::new(None);
/// What the writer's one write returned (+n, or -errno), once it has.
static WROTE: AtomicU64 = AtomicU64::new(u64::MAX);
/// The probe's sleeping reader (fix round 2): its read end, the bytes it
/// has read, and how its reads ended (u64::MAX: not yet; 0: end of file;
/// otherwise the errno).
static PROBE_R: SpinLock<Option<FileRef>> = SpinLock::new(None);
static RDR_GOT: AtomicU64 = AtomicU64::new(0);
static RDR_END: AtomicU64 = AtomicU64::new(u64::MAX);

fn probe_byte(i: usize) -> u8 {
    (i * 7 + i / 4096) as u8
}

/// The writer task: one write of `PROBE_BYTES`, then its end goes.
fn writer() {
    let w = quiet(|| PROBE_W.lock().take());
    let Some(w) = w else { return };
    let buf: Vec<u8> = (0..PROBE_BYTES).map(probe_byte).collect();
    let r = match w.write(&buf) {
        Ok(n) => n as u64,
        Err(e) => (-e) as u64,
    };
    WROTE.store(r, Ordering::SeqCst);
    drop(w);
}

/// A kernel task that reads the probe's second pipe until end of file,
/// sleeping on it while it is empty.
fn reader() {
    let r = quiet(|| PROBE_R.lock().take());
    let Some(r) = r else { return };
    let mut b = [0u8; 16];
    loop {
        match r.read(&mut b) {
            Ok(0) => {
                RDR_END.store(0, Ordering::SeqCst);
                break;
            }
            Ok(n) => {
                RDR_GOT.fetch_add(n as u64, Ordering::SeqCst);
            }
            Err(e) => {
                RDR_END.store(e as u64, Ordering::SeqCst);
                break;
            }
        }
    }
    drop(r);
}

/// The vehicle's yields until task `t` has run past slice count `s`
/// (u64::MAX: not within `ASLEEP_MS`).
fn yields_until_ran(t: Tid, s: u64) -> u64 {
    let mut k = 0u64;
    let mut c = Clock::new();
    while sched::slices(t) == s && c.ms() < ASLEEP_MS {
        sched::yield_now();
        k += 1;
    }
    if sched::slices(t) == s { u64::MAX } else { k }
}

/// What the vehicle saw right after an event that must wake task `t`
/// (G1166, B19 (i')): one look with interrupts off, so no tick falls
/// between its two reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Woke {
    /// `t` ran since slice count `s`, inside the call: credited to the call
    /// when no PIT tick touched the round (G1983), else not judged.
    Raced,
    /// `t` had not run and was in the run queue.
    Queued,
    /// `t` had not run and was not in the run queue.
    Unqueued,
}

/// Right after a wake event: whether `t` ran past slice count `s`, and if
/// not, whether it is in the run queue (`tt.ready`, the queue lab 11's
/// `TaskTable` names). Its two reads are made with interrupts off, one after
/// the other (never one inside the other: a learner's `slices` may take the
/// task table's lock itself).
fn after_wake(t: Tid, s: u64) -> Woke {
    quiet(|| {
        let now = sched::slices(t);
        let q = task::with_table(|tt| tt.ready.contains(&t));
        if now != s {
            Woke::Raced
        } else if q {
            Woke::Queued
        } else {
            Woke::Unqueued
        }
    })
}

/// One kind of wake event's rounds: judged (queued or not) and raced, on
/// the clean rounds; and the rounds a PIT tick may have touched, by cause.
#[derive(Clone, Copy, Default)]
struct Tally {
    judged: u32,
    queued: u32,
    raced: u32,
    reload: u32,
    near: u32,
}

impl Tally {
    fn add(&mut self, w: Woke, taint: Option<Taint>) {
        match (taint, w) {
            (Some(Taint::Reload), _) => self.reload += 1,
            (Some(Taint::Near), _) => self.near += 1,
            (None, Woke::Raced) => self.raced += 1,
            (None, Woke::Queued) => {
                self.judged += 1;
                self.queued += 1;
            }
            (None, Woke::Unqueued) => self.judged += 1,
        }
    }
    fn missed(&self) -> u32 {
        self.judged - self.queued
    }
    /// Rounds no PIT tick touched.
    fn clean(&self) -> u32 {
        self.judged + self.raced
    }
    fn tainted(&self) -> u32 {
        self.reload + self.near
    }
}

/// A quiet window: `QUIET` of the vehicle's yields while `t` sleeps with
/// nothing to wake it. Returns the slices `t` was given and the most it may
/// be given: 1 plus the PIT ticks the window spans by the PM timer.
fn quiet_window(t: Tid) -> (u64, u64) {
    let s0 = sched::slices(t);
    let mut c = Clock::new();
    for _ in 0..QUIET {
        sched::yield_now();
    }
    let n = sched::slices(t).saturating_sub(s0);
    let spans = c.us() * PIT_INPUT_HZ / PIT_RELOAD / 1_000_000;
    (n, 1 + spans)
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

fn probe_pipe() {
    let (r, w) = match pipe::open(0) {
        Ok(p) => p,
        Err(e) => {
            note(format_args!("pipe probe: pipe::open(0) returned {}", errno_text(e)));
            row(false, SLEEP_ROW, SLEEP_ID);
            return;
        }
    };
    // Channel 0 as lab 06 left it, before any round is read by it (fail
    // closed: a count this row cannot read judges nothing).
    let span = match pit_check() {
        Ok(s) => Some(s),
        Err(why) => {
            note(format_args!(
                "{SLEEP_ID}: channel 0 of the PIT reads {why} (lab 06 leaves it in mode 2, low byte then high, reloading from 1193): this row tells a pipe call's own wakeup from the tick's by that count, and cannot"
            ));
            None
        }
    };
    let pit_sane = span.is_some();
    let span = span.unwrap_or(PIT_PM);
    // The read end non-blocking, so this vehicle never sleeps on it.
    r.flags.store(r.flags.load(Ordering::SeqCst) | O_NONBLOCK, Ordering::SeqCst);
    quiet(|| *PROBE_W.lock() = Some(w));
    let t = task::spawn(writer);
    // Let the writer fill the ring. Asleep: off the CPU and out of the run
    // queue while its write is unreturned. A writer that yields and retries
    // is always in the queue.
    for _ in 0..5 {
        sched::yield_now();
    }
    let asleep = yield_until(ASLEEP_MS, || off_cpu(t));
    let early = WROTE.load(Ordering::SeqCst);
    // Quiet windows: (slices given, most allowed) for the writer each time it
    // sleeps on the full ring before the vehicle reads, and for each reader.
    let mut wq: Vec<(u64, u64)> = Vec::new();
    let mut rq: Vec<(u64, u64)> = Vec::new();
    if asleep && early == u64::MAX {
        wq.push(quiet_window(t));
    }
    let mut got = 0usize;
    let mut bad: Option<usize> = None;
    let mut reads = 0u32;
    let mut again = 0u32;
    let mut end = None;
    let mut buf = [0u8; 512];
    // Yields from each clean read the writer had not run during until it ran
    // (u64::MAX: not within ASLEEP_MS).
    let mut waits: Vec<u64> = Vec::new();
    let mut on_read = Tally::default();
    let mut sleeping = asleep;
    let mut c = Clock::new();
    // The PM time of the last byte read (or of the start).
    let mut last = 0u64;
    let mut capped = false;
    'probe: loop {
        let now = c.ms();
        if now.saturating_sub(last) >= PROBE_IDLE_MS {
            break 'probe;
        }
        if now >= PROBE_CAP_MS {
            capped = true;
            break 'probe;
        }
        // One read: with the writer asleep on the full ring, it makes room.
        let s_d = sched::slices(t);
        let b = before();
        let res = r.read(&mut buf);
        let (woke, taint) = after(t, s_d, &b, span);
        let mut freed = false;
        match res {
            Ok(0) => {
                end = Some(0);
                break 'probe;
            }
            Ok(n) => {
                reads += 1;
                freed = true;
                last = c.ms();
                for (k, &b) in buf[..n].iter().enumerate() {
                    if bad.is_none() && b != probe_byte(got + k) {
                        bad = Some(got + k);
                    }
                }
                got += n;
            }
            Err(e) => {
                again += 1;
                if e != 11 {
                    end = Some(-e);
                    break 'probe;
                }
            }
        }
        let late = waits.last().is_some_and(|&k| k > WAKE_BOUND);
        if sleeping && freed && !late && WROTE.load(Ordering::SeqCst) == u64::MAX {
            on_read.add(woke, taint);
            if taint.is_none() && woke != Woke::Raced {
                waits.push(yields_until_ran(t, s_d));
            }
        } else if !freed {
            sched::yield_now();
        }
        // The next read is judged only if the writer is asleep again first.
        sleeping = yield_until(ASLEEP_MS, || WROTE.load(Ordering::SeqCst) != u64::MAX || off_cpu(t))
            && WROTE.load(Ordering::SeqCst) == u64::MAX;
        if sleeping {
            wq.push(quiet_window(t));
        }
    }
    let probe_ms = c.ms();
    let wrote = WROTE.load(Ordering::SeqCst);
    let prompt = on_read.clean() >= MIN_CLEAN_READS && waits.iter().all(|&k| k <= WAKE_BOUND);
    note(format_args!(
        "pipe probe: a kernel task wrote {PROBE_BYTES} bytes in one call; {}; it {} asleep within {ASLEEP_MS} ms; the test read {got} bytes in {reads} reads ({again} -EAGAIN) in {probe_ms} ms{}; then read returned {}; the write returned {}",
        if early == u64::MAX { "was still inside the write after the test had yielded" } else { "had already returned before the test read" },
        if asleep { "was seen" } else { "was never seen" },
        match bad {
            Some(i) => format!(", first wrong byte at {i}"),
            None => String::new(),
        },
        match end {
            Some(v) => format!("{v}"),
            None if capped => format!(
                "nothing more: the test had read {got} of {PROBE_BYTES} bytes in {reads} reads after {} s, about {} ms per read",
                PROBE_CAP_MS / 1000,
                PROBE_CAP_MS / u64::from(reads.max(1))
            ),
            None => format!("nothing within {} s of the last byte read", PROBE_IDLE_MS / 1000),
        },
        if wrote == u64::MAX { String::from("nothing yet") } else { format!("{}", wrote as i64) },
    ));
    note(format_args!(
        "pipe probe: each time the test read once from the full ring with the writer asleep, in a read no PIT tick touched and the writer did not run during, the writer ran again after {} of the test's yields (at most {WAKE_BOUND}; -1: not within {ASLEEP_MS} ms)",
        if waits.is_empty() {
            String::from("[none]")
        } else {
            format!("{:?}", waits.iter().map(|&k| if k == u64::MAX { -1 } else { k as i64 }).collect::<Vec<_>>())
        },
    ));
    if !prompt && !waits.iter().all(|&k| k <= WAKE_BOUND) {
        note(format_args!(
            "{SLEEP_ID}: a writer asleep on a full pipe did not run within {WAKE_BOUND} yields of a read making room: a read that frees space wakes the writers asleep on the pipe, so the writer runs on the next turn"
        ));
    }
    // The other side: a reader asleep on an empty pipe runs again within
    // WAKE_BOUND of the vehicle's yields once the vehicle writes a byte, and
    // again once the vehicle drops the only write end (end of file). One
    // pipe and one reader task per round, until both floors of clean rounds
    // are met (at most READER_MAX pipes), so a tick that happens to land
    // inside a call is retried, not judged.
    let fmt_k = |k: u64| if k == u64::MAX { -1 } else { k as i64 };
    let mut on_write: Vec<i64> = Vec::new();
    let mut on_close: Vec<i64> = Vec::new();
    let mut woke_reader = true;
    let mut seen = 0u32;
    let mut rounds = 0u32;
    let mut ends: Vec<String> = Vec::new();
    let mut on_byte = Tally::default();
    let mut on_drop = Tally::default();
    while rounds < READER_MAX && (on_byte.clean() < MIN_CLEAN_BYTES || on_drop.clean() < MIN_CLEAN_DROPS) {
        let (r3, w3) = match pipe::open(0) {
            Ok(p) => p,
            Err(e) => {
                ends.push(format!("pipe::open {}", errno_text(e)));
                woke_reader = false;
                break;
            }
        };
        rounds += 1;
        RDR_GOT.store(0, Ordering::SeqCst);
        RDR_END.store(u64::MAX, Ordering::SeqCst);
        quiet(|| *PROBE_R.lock() = Some(r3));
        let rt = task::spawn(reader);
        let a1 = yield_until(ASLEEP_MS, || off_cpu(rt));
        if a1 {
            rq.push(quiet_window(rt));
        }
        let s1 = sched::slices(rt);
        let b1 = before();
        let wr = w3.write(b"x");
        let (woke_w, taint_w) = after(rt, s1, &b1, span);
        if a1 {
            on_byte.add(woke_w, taint_w);
        }
        let k1 = if a1 { yields_until_ran(rt, s1) } else { u64::MAX };
        let a2 = yield_until(ASLEEP_MS, || RDR_GOT.load(Ordering::SeqCst) == 1 && off_cpu(rt));
        if a2 {
            rq.push(quiet_window(rt));
        }
        let s2 = sched::slices(rt);
        let b2 = before();
        drop(w3);
        let (woke_d, taint_d) = after(rt, s2, &b2, span);
        if a2 {
            on_drop.add(woke_d, taint_d);
        }
        let k2 = if a2 { yields_until_ran(rt, s2) } else { u64::MAX };
        let _ = yield_until(100, || RDR_END.load(Ordering::SeqCst) != u64::MAX);
        let _ = yield_until(100, || task::state(rt).is_none_or(|s| s == task::State::Dead));
        let e = RDR_END.load(Ordering::SeqCst);
        if a1 && a2 {
            seen += 1;
        }
        on_write.push(fmt_k(k1));
        on_close.push(fmt_k(k2));
        ends.push(if e == u64::MAX { String::from("none") } else if e == 0 { String::from("0") } else { format!("-{e}") });
        woke_reader &= a1 && a2 && wr == Ok(1) && k1 <= WAKE_BOUND && k2 <= WAKE_BOUND && RDR_GOT.load(Ordering::SeqCst) == 1 && e == 0;
        if !woke_reader {
            break;
        }
    }
    note(format_args!(
        "pipe probe: {seen} of {rounds} reader tasks were seen asleep on an empty pipe before and after the test wrote it one byte; each ran again after {on_write:?} of the test's yields once the byte was written, and after {on_close:?} once the test dropped the only write end (at most {WAKE_BOUND}; -1: not within {ASLEEP_MS} ms, or never seen asleep); their last reads returned {ends:?}"
    ));
    if !woke_reader && seen == rounds {
        note(format_args!(
            "{SLEEP_ID}: a reader asleep on an empty pipe did not run within {WAKE_BOUND} yields of a byte being written or of the last write end being dropped: a write wakes the readers asleep on the pipe, and so does the last write end going away"
        ));
    }
    // The queue check (G1166), on the clean rounds only (G1983): right after
    // each event, a sleeper that had not run yet must already be in the run
    // queue, and one that ran during a call no tick touched was run by it.
    let fmt_t = |t: &Tally| {
        format!(
            "{} of {} ({} more it ran during; not judged: {} a reload fell inside, {} began with IRQ 0 waiting or within {RELOAD_SLACK} counts of a reload)",
            t.queued, t.judged, t.raced, t.reload, t.near
        )
    };
    note(format_args!(
        "pipe probe: right after each event that must wake a sleeper, the test looked at the run queue with interrupts off, judging only the calls no PIT tick touched by channel 0's latched count: after a read making room, the writer was in it {}; after a one-byte write, the reader {}; after the last write end was dropped, the reader {}",
        fmt_t(&on_read),
        fmt_t(&on_byte),
        fmt_t(&on_drop),
    ));
    let woke_line = |what: &str, who: &str, t: &Tally| {
        if t.missed() > 0 {
            note(format_args!(
                "{SLEEP_ID}: right after {what}, the {who} asleep on the pipe had not run and was not in the run queue ({} of {} times): the {who} must be back in the run queue by the time the call returns. The pipe calls `wakeup` on the channel the {who} sleeps on, and lab 14's `wakeup` puts every task asleep there back in the run queue at once, not at a later yield or tick",
                t.missed(),
                t.judged,
            ));
        }
    };
    woke_line("a read made room on the full pipe", "writer", &on_read);
    woke_line("a one-byte write to the empty pipe", "reader", &on_byte);
    woke_line("the pipe's last write end was dropped", "reader", &on_drop);
    let queued_ok = on_read.missed() == 0 && on_byte.missed() == 0 && on_drop.missed() == 0;
    // The floors (G1983): enough calls no tick touched, on each side. A call
    // that wakes no one and waits for the tick inside it has none.
    let floor_line = |what: &str, who: &str, t: &Tally, min: u32| {
        if t.clean() < min {
            note(format_args!(
                "{SLEEP_ID}: of {} {what} with the {who} asleep, only {} had no PIT tick inside the call ({} spanned a reload of channel 0, {} began with IRQ 0 waiting or just after a reload); the row needs {min}: {}",
                t.clean() + t.tainted(),
                t.clean(),
                t.reload,
                t.near,
                // What was seen (sprint 7 #82, 2026-11-06): calls that a
                // reload fell inside waited for a tick; calls that began with
                // IRQ 0 already waiting were never judged at all, which is
                // the machine's late ticks, not the pipe (G2145). Only when
                // enough calls were made to meet the floor had they been clean.
                if t.near > t.reload && t.clean() + t.tainted() >= min {
                    format!(
                        "{} of them began with IRQ 0 already waiting or just after a reload: the machine's ticks arrived late, so too few calls could be judged",
                        t.near
                    )
                } else {
                    format!("the call that makes room or ends the wait wakes the {who} itself and returns in microseconds, with no tick inside it")
                },
            ));
        }
    };
    floor_line("reads making room on the full pipe", "writer", &on_read, MIN_CLEAN_READS);
    floor_line("one-byte writes to an empty pipe", "reader", &on_byte, MIN_CLEAN_BYTES);
    floor_line("drops of a pipe's last write end", "reader", &on_drop, MIN_CLEAN_DROPS);
    let floors_ok = on_read.clean() >= MIN_CLEAN_READS && on_byte.clean() >= MIN_CLEAN_BYTES && on_drop.clean() >= MIN_CLEAN_DROPS;
    // Channel 0 again, after every round: still counting as at the start,
    // so a round whose count stood still (a call shorter than one PIT input
    // clock) was clean, not a stopped channel.
    let pit_after = pit_check();
    if let (true, Err(why)) = (pit_sane, &pit_after) {
        note(format_args!(
            "{SLEEP_ID}: after the probe's rounds, channel 0 of the PIT reads {why}; it read as lab 06 leaves it before them: it was reprogrammed while the pipe calls ran, and this row cannot tell a call's own wakeup from the tick's"
        ));
    }
    let pit_ok = pit_sane && pit_after.is_ok() && !PIT_ABOVE.load(Ordering::SeqCst);
    if PIT_ABOVE.load(Ordering::SeqCst) {
        note(format_args!(
            "{SLEEP_ID}: channel 0 of the PIT read above 1193 while the pipe calls ran: it was reprogrammed, and this row cannot tell a call's own wakeup from the tick's"
        ));
    }
    // Every window held: the writer's (at least the first) and two per reader.
    let fmt_q = |v: &[(u64, u64)]| format!("{:?}", v.iter().map(|&(n, a)| format!("{n}/{a}")).collect::<Vec<_>>());
    let held = |v: &[(u64, u64)]| v.iter().all(|&(n, a)| n <= a);
    let quiet_ok = !wq.is_empty() && rq.len() == 2 * rounds as usize && held(&wq) && held(&rq);
    note(format_args!(
        "pipe probe: with nothing to wake it, over {QUIET} of the test's yields each time, the writer asleep on the full ring was given {} slices, and the readers asleep on empty pipes {} (given/most: 1 plus one per PIT tick the window spanned by the PM timer)",
        fmt_q(&wq),
        fmt_q(&rq),
    ));
    if !(held(&wq) && held(&rq)) {
        note(format_args!(
            "{SLEEP_ID}: in one of the test's quiet windows ({QUIET} of its yields, with nothing to wake the sleeper) a sleeper was given more slices than the window allows: 1 plus one per PIT tick the window spanned (given/most above). A sleeper runs again when the read, write or release it waits for happens; a kernel that puts sleepers back in the run queue on its own, more often than the tick, gives them slices here"
        ));
    }
    // The ring reuses the room a read frees before it empties (G1972).
    let reuse = ring_reuse();
    note(format_args!(
        "pipe probe: on a pipe of the test's own, filled until a write waited, a read took 512, and a 512-byte write then {}",
        match &reuse {
            Ok(cap) => format!("went in whole at once (the ring took {cap} bytes before a write waited)"),
            Err(_) => String::from("did not (below)"),
        }
    ));
    if let Err(why) = &reuse {
        note(format_args!("{SLEEP_ID}: {why}"));
    }
    let reuse_ok = reuse.is_ok();
    let ok = asleep && early == u64::MAX && prompt && woke_reader && queued_ok && floors_ok && pit_ok && quiet_ok && reuse_ok && got == PROBE_BYTES && bad.is_none() && end == Some(0) && wrote == PROBE_BYTES as u64;
    row(ok, SLEEP_ROW, SLEEP_ID);
    drop(r);
    // -EPIPE: a write with every read end gone. -EBADF: a closed fd.
    let epipe = match pipe::open(0) {
        Ok((r2, w2)) => {
            drop(r2);
            let x = w2.write(b"x");
            drop(w2);
            format!("{x:?}")
        }
        Err(e) => format!("pipe::open: {}", errno_text(e)),
    };
    let ebadf = console::with_fds(|t| t.get(console::NFD as u64 - 1).map(|_| ()));
    note(format_args!(
        "pipe probe: write with no read end open = {epipe} (EPIPE is 32); the file behind fd {} with nothing open there = {ebadf:?} (EBADF is 9)",
        console::NFD - 1
    ));
    let _ = yield_until(100, || task::state(t).is_none_or(|s| s == task::State::Dead));
}

// ── G1972: the ring reuses the room a read frees (Q2 (b), 2026-11-04) ───

/// A write job for `ring_writer`: an end and a length; the end comes back
/// in `RING_BACK` before `RING_WROTE` is set.
static RING_W: SpinLock<Option<(FileRef, usize)>> = SpinLock::new(None);
static RING_BACK: SpinLock<Option<FileRef>> = SpinLock::new(None);
static RING_WROTE: AtomicU64 = AtomicU64::new(u64::MAX);

/// One blocking write of the job's length, then the end is handed back.
fn ring_writer() {
    let job = quiet(|| RING_W.lock().take());
    let Some((w, n)) = job else { return };
    let buf: Vec<u8> = (0..n).map(probe_byte).collect();
    let r = match w.write(&buf) {
        Ok(k) => k as u64,
        Err(e) => (-e) as u64,
    };
    quiet(|| *RING_BACK.lock() = Some(w));
    RING_WROTE.store(r, Ordering::SeqCst);
}

/// How one helper write ended: returned (+n or -errno), asleep, or neither
/// within `ASLEEP_MS`.
enum Wrote {
    Returned(u64),
    Asleep,
    Neither,
}

/// Asleep is decided only once the write has had all of `ASLEEP_MS` to
/// return and the writer is then off the CPU and out of the run queue: a
/// writer the tick preempted, or one that yields inside its write, is in the
/// run queue, and a short off-CPU stretch that ends in time is not a sleep.
fn ring_write(w: FileRef, n: usize) -> Wrote {
    RING_WROTE.store(u64::MAX, Ordering::SeqCst);
    quiet(|| *RING_W.lock() = Some((w, n)));
    let t = task::spawn(ring_writer);
    let _ = yield_until(ASLEEP_MS, || RING_WROTE.load(Ordering::SeqCst) != u64::MAX);
    match RING_WROTE.load(Ordering::SeqCst) {
        u64::MAX if off_cpu(t) => Wrote::Asleep,
        u64::MAX => Wrote::Neither,
        v => Wrote::Returned(v),
    }
}

/// The most bytes row 7's fill puts into a pipe no one reads before it fails
/// the row (G2058, 2026-11-06): the probe's own write above, `PROBE_BYTES`,
/// which must already have slept on the full ring, so a ring that holds this
/// much without a write waiting was refused there too, and the fill is
/// bounded whatever the learner's ring does.
const FILL_MAX: usize = PROBE_BYTES;
/// The fill's step after its first `PIPE_BUF` (4096) bytes, and the size of
/// the read and the write the reuse check makes.
const FILL_STEP: usize = 512;

/// One step of a fill: the write returned all its bytes and the end came
/// back, or it waited (no room: still unreturned after `ASLEEP_MS`, asleep
/// or not; the end stays with the writer task until `ring_drain` lets it
/// finish).
enum Step {
    Took(FileRef),
    Waited,
}

fn ring_step(w: FileRef, n: usize, at: usize) -> Result<Step, String> {
    match ring_write(w, n) {
        Wrote::Returned(v) if v == n as u64 => quiet(|| RING_BACK.lock().take())
            .map(Step::Took)
            .ok_or_else(|| String::from("the write end did not come back from the writer task")),
        Wrote::Returned(v) => Err(format!("with {at} bytes in a pipe no one read, a {n}-byte write returned {}", v as i64)),
        Wrote::Asleep | Wrote::Neither => Ok(Step::Waited),
    }
}

/// The first write into an empty pipe: 4096 bytes (PIPE_BUF), which lab 15's
/// ring takes whole at once.
fn ring_first(w: FileRef) -> Result<FileRef, String> {
    let full = ring_write(w, 4096);
    let Wrote::Returned(4096) = full else {
        return Err(match full {
            Wrote::Returned(v) => format!("a write of 4096 bytes (PIPE_BUF) to an empty pipe returned {}", v as i64),
            Wrote::Asleep => String::from("a write of 4096 bytes (PIPE_BUF) to an empty pipe slept: lab 15's ring takes a write of at most PIPE_BUF bytes as soon as that much room is free"),
            Wrote::Neither => format!("a write of 4096 bytes (PIPE_BUF) to an empty pipe neither returned nor slept within {ASLEEP_MS} ms"),
        });
    };
    quiet(|| RING_BACK.lock().take()).ok_or_else(|| String::from("the write end did not come back from the writer task"))
}

/// Read the pipe empty until the helper writer's last write has returned
/// (bounded, and never sleeping: the read end is non-blocking), then drop
/// the end it hands back.
fn ring_drain(r: &FileRef) {
    let mut buf = [0u8; FILL_STEP];
    let _ = yield_until(ASLEEP_MS, || {
        while let Ok(n) = r.read(&mut buf) {
            if n == 0 {
                break;
            }
        }
        RING_WROTE.load(Ordering::SeqCst) != u64::MAX
    });
    drop(quiet(|| RING_BACK.lock().take()));
}

/// A pipe of the test's own, open for reading (non-blocking) and writing.
fn ring_pipe() -> Result<(FileRef, FileRef), String> {
    let (r, w) = pipe::open(0).map_err(|e| format!("pipe::open(0) returned {}", errno_text(e)))?;
    r.flags.store(r.flags.load(Ordering::SeqCst) | O_NONBLOCK, Ordering::SeqCst);
    Ok((r, w))
}

/// How many bytes a pipe no one reads takes before a write waits, whatever
/// the ring's length (G2058): 4096 at once, then `FILL_STEP` at a time
/// until a write is still unreturned after `ASLEEP_MS`; more than `FILL_MAX`
/// with no write waiting fails the row by itself. The pipe is then read
/// empty, so the waiting writer finishes, and dropped.
fn ring_capacity() -> Result<usize, String> {
    let (r, w) = ring_pipe()?;
    let res = match ring_first(w) {
        Err(e) => Err(e),
        Ok(mut w) => {
            let mut cap = 4096;
            loop {
                if cap >= FILL_MAX {
                    drop(w);
                    break Err(format!(
                        "a pipe no one read took {cap} bytes and no write waited: a write with no room in the ring waits for a read, and the probe's writer above must already have slept on a full ring of fewer bytes than that"
                    ));
                }
                match ring_step(w, FILL_STEP, cap) {
                    Ok(Step::Took(next)) => {
                        w = next;
                        cap += FILL_STEP;
                    }
                    Ok(Step::Waited) => break Ok(cap),
                    Err(e) => break Err(e),
                }
            }
        }
    };
    ring_drain(&r);
    drop(r);
    res
}

/// Row 7's reuse check (G1972, 2026-11-04; filled to the ring's own length
/// since G2058, 2026-11-06): a first pipe of the test's own measures how many
/// bytes the ring takes before a write waits (`ring_capacity`); a second,
/// with no task asleep on it while the test reads, takes that many again (so
/// it is full), gives 512 to a read, and must then take a 512-byte write with
/// 512 bytes of room free at the ring's front. Lab 15's ring takes a write of
/// at most PIPE_BUF bytes as soon as that much room is free (this course's
/// rule, CONTRACT §B.2-15; POSIX does not ask it), so it returns; a ring that
/// uses its bytes front to back and reuses them only once it is empty
/// (G1972's never-wraps) has no room after its end, however long the ring,
/// and puts the writer to sleep. Before G2058 the check filled exactly 4096
/// bytes, so a never-wrapping ring 512 or more bytes longer than PIPE_BUF
/// still had room at its tail and scored 11/11. Within `ASLEEP_MS` the
/// verdict is the ring's state, not a time: no tick and no yield inside a
/// call can move it, and the read wakes no one, so a read that yields after
/// a wakeup has nothing to yield to. A write still unreturned at `ASLEEP_MS`
/// with its writer runnable is `Wrote::Neither`, which fails the row too.
/// Ok is the ring's measured capacity.
fn ring_reuse() -> Result<usize, String> {
    let cap = ring_capacity()?;
    let (r, w) = ring_pipe()?;
    let verdict = (|| {
        let mut w = ring_first(w)?;
        let mut put = 4096;
        while put < cap {
            match ring_step(w, FILL_STEP, put)? {
                Step::Took(next) => {
                    w = next;
                    put += FILL_STEP;
                }
                Step::Waited => {
                    return Err(format!(
                        "a pipe no one read took {cap} bytes before a write waited, and a second one only {put}"
                    ))
                }
            }
        }
        let mut buf = [0u8; FILL_STEP];
        match r.read(&mut buf) {
            Ok(FILL_STEP) => {}
            other => return Err(format!("then a 512-byte read from the full pipe ({cap} bytes) returned {other:?}")),
        }
        match ring_write(w, FILL_STEP) {
            Wrote::Returned(512) => Ok(cap),
            Wrote::Returned(v) => Err(format!("then a 512-byte write with 512 bytes of room free returned {}", v as i64)),
            Wrote::Asleep => Err(format!(
                "{cap} bytes went into an empty pipe, filling it (on a pipe of the test's own, a further write waited), and a read took 512 of them; then a 512-byte write slept with 512 bytes of room free: the ring does not reuse the room a read frees at its front until it is empty. Lab 15's ring takes a write of at most PIPE_BUF bytes as soon as that much room is free, wherever in the ring it lies"
            )),
            Wrote::Neither => Err(format!("then a 512-byte write with 512 bytes of room free neither returned nor slept within {ASLEEP_MS} ms")),
        }
    })();
    // Drain, so a sleeping writer goes in and returns.
    ring_drain(&r);
    drop(r);
    verdict
}

// ── Row 6: a last write end dropped by exec and by dup2 ──────────────────

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

/// A static ELF of one R X `PT_LOAD` at `proc::BASE` (file offset 0), the
/// code at offset 120 — lab 13's `exec-probe` layout.
fn program(code: &[u8]) -> Vec<u8> {
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
    b[120..].copy_from_slice(code);
    b
}

/// `exec-probe` (lab 13's): `execve(argv[1], &argv[2], envp)`; if that
/// returns, `exit(100 + errno)`.
const EXEC_PROBE_CODE: [u8; 42] = [
    0x48, 0x8b, 0x7c, 0x24, 0x10, // mov rdi, [rsp+16]          argv[1]: the path
    0x48, 0x8d, 0x74, 0x24, 0x18, // lea rsi, [rsp+24]          &argv[2]: the new argv
    0x48, 0x8b, 0x0c, 0x24, //       mov rcx, [rsp]             argc
    0x48, 0x8d, 0x54, 0xcc, 0x10, // lea rdx, [rsp+rcx*8+16]    envp
    0xb8, 59, 0, 0, 0, //            mov eax, 59 (execve)
    0x0f, 0x05, //                   syscall
    0xf7, 0xd8, //                   neg eax
    0x83, 0xc0, PROBE_FAILED as u8, // add eax, 100
    0x89, 0xc7, //                   mov edi, eax
    0xb8, 60, 0, 0, 0, //            mov eax, 60 (exit)
    0x0f, 0x05, //                   syscall
    0x0f, 0x0b, //                   ud2
];

/// `dup2-probe`: `dup2(0, 3)`; if that returns 3, park (as `parker`);
/// else `exit(100 + errno)`.
const DUP2_PROBE_CODE: [u8; 44] = [
    0x31, 0xff, //                   xor edi, edi               old = 0
    0xbe, 3, 0, 0, 0, //             mov esi, 3                 new = 3
    0xb8, 33, 0, 0, 0, //            mov eax, 33 (dup2)
    0x0f, 0x05, //                   syscall
    0x83, 0xf8, 3, //                cmp eax, 3
    0x75, 9, //                      jne fail
    0xb8, 24, 0, 0, 0, //      park: mov eax, 24 (sched_yield)
    0x0f, 0x05, //                   syscall
    0xeb, 0xf7, //                   jmp park
    0xf7, 0xd8, //             fail: neg eax
    0x83, 0xc0, PROBE_FAILED as u8, // add eax, 100
    0x89, 0xc7, //                   mov edi, eax
    0xb8, 60, 0, 0, 0, //            mov eax, 60 (exit)
    0x0f, 0x05, //                   syscall
    0x0f, 0x0b, //                   ud2
];

/// `parker`: `sched_yield` for ever. It never exits, so a process parked
/// here never closes its fds.
const PARKER_CODE: [u8; 9] = [
    0xb8, 24, 0, 0, 0, //            mov eax, 24 (sched_yield)
    0x0f, 0x05, //                   syscall
    0xeb, 0xf7, //                   jmp back
];

fn mount_ktest() -> bool {
    let files: Vec<(&'static str, InodeRef)> = alloc::vec![
        ("exec-probe", Arc::new(MemFile { bytes: program(&EXEC_PROBE_CODE), ino: 2 }) as InodeRef),
        ("dup2-probe", Arc::new(MemFile { bytes: program(&DUP2_PROBE_CODE), ino: 3 })),
        ("parker", Arc::new(MemFile { bytes: program(&PARKER_CODE), ino: 4 })),
    ];
    match vfs::mount("/ktest", Arc::new(KtestDir { files })) {
        Ok(()) => true,
        Err(e) => {
            note(format_args!("{DROP_ID}: vfs::mount(\"/ktest\", the test's directory) returned {}", errno_text(e)));
            false
        }
    }
}

/// The read end the drop case's reader task takes, and what its one read
/// returned (+n, or -errno), once it has.
static DROP_R: SpinLock<Option<FileRef>> = SpinLock::new(None);
static DROP_READING: AtomicBool = AtomicBool::new(false);
static DROP_GOT: AtomicU64 = AtomicU64::new(u64::MAX);

/// The reader task: one blocking read of the pipe; the ring stays empty, so
/// it sleeps until the last write end goes.
fn drop_reader() {
    let r = quiet(|| DROP_R.lock().take());
    let Some(r) = r else { return };
    DROP_READING.store(true, Ordering::SeqCst);
    let mut buf = [0u8; 16];
    let v = match r.read(&mut buf) {
        Ok(n) => n as u64,
        Err(e) => (-e) as u64,
    };
    DROP_GOT.store(v, Ordering::SeqCst);
    drop(r);
}

/// Task `t` is asleep: not on the CPU, not in the run queue, not ended.
fn off_cpu(t: Tid) -> bool {
    task::with_table(|tt| tt.current != t && !tt.ready.contains(&t) && tt.get(t).is_some_and(|x| x.state != task::State::Dead))
}

fn task_of(pid: Pid) -> Option<Tid> {
    let mut tid = None;
    proc::each(|p| {
        if p.pid == pid {
            tid = Some(p.task);
        }
    });
    tid
}

fn record(pid: Pid) -> (Vec<String>, Option<i32>) {
    let mut out = (Vec::new(), None);
    quiet(|| {
        proc::each(|p| {
            if p.pid == pid {
                out = (p.argv.clone(), p.exit);
            }
        })
    });
    out
}

/// One sub-case: a reader asleep on a new pipe; `path` started with `argv`
/// and handed the pipe's only write end as fd 3 (`cloexec` as given), then
/// let run. Ok when the reader was seen asleep, its read returned 0, and
/// the program is still running with `argv` = `after` (it parked: so the
/// exec's or the dup2's drop, not an exit, ended the read).
fn drop_case(what: &str, path: &str, argv: &[&str], cloexec: bool, after: &[&str]) -> bool {
    let (r, w) = match pipe::open(0) {
        Ok(p) => p,
        Err(e) => {
            note(format_args!("{DROP_ID}: {what}: pipe::open(0) returned {}", errno_text(e)));
            return false;
        }
    };
    DROP_READING.store(false, Ordering::SeqCst);
    DROP_GOT.store(u64::MAX, Ordering::SeqCst);
    quiet(|| *DROP_R.lock() = Some(r));
    let t = task::spawn(drop_reader);
    let asleep = yield_until(ASLEEP_MS, || DROP_READING.load(Ordering::SeqCst) && DROP_GOT.load(Ordering::SeqCst) == u64::MAX && off_cpu(t));
    if !asleep {
        // The write end goes here, with no lock held; the reader may wake.
        drop(w);
        let got = DROP_GOT.load(Ordering::SeqCst);
        note(format_args!(
            "{DROP_ID}: {what}: the reader task was not seen asleep on the empty pipe within {ASLEEP_MS} ms (its read {}); a reader that finds the ring empty with a writer open must sleep",
            if got == u64::MAX { String::from("had not returned") } else { format!("returned {}", got as i64) }
        ));
        return false;
    }
    // Started and handed the write end with interrupts off: the program must
    // not run before fd 3 is in its table.
    let started = quiet(|| {
        let pid = match exec::spawn(path, argv, &[]) {
            Ok(pid) => pid,
            Err(e) => return Err((format!("exec::spawn(\"{path}\", …) returned {}", errno_text(e)), Some(w))),
        };
        let Some(tid) = task_of(pid) else {
            return Err((format!("pid {} has no task in proc::each", pid.0), Some(w)));
        };
        let d = Fd { file: w, cloexec };
        let back: Result<Option<Fd>, Fd> = task::with_table(move |tt| match tt.get_mut(tid) {
            Some(task) => Ok(task.fds.install(DROP_FD, d).ok().flatten()),
            None => Err(d),
        });
        match back {
            // Nothing was at fd 3 in a program just started (fds 0-2 only).
            Ok(old) => drop(old),
            Err(d) => return Err((format!("task {:?} of pid {} vanished before it ran", tid, pid.0), Some(d.file))),
        }
        // Printed before the program can run: if its drop hangs the
        // machine, this is the line before the `# deadlock:` one.
        note(format_args!(
            "{DROP_ID}: {what}: the reader task is asleep on an empty pipe; pid {} ({path} {argv:?}) holds the pipe's only write end as fd {DROP_FD}{}",
            pid.0,
            if cloexec { ", close-on-exec" } else { "" }
        ));
        Ok(pid)
    });
    let pid = match started {
        Ok(pid) => pid,
        Err((why, w)) => {
            drop(w);
            note(format_args!("{DROP_ID}: {what}: {why}"));
            let _ = yield_until(EOF_MS, || DROP_GOT.load(Ordering::SeqCst) != u64::MAX);
            return false;
        }
    };
    let mut c = Clock::new();
    let woke = yield_until(EOF_MS, || DROP_GOT.load(Ordering::SeqCst) != u64::MAX);
    let us = c.us();
    let got = DROP_GOT.load(Ordering::SeqCst);
    let (now, exit) = record(pid);
    let parked = exit.is_none() && now.iter().map(|s| s.as_str()).eq(after.iter().copied());
    note(format_args!(
        "{DROP_ID}: {what}: the reader's read {}; pid {} is now {now:?}, {}",
        if woke { format!("returned {} after {us} µs", got as i64) } else { format!("had not returned after {EOF_MS} ms: the write end was never released, or its release did not wake the reader") },
        pid.0,
        match exit {
            None => String::from("still running"),
            Some(s) => format!("{} — {}", status_text(Some(s)), match (s & 0x7f, (s >> 8) & 0xff) {
                (0, code) if code > PROBE_FAILED => format!("its call returned {}", errno_text((code - PROBE_FAILED) as Errno)),
                _ => String::from("it was due to park, not end"),
            }),
        }
    ));
    let _ = yield_until(100, || task::state(t).is_none_or(|s| s == task::State::Dead));
    woke && got == 0 && parked
}

/// `FdTable::fcntl` (lab 15 on) where the cut has it: a path to an
/// associated function takes the inherent one first, so before lab 15 this
/// answers `-ENOSYS` and this file still builds.
trait FcntlOf {
    #[allow(dead_code)]
    fn fcntl(&mut self, fd: u64, cmd: u32, arg: u64) -> Result<i64, Errno>;
}

impl FcntlOf for FdTable {
    fn fcntl(&mut self, _: u64, _: u32, _: u64) -> Result<i64, Errno> {
        Err(38)
    }
}

/// Row 8: `fcntl(F_DUPFD_CLOEXEC)` on the vehicle's own fd table (the
/// module doc). Each `Fd` is read by swapping it out with
/// `install`, so its own `cloexec` and file are what is checked.
fn dupfd_cloexec() -> bool {
    let (a, b) = match (FileRef::open(FileObj::CONSOLE, O_RDWR), FileRef::open(FileObj::CONSOLE, O_RDWR)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            note(format_args!("{DUPFD_ID}: FileRef::open(the console, O_RDWR) returned {}", errno_text(e)));
            return false;
        }
    };
    let want = DUPFD_MIN + DUPFD_TAKEN;
    let (got, new_fd, src_fd, back, closed) = console::with_fds(|t| {
        let mut back = Vec::new();
        back.push(t.install(DUPFD_SRC, Fd { file: a.clone(), cloexec: false }));
        for i in 0..DUPFD_TAKEN {
            back.push(t.install(DUPFD_MIN + i, Fd { file: b.clone(), cloexec: false }));
        }
        let got = FdTable::fcntl(t, DUPFD_SRC as u64, DUPFD_CLOEXEC, DUPFD_MIN as u64);
        let n = got.ok().filter(|&n| n >= 0 && (n as usize) < console::NFD).map(|n| n as usize);
        let new_fd = n.and_then(|n| t.install(n, Fd { file: b.clone(), cloexec: false }).ok().flatten());
        let src_fd = t.install(DUPFD_SRC, Fd { file: b.clone(), cloexec: false }).ok().flatten();
        let mut used: Vec<usize> = (DUPFD_MIN..DUPFD_MIN + DUPFD_TAKEN).collect();
        used.push(DUPFD_SRC);
        used.extend(n.filter(|n| !used.contains(n)));
        let closed: Vec<_> = used.into_iter().map(|i| t.close(i as u64)).collect();
        (got, new_fd, src_fd, back, closed)
    });
    let same = |d: &Option<Fd>| d.as_ref().is_some_and(|d| core::ptr::eq(&*d.file, &*a));
    let ok = got == Ok(want as i64)
        && same(&new_fd)
        && new_fd.as_ref().is_some_and(|d| d.cloexec)
        && same(&src_fd)
        && src_fd.as_ref().is_some_and(|d| !d.cloexec);
    let mark = |d: &Option<Fd>| match d {
        None => String::from("nothing"),
        Some(d) => format!(
            "{}, close-on-exec {}",
            if core::ptr::eq(&*d.file, &*a) { "the open file fd 4 was given" } else { "another open file" },
            if d.cloexec { "set" } else { "clear" }
        ),
    };
    note(format_args!(
        "{DUPFD_ID}: fcntl({DUPFD_SRC}, F_DUPFD_CLOEXEC, {DUPFD_MIN}) with fds {DUPFD_MIN} and {} taken returned {}; the new fd names {}; fd {DUPFD_SRC} names {}{}",
        DUPFD_MIN + DUPFD_TAKEN - 1,
        match got {
            Ok(n) => format!("{n}"),
            Err(e) => errno_text(e),
        },
        mark(&new_fd),
        mark(&src_fd),
        if ok {
            String::new()
        } else {
            format!("; F_DUPFD_CLOEXEC (1030) is F_DUPFD that also marks the new fd close-on-exec: it must return {want}, the lowest free fd from {DUPFD_MIN} up, naming fd {DUPFD_SRC}'s open file with the mark set, and leave fd {DUPFD_SRC}'s own mark clear")
        }
    ));
    drop((new_fd, src_fd, back, closed));
    ok
}

fn drop_row(mounted: bool) {
    let ok = mounted && mount_ktest() && {
        let a = drop_case("exec", EXEC_PROBE, &["exec-probe", PARKER, "parker"], true, &["parker"]);
        let b = drop_case("dup2", DUP2_PROBE, &["dup2-probe"], false, &["dup2-probe"]);
        a && b
    };
    if !mounted {
        note(format_args!("{DROP_ID}: not run, / is not mounted"));
    }
    row(ok, DROP_ROW, DROP_ID);
}

// ── The run ──────────────────────────────────────────────────────────────

/// What the run measured.
struct Run {
    init_exit: Option<i32>,
    left: Vec<Seen>,
    ms: u64,
    peak_procs: usize,
}

fn run(init: Pid) -> Run {
    let mut r = Run { init_exit: None, left: Vec::new(), ms: 0, peak_procs: 0 };
    let mut c = Clock::new();
    loop {
        let all = snapshot();
        r.ms = c.ms();
        r.peak_procs = r.peak_procs.max(all.len());
        r.init_exit = find(&all, init).and_then(|s| s.exit);
        if r.init_exit.is_some() || r.ms >= RUN_MS {
            r.left = all.into_iter().filter(|s| s.exit.is_none()).collect();
            return r;
        }
        sched::yield_now();
    }
}

fn report(init: Pid, r: &Run) {
    note(format_args!(
        "init: pid {} {} after {} ms",
        init.0,
        match r.init_exit {
            Some(_) => status_text(r.init_exit),
            None => format!("did not end within {} s", RUN_MS / 1000),
        },
        r.ms
    ));
    if !r.left.is_empty() {
        let who: Vec<String> = r.left.iter().map(|s| format!("pid {} (parent {})", s.pid.0, s.ppid.0)).collect();
        note(format_args!("still running at the end: {}; any row they had not printed is missing", who.join(", ")));
    }
}

fn programs_never_ran() {
    for (i, id) in ROW_IDS.iter().enumerate() {
        row(false, i + 1, id);
    }
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
    let nonce = nonce_str();

    probe_pipe();
    let probe_waits = pipe::full_waits();
    // Second, before init: no program's run or drop can leave it missing.
    row(dupfd_cloexec(), DUPFD_ROW, DUPFD_ID);

    // The stream producer writes and consumer checks (user/producer/stream.h).
    let seed = ktap::nonce().map_or(0, |n| ktap::derive(&n, CHAIN_ID));
    let len = 4096 + seed % 8192;
    let mut fnv: u64 = 0xcbf2_9ce4_8422_2325;
    for i in 0..len {
        let b = (ktap::splitmix64(seed ^ (i >> 3)) >> (8 * (i & 7))) as u8;
        fnv = (fnv ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    note(format_args!("{CHAIN_ID}: derive(nonce, \"{CHAIN_ID}\") = {seed:016x}: the stream is {len} bytes, fnv1a64 {fnv:016x}"));

    let mounted = mount_archive();
    let envp = format!("SBOOT_NONCE={nonce}");
    let spawned = if mounted {
        exec::spawn(INIT, &[INIT, nonce.as_str(), "1", "15"], &[envp.as_str()])
    } else {
        note(format_args!("init: not run, / is not mounted"));
        Err(0)
    };
    match spawned {
        Ok(init) => {
            let r = run(init);
            report(init, &r);
            note(format_args!(
                "pipe: writers slept on a full ring {} times while init ran ({} in the probe before it); 0 means no writer ever waited for a reader",
                pipe::full_waits().saturating_sub(probe_waits),
                probe_waits
            ));
            note(format_args!("at the busiest look: {} processes alive or not yet collected", r.peak_procs));
        }
        Err(e) => {
            if mounted {
                note(format_args!("init: exec::spawn(\"{INIT}\", …) returned {}; nothing ran", errno_text(e)));
            }
            programs_never_ran();
        }
    }
    // Last: a kernel that drops the write end under the task table's lock
    // hangs here, and nothing after this would print.
    drop_row(mounted);
    sched::yield_now();
    quiet(ktap::end);
    halt()
}
