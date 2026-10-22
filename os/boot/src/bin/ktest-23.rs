//! Lab 23's vehicle (`23-the-shell`): the console's line discipline, the
//! shift map, the working directory — and the course's shell, `/bin/sh`,
//! run on them with the grader typing into it.
//!
//! The disk is `ktest-23.img` as the course's image writer made it (CONTRACT
//! §A.7); the guest writes it, so the course boots a copy. The vehicle
//! first checks `kbd::translate` against a run of scancodes drawn for this
//! run (step 1: both shift keys, their releases, control, a break code),
//! then mounts what lab 22's does (ext2 at `/` from `/dev/hda2` through the
//! learner's `Ata`, `partition2` and `Ext2::mount`, the boot archive at
//! `/boot`, a `ProcFs` at `/proc`, a `DevFs` at `/dev`), makes this run's
//! directory in ext2's `/tmp`, then starts `/bin/sh` off ext2 through the learner's exec with three
//! variables: `SBOOT_KEY` (32 hex: the nonce's derive, then an unpublished
//! word), `SBOOT_WORD` (an unpublished word) and `SBOOT_DIR` (`/tmp/` and
//! this run's directory). Every word drawn from the time-stamp counter is
//! printed only after the row that carries it has read it back (CONTRACT
//! §A.1-5). The shell's fd 0 is the console (`proc::start`'s three standard
//! fds); its fds 1 and 2 are a pipe this file drains — the TEE. Everything
//! the shell and its children write comes through the tee: this file keeps
//! it (the rows read it) and forwards it to the serial line with `# ` at the
//! start of every line, so a program's own KTAP lines (`hi`'s `ok 1 -
//! ring3.cs.rpl`) are comments here. A typed line is echoed by the
//! learner's console, not by this file: on the serial line it follows the
//! forwarded prompt.
//!
//! At the shell's first prompt this file types a line of its own through
//! `kbd::push`, one scancode at a time: `echo `, a word drawn for this run
//! (letters and digits, some shifted with either shift key, a wrong key and
//! a backspace inside it), Enter. The shell must print the word: a console
//! that does not edit typed keys into lines fails every row but row 7. While
//! it types that line, COM1 is in loopback (MCR bit 4, as ktest-22's row 6)
//! with its received-data interrupt off (IER bit 0, restored after, as the
//! MCR is), so what the console echoes comes back in the UART's receive FIFO
//! instead of leaving the machine, and an IRQ 4 handler does not take it.
//! Any other reader of COM1's receive still can: interrupts stay on while
//! the shell echoes, so a timer tick or a task that drains the FIFO takes
//! the echo first, and nothing here can park it (CONTRACT §B.2-23, G1342).
//! After each key that types a byte (a printable
//! key, backspace, Enter) the next key waits until the echo since the key
//! holds that byte (backspace: 0x08; Enter: `\n`), up to `ECHO_MS`. The
//! first key not echoed in time is noted, and the second prompt goes out as
//! `$~ `; the line's echo is then forwarded as it came. The second prompt is
//! held until `# sboot keys-ready` has been printed, which is the rubric
//! script's first `wait`; the script then types one command group at a time
//! and waits for that group's last line, the shell's own `echo $? "q"N` (the
//! quotes keep `qN` out of the echo). The shell ends on `exit`; the rows are
//! judged on what came through the tee, split at each prompt (`$ `) into one
//! segment per command typed.
//!
//! The rubric's first row, `sh.prompt.echo.paced`, is a serial row: the
//! substring `$ echo "q"1` — the forwarded second prompt followed by the
//! console's echo of the typed line, shifted quote included. This file
//! forwards that prompt as `$~ ` instead when the console did not echo its
//! own first line key by key (below), so the row also stands for that
//! echo. The seven rows here:
//!
//!   1 `sh.ls.proc` (plain) — `cd /procx`, a backspace, Enter: no output;
//!     `ls` prints exactly `/proc`'s names: `meminfo`, `mounts`, `sboot`,
//!     `uptime`, the shell's pid S and `ls`'s own pid S+3 (the two reaped
//!     `echo`s, S+1 and S+2, are gone); `pwd`, a forked child, prints
//!     `/proc`; `$?` is 0.
//!   2 `sh.redirect.devnull` (derived) — `cat /proc/sboot/nonce > /dev/null`
//!     and `cat < /dev/null` print nothing (the second reads end of file at
//!     once); `cd $SBOOT_DIR`, `echo $SBOOT_WORD > k23n` print nothing and
//!     `cat < k23n` prints the word: `>` and `<` reached the files they
//!     name, relative to the cwd; `$?` is 0.
//!   3 `sh.pipe.two` (derived) — `producer $SBOOT_KEY 1 chain | consumer
//!     $SBOOT_KEY 1 chain 3 4`: the consumer's three `ok` lines, and the
//!     byte count and FNV-1a 64 it read (and the sum it wanted) equal to the
//!     stream this file computes from the key (user/producer/stream.h's
//!     formula); `$?` is 0.
//!   4 `sh.hi.by_name` (derived) — `hi $SBOOT_KEY 1`, a bare name the shell
//!     runs as `/bin/hi` from this run's directory: `cs=0x2b rpl=3`, the
//!     key, its `# derive=` line equal to `ktap::derive(key,
//!     "ring3.cs.rpl")`, and its `ok 1`; `$?` is 0.
//!   5 `sh.exit_code.shown` (plain) — `cat` with `ab`, Enter and ctrl+d
//!     typed into it prints `ab` and `$?` is 0; `cat /nonexistent` prints
//!     its ENOENT line and `$?` is 1; `nosuch` gives the shell's ENOENT line
//!     and `$?` is 127.
//!   6 `sh.remount.survives` (derived) — `echo $SBOOT_KEY > k23` in this
//!     run's directory, then `exit`: the shell ends with status 0; this file
//!     calls `Ext2::sync`, mounts partition 2 a second time (a new drive, a
//!     new cache) and reads `/tmp/<dir>/k23` through that mount alone: the
//!     key and a newline.
//!   7 `sh.cwd.calls` (derived) — a program this file assembles, run through
//!     the learner's exec after the shell has ended: its own cwd `/` (not the
//!     shell's), `chdir("")` and a missing path refused and the cwd left,
//!     `open` of this run's directory by a relative path, `chdir` through
//!     `..`, `getcwd` with room to spare, one byte short and exactly enough,
//!     `open` and `chdir` of a drawn file in it, `chdir("nosuch/..")` refused
//!     (the walk before the fold), then the same twelve path shapes on every
//!     run, each through `chdir` and through `fchdir` onto an fd it opened:
//!     a `.` at the start, in the middle, at the end and right after `/`;
//!     `//` at the start, in the middle and at the end; a trailing `/`; one,
//!     two and three `..` above `/`; and `<s>/..` beside a `//`. Only the
//!     names in them are drawn. `chdir("/..")` lands on `/`, a forked child's
//!     `chdir` leaves its parent where it was, `fchdir` back to the first
//!     directory opened (neither the last cwd nor the last directory opened)
//!     and onto three `.`/`..` paths to the drawn subdirectory, folded, and
//!     `fchdir` onto the file and onto a pipe end refused, the cwd left.
//!     Then, in further programs, every path of one to four parts built
//!     from an empty part, `.`, `..`, `tmp`, this run's directory and
//!     subdirectory and `nosuch`, and four longer absolute ones, each
//!     `chdir`ed from this run's directory against where walking it in
//!     order lands (`-ENOENT` where the walk meets a missing name), and each
//!     that lands also through `open` and `fchdir`. Every `..` stays on `/`,
//!     `/tmp` and this run's directory, never under `/proc` or `/dev`.
//!
//! Whenever the serial row cannot pass because of something this file saw
//! (no shell ran, a key of the first line went unechoed, the shell never
//! printed its second prompt), it says so on a `# sh.prompt.echo.paced: `
//! line; the rubric's row `lacks` that prefix, so the engine quotes it.
//! A time appears in this file's output only on a failure, beside its limit.
//!
//! When `kbd::translate` is wrong, or the four mounts fail, no shell runs and
//! every row says why. The session ends when the shell closes its output, when
//! it has printed nothing for `STALL_MS`, or after `SH_MS`, all by the PM
//! timer; a script that stalls leaves the rows to say which marker never came.
//!
//! A run that ends without all eight markers can never finish the typed
//! script — each marker is one of its `wait`s, and the engine counts `# sboot
//! end` only once the script has run out — so it would wait out the rubric's
//! whole timeout. Instead, after the seven rows, this file panics through the
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

const PLAN: usize = 7;
const LS_ID: &str = "sh.ls.proc";
const REDIRECT_ID: &str = "sh.redirect.devnull";
const PIPE_ID: &str = "sh.pipe.two";
const HI_ID: &str = "sh.hi.by_name";
const STATUS_ID: &str = "sh.exit_code.shown";
const REMOUNT_ID: &str = "sh.remount.survives";
const CWD_ID: &str = "sh.cwd.calls";

/// What the test mounts, as lab 22's does.
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

/// How long one key of the first line may go unechoed before the next key
/// comes anyway. An honest console echoes a key as the shell's `read` takes
/// it, a few scheduler turns after `push`; after the first key past this
/// bound the rest of the line goes in without waiting.
const ECHO_MS: u64 = 3_000;

/// The serial row's id (the rubric's; no ktap line carries it).
const PROMPT_ID: &str = "sh.prompt.echo.paced";

/// COM1, the serial line every console in the course writes to (lab 02).
const COM1: u16 = 0x3f8;

fn port_in(port: u16) -> u8 {
    let v: u8;
    // SAFETY: a port read touches no memory.
    unsafe { asm!("in al, dx", out("al") v, in("dx") port, options(nomem, nostack, preserves_flags)) };
    v
}

fn port_out(port: u16, v: u8) {
    // SAFETY: a port write touches no memory; COM1 is the serial line.
    unsafe { asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack, preserves_flags)) };
}

/// Spin until `done` or a bounded number of tries.
fn port_wait(done: impl Fn() -> bool) {
    let mut n = 0u32;
    while !done() && n < 1_000_000 {
        n += 1;
        core::hint::spin_loop();
    }
}

/// `/proc`'s fixed names (procfs.rs).
const PROC_TOP: [&str; 4] = ["meminfo", "mounts", "sboot", "uptime"];

const O_NONBLOCK: u32 = 0x800;

/// Row 7's program (ktest-22's probe): its data segment, and how many
/// system calls it makes at most.
const DATA: u64 = 0x0110_0000;
const MAX_STEPS: usize = 104;
/// How long row 7's program may run.
const RUN_MS: u64 = 10_000;

const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_GETCWD: u64 = 79;
const SYS_CHDIR: u64 = 80;
const SYS_FCHDIR: u64 = 81;
const SYS_CLOSE: u64 = 3;
const SYS_FORK: u64 = 57;
const SYS_WAIT4: u64 = 61;

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
/// the PM timer, mixed (ktest-22's). Each word below that takes one is
/// printed only after the row that carries it has read it back.
fn unpublished() -> u64 {
    // SAFETY: rdtsc reads a counter; no memory is touched.
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
    ktap::splitmix64(tsc ^ (pm() as u64) << 40)
}

/// A sequence of draws from one seed.
struct Draw(u64);

impl Draw {
    fn below(&mut self, n: usize) -> usize {
        self.0 = ktap::splitmix64(self.0.wrapping_add(0x9e37_79b9_7f4a_7c15));
        (self.0 % n as u64) as usize
    }
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

/// `n` with a comma between each group of three digits (a time in a
/// failure line, beside its limit).
fn grouped(n: u64) -> String {
    let d = format!("{n}");
    let mut out = String::new();
    for (i, c) in d.chars().enumerate() {
        if i > 0 && (d.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
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
        34 => "ERANGE",
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

fn setup() -> Option<(Arc<Ext2>, InodeRef)> {
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
    if !all {
        return None;
    }
    match quiet(|| ext.lookup("tmp")) {
        Ok(tmp) => Some((ext2, tmp)),
        Err(e) => {
            note(format_args!("disk: the root's lookup(\"tmp\") returned {}", errno_text(e)));
            None
        }
    }
}

// ── Step 1: the shift map, before anything is typed ──────────────────────

/// The byte a set-1 make code types alone and with shift held (US layout:
/// lab 06's `to_ascii` rows and the lesson's symbol rows); `None` for a
/// code outside the four rows.
fn plain_shifted(sc: u8) -> Option<(u8, u8)> {
    const PLAIN: [&[u8]; 4] = [b"1234567890-=", b"qwertyuiop[]", b"asdfghjkl;'`", b"\\zxcvbnm,./"];
    const SHIFTED: [&[u8]; 4] = [b"!@#$%^&*()_+", b"QWERTYUIOP{}", b"ASDFGHJKL:\"~", b"|ZXCVBNM<>?"];
    let (row, at) = match sc {
        0x02..=0x0d => (0, sc - 0x02),
        0x10..=0x1b => (1, sc - 0x10),
        0x1e..=0x29 => (2, sc - 0x1e),
        0x2b..=0x35 => (3, sc - 0x2b),
        _ => return None,
    };
    Some((PLAIN[row][at as usize], SHIFTED[row][at as usize]))
}

/// The make codes of the four rows, and of their letters.
fn row_keys(letters_only: bool) -> Vec<u8> {
    (0x02..=0x35u8).filter(|&sc| plain_shifted(sc).is_some_and(|(p, _)| !letters_only || p.is_ascii_lowercase())).collect()
}

/// `kbd::translate` over a run drawn for this run: one shift key held over
/// two keys and released, a key, the other shift key over one key and
/// released, control held over a letter and a break code, released, a
/// letter. Ends with no modifier held, so the shell starts from a clean
/// state.
fn translate_control(seed: u64) -> Result<(), String> {
    let mut d = Draw(seed);
    let keys = row_keys(false);
    let letters = row_keys(true);
    let any = |d: &mut Draw| keys[d.below(keys.len())];
    let (k1, k2, k3, k4, k5) = (any(&mut d), any(&mut d), any(&mut d), any(&mut d), any(&mut d));
    let (l1, l2) = (letters[d.below(letters.len())], letters[d.below(letters.len())]);
    let (first, second) = if d.below(2) == 0 { ((0x2a, 0xaa), (0x36, 0xb6)) } else { ((0x36, 0xb6), (0x2a, 0xaa)) };
    let up = |sc: u8| plain_shifted(sc).map(|(_, s)| s);
    let down = |sc: u8| plain_shifted(sc).map(|(p, _)| p);
    let run: [(u8, Option<u8>, &str); 14] = [
        (first.0, None, "shift"),
        (k1, up(k1), "shift"),
        (k2, up(k2), "shift"),
        (first.1, None, "no modifier"),
        (k3, down(k3), "no modifier"),
        (second.0, None, "shift"),
        (k4, up(k4), "shift"),
        (second.1, None, "no modifier"),
        (0x1d, None, "control"),
        (l1, down(l1).map(|c| c & 0x1f), "control"),
        (k5 | 0x80, None, "control"),
        (0x9d, None, "no modifier"),
        (l2, down(l2), "no modifier"),
        (l2 | 0x80, None, "no modifier"),
    ];
    for (i, &(sc, want, held)) in run.iter().enumerate() {
        let got = kbd::translate(sc);
        if got != want {
            let all: Vec<String> = run.iter().map(|&(c, _, _)| format!("{c:02x}")).collect();
            return Err(format!(
                "kbd::translate({sc:#04x}), scancode {} of the run {}, with {held} held, returned {got:?}, expected {want:?}",
                i + 1,
                all.join(" ")
            ));
        }
    }
    Ok(())
}

/// What this run carries, drawn from the nonce and the time-stamp counter.
struct Words {
    /// `SBOOT_KEY`: 32 hex, `derive(nonce, "sh.key")` then an unpublished
    /// half. `producer`, `consumer` and `hi` take it as their nonce
    /// argument; `k23` holds it.
    key: [u8; 32],
    /// `SBOOT_WORD`: what `k23n` carries.
    word: String,
    /// `SBOOT_DIR`'s last name, made in ext2's `/tmp` before the shell runs.
    dir: String,
}

fn words() -> Words {
    let n = ktap::nonce().unwrap_or([b'0'; 32]);
    let k = format!("{}{}", hex16(ktap::derive(&n, "sh.key")), hex16(unpublished()));
    let mut key = [0u8; 32];
    key.copy_from_slice(k.as_bytes());
    let word = format!("w{:012x}", (ktap::derive(&n, "sh.word") ^ unpublished()) & 0xffff_ffff_ffff);
    let dir = format!("d{:08x}", (ktap::derive(&n, "sh.dir") ^ unpublished()) & 0xffff_ffff);
    Words { key, word, dir }
}

/// The line this file types itself at the shell's first prompt, as
/// scancodes (each key's make code, then its break code): `echo `, a word
/// of ten letters and digits — some shifted, with either shift key, and a
/// wrong key and a backspace inside it — then Enter. Each scancode comes
/// with the byte its echo must hold (`None` for a modifier or a break code).
/// And the word.
fn first_line(seed: u64) -> (Vec<(u8, Option<u8>)>, String) {
    let mut d = Draw(seed);
    let mut sc = Vec::new();
    let tap = |sc: &mut Vec<(u8, Option<u8>)>, k: u8, echo: u8| {
        sc.push((k, Some(echo)));
        sc.push((k | 0x80, None));
    };
    for (k, c) in [(0x12, b'e'), (0x2e, b'c'), (0x23, b'h'), (0x18, b'o'), (0x39, b' ')] {
        tap(&mut sc, k, c);
    }
    // Letters and digits; shift+4 (`$`) would be the shell's.
    let pool: Vec<u8> = (0x02..=0x35u8).filter(|&k| plain_shifted(k).is_some_and(|(p, _)| p.is_ascii_alphanumeric())).collect();
    let letters = row_keys(true);
    let wrong_at = 1 + d.below(8);
    let mut word = String::new();
    for i in 0..10 {
        if i == wrong_at {
            let l = letters[d.below(letters.len())];
            tap(&mut sc, l, plain_shifted(l).map_or(b'x', |(p, _)| p));
            tap(&mut sc, 0x0e, 0x08); // backspace
        }
        let k = pool[d.below(pool.len())];
        let (plain, shifted) = plain_shifted(k).unwrap_or((b'x', b'X'));
        if (i % 3 == 1 || d.below(4) == 0) && k != 0x05 {
            let (make, brk) = if d.below(2) == 0 { (0x2a, 0xaa) } else { (0x36, 0xb6) };
            sc.push((make, None));
            tap(&mut sc, k, shifted);
            sc.push((brk, None));
            word.push(shifted as char);
        } else {
            tap(&mut sc, k, plain);
            word.push(plain as char);
        }
    }
    tap(&mut sc, 0x1c, b'\n'); // Enter
    (sc, word)
}

// ── The shell, through the tee ───────────────────────────────────────────

/// The serial side of the tee: `# ` at the start of each line, where a
/// prompt (`$ ` at the start of a line) also ends one.
struct Tee {
    at_start: bool,
    dollar: bool,
    /// Prompts seen so far.
    prompts: u32,
    /// From the first prompt until the second is out: what to print after
    /// `keys-ready`.
    held: Option<Vec<u8>>,
    /// The last byte forwarded to the serial line, if any.
    last: Option<u8>,
    /// Why the first line was not echoed key by key, once its check is
    /// over: the second prompt then goes out as `$~ `.
    unechoed: Option<String>,
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
                    self.prompts += 1;
                    if self.prompts == 1 {
                        // The first prompt goes out as it comes: the line this
                        // file types at it is echoed after it.
                        self.send(&w);
                        w.clear();
                        self.held = Some(Vec::new());
                    } else if let Some(h) = self.held.take() {
                        let mut first = h;
                        if let Some(why) = &self.unechoed {
                            note(format_args!("{PROMPT_ID}: {why}; the prompt after it goes out as `$~ `"));
                            let n = w.len();
                            w.splice(n - 1..n - 1, [b'~']);
                        }
                        first.extend_from_slice(&w);
                        quiet(|| kernel::kprintln!("# sboot keys-ready"));
                        self.send(&first);
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
            None => self.send(&w),
        }
    }
    fn send(&mut self, bytes: &[u8]) {
        if let Some(&b) = bytes.last() {
            self.last = Some(b);
        }
        forward(bytes);
    }
    /// Ends the shell's last line, so what this file prints next starts a
    /// line of its own — also after a prompt (`$ `), which leaves
    /// `at_start` set although the serial line is not at a line start.
    fn flush(&mut self) {
        if let Some(h) = self.held.take() {
            self.send(&h);
        }
        if self.last.is_some_and(|b| b != b'\n') {
            self.send(b"\n");
        }
    }
}

/// The first line's echo, key by key, with COM1 in loopback while the line
/// is typed: what the console echoes comes back in the UART's receive FIFO
/// (16 bytes), which this drains every turn of the session's loop. IER bit 0
/// (received data available) is clear for the same window, so a kernel whose
/// IRQ 4 handler takes COM1's received bytes does not race this file for the
/// echo (CONTRACT §B.2-23, fix round 1c).
struct EchoCheck {
    /// The MCR as it was, while loopback is on.
    mcr: Option<u8>,
    /// The IER as it was, while loopback is on (its bit 0 is clear then).
    ier: u8,
    /// Every byte echoed since loopback went on.
    cap: Vec<u8>,
    /// What the key pushed last must echo: its byte, its place in the line,
    /// where its echo starts in `cap`, and when it went in.
    want: Option<(u8, usize, usize, u64)>,
    /// The first key not echoed in time.
    fail: Option<String>,
    done: bool,
}

impl EchoCheck {
    fn start(&mut self) {
        quiet(|| {
            // Received-data interrupt off first: from here until `stop`,
            // nothing but this file reads the receive FIFO.
            let ier = port_in(COM1 + 1);
            port_out(COM1 + 1, ier & !1);
            self.ier = ier;
            port_wait(|| port_in(COM1 + 5) & 0x40 != 0);
            let mcr = port_in(COM1 + 4);
            port_out(COM1 + 2, 0xC7);
            let mut n = 0;
            while port_in(COM1 + 5) & 1 != 0 && n < 64 {
                let _ = port_in(COM1);
                n += 1;
            }
            port_out(COM1 + 4, mcr | 0x10);
            self.mcr = Some(mcr);
        });
    }
    fn drain(&mut self) {
        if self.mcr.is_some() {
            quiet(|| {
                while port_in(COM1 + 5) & 1 != 0 && self.cap.len() < 4096 {
                    self.cap.push(port_in(COM1));
                }
            });
        }
    }
    /// Whether the next key may go in: the last one's echo came, or it
    /// timed out (noted once; after that nothing is waited for).
    fn ready(&mut self, now: u64, line_len: usize) -> bool {
        let Some((b, i, from, at)) = self.want else { return true };
        if self.cap[from..].contains(&b) {
            self.want = None;
            return true;
        }
        if now - at < ECHO_MS {
            return false;
        }
        self.fail.get_or_insert_with(|| {
            format!(
                "scancode {} of the {line_len} the test typed at the first prompt ({}) was not echoed within the limit of {} ms; the serial line carried {:?} after it",
                i + 1,
                key_name(b),
                grouped(ECHO_MS),
                String::from_utf8_lossy(&self.cap[from..])
            )
        });
        self.want = None;
        true
    }
    /// Loopback off and the IER restored; the echo goes out on the serial
    /// line as it came, ended by a newline when it has none, so a note after
    /// it starts a line.
    fn stop(&mut self) {
        self.drain();
        if let Some(mcr) = self.mcr.take() {
            quiet(|| {
                port_wait(|| port_in(COM1 + 5) & 0x40 != 0);
                port_out(COM1 + 4, mcr);
                // What looped back after the last drain is this file's, not
                // input: dropped before the kernel's interrupt can take it.
                // Unpinned: no measured kernel was red without this drain
                // (TL review 2026-10-22, N2), so no case would notice it gone.
                let mut n = 0;
                while port_in(COM1 + 5) & 1 != 0 && n < 64 {
                    let _ = port_in(COM1);
                    n += 1;
                }
                port_out(COM1 + 1, self.ier);
            });
            forward(&self.cap);
            if self.cap.last() != Some(&b'\n') {
                forward(b"\n");
            }
        }
        self.done = true;
    }
}

/// What a scancode of the first line types, for the note.
fn key_name(b: u8) -> String {
    match b {
        0x08 => String::from("backspace"),
        b'\n' => String::from("Enter"),
        c => format!("`{}`", c as char),
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

fn session(words: &Words, line: &[(u8, Option<u8>)]) -> Result<Session, String> {
    let (r, w) = pipe::open(0).map_err(|e| format!("pipe::open(0) returned {}", errno_text(e)))?;
    r.flags.store(r.flags.load(Ordering::SeqCst) | O_NONBLOCK, Ordering::SeqCst);
    let env = [
        format!("SBOOT_KEY={}", String::from_utf8_lossy(&words.key)),
        format!("SBOOT_WORD={}", words.word),
        format!("SBOOT_DIR=/tmp/{}", words.dir),
    ];
    let envp: Vec<&str> = env.iter().map(|e| e.as_str()).collect();
    let sh = start_with("/bin/sh", || exec::spawn("/bin/sh", &["sh"], &envp), &[(1, &w), (2, &w)]);
    drop(w);
    let sh = sh?;
    let mut tee = Tee { at_start: true, dollar: false, prompts: 0, held: None, last: None, unechoed: None };
    // The first line's scancodes go in one per turn of the loop once the
    // first prompt is out, each after the last one's echo, so the shell
    // takes them as they come.
    let mut typed = 0;
    let mut ek = EchoCheck { mcr: None, ier: 0, cap: Vec::new(), want: None, fail: None, done: false };
    let mut out = Vec::new();
    let mut buf = alloc::vec![0u8; 4096];
    let mut c = Clock::new();
    let mut closed = false;
    let mut cut = None;
    // When the shell last printed.
    let mut last = 0;
    loop {
        if tee.prompts >= 1 && !ek.done {
            if ek.mcr.is_none() {
                ek.start();
            }
            ek.drain();
            let now = c.ms();
            if ek.ready(now, line.len()) {
                if typed < line.len() {
                    let (sc, echo) = line[typed];
                    quiet(|| kbd::push(sc));
                    let from = ek.cap.len();
                    ek.want = echo.filter(|_| ek.fail.is_none()).map(|b| (b, typed, from, now));
                    typed += 1;
                } else {
                    ek.stop();
                    tee.unechoed = ek.fail.clone();
                }
            }
        }
        match r.read(&mut buf) {
            Ok(0) => {
                closed = true;
                break;
            }
            Ok(n) => {
                // The shell has printed: the line's check is over (an echo
                // still owed is late), and loopback must be off before the
                // tee forwards anything.
                if !ek.done && ek.mcr.is_some() {
                    ek.drain();
                    if let Some((b, i, from, _)) = ek.want.take() {
                        if !ek.cap[from..].contains(&b) {
                            ek.fail.get_or_insert_with(|| {
                                format!("scancode {} of the {} the test typed at the first prompt ({}) was not echoed before the shell printed", i + 1, line.len(), key_name(b))
                            });
                        }
                    }
                    if typed < line.len() {
                        ek.fail.get_or_insert_with(|| format!("the shell printed before the test had typed its first line ({typed} of {} scancodes)", line.len()));
                    }
                    ek.stop();
                    tee.unechoed = ek.fail.clone();
                }
                last = c.ms();
                out.extend_from_slice(&buf[..n]);
                tee.feed(&buf[..n]);
            }
            Err(11) => {
                let now = c.ms();
                if now - last >= STALL_MS {
                    cut = Some(format!("the shell printed nothing for {} ms, {}; the limit is {} ms", grouped(now - last), place(&out), grouped(STALL_MS)));
                    break;
                }
                if now >= SH_MS {
                    cut = Some(format!("the shell was still running at the limit of {} ms, {}", grouped(SH_MS), place(&out)));
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
    if !ek.done {
        ek.stop();
    }
    tee.flush();
    if tee.prompts < 2 && tee.unechoed.is_none() {
        note(format_args!(
            "{PROMPT_ID}: the shell printed {} prompt(s) and never the one after the line typed at its first, so nothing typed later was echoed after a prompt",
            tee.prompts
        ));
    }
    let _ = yield_until(500, || exit_of(sh).is_some());
    let exit = exit_of(sh);
    note(format_args!(
        "tee: /bin/sh (pid {}), {} bytes through the tee, {}; {}",
        sh.0,
        out.len(),
        status_text(exit),
        match (&cut, closed) {
            (Some(why), _) => why.as_str(),
            (None, true) => "every write end closed",
            (None, false) => "its output still open",
        }
    ));
    Ok(Session { sh, out, exit, cut })
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

/// Why the first line, which this file typed itself, did not print its word,
/// or `None` if it did: `segs[1]` is what the shell printed for it.
fn first_line_check(segs: &[&[u8]], word: &str) -> Option<String> {
    let mut want = Vec::from(word.as_bytes());
    want.push(b'\n');
    match segs.get(1) {
        Some(got) if *got == &want[..] => None,
        Some(got) => Some(format!(
            "the line the test typed at the first prompt (`echo `, a word with shifted keys, a mistyped key and a backspace, Enter) printed {:?}, expected {:?}",
            String::from_utf8_lossy(got),
            String::from_utf8_lossy(&want)
        )),
        None => Some(String::from("the shell never printed a second prompt after the line the test typed at the first one (`echo `, a word with shifted keys, a mistyped key and a backspace, Enter)")),
    }
}

/// The line typed at the second prompt is `echo `, a quote typed with shift
/// and the apostrophe key, `q`, the same quote, `1`, Enter; `segs[2]` is what
/// the shell printed for it. An apostrophe there means the key map gave `'`
/// for that shifted key, which the console then also echoed: the serial row
/// is red already, and this line says why, on the row's own prefix.
fn second_line_note(segs: &[&[u8]]) {
    if let Some(got) = segs.get(2).filter(|s| s.contains(&b'\'')) {
        note(format_args!(
            "{PROMPT_ID}: the shell printed {:?} for the line typed at the second prompt, whose quotes are typed with shift and the apostrophe key: that key reached it as an apostrophe, not as a double quote",
            String::from_utf8_lossy(got)
        ));
    }
}

/// A row fails, first, when the first line did not print its word.
fn gated(id: &'static str, pre: &Option<String>) -> Row {
    let mut r = Row::new(id);
    if let Some(why) = pre {
        r.fail(format_args!("{id}: {why}"));
    }
    r
}

fn lines(s: &[u8]) -> Vec<&str> {
    core::str::from_utf8(s).unwrap_or("").lines().collect()
}

fn hex16(v: u64) -> String {
    format!("{v:016x}")
}

// ── Rows ─────────────────────────────────────────────────────────────────

fn ls_row(s: &Session, segs: &[&[u8]], pre: &Option<String>) {
    let mut r = gated(LS_ID, pre);
    // q1 then: cd /procx<BS>, ls, pwd, echo $? "q"2.
    if let Some(g) = group(&mut r, segs, 2, 0, 3) {
        r.expect_out("cd /procx<backspace>", g[0], b"");
        let mut want: Vec<String> = PROC_TOP.iter().map(|s| String::from(*s)).collect();
        want.push(format!("{}", s.sh.0));
        want.push(format!("{}", s.sh.0 + 3));
        want.sort();
        let mut got: Vec<String> = lines(g[1]).iter().map(|s| String::from(*s)).collect();
        got.sort();
        if got != want {
            r.fail(format_args!(
                "{LS_ID}: `ls` in the cwd printed {:?}, expected /proc's names {want:?} (the shell is pid {}, the `ls` pid {}, the reaped `echo`s before it pids {} and {})",
                String::from_utf8_lossy(g[1]),
                s.sh.0,
                s.sh.0 + 3,
                s.sh.0 + 1,
                s.sh.0 + 2
            ));
        }
        r.expect_out("pwd", g[2], b"/proc\n");
    }
    r.finish(1);
}

fn redirect_row(segs: &[&[u8]], words: &Words, pre: &Option<String>) {
    let mut r = gated(REDIRECT_ID, pre);
    r.nonce();
    // cat /proc/sboot/nonce > /dev/null; cat < /dev/null; cd $SBOOT_DIR;
    // echo $SBOOT_WORD > k23n; cat < k23n; echo $? "q"3.
    if let Some(g) = group(&mut r, segs, 3, 0, 5) {
        r.expect_out("cat /proc/sboot/nonce > /dev/null", g[0], b"");
        r.expect_out("cat < /dev/null", g[1], b"");
        r.expect_out("cd $SBOOT_DIR", g[2], b"");
        r.expect_out("echo $SBOOT_WORD > k23n", g[3], b"");
        let mut want = Vec::from(words.word.as_bytes());
        want.push(b'\n');
        r.expect_out("cat < k23n", g[4], &want);
    }
    r.finish(2);
}

/// The stream `producer chain` writes for a nonce argument, as this file
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

fn pipe_row(segs: &[&[u8]], words: &Words, pre: &Option<String>) {
    let mut r = gated(PIPE_ID, pre);
    r.nonce();
    let (len, fnv) = stream_sum(&words.key);
    note(format_args!("{PIPE_ID}: derive(SBOOT_KEY, \"pipe.chain.checksum\"): the stream is {len} bytes, fnv1a64 {}", hex16(fnv)));
    if let Some(g) = group(&mut r, segs, 4, 0, 1) {
        let ls = lines(g[0]);
        for want in ["ok 1 - pipe.chain.checksum", "ok 2 - pipe.eof.last_writer", "ok 3 - pipe.cloexec"] {
            if !ls.contains(&want) {
                r.fail(format_args!("{PIPE_ID}: the pipeline printed no line `{want}`; it printed {:?}", String::from_utf8_lossy(g[0])));
            }
        }
        // `# pipe.chain.checksum: <total> of <len> bytes in <n> reads, fnv1a64 <got> want <want>`:
        // the bytes the consumer read and their sum, against this file's own
        // computation from the key (not the consumer's `want`).
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
                "{PIPE_ID}: the consumer's checksum line was {other:?} (bytes read, fnv1a64, its want), expected ({want_len:?}, {want_sum:?}, {want_sum:?}): the stream $SBOOT_KEY gives"
            )),
        }
    }
    r.finish(3);
}

fn hi_row(segs: &[&[u8]], words: &Words, pre: &Option<String>) {
    let mut r = gated(HI_ID, pre);
    r.nonce();
    let key = words.key;
    if let Some(g) = group(&mut r, segs, 5, 0, 1) {
        let n = String::from_utf8_lossy(&key).into_owned();
        let want = [
            format!("hi: cs=0x2b rpl=3 nonce={n}"),
            format!("# derive={}", hex16(ktap::derive(&key, "ring3.cs.rpl"))),
            String::from("ok 1 - ring3.cs.rpl"),
        ];
        let ls = lines(g[0]);
        for w in &want {
            if !ls.iter().any(|l| l == w) {
                r.fail(format_args!("{HI_ID}: `hi $SBOOT_KEY 1` printed no line `{w}`; it printed {:?}", String::from_utf8_lossy(g[0])));
            }
        }
    }
    r.finish(4);
}

fn status_row(segs: &[&[u8]], pre: &Option<String>) {
    let mut r = gated(STATUS_ID, pre);
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

fn remount_row(s: &Session, segs: &[&[u8]], ext2: &Arc<Ext2>, words: &Words, pre: &Option<String>) {
    let mut r = gated(REMOUNT_ID, pre);
    r.nonce();
    let dir = words.dir.as_str();
    // After q8: echo $SBOOT_KEY > k23, then exit.
    match marker(segs, 8) {
        Some((i, _)) if segs.len() == i + 3 => r.expect_out("echo $SBOOT_KEY > k23", segs[i + 1], b""),
        Some((i, _)) => r.fail(format_args!("{REMOUNT_ID}: {} prompts after marker 8, expected 2 (`echo $SBOOT_KEY > k23`, `exit`)", segs.len() - 1 - i)),
        None => r.fail(format_args!("{REMOUNT_ID}: not reached: the shell never printed marker 8")),
    }
    if s.exit != Some(0) {
        r.fail(format_args!("{REMOUNT_ID}: /bin/sh ended {}, expected exit(0) from `exit`", status_text(s.exit)));
    }
    if let Err(e) = quiet(|| ext2.sync()) {
        r.fail(format_args!("{REMOUNT_ID}: Ext2::sync() returned {}", errno_text(e)));
    }
    let read = mount_ext2().and_then(|fs2| {
        let root = quiet(|| fs2.root()).map_err(|e| format!("the second mount's root() returned {}", errno_text(e)))?;
        let f = quiet(|| root.lookup("tmp").and_then(|t| t.lookup(dir)).and_then(|d| d.lookup("k23")))
            .map_err(|e| format!("tmp/{dir}/k23 through the second mount: {}", errno_text(e)))?;
        let mut buf = [0u8; 64];
        let n = quiet(|| f.read_at(0, &mut buf)).map_err(|e| format!("reading tmp/{dir}/k23 through the second mount: {}", errno_text(e)))?;
        Ok(Vec::from(&buf[..n]))
    });
    let mut want = Vec::from(&words.key[..]);
    want.push(b'\n');
    match read {
        Ok(got) if got == want => {}
        Ok(got) => r.fail(format_args!(
            "{REMOUNT_ID}: tmp/{dir}/k23 read through a second mount of partition 2 held {:?}, expected {:?}",
            String::from_utf8_lossy(&got),
            String::from_utf8_lossy(&want)
        )),
        Err(why) => r.fail(format_args!("{REMOUNT_ID}: {why}")),
    }
    r.finish(6);
}

// ── Row 7: the cwd system calls, from a program of their own ─────────────

/// A syscall argument: an immediate, the result of an earlier step, or the
/// address of a place in the data segment.
#[derive(Clone, Copy)]
enum A {
    I(u64),
    R(usize),
    D(usize),
}

/// `mov <arg reg>, imm64` for rdi, rsi, rdx.
const MOV_IMM: [[u8; 2]; 3] = [[0x48, 0xbf], [0x48, 0xbe], [0x48, 0xba]];
/// `mov <arg reg>, [disp32]` (SIB, no base, no index).
const MOV_MEM: [[u8; 4]; 3] = [[0x48, 0x8b, 0x3c, 0x25], [0x48, 0x8b, 0x34, 0x25], [0x48, 0x8b, 0x14, 0x25]];

/// A straight run of syscalls; step k's `rax` is stored at `DATA + 8k`.
/// The program ends by writing its whole data segment to fd 1, then
/// `exit(0)` (ktest-22's probe).
struct Probe {
    code: Vec<u8>,
    data: Vec<u8>,
    steps: usize,
    max: usize,
}

impl Probe {
    fn new() -> Probe {
        Probe::sized(MAX_STEPS)
    }
    /// A program of at most `max` steps, the last its write.
    fn sized(max: usize) -> Probe {
        Probe { code: Vec::new(), data: alloc::vec![0; 8 * max], steps: 0, max }
    }
    fn str(&mut self, s: &str) -> A {
        let at = self.data.len();
        self.data.extend_from_slice(s.as_bytes());
        self.data.push(0);
        self.data.resize(self.data.len().next_multiple_of(8), 0);
        A::D(at)
    }
    /// `n` bytes of the data segment, each `byte`.
    fn filled(&mut self, n: usize, byte: u8) -> (A, usize) {
        let at = self.data.len();
        self.data.resize(at + n.next_multiple_of(8), byte);
        (A::D(at), at)
    }
    fn call(&mut self, nr: u64, args: &[A]) -> usize {
        for (i, a) in args.iter().enumerate() {
            match *a {
                A::I(v) => {
                    self.code.extend_from_slice(&MOV_IMM[i]);
                    self.code.extend_from_slice(&v.to_le_bytes());
                }
                A::D(off) => {
                    self.code.extend_from_slice(&MOV_IMM[i]);
                    self.code.extend_from_slice(&(DATA + off as u64).to_le_bytes());
                }
                A::R(k) => {
                    self.code.extend_from_slice(&MOV_MEM[i]);
                    self.code.extend_from_slice(&((DATA + 8 * k as u64) as u32).to_le_bytes());
                }
            }
        }
        self.code.push(0xb8); // mov eax, imm32
        self.code.extend_from_slice(&(nr as u32).to_le_bytes());
        self.code.extend_from_slice(&[0x0f, 0x05]); // syscall
        let k = self.steps;
        assert!(k < self.max, "a probe of more than {} steps", self.max);
        self.steps += 1;
        self.code.extend_from_slice(&[0x48, 0x89, 0x04, 0x25]); // mov [disp32], rax
        self.code.extend_from_slice(&((DATA + 8 * k as u64) as u32).to_le_bytes());
        k
    }
    /// A `fork` step: the child runs `child`'s steps, then `exit`s with the
    /// last one's result (`rax & 0xff`), and never reaches the steps after
    /// it, which the parent jumps to.
    fn fork(&mut self, child: impl FnOnce(&mut Probe)) -> usize {
        let k = self.call(SYS_FORK, &[]);
        // test rax, rax; jnz rel32 (the parent: a pid, or an error)
        self.code.extend_from_slice(&[0x48, 0x85, 0xc0, 0x0f, 0x85, 0, 0, 0, 0]);
        let from = self.code.len();
        child(self);
        // mov rdi, rax; mov eax, 60 (exit); syscall; ud2
        self.code.extend_from_slice(&[0x48, 0x89, 0xc7, 0xb8, 60, 0, 0, 0, 0x0f, 0x05, 0x0f, 0x0b]);
        let rel = (self.code.len() - from) as u32;
        self.code[from - 4..from].copy_from_slice(&rel.to_le_bytes());
        k
    }
    /// The ELF: text (R X) at `proc::BASE` from file offset 0, the data
    /// segment (RW) at `DATA` from the next page of the file.
    fn elf(mut self) -> (Vec<u8>, usize) {
        let len = self.data.len();
        // One write of at most 4096 bytes (a pipe's PIPE_BUF) goes in whole.
        assert!(len <= 4096, "row 7's data segment is {len} bytes, more than one pipe write");
        self.call(SYS_WRITE, &[A::I(1), A::I(DATA), A::I(len as u64)]);
        // xor edi, edi; mov eax, 60 (exit); syscall; ud2
        self.code.extend_from_slice(&[0x31, 0xff, 0xb8, 60, 0, 0, 0, 0x0f, 0x05, 0x0f, 0x0b]);
        let text = 176 + self.code.len();
        let data_off = text.next_multiple_of(0x1000);
        let mut b = alloc::vec![0u8; data_off + len];
        let mut put = |off: usize, v: u64, n: usize| b[off..off + n].copy_from_slice(&v.to_le_bytes()[..n]);
        put(0, 0x0101_0246_4c45_7f, 7); // \x7fELF, 64-bit, little-endian, version 1
        put(16, 2, 2); // ET_EXEC
        put(18, 0x3e, 2); // x86-64
        put(20, 1, 4);
        put(24, proc::BASE + 176, 8); // e_entry
        put(32, 64, 8); // e_phoff
        put(52, 64, 2);
        put(54, 56, 2);
        put(56, 2, 2); // e_phnum
        put(64, 1, 4); // PT_LOAD: the text
        put(68, 5, 4);
        put(80, proc::BASE, 8);
        put(88, proc::BASE, 8);
        put(96, text as u64, 8);
        put(104, text as u64, 8);
        put(112, 0x1000, 8);
        put(120, 1, 4); // PT_LOAD: the data
        put(124, 6, 4);
        put(128, data_off as u64, 8);
        put(136, DATA, 8);
        put(144, DATA, 8);
        put(152, len as u64, 8);
        put(160, len as u64, 8);
        put(168, 0x1000, 8);
        b[176..text].copy_from_slice(&self.code);
        b[data_off..].copy_from_slice(&self.data);
        (b, len)
    }
}

fn le64(b: &[u8], at: usize) -> u64 {
    let mut v = [0u8; 8];
    v.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(v)
}

/// The program `p`, written to ext2's `/tmp/k23-<what>-<drawn>` and run through
/// the learner's exec with a pipe as fds 1 and 2: its data segment as it
/// wrote it back.
fn run_probe(tmp: &InodeRef, what: &str, p: Probe) -> Result<Vec<u8>, String> {
    let (elf, len) = p.elf();
    // Drawn per run, so an image booted again never meets the last run's
    // file (lab 17's rule): a create that finds one is a failure.
    let n = ktap::nonce().unwrap_or([b'0'; 32]);
    let name = format!("k23-{what}-{:08x}", (ktap::derive(&n, "sh.probe.file") ^ unpublished()) & 0xffff_ffff);
    let f = quiet(|| tmp.create(&name, 0o755)).map_err(|e| format!("create(\"/tmp/{name}\") returned {}", errno_text(e)))?;
    match quiet(|| f.write_at(0, &elf)) {
        Ok(n) if n == elf.len() => {}
        other => return Err(format!("write_at of the program's {} bytes returned {other:?}", elf.len())),
    }
    let path = format!("/tmp/{name}");
    let (r, w) = pipe::open(0).map_err(|e| format!("pipe::open(0) returned {}", errno_text(e)))?;
    r.flags.store(r.flags.load(Ordering::SeqCst) | O_NONBLOCK, Ordering::SeqCst);
    let pid = start_with(&path, || exec::spawn(&path, &["probe"], &[]), &[(1, &w), (2, &w)]);
    drop(w);
    let pid = pid?;
    let mut out = Vec::new();
    let mut buf = alloc::vec![0u8; 4096];
    let mut c = Clock::new();
    loop {
        match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(11) if c.ms() < RUN_MS => sched::yield_now(),
            Err(11) => break,
            Err(e) => return Err(format!("a read of its pipe returned {}", errno_text(e))),
        }
    }
    let ms = c.ms();
    let _ = yield_until(500, || exit_of(pid).is_some());
    let exit = exit_of(pid);
    if exit != Some(0) || out.len() != len {
        let late = if ms >= RUN_MS { format!(", still running at the limit of {} ms", grouped(RUN_MS)) } else { String::new() };
        return Err(format!("the program wrote {} of its {len} bytes and ended {}{late}", out.len(), status_text(exit)));
    }
    Ok(out)
}

/// What one step of row 7's program must return.
enum Want {
    /// This value.
    Eq(i64),
    /// An fd: 0 or more.
    Fd,
    /// A child's pid: more than 0.
    Child,
    /// The result of the step named (the child's pid, for `wait4`).
    Same(usize),
    /// The path's length with its NUL, and the path and its NUL in the
    /// buffer of this size at this place of the data segment.
    Path(String, usize, usize),
}

/// Every path shape row 7 folds, on every run: which shapes are tried is
/// fixed, and only the names in them are drawn. Each is `(path, where it
/// lands)`, `<d>` for this run's directory and `<d>/<s>` for the
/// directory in it; the two relative ones are read from `<d>`. Where each
/// lands alternates, so a call that moves nothing never lands right.
fn fold_shapes(name: &str, sub: &str) -> [(String, bool); 12] {
    [
        (format!("./{sub}"), true),                    // a `.` at the start
        (format!("/tmp/./{name}"), false),             // a `.` in the middle
        (format!("{sub}/."), true),                    // a `.` at the end
        (format!("/./tmp/{name}"), false),             // a `.` right after `/`
        (format!("//tmp/{name}/{sub}"), true),         // `//` at the start
        (format!("/tmp//{name}"), false),              // `//` in the middle
        (format!("/tmp/{name}/{sub}//"), true),        // `//` at the end
        (format!("/tmp/{name}/"), false),              // a trailing `/`
        (format!("/../tmp/{name}/{sub}"), true),       // one `..` above `/`
        (format!("/../../tmp/{name}"), false),         // two
        (format!("/../../../tmp/{name}/{sub}"), true), // three
        (format!("/tmp/{name}/{sub}//.."), false),     // `<s>/..` beside a `//`
    ]
}

/// The parts row 7's walk paths are built from: an empty part (a leading
/// one makes the path absolute, a later one a `//` or a trailing `/`),
/// `.`, `..`, `tmp`, this run's `<d>` and `<s>`, and a name no directory
/// holds.
fn walk_parts<'a>(name: &'a str, sub: &'a str) -> [&'a str; 7] {
    ["", ".", "..", "tmp", name, sub, "nosuch"]
}

/// Every path of one to four parts, in a fixed order, then four longer
/// absolute ones and, once more, `../..` and `../../..`: which paths are
/// tried is fixed, and only `<d>` and `<s>` are drawn. Each is read from
/// the cwd `<d>`. Every `..` in them stays on `/`, `/tmp` and `<d>`, never
/// under `/proc` or `/dev`.
fn walk_paths(name: &str, sub: &str) -> Vec<String> {
    let parts = walk_parts(name, sub);
    let mut out = Vec::new();
    for k in 1..=4 {
        let mut ix = [0usize; 4];
        'odometer: loop {
            let names: Vec<&str> = ix[..k].iter().map(|&i| parts[i]).collect();
            out.push(names.join("/"));
            let mut j = k;
            loop {
                if j == 0 {
                    break 'odometer;
                }
                j -= 1;
                ix[j] += 1;
                if ix[j] < parts.len() {
                    break;
                }
                ix[j] = 0;
            }
        }
    }
    for p in [
        format!("/tmp/../../tmp/{name}"),         // a `..` above `/` after a pop
        format!("/tmp/{name}/{sub}/../../{name}"), // two `..` in a row, each popping a name
        format!("/tmp/{name}/{sub}/./.."),          // a `.` right before a `..`
        format!("/tmp/{name}/nosuch/.."),           // absolute: the walk before the fold
        String::from("../.."),
        String::from("../../.."),
    ] {
        out.push(p);
    }
    out
}

/// Where `chdir(path)` from the cwd `<d>` lands, walked in order as the
/// lesson walks it: `.` and an empty name stay, `..` goes back to where the
/// walk was before the last name (at `/` it stays), and a name must be
/// there. `None` is `-ENOENT`. `/` holds `tmp`, `/tmp` holds `<d>`, `<d>`
/// holds `<s>`, and none of them holds another of the parts.
fn walked(name: &str, sub: &str, path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let mut at: Vec<&str> = if path.starts_with('/') { Vec::new() } else { alloc::vec!["tmp", name] };
    for c in path.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                at.pop();
            }
            c => {
                let there = match at.as_slice() {
                    [] => c == "tmp",
                    ["tmp"] => c == name,
                    ["tmp", d] => *d == name && c == sub,
                    _ => false,
                };
                if !there {
                    return None;
                }
                at.push(c);
            }
        }
    }
    Some(if at.is_empty() { String::from("/") } else { format!("/{}", at.join("/")) })
}

/// One of row 7's walk programs and what each of its steps must return.
struct Walk {
    out: Result<Vec<u8>, String>,
    want: Vec<(usize, String, Want)>,
}

/// Row 7's walk programs: every path of `walk_paths`, each through `chdir`
/// from `<d>` against where `walked` lands, and each that lands through
/// `open` from `<d>` and then `fchdir`, as many programs as their data
/// segments need. A path that lands:
///   open(P, O_RDONLY)            an fd
///   chdir(P)                     0
///   getcwd(buf, 48)              where it lands
///   chdir("/") or chdir("<d>")   0: somewhere else
///   fchdir(fd)                   0
///   getcwd(buf, 48)              where it lands
///   close(fd)                    0
///   chdir("<d>")                 0: back to `<d>` for the next path
/// A path that does not: chdir(P) is -ENOENT, and the cwd stays `<d>`.
fn walk_probes(tmp: &InodeRef, name: &str, sub: &str) -> Vec<Walk> {
    let d = format!("/tmp/{name}");
    let paths = walk_paths(name, sub);
    let pad = |n: usize| (n + 1).next_multiple_of(8);
    // Each path's steps and data bytes: (path, where it lands, steps, bytes).
    let plan: Vec<(String, Option<String>, usize, usize)> = paths
        .into_iter()
        .map(|p| {
            let to = walked(name, sub, &p);
            let (steps, bufs) = if to.is_some() { (8, 96) } else { (1, 0) };
            let bytes = 8 * steps + bufs + pad(p.len());
            (p, to, steps, bytes)
        })
        .collect();
    let mut runs = Vec::new();
    let mut i = 0;
    let fixed = pad(d.len()) + pad(1);
    while i < plan.len() {
        // As many paths as one pipe write of the data segment holds.
        let (mut steps, mut bytes, mut j) = (1, 8 + fixed, i);
        while j < plan.len() && bytes + plan[j].3 + 8 <= 4096 {
            steps += plan[j].2;
            bytes += plan[j].3;
            j += 1;
        }
        let mut p = Probe::sized(steps + 1);
        let mut want = Vec::new();
        let at_d = p.str(&d);
        let at_root = p.str("/");
        want.push((p.call(SYS_CHDIR, &[at_d]), format!("chdir(\"{d}\")"), Want::Eq(0)));
        for (path, to, _, _) in &plan[i..j] {
            let a = p.str(path);
            match to {
                None => {
                    want.push((p.call(SYS_CHDIR, &[a]), format!("chdir(\"{path}\") with the cwd {d}"), Want::Eq(-2)));
                }
                Some(to) => {
                    let fd = p.call(SYS_OPEN, &[a, A::I(0), A::I(0)]);
                    want.push((fd, format!("open(\"{path}\", O_RDONLY) with the cwd {d}"), Want::Fd));
                    want.push((p.call(SYS_CHDIR, &[a]), format!("chdir(\"{path}\") with the cwd {d}"), Want::Eq(0)));
                    let (b, off) = p.filled(48, 0xff);
                    let k = p.call(SYS_GETCWD, &[b, A::I(48)]);
                    want.push((k, format!("getcwd(buf, 48) after chdir(\"{path}\") with the cwd {d}"), Want::Path(to.clone(), off, 48)));
                    let (away, there) = if to == "/" { (at_d, d.as_str()) } else { (at_root, "/") };
                    want.push((p.call(SYS_CHDIR, &[away]), format!("chdir(\"{there}\") with the cwd {to}"), Want::Eq(0)));
                    want.push((p.call(SYS_FCHDIR, &[A::R(fd)]), format!("fchdir(<the fd open(\"{path}\") returned>) with the cwd {there}"), Want::Eq(0)));
                    let (b, off) = p.filled(48, 0xff);
                    let k = p.call(SYS_GETCWD, &[b, A::I(48)]);
                    want.push((k, format!("getcwd(buf, 48) after fchdir onto the directory open(\"{path}\") opened from {d}"), Want::Path(to.clone(), off, 48)));
                    want.push((p.call(SYS_CLOSE, &[A::R(fd)]), format!("close(<the fd open(\"{path}\") returned>)"), Want::Eq(0)));
                    want.push((p.call(SYS_CHDIR, &[at_d]), format!("chdir(\"{d}\") with the cwd {to}"), Want::Eq(0)));
                }
            }
        }
        let out = run_probe(tmp, "walk", p);
        runs.push(Walk { out, want });
        i = j;
    }
    runs
}

/// Row 7's program, after the shell: a process the kernel starts, so its
/// cwd starts at `/` whatever the shell's was. `<d>` is this run's
/// directory (`/tmp/d…`, drawn per run), which holds a file `<f>` and a
/// directory `<s>` (each drawn per run) this file makes just before.
/// Every `..` stays on `/` and `<d>`, never under `/proc` or `/dev`.
///   getcwd(buf, 64)                    2, "/\0": its own cwd, not the shell's
///   chdir("")                          -ENOENT
///   chdir("/tmp")                      0
///   open("<name>", O_RDONLY)           an fd, by a relative path: A
///   open("/dev", O_RDONLY)             an fd: a second directory open
///   chdir("<name>/nosuch")             -ENOENT
///   getcwd(buf, 64)                    5, "/tmp\0": a failed chdir moves nothing
///   chdir("<d>/../<name>")             0: `..` folds away
///   getcwd(buf, 64)                    len+1, "<d>\0": the length counts the NUL
///   getcwd(buf, len)                   -ERANGE: one byte short
///   getcwd(buf, len+1)                 len+1, "<d>\0": exactly enough
///   open("<f>", O_RDONLY)              an fd on a file, by a relative path: B
///   chdir("<f>")                       -ENOTDIR: a file, relative to <d>
///   chdir("nosuch/..")                 -ENOENT: the walk meets `nosuch` before
///                                      the `..` that would fold it away
///   getcwd(buf, 64)                    "<d>\0"
///   open(<each fold shape>)            12 fds, with the cwd <d>
///   open(<each of three paths to <s>>) 3 fds: C1–C3, `./<s>/../<s>`,
///                                      `<s>/./../<s>/.` and `<s>/.././<s>`
///   chdir(<each fold shape>)           0, then getcwd(buf, 48): where it lands
///   chdir("/..")                       0: `..` at `/` stays at `/`
///   getcwd(buf, 48)                    2, "/\0"
///   chdir("/proc")                     0
///   fork()                             the child runs chdir("/"), then exits
///                                      with its result
///   wait4(child, &status, 0)           the child's pid, status 0
///   getcwd(buf, 48)                    6, "/proc\0": the child moved only itself
///   fchdir(A)                          0: neither the last cwd nor the last
///                                      directory opened
///   getcwd(buf, 48)                    len+1, "<d>\0"
///   fchdir(<each fold shape's fd>)     0, then getcwd(buf, 48): where it lands
///   fchdir(C1), fchdir(A), fchdir(C2), fchdir(A), fchdir(C3)
///                                      0, then getcwd(buf, 48): <d>/<s> or <d>
///   fchdir(B)                          -ENOTDIR: an fd on a file
///   getcwd(buf, 48)                    "<d>/<s>\0": a refused fchdir moves nothing
///   fchdir(1)                          -ENOTDIR: a pipe end has no path
struct CwdRun {
    out: Result<Vec<u8>, String>,
    want: Vec<(usize, String, Want)>,
    /// `wait4`'s step and where it wrote the child's status.
    status: (usize, usize),
    /// The walk programs (`walk_probes`), run after it.
    walks: Vec<Walk>,
}

fn cwd_probe(tmp: &InodeRef, name: &str) -> CwdRun {
    let n = ktap::nonce().unwrap_or([b'0'; 32]);
    let sub = format!("s{:06x}", (ktap::derive(&n, "sh.sub") ^ unpublished()) & 0xff_ffff);
    let file = format!("f{:06x}", (ktap::derive(&n, "sh.file") ^ unpublished()) & 0xff_ffff);
    let d = format!("/tmp/{name}");
    let ds = format!("{d}/{sub}");
    let made = quiet(|| {
        let dir = tmp.lookup(name)?;
        dir.create(&file, 0o644)?;
        dir.mkdir(&sub, 0o755).map(|_| ())
    });
    let lands = |deeper: bool| if deeper { ds.as_str() } else { d.as_str() };
    let len = d.len() as i64 + 1;
    let mut p = Probe::new();
    let mut want = Vec::new();
    let path = |p: &mut Probe, size: usize, call: String, at: &str, want: &mut Vec<(usize, String, Want)>| {
        let (b, off) = p.filled(size, 0xff);
        let k = p.call(SYS_GETCWD, &[b, A::I(size as u64)]);
        want.push((k, call, Want::Path(String::from(at), off, size)));
    };
    path(&mut p, 64, String::from("getcwd(buf, 64), the first call of a program the kernel started"), "/", &mut want);
    let a = p.str("");
    want.push((p.call(SYS_CHDIR, &[a]), String::from("chdir(\"\")"), Want::Eq(-2)));
    let a = p.str("/tmp");
    want.push((p.call(SYS_CHDIR, &[a]), String::from("chdir(\"/tmp\")"), Want::Eq(0)));
    let a = p.str(name);
    let fd = p.call(SYS_OPEN, &[a, A::I(0), A::I(0)]);
    want.push((fd, format!("open(\"{name}\", O_RDONLY) with the cwd /tmp"), Want::Fd));
    let a = p.str("/dev");
    want.push((p.call(SYS_OPEN, &[a, A::I(0), A::I(0)]), String::from("open(\"/dev\", O_RDONLY)"), Want::Fd));
    let a = p.str(&format!("{name}/nosuch"));
    want.push((p.call(SYS_CHDIR, &[a]), format!("chdir(\"{name}/nosuch\") with the cwd /tmp"), Want::Eq(-2)));
    path(&mut p, 64, format!("getcwd(buf, 64) after chdir(\"{name}/nosuch\")"), "/tmp", &mut want);
    let a = p.str(&format!("{d}/../{name}"));
    want.push((p.call(SYS_CHDIR, &[a]), format!("chdir(\"{d}/../{name}\")"), Want::Eq(0)));
    path(&mut p, 64, format!("getcwd(buf, 64) after chdir(\"{d}/../{name}\")"), &d, &mut want);
    let (b, _) = p.filled(64, 0xff);
    want.push((p.call(SYS_GETCWD, &[b, A::I(len as u64 - 1)]), format!("getcwd(buf, {}) with the cwd {d}", len - 1), Want::Eq(-34)));
    path(&mut p, len as usize, format!("getcwd(buf, {len}) with the cwd {d}"), &d, &mut want);
    let a = p.str(&file);
    let ffd = p.call(SYS_OPEN, &[a, A::I(0), A::I(0)]);
    want.push((ffd, format!("open(\"{file}\", O_RDONLY), a file, with the cwd {d}"), Want::Fd));
    want.push((p.call(SYS_CHDIR, &[a]), format!("chdir(\"{file}\"), a file, with the cwd {d}"), Want::Eq(-20)));
    // The walk comes before the fold: `nosuch` is missing, whatever follows it.
    let a = p.str("nosuch/..");
    want.push((p.call(SYS_CHDIR, &[a]), format!("chdir(\"nosuch/..\") with the cwd {d}"), Want::Eq(-2)));
    path(&mut p, 64, String::from("getcwd(buf, 64) after chdir(\"nosuch/..\")"), &d, &mut want);
    // Every fold shape, and the three paths to <s>, opened with the cwd <d>.
    let shapes = fold_shapes(name, &sub);
    let mut opened = Vec::new();
    for (sp, _) in &shapes {
        let a = p.str(sp);
        let k = p.call(SYS_OPEN, &[a, A::I(0), A::I(0)]);
        want.push((k, format!("open(\"{sp}\", O_RDONLY) with the cwd {d}"), Want::Fd));
        opened.push((a, k));
    }
    let mut to_sub = Vec::new();
    for c in [format!("./{sub}/../{sub}"), format!("{sub}/./../{sub}/."), format!("{sub}/.././{sub}")] {
        let a = p.str(&c);
        let k = p.call(SYS_OPEN, &[a, A::I(0), A::I(0)]);
        want.push((k, format!("open(\"{c}\", O_RDONLY) with the cwd {d}"), Want::Fd));
        to_sub.push((c, k));
    }
    let mut at = d.clone();
    for ((sp, deeper), (a, _)) in shapes.iter().zip(&opened) {
        want.push((p.call(SYS_CHDIR, &[*a]), format!("chdir(\"{sp}\") with the cwd {at}"), Want::Eq(0)));
        path(&mut p, 48, format!("getcwd(buf, 48) after chdir(\"{sp}\")"), lands(*deeper), &mut want);
        at = String::from(lands(*deeper));
    }
    let a = p.str("/..");
    want.push((p.call(SYS_CHDIR, &[a]), format!("chdir(\"/..\") with the cwd {at}"), Want::Eq(0)));
    path(&mut p, 48, String::from("getcwd(buf, 48) after chdir(\"/..\")"), "/", &mut want);
    let a = p.str("/proc");
    want.push((p.call(SYS_CHDIR, &[a]), String::from("chdir(\"/proc\")"), Want::Eq(0)));
    let root = p.str("/");
    let child = p.fork(|p| {
        p.call(SYS_CHDIR, &[root]);
    });
    want.push((child, String::from("fork() with the cwd /proc"), Want::Child));
    let (st, st_at) = p.filled(8, 0xff);
    let w = p.call(SYS_WAIT4, &[A::R(child), st, A::I(0)]);
    want.push((w, String::from("wait4(<the child's pid>, &status, 0)"), Want::Same(child)));
    path(&mut p, 48, String::from("getcwd(buf, 48) in the parent once its child, which ran chdir(\"/\"), had ended"), "/proc", &mut want);
    want.push((p.call(SYS_FCHDIR, &[A::R(fd)]), format!("fchdir(<the fd open(\"{name}\") returned>) with the cwd /proc"), Want::Eq(0)));
    path(&mut p, 48, String::from("getcwd(buf, 48) after that fchdir"), &d, &mut want);
    let mut at = d.clone();
    for ((sp, deeper), (_, k)) in shapes.iter().zip(&opened) {
        want.push((p.call(SYS_FCHDIR, &[A::R(*k)]), format!("fchdir(<the fd open(\"{sp}\") returned>) with the cwd {at}"), Want::Eq(0)));
        path(&mut p, 48, format!("getcwd(buf, 48) after fchdir onto the directory open(\"{sp}\") opened"), lands(*deeper), &mut want);
        at = String::from(lands(*deeper));
    }
    for (i, (c, k)) in to_sub.iter().enumerate() {
        if i > 0 {
            want.push((p.call(SYS_FCHDIR, &[A::R(fd)]), format!("fchdir(<the fd open(\"{name}\") returned>) with the cwd {ds}"), Want::Eq(0)));
            path(&mut p, 48, format!("getcwd(buf, 48) after fchdir onto {name} from {ds}"), &d, &mut want);
        }
        want.push((p.call(SYS_FCHDIR, &[A::R(*k)]), format!("fchdir(<the fd open(\"{c}\") returned>) with the cwd {d}"), Want::Eq(0)));
        path(&mut p, 48, format!("getcwd(buf, 48) after fchdir onto the directory open(\"{c}\") opened"), &ds, &mut want);
    }
    want.push((p.call(SYS_FCHDIR, &[A::R(ffd)]), format!("fchdir(<the fd open(\"{file}\") returned>), a file, with the cwd {ds}"), Want::Eq(-20)));
    path(&mut p, 48, format!("getcwd(buf, 48) after fchdir onto {file}"), &ds, &mut want);
    want.push((p.call(SYS_FCHDIR, &[A::I(1)]), String::from("fchdir(1) on a pipe end"), Want::Eq(-20)));
    let (out, walks) = match made {
        Err(e) => (Err(format!("making {d}/{file} and {d}/{sub} for it: {}", errno_text(e))), Vec::new()),
        Ok(()) => (run_probe(tmp, "cwd", p), walk_probes(tmp, name, &sub)),
    };
    CwdRun { out, want, status: (w, st_at), walks }
}

/// Each step of one row 7 program that did not return what it must: one
/// line per step, for `cwd_row` to print.
fn wrong_steps(out: &[u8], want: &[(usize, String, Want)], bad: &mut Vec<String>) {
    let res = |k: usize| le64(out, 8 * k) as i64;
    let exp = |v: i64| if v < 0 { errno_text(-v as Errno) } else { format!("{v}") };
    for (k, call, w) in want {
        let got = res(*k);
        let (good, wanted, shown) = match w {
            Want::Eq(v) => (got == *v, exp(*v), String::new()),
            Want::Fd => (got >= 0, String::from("an fd"), String::new()),
            Want::Child => (got > 0, String::from("the child's pid"), String::new()),
            Want::Same(j) => (got == res(*j), format!("{}, the child's pid", res(*j)), String::new()),
            Want::Path(p, at, size) => {
                let mut b = Vec::from(p.as_bytes());
                b.push(0);
                let t = &out[*at..*at + *size];
                // The buffer up to its first NUL, NUL included: the rest is fill.
                let m = t.iter().position(|&c| c == 0).map_or(t.len(), |i| i + 1);
                // A refusal leaves the buffer as it was: nothing to show.
                let shown = if got < 0 { String::new() } else { format!(" with {:?} in its buffer", String::from_utf8_lossy(&t[..m])) };
                (got == b.len() as i64 && t.get(..b.len()) == Some(&b[..]), format!("{} with \"{p}\\0\"", b.len()), shown)
            }
        };
        if !good {
            bad.push(format!("{call} returned {got}{shown}, expected {wanted}"));
        }
    }
}

/// How many of row 7's wrong steps it prints; the rest it counts.
const CWD_SHOWN: usize = 12;

fn cwd_row(run: CwdRun) {
    let mut r = Row::new(CWD_ID);
    r.nonce();
    let out = match run.out {
        Ok(out) => out,
        Err(why) => {
            r.fail(format_args!("{CWD_ID}: a program making the cwd system calls (open, chdir, getcwd, fork, wait4, fchdir): {why}"));
            return r.finish(7);
        }
    };
    let mut bad = Vec::new();
    wrong_steps(&out, &run.want, &mut bad);
    let (w, at) = run.status;
    let st = le64(&out, at) as u32;
    if le64(&out, 8 * w) as i64 > 0 && st != 0 {
        let code = ((st >> 8) & 0xff) as u8 as i8 as i64;
        let shown = if code < 0 { errno_text(-code as Errno) } else { format!("{code}") };
        bad.push(format!(
            "wait4 wrote the child's status as {st:#x}, expected 0; the child exits with what its chdir(\"/\") returned, which reads back as {shown}"
        ));
    }
    let total = run.walks.len();
    for (i, walk) in run.walks.iter().enumerate() {
        match &walk.out {
            Ok(out) => wrong_steps(out, &walk.want, &mut bad),
            Err(why) => bad.push(format!(
                "a program walking paths with chdir, open and fchdir (the {} of {total}): {why}",
                ordinal(i + 1)
            )),
        }
    }
    for b in bad.iter().take(CWD_SHOWN) {
        r.fail(format_args!("{CWD_ID}: {b}"));
    }
    if bad.len() > CWD_SHOWN {
        r.fail(format_args!("{CWD_ID}: and {} more calls that did not return what they must", bad.len() - CWD_SHOWN));
    }
    r.finish(7);
}

fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

fn not_run(why: &str) {
    note(format_args!("{PROMPT_ID}: not run: {why}"));
    for (n, id) in [LS_ID, REDIRECT_ID, PIPE_ID, HI_ID, STATUS_ID, REMOUNT_ID, CWD_ID].into_iter().enumerate() {
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
    let n = ktap::nonce().unwrap_or([b'0'; 32]);
    let unfinished = match translate_control(ktap::derive(&n, "kbd.translate") ^ unpublished()) {
        Err(why) => early(why),
        Ok(()) => match setup() {
            None => early(String::from("the four mounts, or ext2's /tmp, did not all succeed")),
            Some((ext2, tmp)) => {
                let words = words();
                match quiet(|| tmp.mkdir(&words.dir, 0o755)) {
                    Err(e) => early(format!("making this run's directory in /tmp: mkdir returned {}", errno_text(e))),
                    Ok(_) => {
                        let (line, word) = first_line(ktap::derive(&n, "sh.line") ^ unpublished());
                        match session(&words, &line) {
                            Err(why) => early(why),
                            Ok(s) => {
                                // Row 7's program runs after the shell, so `ls`
                                // in /proc sees no process of its own.
                                let cwd = cwd_probe(&tmp, &words.dir);
                                let segs = segments(&s.out);
                                let pre = first_line_check(&segs, &word);
                                second_line_note(&segs);
                                ls_row(&s, &segs, &pre);
                                redirect_row(&segs, &words, &pre);
                                pipe_row(&segs, &words, &pre);
                                hi_row(&segs, &words, &pre);
                                status_row(&segs, &pre);
                                remount_row(&s, &segs, &ext2, &words, &pre);
                                cwd_row(cwd);
                                unfinished(&s, &segs)
                            }
                        }
                    }
                }
            }
        },
    };
    sched::yield_now();
    if let Some(why) = unfinished {
        // The rows are out; the script can never finish, so end the capture
        // on the panic report rather than leave it to the rubric's timeout.
        panic!("the typed script cannot finish ({why}); the seven rows above are this run's result");
    }
    quiet(ktap::end);
    halt()
}
