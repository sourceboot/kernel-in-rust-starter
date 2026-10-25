//! Lab 25's vehicle (`25-linux-program`): three ordinary static musl
//! programs, built by the course (`user/build.sh`, layout `musl`) and run
//! unmodified on the learner's kernel.
//!
//! The disk is `ktest-25.img` as the course's image writer made it (CONTRACT
//! §A.7): `/bin` holds `hello`, `tls` and `mem`. The vehicle mounts ext2 at
//! `/` from `/dev/hda2` through the learner's `Ata`, `partition2` and
//! `Ext2::mount`, then starts each program with the learner's exec. Each
//! program's fds 1 and 2 are a pipe this file drains (its output is kept,
//! never forwarded); `hello`'s fd 0 is a second pipe holding one line. Every
//! value a row expects is computed here from the run's nonce and from words
//! drawn from the time-stamp counter, each printed only after the row that
//! carries it has read it back (CONTRACT §A.1-5).
//!
//! Before any program runs, the vehicle calls the learner's
//! `VmaList::carve` (lab 25 step 1) on a list it builds: the cut that
//! `munmap` and `mremap` stand on, checked where a failure can name it.
//!
//! The four rows, printed in this order:
//!
//!   1 `musl.hello.line` (derived) — `hello <16 hex: derive>` with
//!     `t<12 hex: a drawn word>` on stdin prints exactly
//!     `hello, musl: <splitmix64(a ^ w)> t<w>` and `ids 0 0 0 0 pid <its
//!     pid> cpus 1 slept <20 + a % 40> ms realtime <s> sig ok futex ok`,
//!     where `s` is `fs::ext2::EPOCH` plus the monotonic clock's seconds
//!     while it ran; it runs at least the sleep it asked for by the PM
//!     timer; it exits 0. Red when musl's start-up (`arch_prctl`), `readv`,
//!     `writev`, an id, the CPU set, a clock, the sleep, the signal tables
//!     or the futex answer wrong — `hello` checks each and says which.
//!   2 `musl.tls.survives.preempt` (derived) — two copies of `tls`, run at
//!     once with different words and slots, each print
//!     `tls <splitmix64(s ^ splitmix64(s ^ 0x7115))> t<w> gaps A B turns C D`
//!     (s = a ^ w) and exit 0, with A and B (the stretches each copy spent
//!     off the CPU in its two phases) both at least 1. Red when the FS base
//!     or the SSE registers are not each task's own across a switch, when
//!     `ARCH_SET_FS`/`ARCH_GET_FS` do not set and give back the base, or
//!     when the copies were never preempted.
//!   3 `musl.mem.32m.checksum` (derived) — `VmaList::carve` gives the
//!     ranges and the list five cases expect (a head, a tail past the end,
//!     a hole, across two VMAs, nothing); `mem <16 hex> t<12 hex>` churns
//!     32 MiB through musl's malloc (mmap, munmap, mremap), grows its stack,
//!     asks the mmap family directly, and prints exactly
//!     `mem <checksum> t<w> allocs <n> reallocs <n> frees <n>` as this file
//!     computes them; it exits 0; and once it has ended, the frames in use
//!     are back within `FRAME_SLACK` of what they were before it ran. Then,
//!     once `mem` has reached its exit with every call answered, a process this file builds (`probe_image`) maps
//!     two anonymous mappings of `PROBE_PAGES` pages with the learner's
//!     `mm::mmap` and writes to every page in ring 3; the learner's
//!     `mm::munmap` takes three pages from the middle of one and `mm::mremap`
//!     shrinks the other in place to five pages, then with `MREMAP_MAYMOVE`
//!     to three, and after each call this file reads
//!     the space's leaf entries (`AddressSpace::entry_mut`) and the frame
//!     pool (`frame::refs`, `frame::stats`): no page given back may keep a
//!     present entry, and every frame it held must be back (`probe_unmap`).
//!   4 `musl.enosys.none` (plain) — `syscall::enosys_count()` did not move
//!     while the three programs ran, and each ran to its exit. A failure
//!     names the programs whose calls moved it.
//!
//! Plan 4. A program that is still running after `RUN_MS` (5 s) by the PM
//! timer fails its row; the vehicle then goes on to the next.

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
use kernel::blk::ata::{self, Ata, BlockDevice};
use kernel::console::{Fd, FileRef};
use kernel::exec;
use kernel::fs::ext2::{self, Ext2};
use kernel::fs::mount;
use kernel::frame::Frame;
use kernel::fs::vfs::InodeRef;
use kernel::idt::TrapFrame;
use kernel::ktap::{self, splitmix64};
use kernel::mm::{self, Kill, KillReason};
use kernel::paging::flags;
use kernel::pipe;
use kernel::proc::{self, prot, Pid, Vma, VmaKind, VmaList};
use kernel::sync::SpinLock;
use kernel::syscall::{self, Errno};
use kernel::task::{self, Tid};
use kernel::{bootinfo, frame, gdt, heap, idt, irq, sched, serial, time};

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 64;
const PM_HZ: u64 = 3_579_545;
const O_NONBLOCK: u32 = 0x800;

const PLAN: usize = 4;
const HELLO_ID: &str = "musl.hello.line";
const TLS_ID: &str = "musl.tls.survives.preempt";
const MEM_ID: &str = "musl.mem.32m.checksum";
const ENOSYS_ID: &str = "musl.enosys.none";

/// How long one row's programs may run, by the PM timer: max(5 s, 3 x the
/// slowest honest program under stress), rounded up to a whole second. The
/// slowest measured 2026-10-23 was `tls`, 1.228 s with 24 busy loops on the
/// host, so the 5 s floor holds. A kernel whose three programs all hang ends
/// its run after three bounds, about 17 s on a quiet host.
const RUN_MS: u64 = 5_000;
/// How long each `tls` copy spins in each phase (its last argument).
const TLS_MS: u64 = 150;
/// The 48 bits of a word handed to a program as `t<12 hex>`.
const WORD: u64 = 0xffff_ffff_ffff;
/// `mem.c`'s constants: the golden-ratio step, its slots, what it asks for.
const K: u64 = 0x9e37_79b9_7f4a_7c15;
const SLOTS: usize = 48;
const TOTAL: u64 = 32 << 20;

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
/// the PM timer, mixed (ktest-22's).
fn unpublished() -> u64 {
    // SAFETY: rdtsc reads a counter; no memory is touched.
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
    splitmix64(tsc ^ (pm() as u64) << 40)
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

/// `f` with interrupts off: the learner's drive and cache hold their locks
/// across a read, and a preempted holder would leave a syscall spinning.
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
        9 => "EBADF",
        12 => "ENOMEM",
        13 => "EACCES",
        14 => "EFAULT",
        19 => "ENODEV",
        20 => "ENOTDIR",
        22 => "EINVAL",
        23 => "ENFILE",
        24 => "EMFILE",
        38 => "ENOSYS",
        _ => "?",
    };
    format!("-{e} ({name})")
}

fn hex16(v: u64) -> String {
    format!("{v:016x}")
}

// ── One row's recorder (ktap's shapes, printed in this file's order) ─────

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
        note(why);
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

// ── Processes ────────────────────────────────────────────────────────────

fn task_of(pid: Pid) -> Option<Tid> {
    let mut tid = None;
    proc::each(|p| {
        if p.pid == pid {
            tid = Some(p.task);
        }
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

fn status_text(s: Option<i32>) -> String {
    match s {
        None => String::from("was still running"),
        Some(v) if v & 0x7f == 0 => format!("ended exit({})", (v >> 8) & 0xff),
        Some(v) => format!("was killed by signal {} (status {v:#x})", v & 0x7f),
    }
}

/// `start`, then `fds` installed in the new process's table before it runs
/// (ktest-23's).
fn start_with(what: &str, start: impl FnOnce() -> Result<Pid, Errno>, fds: &[(usize, &FileRef)]) -> Result<Pid, String> {
    let ends: Vec<(usize, Fd)> = fds.iter().map(|&(n, f)| (n, Fd { file: f.clone(), cloexec: false })).collect();
    let started = quiet(|| -> Result<(Pid, Vec<Fd>), (String, Vec<Fd>)> {
        let pid = start().map_err(|e| (format!("starting {what} returned {}", errno_text(e)), Vec::new()))?;
        let Some(tid) = task_of(pid) else {
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

/// One program started by this file: its pid, its task, the read end of
/// its output pipe and what has come out of it.
struct Run {
    path: &'static str,
    pid: Pid,
    tid: Option<Tid>,
    r: FileRef,
    out: Vec<u8>,
    open: bool,
    err: Option<Errno>,
}

/// `path` with `argv`, its fds 1 and 2 a pipe this file reads, and its fd 0
/// a pipe holding `stdin` when there is one.
fn launch(path: &'static str, argv: &[&str], stdin: Option<&[u8]>) -> Result<Run, String> {
    let (r, w) = pipe::open(0).map_err(|e| format!("pipe::open(0) returned {}", errno_text(e)))?;
    r.flags.store(r.flags.load(Ordering::SeqCst) | O_NONBLOCK, Ordering::SeqCst);
    let input = match stdin {
        Some(bytes) => {
            let (ir, iw) = pipe::open(0).map_err(|e| format!("pipe::open(0) returned {}", errno_text(e)))?;
            match iw.write(bytes) {
                Ok(n) if n == bytes.len() => {}
                other => return Err(format!("writing {} bytes into a new pipe returned {other:?}", bytes.len())),
            }
            Some(ir)
        }
        None => None,
    };
    let mut fds: Vec<(usize, &FileRef)> = alloc::vec![(1, &w), (2, &w)];
    if let Some(ir) = &input {
        fds.push((0, ir));
    }
    let pid = start_with(path, || exec::spawn(path, argv, &[]), &fds);
    drop(fds);
    drop(w);
    drop(input);
    let pid = pid?;
    Ok(Run { path, pid, tid: task_of(pid), r, out: Vec::new(), open: true, err: None })
}

/// Read every run's pipe until each is at end of file or `RUN_MS` has
/// passed; then give them up to half a second to be recorded as ended.
/// Returns how long the reading took, by the PM timer.
fn drain(runs: &mut [Run]) -> u64 {
    let mut buf = alloc::vec![0u8; 4096];
    let mut c = Clock::new();
    loop {
        let mut moved = false;
        for run in runs.iter_mut().filter(|r| r.open) {
            match run.r.read(&mut buf) {
                Ok(0) => run.open = false,
                Ok(n) => {
                    run.out.extend_from_slice(&buf[..n]);
                    moved = true;
                }
                Err(11) => {}
                Err(e) => {
                    run.err = Some(e);
                    run.open = false;
                }
            }
        }
        if runs.iter().all(|r| !r.open) || c.ms() >= RUN_MS {
            break;
        }
        if !moved {
            sched::yield_now();
        }
    }
    let ms = c.ms();
    let _ = yield_until(500, || runs.iter().all(|r| exit_of(r.pid).is_some()));
    ms
}

/// How a run ended, in a sentence that stands alone.
fn ended(run: &Run) -> Option<String> {
    let exit = exit_of(run.pid);
    let printed = String::from_utf8_lossy(&run.out);
    if exit == Some(0) && !run.open && run.err.is_none() {
        return None;
    }
    let read = run.err.map(|e| format!(" (a read of its output returned {})", errno_text(e))).unwrap_or_default();
    let open = if run.open { format!(", its output still open after {} s", RUN_MS / 1000) } else { String::new() };
    Some(format!("{} {}{open}{read}, having printed {printed:?}", run.path, status_text(exit)))
}

// ── Mounting ─────────────────────────────────────────────────────────────

/// Partition 2 as ext2, mounted at `/`.
fn mount_root() -> Result<(), String> {
    let drive: Arc<dyn BlockDevice> = Arc::new(quiet(Ata::probe).map_err(|e| format!("Ata::probe() returned {}", errno_text(e)))?);
    let fs = quiet(|| ata::partition2(drive))
        .and_then(Ext2::mount)
        .map_err(|e| format!("partition2 then Ext2::mount returned {}", errno_text(e)))?;
    let fs = Arc::new(fs);
    let root: InodeRef = quiet(|| fs.root()).map_err(|e| format!("Ext2::root() returned {}", errno_text(e)))?;
    mount::mount("/dev/hda2", "/", "ext2", root).map_err(|e| format!("mount::mount(\"/dev/hda2\", \"/\", \"ext2\", its root) returned {}", errno_text(e)))
}

/// `VmaList::carve` arrives in lab 25 step 1. This twin lets the file build
/// against the cuts before it and the learner's template: the inherent
/// method wins once the cut has it, and before that `carve` takes nothing,
/// so step 1 fails as it should (2026-10-22).
trait CarveOf {
    #[allow(dead_code)]
    fn carve(&mut self, start: VirtAddr, end: VirtAddr) -> Vec<(VirtAddr, VirtAddr)>;
}
impl CarveOf for VmaList {
    fn carve(&mut self, _: VirtAddr, _: VirtAddr) -> Vec<(VirtAddr, VirtAddr)> {
        Vec::new()
    }
}

// ── Step 1: VmaList::carve on a list this file builds ────────────────────

const PAGE: u64 = 4096;

fn vma(start: u64, end: u64, prot: u8, kind: VmaKind) -> Vma {
    Vma { start: VirtAddr(start), end: VirtAddr(end), prot, kind }
}

fn list_text(l: &[Vma]) -> String {
    let mut s = String::from("[");
    for (i, v) in l.iter().enumerate() {
        let p = |b: u8, c: char| if v.prot & b != 0 { c } else { '-' };
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&format!(
            "{:#x}-{:#x} {}{}{} {:?}",
            v.start.0,
            v.end.0,
            p(prot::READ, 'r'),
            p(prot::WRITE, 'w'),
            p(prot::EXEC, 'x'),
            v.kind
        ));
    }
    s.push(']');
    s
}

fn ranges_text(r: &[(u64, u64)]) -> String {
    let parts: Vec<String> = r.iter().map(|(a, b)| format!("{a:#x}-{b:#x}")).collect();
    format!("[{}]", parts.join(", "))
}

/// Five cuts in a row on one list: a head, a tail past the list's end, a
/// hole in the middle, a range across two VMAs and a gap, and a range
/// where nothing is. Each returns the ranges taken and leaves the list
/// shown. Every failure, in one line each.
fn carve_check() -> Vec<String> {
    let rx = prot::READ | prot::EXEC;
    let rw = prot::READ | prot::WRITE;
    let m = 0x7eff_ffe8_0000u64;
    let p = PAGE;
    let mut l = VmaList::new();
    for v in [vma(0x100_0000, 0x100_3000, rx, VmaKind::Text), vma(0x100_3000, 0x100_5000, rw, VmaKind::Data), vma(m, m + 24 * p, rw, VmaKind::Data)] {
        if let Err(e) = l.insert(v) {
            return alloc::vec![format!("VmaList::insert of {} returned {}", list_text(&[v]), errno_text(e))];
        }
    }
    let text = vma(0x100_0000, 0x100_3000, rx, VmaKind::Text);
    let data = vma(0x100_3000, 0x100_5000, rw, VmaKind::Data);
    type Case = ((u64, u64), Vec<(u64, u64)>, Vec<Vma>);
    let cases: [Case; 5] = [
        ((m, m + 4 * p), alloc::vec![(m, m + 4 * p)], alloc::vec![text, data, vma(m + 4 * p, m + 24 * p, rw, VmaKind::Data)]),
        ((m + 20 * p, m + 28 * p), alloc::vec![(m + 20 * p, m + 24 * p)], alloc::vec![text, data, vma(m + 4 * p, m + 20 * p, rw, VmaKind::Data)]),
        (
            (m + 10 * p, m + 12 * p),
            alloc::vec![(m + 10 * p, m + 12 * p)],
            alloc::vec![text, data, vma(m + 4 * p, m + 10 * p, rw, VmaKind::Data), vma(m + 12 * p, m + 20 * p, rw, VmaKind::Data)],
        ),
        (
            (0x100_2000, 0x100_4000),
            alloc::vec![(0x100_2000, 0x100_3000), (0x100_3000, 0x100_4000)],
            alloc::vec![
                vma(0x100_0000, 0x100_2000, rx, VmaKind::Text),
                vma(0x100_4000, 0x100_5000, rw, VmaKind::Data),
                vma(m + 4 * p, m + 10 * p, rw, VmaKind::Data),
                vma(m + 12 * p, m + 20 * p, rw, VmaKind::Data),
            ],
        ),
        (
            (m, m + 4 * p),
            Vec::new(),
            alloc::vec![
                vma(0x100_0000, 0x100_2000, rx, VmaKind::Text),
                vma(0x100_4000, 0x100_5000, rw, VmaKind::Data),
                vma(m + 4 * p, m + 10 * p, rw, VmaKind::Data),
                vma(m + 12 * p, m + 20 * p, rw, VmaKind::Data),
            ],
        ),
    ];
    let mut fails = Vec::new();
    for ((a, b), want_taken, want_list) in cases {
        let before: Vec<Vma> = l.iter().copied().collect();
        let taken: Vec<(u64, u64)> = l.carve(VirtAddr(a), VirtAddr(b)).iter().map(|(x, y)| (x.0, y.0)).collect();
        let after: Vec<Vma> = l.iter().copied().collect();
        if taken != want_taken || after != want_list {
            fails.push(format!(
                "VmaList::carve({a:#x}, {b:#x}) on {} returned {} and left {}; expected {} and {}",
                list_text(&before),
                ranges_text(&taken),
                list_text(&after),
                ranges_text(&want_taken),
                list_text(&want_list)
            ));
            // Go on from the list a right cut leaves.
            l = VmaList::new();
            for v in want_list {
                let _ = l.insert(v);
            }
        }
    }
    fails
}

// ── Row 1: hello ─────────────────────────────────────────────────────────

fn hello_row(n: usize) -> Option<Run> {
    let mut row = Row::new(HELLO_ID);
    let nonce = row.nonce();
    let a = ktap::derive(&nonce, HELLO_ID);
    let w = unpublished() & WORD;
    let input = format!("t{w:012x}\n");
    let arg = hex16(a);
    let t0 = time::now_ns();
    let run = match launch("/bin/hello", &["hello", &arg], Some(input.as_bytes())) {
        Ok(run) => run,
        Err(why) => {
            row.fail(format_args!("{HELLO_ID}: /bin/hello did not start: {why}"));
            row.finish(n);
            return None;
        }
    };
    let mut runs = [run];
    let ms = drain(&mut runs);
    let t1 = time::now_ns();
    let [run] = runs;
    let nap = 20 + a % 40;
    if let Some(why) = ended(&run) {
        row.fail(format_args!("{HELLO_ID}: {why}"));
    } else {
        let line1 = format!("hello, musl: {:016x} t{w:012x}\n", splitmix64(a ^ w));
        let pre2 = format!("ids 0 0 0 0 pid {} cpus 1 slept {nap} ms realtime ", run.pid.0);
        let post2 = " sig ok futex ok\n";
        let out = String::from_utf8_lossy(&run.out);
        let lines: Vec<&str> = out.split_inclusive('\n').collect();
        if lines.len() != 2 || lines[0] != line1 {
            row.fail(format_args!("{HELLO_ID}: /bin/hello printed {out:?}; expected two lines, the first {line1:?}"));
        } else {
            let l2 = lines[1];
            let secs = l2.strip_prefix(pre2.as_str()).and_then(|r| r.strip_suffix(post2)).and_then(|s| s.parse::<u64>().ok());
            let lo = ext2::EPOCH as u64 + t0 / 1_000_000_000;
            let hi = ext2::EPOCH as u64 + t1 / 1_000_000_000;
            match secs {
                None => row.fail(format_args!("{HELLO_ID}: /bin/hello's second line is {l2:?}; expected {pre2:?}, the seconds of CLOCK_REALTIME, then {post2:?}")),
                Some(s) if s < lo || s > hi => row.fail(format_args!(
                    "{HELLO_ID}: CLOCK_REALTIME read {s} s; it is fs::ext2::EPOCH ({}) plus the monotonic clock, which was {lo}..={hi} while hello ran",
                    ext2::EPOCH
                )),
                Some(_) => {}
            }
        }
        if ms < nap {
            row.fail(format_args!(
                "{HELLO_ID}: hello slept {nap} ms in nanosleep, yet ran for {ms} ms in all by a clock outside the kernel: the sleep, or CLOCK_MONOTONIC, is short"
            ));
        }
    }
    note(format_args!("{HELLO_ID}: argv[1] {arg}, stdin t{w:012x}, pid {}, {ms} ms by the PM timer, nanosleep {nap} ms", run.pid.0));
    row.finish(n);
    Some(run)
}

// ── Row 2: tls, two copies at once ───────────────────────────────────────

/// `gaps A B turns C D\n` after the prefix.
fn tls_counts(rest: &str) -> Option<[u64; 4]> {
    let rest = rest.strip_suffix('\n')?;
    let f: Vec<&str> = rest.split(' ').collect();
    if f.len() != 5 || f[2] != "turns" {
        return None;
    }
    Some([f[0].parse().ok()?, f[1].parse().ok()?, f[3].parse().ok()?, f[4].parse().ok()?])
}

fn tls_row(n: usize) -> Vec<Run> {
    let mut row = Row::new(TLS_ID);
    let nonce = row.nonce();
    let a = ktap::derive(&nonce, TLS_ID);
    let w0 = unpublished() & WORD;
    let mut w1 = unpublished() & WORD;
    if w1 == w0 {
        w1 ^= 1;
    }
    let words = [w0, w1];
    let arg = hex16(a);
    let ms = format!("{TLS_MS}");
    let mut runs = Vec::new();
    let sw0 = sched::switches();
    for (i, w) in words.iter().enumerate() {
        let t = format!("t{w:012x}");
        let idx = if i == 0 { "0" } else { "1" };
        match launch("/bin/tls", &["tls", &arg, &t, idx, &ms], None) {
            Ok(r) => runs.push(r),
            Err(why) => row.fail(format_args!("{TLS_ID}: copy {i} of /bin/tls did not start: {why}")),
        }
    }
    let took = drain(&mut runs);
    let sw = sched::switches() - sw0;
    let mut seen = Vec::new();
    for (i, run) in runs.iter().enumerate() {
        let w = words[i];
        let s = a ^ w;
        let want = format!("tls {:016x} t{w:012x} gaps ", splitmix64(s ^ splitmix64(s ^ 0x7115)));
        if let Some(why) = ended(run) {
            row.fail(format_args!("{TLS_ID}: copy {i}: {why}"));
            continue;
        }
        let out = String::from_utf8_lossy(&run.out);
        match out.strip_prefix(want.as_str()).and_then(tls_counts) {
            None => row.fail(format_args!("{TLS_ID}: copy {i} of /bin/tls printed {out:?}; expected {want:?} and its four counts")),
            Some([ga, gb, ta, tb]) => {
                if ga == 0 || gb == 0 {
                    row.fail(format_args!(
                        "{TLS_ID}: copy {i} of /bin/tls spent no stretch off the CPU in {} (gaps {ga} {gb}) while another copy spun beside it: a preemptive scheduler takes the CPU from it, and the row needs it taken",
                        if ga == 0 { "its first phase (musl's thread block)" } else { "its second phase (its own FS base)" }
                    ));
                }
                seen.push(format!("copy {i} gaps {ga} {gb} turns {ta} {tb}, {} slices", run.tid.map_or(0, sched::slices)));
            }
        }
    }
    note(format_args!("{TLS_ID}: argv[1] {arg}, words t{w0:012x} t{w1:012x}; {}; {sw} context switches in {took} ms", seen.join("; ")));
    row.finish(n);
    runs
}

// ── Row 3: mem ───────────────────────────────────────────────────────────

/// What `mem.c` prints for `a ^ w`: its checksum and its three counts.
/// Every word it checks holds what it wrote, so the sums are computed from
/// the keys alone.
fn mem_want(seed: u64) -> (u64, u64, u64, u64) {
    let mut state = seed;
    let mut sum = seed;
    let mut draw = || {
        state = state.wrapping_add(K);
        splitmix64(state)
    };
    // (words, key) of a full slot.
    let mut slots: [Option<(u64, u64)>; SLOTS] = [None; SLOTS];
    let fold = |sum: &mut u64, words: u64, key: u64| {
        let mut j = 0;
        while j < words {
            *sum = splitmix64(*sum ^ key ^ j.wrapping_mul(K));
            j += 64;
        }
        *sum = splitmix64(*sum ^ (key ^ (words - 1).wrapping_mul(K)) ^ words);
    };
    let size_of = |r: u64| -> u64 {
        let c = ((r >> 16) % 16) as u32;
        let x = (r >> 24) as u32;
        (if c < 10 {
            1 + x % 512
        } else if c < 14 {
            512 + x % 16384
        } else {
            16384 + x % 131072
        }) as u64
    };
    let (mut allocs, mut reallocs, mut frees, mut asked) = (0u64, 0u64, 0u64, 0u64);
    while asked < TOTAL {
        let r = draw();
        let i = (r % SLOTS as u64) as usize;
        match slots[i] {
            None => {
                let words = size_of(r);
                let key = draw();
                slots[i] = Some((words, key));
                asked += words * 8;
                allocs += 1;
            }
            Some((words, key)) => {
                let op = ((r >> 8) as u32) % 4;
                if op == 0 && words < 65536 {
                    let n = words * 2 + (r >> 40) % 64;
                    asked += (n - words) * 8;
                    slots[i] = Some((n, key));
                    reallocs += 1;
                } else if op == 1 && words >= 2 {
                    slots[i] = Some((words / 2, key));
                    reallocs += 1;
                } else {
                    fold(&mut sum, words, key);
                    slots[i] = None;
                    frees += 1;
                }
            }
        }
    }
    for s in slots.iter_mut() {
        if let Some((words, key)) = s.take() {
            fold(&mut sum, words, key);
            frees += 1;
        }
    }
    // deep(): every 512th word of a 32768-word buffer.
    let mut t = 0u64;
    let mut i = 0u64;
    while i < 32768 {
        t = splitmix64(t ^ sum ^ i.wrapping_mul(K));
        i += 512;
    }
    sum = splitmix64(sum ^ t);
    // The 16 pages the mremap kept: pages 12..20 of the hinted mapping (k + 13
    // after the move), then the pages it wrote.
    let mut t = 0u64;
    for k in 0..16u64 {
        t = splitmix64(t ^ if k < 8 { k + 13 } else { k });
    }
    (splitmix64(sum ^ t), allocs, reallocs, frees)
}

/// How many more frames may be in use after `/bin/mem` has ended than
/// before it (the kernel heap may grow by a frame or two); a lost `munmap`
/// leaks thousands.
const FRAME_SLACK: usize = 16;

// ── Row 3's probe: what `munmap` and a shrinking `mremap` leave behind ────

/// The pages in each of the probe's two mappings.
const PROBE_PAGES: u64 = 8;
/// `mmap`'s flags for both: `MAP_PRIVATE | MAP_ANONYMOUS` (Linux's values).
const MAP_PRIVATE_ANON: u32 = 0x22;
/// `mremap`'s flag: the mapping may move.
const MREMAP_MAYMOVE: u32 = 1;
/// A leaf entry's frame bits.
const ADDR_BITS: u64 = 0x000f_ffff_ffff_f000;

/// The probe's code, at `proc::BASE + 120` (offsets in the comments):
/// `ud2` (this file's `on_ud` maps the two mappings, and hands their
/// addresses back in rdi and rsi and the page count in rcx), a loop that
/// writes one word to every page of each, `ud2` again (the checks), then
/// `exit(0)`. Assembled with clang 2026-10-22 and checked with llvm-objdump.
const PROBE_CODE: [u8; 40] = [
    0x0f, 0x0b, //                               0: ud2
    0x48, 0x89, 0x3f, //                         2: mov [rdi], rdi
    0x48, 0x89, 0x36, //                         5: mov [rsi], rsi
    0x48, 0x81, 0xc7, 0x00, 0x10, 0x00, 0x00, // 8: add rdi, 0x1000
    0x48, 0x81, 0xc6, 0x00, 0x10, 0x00, 0x00, // 15: add rsi, 0x1000
    0x48, 0xff, 0xc9, //                         22: dec rcx
    0x75, 0xe7, //                               25: jnz 2
    0x0f, 0x0b, //                               27: ud2
    0x31, 0xff, //                               29: xor edi, edi
    0xb8, 60, 0, 0, 0, //                        31: mov eax, 60 (exit)
    0x0f, 0x05, //                               36: syscall
    0x0f, 0x0b, //                               38: ud2
];
/// Where the code goes on after its second `ud2`, or after a failed first.
const PROBE_EXIT: u64 = proc::BASE + 120 + 29;

/// A static ELF of one PT_LOAD at `proc::BASE` holding `PROBE_CODE`
/// (ktest-14's shape).
fn probe_image() -> Vec<u8> {
    let len = 120 + PROBE_CODE.len();
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
    b[120..].copy_from_slice(&PROBE_CODE);
    b
}

/// What the probe's process has done so far, for `on_ud`.
struct ProbeState {
    /// How many of its `ud2`s have been taken.
    step: u32,
    /// The two mappings `mmap` returned.
    a: u64,
    b: u64,
    /// Every failure, in one line each.
    fails: Vec<String>,
    /// What each call gave back, for the row's note.
    seen: Vec<String>,
}

static PROBE_PID: AtomicU64 = AtomicU64::new(0);
static PROBE: SpinLock<ProbeState> = SpinLock::new(ProbeState { step: 0, a: 0, b: 0, fails: Vec::new(), seen: Vec::new() });

/// The running space's leaf entry for `va` (0 when no table on the way
/// exists): the learner's `AddressSpace::entry_mut`, which creates nothing.
fn leaf(va: u64) -> u64 {
    task::with_current(|t| t.aspace.as_mut().and_then(|a| a.entry_mut(VirtAddr(va)).map(|e| e.0))).unwrap_or(0)
}

fn bits(e: u64) -> String {
    let mut s = String::new();
    for (b, n) in [(flags::PRESENT, "present"), (flags::WRITE, "writable"), (flags::USER, "user"), (flags::NX, "no-execute")] {
        if e & b != 0 {
            if !s.is_empty() {
                s.push_str(", ");
            }
            s.push_str(n);
        }
    }
    format!("{e:#018x} ({s}; frame {:#x})", e & ADDR_BITS)
}

/// The #UD handler while the probe runs: on the probe's task, in ring 0,
/// interrupts off — where a system call runs. Any other #UD is what lab 12's
/// kill makes of it.
fn on_ud(tf: &mut TrapFrame) {
    let pid = proc::current();
    let probe = PROBE_PID.load(Ordering::SeqCst);
    if tf.cs & 3 == 3 && probe != 0 && pid.is_some_and(|p| p.0 == probe) {
        let step = {
            let mut s = PROBE.lock();
            s.step += 1;
            s.step
        };
        if step == 1 {
            match probe_map() {
                Ok((a, b)) => {
                    let mut s = PROBE.lock();
                    s.a = a;
                    s.b = b;
                    drop(s);
                    tf.regs[9] = a; // rdi
                    tf.regs[10] = b; // rsi
                    tf.regs[12] = PROBE_PAGES; // rcx
                    tf.rip += 2;
                }
                Err(why) => {
                    PROBE.lock().fails.push(why);
                    tf.rip = PROBE_EXIT;
                }
            }
        } else {
            let (a, b) = {
                let s = PROBE.lock();
                (s.a, s.b)
            };
            let (fails, seen) = probe_check(a, b);
            let mut s = PROBE.lock();
            s.fails.extend(fails);
            s.seen.extend(seen);
            tf.rip = PROBE_EXIT;
        }
        return;
    }
    if tf.cs & 3 == 3 {
        if let Some(pid) = pid {
            proc::kill(pid, Kill { reason: KillReason::Ud, rip: tf.rip, addr: 0 });
        }
    }
    panic!("#UD (vector 6) rip={:#x} cs={:#x}", tf.rip, tf.cs);
}

/// The probe's two mappings, each `PROBE_PAGES` pages, readable and writable.
fn probe_map() -> Result<(u64, u64), String> {
    let len = (PROBE_PAGES * PAGE) as usize;
    let rw = prot::READ | prot::WRITE;
    let one = |p: &mut proc::Process| {
        mm::mmap(p, VirtAddr(0), len, rw, MAP_PRIVATE_ANON, None, 0)
            .map(|v| v.0)
            .map_err(|e| format!("mmap(0, {len:#x}, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS) returned {}", errno_text(e)))
    };
    proc::with_current(|p| Ok((one(p)?, one(p)?))).unwrap_or_else(|| Err(String::from("the probe's process was not the running one")))
}

/// After `call` gave back `gone` (page addresses) that held `held` frames:
/// each such page has no present entry, and each frame is back in the pool
/// (`frame::refs` 0, or the frame of a page `kept` now maps: a copy reused
/// it; or the in-use count fell by at least as many: something else did).
fn gone_check(fails: &mut Vec<String>, call: &str, gone: &[u64], held: &[u64], kept: &[u64], used: (usize, usize)) -> usize {
    let stale: Vec<(u64, u64)> = gone.iter().map(|&va| (va, leaf(va))).filter(|(_, e)| e & flags::PRESENT != 0).collect();
    if let Some(&(va, e)) = stale.first() {
        fails.push(format!(
            "{call}, and {} of the {} pages it gave back still {} a present entry; the first, at {va:#x}, is {}",
            stale.len(),
            gone.len(),
            if stale.len() == 1 { "has" } else { "have" },
            bits(e)
        ));
    }
    let now: Vec<u64> = kept.iter().map(|&va| leaf(va)).filter(|e| e & flags::PRESENT != 0).map(|e| e & ADDR_BITS).collect();
    let back = held.iter().filter(|&&f| frame::refs(Frame(PhysAddr(f))) == 0 || now.contains(&f)).count();
    let fell = used.0.saturating_sub(used.1);
    if back < held.len() && fell < held.len() {
        fails.push(format!(
            "{call}; the pages it gave back held {} frames, {back} of them are back in the pool, and frames in use went from {} to {}",
            held.len(),
            used.0,
            used.1
        ));
    }
    back
}

/// The checks, after the probe wrote to every page of both mappings.
fn probe_check(a: u64, b: u64) -> (Vec<String>, Vec<String>) {
    let mut fails = Vec::new();
    let mut seen = Vec::new();
    let pages = |base: u64| (0..PROBE_PAGES).map(move |k| base + k * PAGE);
    let mut frames = Vec::new();
    for va in pages(a).chain(pages(b)) {
        let e = leaf(va);
        if e & flags::PRESENT == 0 {
            fails.push(format!("the page at {va:#x}, which mmap returned and the process then wrote to, has no present entry: {}", bits(e)));
            return (fails, seen);
        }
        frames.push(e & ADDR_BITS);
    }
    // munmap: three pages from the middle of the first mapping.
    let (lo, n) = (a + 2 * PAGE, 3u64);
    let u0 = frame::stats().used;
    let r = proc::with_current(|p| mm::munmap(p, VirtAddr(lo), (n * PAGE) as usize));
    let u1 = frame::stats().used;
    let call = format!("munmap({lo:#x}, {:#x}) on a mapping of {PROBE_PAGES} pages at {a:#x}, each written to", n * PAGE);
    match r {
        Some(Ok(())) => {
            let gone: Vec<u64> = (0..n).map(|k| lo + k * PAGE).collect();
            let kept: Vec<u64> = pages(a).filter(|va| !gone.contains(va)).collect();
            let back = gone_check(&mut fails, &format!("{call} returned 0"), &gone, &frames[2..5], &kept, (u0, u1));
            seen.push(format!("munmap({lo:#x}, {:#x}): {back} of 3 frames back, in use {u0} -> {u1}", n * PAGE));
        }
        Some(Err(e)) => fails.push(format!("{call} returned {}", errno_text(e))),
        None => fails.push(String::from("the probe's process was not the running one")),
    }
    // munmap across two mappings: the first mapping is now [a, a + 2 pages)
    // and [a + 5 pages, a + 8 pages); [a + 1 page, a + 6 pages) takes the
    // tail page of one and the head page of the other.
    let (lo, n) = (a + PAGE, 5u64);
    let u0 = frame::stats().used;
    let r = proc::with_current(|p| mm::munmap(p, VirtAddr(lo), (n * PAGE) as usize));
    let u1 = frame::stats().used;
    let call = format!("munmap({lo:#x}, {:#x}) across two mappings, the last page of one and the first of the other", n * PAGE);
    match r {
        Some(Ok(())) => {
            let gone = [a + PAGE, a + 5 * PAGE];
            let kept = [a, a + 6 * PAGE, a + 7 * PAGE];
            let back = gone_check(&mut fails, &format!("{call} returned 0"), &gone, &[frames[1], frames[5]], &kept, (u0, u1));
            seen.push(format!("munmap({lo:#x}, {:#x}): {back} of 2 frames back, in use {u0} -> {u1}", n * PAGE));
        }
        Some(Err(e)) => fails.push(format!("{call} returned {}", errno_text(e))),
        None => fails.push(String::from("the probe's process was not the running one")),
    }
    // mremap: the second mapping shrunk in place to five pages (no flag:
    // it may not move), then to three with MREMAP_MAYMOVE (it may).
    if let Some(b5) = shrink(&mut fails, &mut seen, b, PROBE_PAGES, 5, 0) {
        let _ = shrink(&mut fails, &mut seen, b5, 5, 3, MREMAP_MAYMOVE);
    }
    (fails, seen)
}

/// `mremap(base, from pages, to pages, fl)` on a mapping whose every page
/// was written to: the pages outside where it now lies have no present
/// entry, and the frames of the cut-off tail are back. Where it now lies.
fn shrink(fails: &mut Vec<String>, seen: &mut Vec<String>, base: u64, from: u64, to: u64, fl: u32) -> Option<u64> {
    let held: Vec<u64> = (to..from).map(|k| leaf(base + k * PAGE) & ADDR_BITS).collect();
    let flag = if fl & MREMAP_MAYMOVE != 0 { "MREMAP_MAYMOVE" } else { "0" };
    let u0 = frame::stats().used;
    let r = proc::with_current(|p| mm::mremap(p, VirtAddr(base), (from * PAGE) as usize, (to * PAGE) as usize, fl));
    let u1 = frame::stats().used;
    let call = format!("mremap({base:#x}, {:#x}, {:#x}, {flag}) on a mapping of {from} pages, each written to", from * PAGE, to * PAGE);
    match r {
        Some(Ok(at)) => {
            let at = at.0;
            let gone: Vec<u64> = (0..from).map(|k| base + k * PAGE).filter(|&va| va < at || va >= at + to * PAGE).collect();
            let kept: Vec<u64> = (0..to).map(|k| at + k * PAGE).collect();
            let back = gone_check(fails, &format!("{call} returned {at:#x}"), &gone, &held, &kept, (u0, u1));
            seen.push(format!("mremap({base:#x}, {:#x}, {:#x}, {flag}) -> {at:#x}: {back} of {} frames back, in use {u0} -> {u1}", from * PAGE, to * PAGE, held.len()));
            Some(at)
        }
        Some(Err(e)) => {
            fails.push(format!("{call} returned {}", errno_text(e)));
            None
        }
        None => {
            fails.push(String::from("the probe's process was not the running one"));
            None
        }
    }
}

/// Run the probe's process to its end; every failure, in one line each.
fn probe_unmap() -> (Vec<String>, Vec<String>) {
    *PROBE.lock() = ProbeState { step: 0, a: 0, b: 0, fails: Vec::new(), seen: Vec::new() };
    idt::set_handler(6, on_ud, 0);
    let image = probe_image();
    let pid = match quiet(|| {
        let r = proc::spawn_user(&image, &["unmap-probe"]);
        if let Ok(pid) = r {
            PROBE_PID.store(pid.0, Ordering::SeqCst);
        }
        r
    }) {
        Ok(pid) => pid,
        Err(e) => return (alloc::vec![format!("starting a process of one page of code returned {}", errno_text(e))], Vec::new()),
    };
    let _ = yield_until(5000, || exit_of(pid).is_some());
    let st = exit_of(pid);
    PROBE_PID.store(0, Ordering::SeqCst);
    let s = core::mem::replace(&mut *PROBE.lock(), ProbeState { step: 0, a: 0, b: 0, fails: Vec::new(), seen: Vec::new() });
    let mut fails = s.fails;
    if s.step < 2 && fails.is_empty() {
        let at = if s.step == 0 { "before it asked for its mappings" } else { "while it wrote to the pages mmap returned" };
        fails.push(format!("a process that maps two mappings and writes to each page {} {at}", status_text(st)));
    }
    (fails, s.seen)
}

fn mem_row(n: usize, carve: &[String]) -> Option<Run> {
    let mut row = Row::new(MEM_ID);
    for why in carve {
        row.fail(format_args!("{MEM_ID}: {why}"));
    }
    let nonce = row.nonce();
    let a = ktap::derive(&nonce, MEM_ID);
    let w = unpublished() & WORD;
    let arg = hex16(a);
    let t = format!("t{w:012x}");
    let f0 = frame::stats().used;
    let en0 = syscall::enosys_count();
    let run = match launch("/bin/mem", &["mem", &arg, &t], None) {
        Ok(run) => run,
        Err(why) => {
            row.fail(format_args!("{MEM_ID}: /bin/mem did not start: {why}"));
            row.finish(n);
            return None;
        }
    };
    let mut runs = [run];
    let ms = drain(&mut runs);
    let [run] = runs;
    let (sum, allocs, reallocs, frees) = mem_want(a ^ w);
    let want = format!("mem {sum:016x} t{w:012x} allocs {allocs} reallocs {reallocs} frees {frees}\n");
    if let Some(why) = ended(&run) {
        row.fail(format_args!("{MEM_ID}: {why}"));
    } else if run.out != want.as_bytes() {
        row.fail(format_args!("{MEM_ID}: /bin/mem printed {:?}; expected {want:?}", String::from_utf8_lossy(&run.out)));
    }
    let f1 = frame::stats().used;
    let en1 = syscall::enosys_count();
    if f1 > f0 + FRAME_SLACK {
        row.fail(format_args!(
            "{MEM_ID}: frames in use rose from {f0} to {f1} across /bin/mem, which has ended: the frames of memory it unmapped, or moved with mremap, did not all go back to the pool"
        ));
    }
    note(format_args!("{MEM_ID}: argv {arg} t{w:012x}, {ms} ms by the PM timer; frames in use {f0} before, {f1} after"));
    // Once `mem` has reached its exit with every call answered, `mmap`,
    // `munmap` and `mremap` are each past their stubs: the probe may call them.
    if matches!(exit_of(run.pid), Some(v) if v & 0x7f == 0) && en1 == en0 {
        let (fails, seen) = probe_unmap();
        for why in fails {
            row.fail(format_args!("{MEM_ID}: {why}"));
        }
        if !seen.is_empty() {
            note(format_args!("{MEM_ID}: a process the tests start, after mem: {}", seen.join("; ")));
        }
    }
    row.finish(n);
    Some(run)
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

fn not_run(why: &str) {
    for (n, id) in [HELLO_ID, TLS_ID, MEM_ID, ENOSYS_ID].into_iter().enumerate() {
        let mut r = Row::new(id);
        r.fail(format_args!("{id}: not run: {why}"));
        r.finish(n + 1);
    }
}

/// How far `enosys_count()` moved while each program ran.
fn enosys_row(n: usize, moved: &[(&str, u64)], before: u64, ends: &[(&str, Option<i32>)]) {
    let mut row = Row::new(ENOSYS_ID);
    let total: u64 = moved.iter().map(|m| m.1).sum();
    let each: Vec<String> = moved.iter().map(|(p, d)| format!("{p} {d}")).collect();
    if total != 0 {
        let who: Vec<&str> = moved.iter().filter(|m| m.1 != 0).map(|m| m.0).collect();
        row.fail(format_args!(
            "{ENOSYS_ID}: {total} system calls named a number the dispatch has no arm for ({}): {} called them; each is in the lab's table of calls a static musl program makes",
            each.join(", "),
            who.join(", ")
        ));
    }
    for (p, st) in ends {
        if !matches!(st, Some(v) if v & 0x7f == 0) {
            row.fail(format_args!("{ENOSYS_ID}: {p} {} instead of reaching its exit, so not all its calls were made", status_text(*st)));
        }
    }
    note(format_args!("{ENOSYS_ID}: enosys_count() {before} before the programs, {} after ({})", before + total, each.join(", ")));
    row.finish(n);
}

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    serial::init();
    ktap::begin(PLAN);
    prelude();
    // Step 1, before anything runs: the VMA cut.
    let carve = carve_check();
    match mount_root() {
        Err(why) => not_run(&format!("the root file system did not mount: {why}")),
        Ok(()) => {
            let e0 = syscall::enosys_count();
            let hello = hello_row(1);
            let e1 = syscall::enosys_count();
            let tls = tls_row(2);
            let e2 = syscall::enosys_count();
            let mem = mem_row(3, &carve);
            let e3 = syscall::enosys_count();
            let moved = [("hello", e1 - e0), ("tls", e2 - e1), ("mem", e3 - e2)];
            let mut ends: Vec<(&str, Option<i32>)> = Vec::new();
            ends.push(("/bin/hello", hello.as_ref().and_then(|r| exit_of(r.pid))));
            for (i, name) in ["/bin/tls (copy 0)", "/bin/tls (copy 1)"].into_iter().enumerate() {
                ends.push((name, tls.get(i).and_then(|r| exit_of(r.pid))));
            }
            ends.push(("/bin/mem", mem.as_ref().and_then(|r| exit_of(r.pid))));
            enosys_row(4, &moved, e0, &ends);
        }
    }
    sched::yield_now();
    quiet(ktap::end);
    halt()
}
