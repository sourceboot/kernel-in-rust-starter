//! Lab 23's vehicle (`23-the-shell`): the console's line discipline, the
//! shift map, the working directory — and the course's shell, `/bin/sh`,
//! run on them with the grader typing into it.
//!
//! The disk is `ktest-23.img` as the course's image writer made it (CONTRACT
//! §A.7); the guest writes it, so the course boots a copy. The vehicle
//! mounts what lab 22's does (ext2 at `/` from `/dev/hda2` through the
//! learner's `Ata`, `partition2` and `Ext2::mount`, the boot archive at
//! `/boot`, a `ProcFs` at `/proc`, a `DevFs` at `/dev`), first checks
//! `kbd::translate` against a fixed run of scancodes (step 1: shift, its
//! release, control), then starts `/bin/sh` off ext2 through the learner's
//! exec with the environment `SBOOT_NONCE=<the run's nonce>`. Its fd 0 is
//! the console (`proc::start`'s three standard fds); its fds 1 and 2 are a
//! pipe this file drains — the TEE. Everything the shell and its children
//! write comes through the tee: this file keeps it (the rows read it) and
//! forwards it to the serial line with `# ` at the start of every line, so
//! a program's own KTAP lines (`hi`'s `ok 1 - ring3.cs.rpl`) are comments
//! here. The line the grader types is echoed by the learner's console, not
//! by this file: on the serial line it follows the forwarded prompt.
//!
//! The first prompt is held until `# sboot keys-ready` has been printed,
//! which is the rubric script's first `wait`; the script then types one
//! command group at a time and waits for that group's last line, the
//! shell's own `echo $? "q"N` (the quotes keep `qN` out of the echo). The
//! shell ends on `exit`; the rows are judged on what came through the tee,
//! split at each prompt (`$ `) into one segment per command typed.
//!
//! The rubric's first row, `sh.prompt.echo.paced`, is a serial row: the
//! substring `$ echo "q"1` — the forwarded prompt followed by the console's
//! echo of the typed line, shifted quote included. The six rows here:
//!
//!   1 `sh.ls.proc` (plain) — `cd /procx`, a backspace, Enter: no output;
//!     `ls` prints exactly `/proc`'s names: `meminfo`, `mounts`, `sboot`,
//!     `uptime`, the shell's pid S and `ls`'s own pid S+2 (the reaped `echo`
//!     S+1 is gone); `pwd`, a forked child, prints `/proc`; `$?` is 0.
//!   2 `sh.redirect.devnull` (derived) — `cat /proc/sboot/nonce > /dev/null`
//!     and `cat < /dev/null` print nothing (the second reads end of file at
//!     once); in `/tmp`, `cat /proc/sboot/nonce > k23n` prints nothing and
//!     `cat < k23n` prints the nonce: `>` and `<` reached the files they
//!     name, relative to the cwd; `$?` is 0.
//!   3 `sh.pipe.two` (derived) — `producer $SBOOT_NONCE 1 chain | consumer
//!     $SBOOT_NONCE 1 chain 3 4`: the consumer's three `ok` lines, and the
//!     byte count and FNV-1a 64 it read (and the sum it wanted) equal to the
//!     stream this file computes from the capture nonce (`ktap::nonce()`,
//!     user/producer/stream.h's formula) — not from the environment the
//!     programs were given; `$?` is 0.
//!   4 `sh.hi.by_name` (derived) — `hi $SBOOT_NONCE 1`, a bare name the
//!     shell runs as `/bin/hi` from `/tmp`: `cs=0x2b rpl=3`, the nonce, its
//!     `# derive=` line equal to `ktap::derive(nonce, "ring3.cs.rpl")`, and
//!     its `ok 1`; `$?` is 0.
//!   5 `sh.exit_code.shown` (plain) — `cat` with `ab`, Enter and ctrl+d
//!     typed into it prints `ab` and `$?` is 0; `cat /nonexistent` prints
//!     its ENOENT line and `$?` is 1; `nosuch` gives the shell's ENOENT line
//!     and `$?` is 127.
//!   6 `sh.remount.survives` (derived) — `echo $SBOOT_NONCE > k23` in
//!     `/tmp`, then `exit`: the shell ends with status 0; this file calls
//!     `Ext2::sync`, mounts partition 2 a second time (a new drive, a new
//!     cache) and reads `/tmp/k23` through that mount alone: the nonce and a
//!     newline.
//!
//! When `kbd::translate` is wrong, or the four mounts fail, no shell runs and
//! every row says why. The session ends when the shell closes its output, when
//! it has printed nothing for `STALL_MS`, or after `SH_MS`, all by the PM
//! timer; a script that stalls leaves the rows to say which marker never came.
//!
//! A run that ends without all eight markers can never finish the typed
//! script — each marker is one of its `wait`s, and the engine counts `# sboot
//! end` only once the script has run out — so it would wait out the rubric's
//! whole timeout. Instead, after the six rows, this file panics through the
//! course's panic path (lab 03's `report`): the engine also ends a capture on
//! a complete panic report. A run with every marker ends on `# sboot end`.
//! The bound is never met by an honest shell (CONTRACT §B.2-23, measured).

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

use kernel::addr::VirtAddr;
use kernel::blk::ata::{self, Ata, BlockDevice};
use kernel::console::{Fd, FileRef};
use kernel::exec;
use kernel::fs::archive::Archive;
use kernel::fs::devfs::DevFs;
use kernel::fs::ext2::Ext2;
use kernel::fs::mount;
use kernel::fs::procfs::ProcFs;
use kernel::fs::vfs::InodeRef;
use kernel::ktap;
use kernel::pipe;
use kernel::proc::{self, Pid};
use kernel::syscall::Errno;
use kernel::task::{self, Tid};
use kernel::{bootinfo, frame, gdt, heap, idt, irq, kbd, sched, serial, syscall, time};

const ARCHIVE_LEN: usize = include_bytes!("../../../user/bin/boot.archive").len();
#[link_section = ".sboot_archive"]
static ARCHIVE: [u8; ARCHIVE_LEN] = *include_bytes!("../../../user/bin/boot.archive");

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 64;
const PM_HZ: u64 = 3_579_545;

const PLAN: usize = 6;
const LS_ID: &str = "sh.ls.proc";
const REDIRECT_ID: &str = "sh.redirect.devnull";
const PIPE_ID: &str = "sh.pipe.two";
const HI_ID: &str = "sh.hi.by_name";
const STATUS_ID: &str = "sh.exit_code.shown";
const REMOUNT_ID: &str = "sh.remount.survives";

/// What the vehicle mounts, as lab 22's does.
const MOUNTS: [(&str, &str, &str); 4] =
    [("/dev/hda2", "/", "ext2"), ("boot.archive", "/boot", "archive"), ("proc", "/proc", "proc"), ("devfs", "/dev", "devfs")];

/// How long the shell may run, by the PM timer: the backstop for a shell that
/// keeps printing and never reaches the script's end. The honest shell runs
/// about 45 s (CONTRACT §B.2-23); a run cut here ends with its rows at
/// about 101 s, inside the rubric's timeout.
const SH_MS: u64 = 100_000;

/// How long the shell may print nothing before the session ends. Every
/// `wait` of the script after the first is the shell's own output, and the
/// grader types one command group only after the last one's marker, so a
/// silent shell has stopped the script for good. The longest pause an honest
/// run has is the time the longest typed line takes to arrive (CONTRACT
/// §B.2-23, measured 2026-10-19); this is several times that.
const STALL_MS: u64 = 30_000;

/// `/proc`'s fixed names (procfs.rs).
const PROC_TOP: [&str; 4] = ["meminfo", "mounts", "sboot", "uptime"];

const O_NONBLOCK: u32 = 0x800;

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
        16 => "EBUSY",
        19 => "ENODEV",
        20 => "ENOTDIR",
        21 => "EISDIR",
        22 => "EINVAL",
        23 => "ENFILE",
        24 => "EMFILE",
        30 => "EROFS",
        36 => "ENAMETOOLONG",
        38 => "ENOSYS",
        _ => "?",
    };
    format!("-{e} ({name})")
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
    /// One command's output against the bytes expected.
    fn expect_out(&mut self, what: &str, got: &[u8], want: &[u8]) {
        if got != want {
            let id = self.id;
            self.fail(format_args!(
                "{id}: `{what}` printed {:?}, expected {:?}",
                String::from_utf8_lossy(got),
                String::from_utf8_lossy(want)
            ));
        }
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
        None => String::from("still running"),
        Some(v) if v & 0x7f == 0 => format!("exit({})", (v >> 8) & 0xff),
        Some(v) => format!("killed by signal {} (status {v:#x})", v & 0x7f),
    }
}

/// `start`, then `fds` installed in the new process's table before it runs.
fn start_with(what: &str, start: impl FnOnce() -> Result<Pid, Errno>, fds: &[(usize, &FileRef)]) -> Result<Pid, String> {
    let ends: Vec<(usize, Fd)> = fds.iter().map(|&(n, f)| (n, Fd { file: f.clone(), cloexec: false })).collect();
    let started = quiet(|| -> Result<(Pid, Vec<Fd>), (String, Vec<Fd>)> {
        let pid = start().map_err(|e| (format!("{what}: starting it returned {}", errno_text(e)), Vec::new()))?;
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

// ── Mounting ─────────────────────────────────────────────────────────────

/// Partition 2 as a new `Ext2`: a new drive, a new cache.
fn mount_ext2() -> Result<Arc<Ext2>, String> {
    let drive: Arc<dyn BlockDevice> = Arc::new(quiet(Ata::probe).map_err(|e| format!("Ata::probe() returned {}", errno_text(e)))?);
    let fs = quiet(|| ata::partition2(drive))
        .and_then(Ext2::mount)
        .map_err(|e| format!("partition2 then Ext2::mount returned {}", errno_text(e)))?;
    Ok(Arc::new(fs))
}

fn setup() -> Option<Arc<Ext2>> {
    let ext2 = match mount_ext2() {
        Ok(fs) => fs,
        Err(why) => {
            note(format_args!("disk: {why}"));
            return None;
        }
    };
    let ext = match quiet(|| ext2.root()) {
        Ok(r) => r,
        Err(e) => {
            note(format_args!("disk: Ext2::root() returned {}", errno_text(e)));
            return None;
        }
    };
    let arch: InodeRef = match Archive::new(&ARCHIVE) {
        Ok(a) => Arc::new(a),
        Err(e) => {
            note(format_args!("vfs: Archive::new over the boot archive returned {}", errno_text(e)));
            return None;
        }
    };
    let procfs: InodeRef = Arc::new(ProcFs::new());
    let devfs: InodeRef = Arc::new(DevFs::new());
    let mut all = true;
    for ((source, target, fstype), root) in MOUNTS.iter().zip([&ext, &arch, &procfs, &devfs]) {
        if let Err(e) = mount::mount(source, target, fstype, root.clone()) {
            note(format_args!("vfs: mount::mount({source:?}, {target:?}, {fstype:?}, its root) returned {}", errno_text(e)));
            all = false;
        }
    }
    all.then_some(ext2)
}

// ── Step 1: the shift map, before anything is typed ──────────────────────

/// `kbd::translate` over a fixed run: shift held over 4 and `.`, released;
/// 4 again; control held over d, released; a break code. Ends with no
/// modifier held, so the shell starts from a clean state.
fn translate_control() -> Result<(), String> {
    const RUN: [(u8, Option<u8>); 10] = [
        (0x2a, None),
        (0x05, Some(b'$')),
        (0x34, Some(b'>')),
        (0xaa, None),
        (0x05, Some(b'4')),
        (0x1d, None),
        (0x20, Some(0x04)),
        (0x9d, None),
        (0xa0, None),
        (0x20, Some(b'd')),
    ];
    for (i, &(sc, want)) in RUN.iter().enumerate() {
        let got = kbd::translate(sc);
        if got != want {
            return Err(format!("kbd::translate({sc:#04x}), scancode {} of the run 2a 05 34 aa 05 1d 20 9d a0 20, returned {got:?}, expected {want:?}", i + 1));
        }
    }
    Ok(())
}

// ── The shell, through the tee ───────────────────────────────────────────

/// The serial side of the tee: `# ` at the start of each line, where a
/// prompt (`$ ` at the start of a line) also ends one.
struct Tee {
    at_start: bool,
    dollar: bool,
    /// Until the first prompt is out: what to print after `keys-ready`.
    held: Option<Vec<u8>>,
}

impl Tee {
    fn feed(&mut self, bytes: &[u8]) {
        let mut w = Vec::new();
        for &b in bytes {
            if self.at_start {
                w.extend_from_slice(b"# ");
                self.at_start = false;
                self.dollar = b == b'$';
            } else if self.dollar {
                self.dollar = false;
                if b == b' ' {
                    w.push(b);
                    self.at_start = true;
                    if let Some(h) = self.held.take() {
                        let mut first = h;
                        first.extend_from_slice(&w);
                        quiet(|| kernel::kprintln!("# sboot keys-ready"));
                        forward(&first);
                        w.clear();
                    }
                    continue;
                }
            }
            w.push(b);
            if b == b'\n' {
                self.at_start = true;
            }
        }
        match self.held.as_mut() {
            Some(h) => h.extend_from_slice(&w),
            None => forward(&w),
        }
    }
    fn flush(&mut self) {
        if let Some(h) = self.held.take() {
            forward(&h);
        }
        if !self.at_start {
            forward(b"\n");
        }
    }
}

/// Bytes to the serial line, as the console writes them (LF → CRLF).
fn forward(bytes: &[u8]) {
    quiet(|| {
        for chunk in bytes.utf8_chunks() {
            serial::write_str(chunk.valid());
            for &b in chunk.invalid() {
                serial::write_byte(b);
            }
        }
    });
}

/// What the shell did.
struct Session {
    sh: Pid,
    out: Vec<u8>,
    exit: Option<i32>,
    ms: u64,
    /// Why the session ended while the shell's output was still open.
    cut: Option<String>,
}

/// Where in the script a point of the shell's output is: after the last
/// marker printed before it. Markers come in order: marker 1 is `echo
/// "q"1`'s line, each later one the shell's `<status> qN` line.
fn place(out: &[u8]) -> String {
    let mut n = 0;
    while n < 8 {
        let tail = format!("q{}\n", n + 1);
        if !out.windows(tail.len()).any(|w| w == tail.as_bytes()) {
            break;
        }
        n += 1;
    }
    if n == 0 {
        String::from("before marker 1")
    } else {
        format!("after marker {n}")
    }
}

fn session(nonce: &str) -> Result<Session, String> {
    let (r, w) = pipe::open(0).map_err(|e| format!("pipe::open(0) returned {}", errno_text(e)))?;
    r.flags.store(r.flags.load(Ordering::SeqCst) | O_NONBLOCK, Ordering::SeqCst);
    let env = format!("SBOOT_NONCE={nonce}");
    let sh = start_with("/bin/sh", || exec::spawn("/bin/sh", &["sh"], &[&env]), &[(1, &w), (2, &w)]);
    drop(w);
    let sh = sh?;
    let mut tee = Tee { at_start: true, dollar: false, held: Some(Vec::new()) };
    let mut out = Vec::new();
    let mut buf = alloc::vec![0u8; 4096];
    let mut c = Clock::new();
    let mut closed = false;
    let mut cut = None;
    // When the shell last printed, and its longest pause with where it fell.
    let mut last = 0;
    let mut pause = (0, String::from("before marker 1"));
    loop {
        match r.read(&mut buf) {
            Ok(0) => {
                closed = true;
                break;
            }
            Ok(n) => {
                let now = c.ms();
                if now - last > pause.0 {
                    pause = (now - last, place(&out));
                }
                last = now;
                out.extend_from_slice(&buf[..n]);
                tee.feed(&buf[..n]);
            }
            Err(11) => {
                let now = c.ms();
                if now - last >= STALL_MS {
                    cut = Some(format!("the shell printed nothing for {} ms, {}", now - last, place(&out)));
                    break;
                }
                if now >= SH_MS {
                    cut = Some(format!("the shell was still running after {SH_MS} ms, {}", place(&out)));
                    break;
                }
                sched::yield_now();
            }
            Err(e) => {
                note(format_args!("tee: a read of the shell's pipe returned {}", errno_text(e)));
                cut = Some(format!("a read of the shell's pipe returned {}, {}", errno_text(e), place(&out)));
                break;
            }
        }
    }
    tee.flush();
    let ms = c.ms();
    let _ = yield_until(500, || exit_of(sh).is_some());
    let exit = exit_of(sh);
    note(format_args!(
        "tee: /bin/sh (pid {}) ran {ms} ms, {} bytes through the tee, {}; {}; its longest pause was {} ms, {}",
        sh.0,
        out.len(),
        status_text(exit),
        match (&cut, closed) {
            (Some(why), _) => why.as_str(),
            (None, true) => "every write end closed",
            (None, false) => "its output still open",
        },
        pause.0,
        pause.1
    ));
    Ok(Session { sh, out, exit, ms, cut })
}

/// Why the typed script cannot have run out, or `None` if it did: its waits
/// after the first are markers 1 to 8, in order (marker 1 is `echo "q"1`'s
/// own line, `q1`), and nothing is typed after the `exit` that follows
/// marker 8.
fn unfinished(s: &Session, segs: &[&[u8]]) -> Option<String> {
    let first = segs.iter().any(|g| *g == b"q1\n");
    let missing = if first { (2..=8).find(|&n| marker(segs, n).is_none())? } else { 1 };
    Some(match &s.cut {
        Some(why) => why.clone(),
        None => format!("the shell closed its output ({}) before marker {missing}", status_text(s.exit)),
    })
}

// ── Reading the session ──────────────────────────────────────────────────

/// The tee's bytes split at each prompt: `segs[k]` is what the k-th command
/// typed printed (`segs[0]` is what came before the first prompt).
fn segments(out: &[u8]) -> Vec<&[u8]> {
    let mut segs = Vec::new();
    let mut from = 0;
    let mut i = 0;
    while i + 1 < out.len() {
        if out[i] == b'$' && out[i + 1] == b' ' {
            segs.push(&out[from..i]);
            i += 2;
            from = i;
        } else {
            i += 1;
        }
    }
    segs.push(&out[from..]);
    segs
}

/// The segment that is marker N's `echo $? "q"N`: `<status> qN\n`. Its
/// index and the status it showed. No note this file prints names a marker
/// as `q` and its number: the script's waits would take it for the shell's.
fn marker(segs: &[&[u8]], n: u32) -> Option<(usize, u32)> {
    let tail = format!(" q{n}\n");
    segs.iter().enumerate().find_map(|(i, s)| {
        let s = core::str::from_utf8(s).ok()?;
        let status = s.strip_suffix(&tail)?;
        Some((i, status.parse().ok()?))
    })
}

/// Marker N, its `$?` expected to be `want`, and the `before` commands
/// typed ahead of it in its group: their outputs, in order.
fn group<'a>(r: &mut Row, segs: &[&'a [u8]], n: u32, want: u32, before: usize) -> Option<Vec<&'a [u8]>> {
    let id = r.id;
    let Some((i, status)) = marker(segs, n) else {
        r.fail(format_args!("{id}: the shell never printed marker {n}, the line its `echo $? \"q\"{n}` prints last in this row (a lost or mistyped key, or the shell ended early)"));
        return None;
    };
    if status != want {
        r.fail(format_args!("{id}: `echo $? \"q\"{n}` showed $? = {status}, expected {want}"));
    }
    if i < before {
        r.fail(format_args!("{id}: marker {n} came {i} prompts in, expected at least {before}"));
        return None;
    }
    Some(segs[i - before..i].to_vec())
}

fn lines(s: &[u8]) -> Vec<&str> {
    core::str::from_utf8(s).unwrap_or("").lines().collect()
}

fn hex16(v: u64) -> String {
    format!("{v:016x}")
}

// ── Rows ─────────────────────────────────────────────────────────────────

fn ls_row(s: &Session, segs: &[&[u8]]) {
    let mut r = Row::new(LS_ID);
    // q1 then: cd /procx<BS>, ls, pwd, echo $? "q"2.
    if let Some(g) = group(&mut r, segs, 2, 0, 3) {
        r.expect_out("cd /procx<backspace>", g[0], b"");
        let mut want: Vec<String> = PROC_TOP.iter().map(|s| String::from(*s)).collect();
        want.push(format!("{}", s.sh.0));
        want.push(format!("{}", s.sh.0 + 2));
        want.sort();
        let mut got: Vec<String> = lines(g[1]).iter().map(|s| String::from(*s)).collect();
        got.sort();
        if got != want {
            r.fail(format_args!(
                "{LS_ID}: `ls` in the cwd printed {:?}, expected /proc's names {want:?} (the shell is pid {}, the `ls` pid {}, the reaped `echo` before it pid {})",
                String::from_utf8_lossy(g[1]),
                s.sh.0,
                s.sh.0 + 2,
                s.sh.0 + 1
            ));
        }
        r.expect_out("pwd", g[2], b"/proc\n");
    }
    r.finish(1);
}

fn redirect_row(segs: &[&[u8]]) {
    let mut r = Row::new(REDIRECT_ID);
    let nonce = r.nonce();
    // cat /proc/sboot/nonce > /dev/null; cat < /dev/null; cd /tmp;
    // cat /proc/sboot/nonce > k23n; cat < k23n; echo $? "q"3.
    if let Some(g) = group(&mut r, segs, 3, 0, 5) {
        r.expect_out("cat /proc/sboot/nonce > /dev/null", g[0], b"");
        r.expect_out("cat < /dev/null", g[1], b"");
        r.expect_out("cd /tmp", g[2], b"");
        r.expect_out("cat /proc/sboot/nonce > k23n", g[3], b"");
        let mut want = Vec::from(&nonce[..]);
        want.push(b'\n');
        r.expect_out("cat < k23n", g[4], &want);
    }
    r.finish(2);
}

/// The stream `producer chain` writes for the capture nonce, as this file
/// computes it (user/producer/stream.h; ktest-15's formula): its length and
/// its FNV-1a 64.
fn stream_sum(nonce: &[u8; 32]) -> (u64, u64) {
    let seed = ktap::derive(nonce, "pipe.chain.checksum");
    let len = 4096 + seed % 8192;
    let mut fnv: u64 = 0xcbf2_9ce4_8422_2325;
    for i in 0..len {
        let b = (ktap::splitmix64(seed ^ (i >> 3)) >> (8 * (i & 7))) as u8;
        fnv = (fnv ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    (len, fnv)
}

fn pipe_row(segs: &[&[u8]]) {
    let mut r = Row::new(PIPE_ID);
    let (len, fnv) = stream_sum(&r.nonce());
    note(format_args!("{PIPE_ID}: derive(nonce, \"pipe.chain.checksum\"): the stream is {len} bytes, fnv1a64 {}", hex16(fnv)));
    if let Some(g) = group(&mut r, segs, 4, 0, 1) {
        let ls = lines(g[0]);
        for want in ["ok 1 - pipe.chain.checksum", "ok 2 - pipe.eof.last_writer", "ok 3 - pipe.cloexec"] {
            if !ls.contains(&want) {
                r.fail(format_args!("{PIPE_ID}: the pipeline printed no line `{want}`; it printed {:?}", String::from_utf8_lossy(g[0])));
            }
        }
        // `# pipe.chain.checksum: <total> of <len> bytes in <n> reads, fnv1a64 <got> want <want>`:
        // the bytes the consumer read and their sum, against this file's own
        // computation from the capture nonce (not the consumer's `want`).
        let sum = ls.iter().find_map(|l| {
            let rest = l.strip_prefix("# pipe.chain.checksum: ")?;
            let total = String::from(rest.split(' ').next()?);
            let at = rest.find("fnv1a64 ")?;
            let mut w = rest[at + 8..].split(' ');
            Some((total, String::from(w.next()?), String::from(w.nth(1)?)))
        });
        let (want_len, want_sum) = (format!("{len}"), hex16(fnv));
        match sum {
            Some((total, got, want)) if total == want_len && got == want_sum && want == want_sum => {}
            other => r.fail(format_args!(
                "{PIPE_ID}: the consumer's checksum line was {other:?} (bytes read, fnv1a64, its want), expected ({want_len:?}, {want_sum:?}, {want_sum:?}): the stream the capture nonce gives"
            )),
        }
    }
    r.finish(3);
}

fn hi_row(segs: &[&[u8]]) {
    let mut r = Row::new(HI_ID);
    let nonce = r.nonce();
    if let Some(g) = group(&mut r, segs, 5, 0, 1) {
        let n = String::from_utf8_lossy(&nonce).into_owned();
        let want = [
            format!("hi: cs=0x2b rpl=3 nonce={n}"),
            format!("# derive={}", hex16(ktap::derive(&nonce, "ring3.cs.rpl"))),
            String::from("ok 1 - ring3.cs.rpl"),
        ];
        let ls = lines(g[0]);
        for w in &want {
            if !ls.iter().any(|l| l == w) {
                r.fail(format_args!("{HI_ID}: `hi $SBOOT_NONCE 1` printed no line `{w}`; it printed {:?}", String::from_utf8_lossy(g[0])));
            }
        }
    }
    r.finish(4);
}

fn status_row(segs: &[&[u8]]) {
    let mut r = Row::new(STATUS_ID);
    if let Some(g) = group(&mut r, segs, 6, 0, 1) {
        r.expect_out("cat, then ab, Enter, ctrl+d typed", g[0], b"ab\n");
    }
    if let Some(g) = group(&mut r, segs, 7, 1, 1) {
        r.expect_out("cat /nonexistent", g[0], b"cat: /nonexistent: ENOENT\n");
    }
    if let Some(g) = group(&mut r, segs, 8, 127, 1) {
        r.expect_out("nosuch", g[0], b"sh: nosuch: ENOENT\n");
    }
    r.finish(5);
}

fn remount_row(s: &Session, segs: &[&[u8]], ext2: &Arc<Ext2>) {
    let mut r = Row::new(REMOUNT_ID);
    let nonce = r.nonce();
    // After q8: echo $SBOOT_NONCE > k23, then exit.
    match marker(segs, 8) {
        Some((i, _)) if segs.len() == i + 3 => r.expect_out("echo $SBOOT_NONCE > k23", segs[i + 1], b""),
        Some((i, _)) => r.fail(format_args!("{REMOUNT_ID}: {} prompts after marker 8, expected 2 (`echo $SBOOT_NONCE > k23`, `exit`)", segs.len() - 1 - i)),
        None => r.fail(format_args!("{REMOUNT_ID}: not reached: the shell never printed marker 8")),
    }
    if s.exit != Some(0) {
        r.fail(format_args!("{REMOUNT_ID}: /bin/sh ended {} after {} ms, expected exit(0) from `exit`", status_text(s.exit), s.ms));
    }
    if let Err(e) = quiet(|| ext2.sync()) {
        r.fail(format_args!("{REMOUNT_ID}: Ext2::sync() returned {}", errno_text(e)));
    }
    let read = mount_ext2().and_then(|fs2| {
        let root = quiet(|| fs2.root()).map_err(|e| format!("the second mount's root() returned {}", errno_text(e)))?;
        let f = quiet(|| root.lookup("tmp").and_then(|t| t.lookup("k23"))).map_err(|e| format!("tmp/k23 through the second mount: {}", errno_text(e)))?;
        let mut buf = [0u8; 64];
        let n = quiet(|| f.read_at(0, &mut buf)).map_err(|e| format!("reading tmp/k23 through the second mount: {}", errno_text(e)))?;
        Ok(Vec::from(&buf[..n]))
    });
    let mut want = Vec::from(&nonce[..]);
    want.push(b'\n');
    match read {
        Ok(got) if got == want => {}
        Ok(got) => r.fail(format_args!(
            "{REMOUNT_ID}: tmp/k23 read through a second mount of partition 2 held {:?}, expected {:?}",
            String::from_utf8_lossy(&got),
            String::from_utf8_lossy(&want)
        )),
        Err(why) => r.fail(format_args!("{REMOUNT_ID}: {why}")),
    }
    r.finish(6);
}

fn not_run(why: &str) {
    for (n, id) in [LS_ID, REDIRECT_ID, PIPE_ID, HI_ID, STATUS_ID, REMOUNT_ID].into_iter().enumerate() {
        let mut r = Row::new(id);
        r.fail(format_args!("{id}: not run: {why}"));
        r.finish(n + 1);
    }
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
    let early = |why: String| {
        not_run(&why);
        Some(format!("no shell ran: {why}"))
    };
    let unfinished = match translate_control() {
        Err(why) => early(why),
        Ok(()) => match setup() {
            None => early(String::from("the four mounts did not all succeed")),
            Some(ext2) => {
                let nonce = ktap::nonce().map(|n| String::from_utf8_lossy(&n).into_owned()).unwrap_or_default();
                match session(&nonce) {
                    Err(why) => early(why),
                    Ok(s) => {
                        // The PM timer wraps every 4.7 s: this clock starts after the
                        // session (the rows take about 4 ms).
                        let mut c = Clock::new();
                        let segs = segments(&s.out);
                        ls_row(&s, &segs);
                        redirect_row(&segs);
                        pipe_row(&segs);
                        hi_row(&segs);
                        status_row(&segs);
                        remount_row(&s, &segs, &ext2);
                        note(format_args!("the rows took {} ms", c.ms()));
                        unfinished(&s, &segs)
                    }
                }
            }
        },
    };
    sched::yield_now();
    if let Some(why) = unfinished {
        // The rows are out; the script can never finish, so end the capture
        // on the panic report rather than leave it to the rubric's timeout.
        panic!("the typed script cannot finish ({why}); the six rows above are this run's result");
    }
    quiet(ktap::end);
    halt()
}
