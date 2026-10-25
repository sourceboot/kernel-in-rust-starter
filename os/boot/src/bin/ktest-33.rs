//! Lab 33's vehicle (`33-termios`): the terminal, through the course's
//! `ttyprobe` (`user/ttyprobe/ttyprobe.c`, layout musl) and busybox's `ash`
//! and `vi`, on the learner's kernel, with keys typed at the machine's
//! keyboard by the rubric's `[[script]]`.
//!
//! The disk is `ktest-33.img` as the course's image writer made it (CONTRACT
//! §A.7): lab 32's, plus `/bin/ttyprobe` (`TERMIOS_PROGRAMS`, from lab 33).
//! The vehicle mounts ext2 at `/` from `/dev/hda2` through the learner's
//! `Ata`, `partition2` and `Ext2::mount`; `/proc` and `/dev` are init's.
//!
//! It starts the learner's `/bin/init <nonce>` through the learner's exec,
//! first, so it is pid 1, as ktest-32 does: fd 0 a pipe this file writes the
//! script into, fds 1 and 2 a second pipe it drains (the tee, forwarded as
//! `# | <line>`, ESC as `\e` and CR as `\r`), and fd 3 the console,
//! close-on-exec. init forks and execs `/bin/sh` (busybox's `ash`,
//! non-interactive: its stdin is a pipe). Each command group ends with
//! `echo ':q<N>:'`, and the group's output is what came through the tee
//! before that marker. Every word in the script, and every file's bytes,
//! derive from the run's nonce (`ktap::derive`).
//!
//! The rows, printed in this order:
//!
//!   1 `tty.tcgets` (K + derived) — `Console::tcgets` (lab 33 step 1) is
//!     Linux's cooked termios, the 36 bytes of `tc::COOKED`; and
//!     `ttyprobe tcgets <hex>`: `ioctl(TCGETS)` on `/dev/tty` writes those
//!     36 bytes and not one more.
//!   2 `tty.tcsets` (derived) — `ttyprobe tcsets <hex>`: a termios set with
//!     `TCSETS` (ICANON and ECHO off, ICRNL flipped, VMIN and VTIME from the
//!     nonce) reads back the same, and so does the first, set again; after
//!     it the console's termios is cooked again (K).
//!   3 `tty.raw` (derived, keys) — `ttyprobe raw <hex>` turns ICANON, ECHO,
//!     ISIG and ICRNL off; the vehicle prints its raw-ready marker and the
//!     script types q, Enter, the left arrow and ctrl+c. Each comes as it is
//!     typed, with no line to wait for: `q`, CR (0x0d: ICRNL off), `ESC [ D`
//!     (`kbd::vt100`) and 0x03 (ISIG off: a byte, not SIGINT). Afterwards
//!     the termios is cooked again and no key is left over.
//!   4 `tty.access` (derived) — `ttyprobe access <hex>`: `access` (21) as
//!     root: what is there is readable and writable whatever its mode, only
//!     a file with an execute bit (or a directory) may be run (`-EACCES`
//!     otherwise), a missing name is `-ENOENT`, a mode above 7 `-EINVAL`.
//!     Any of the three execute bits will do (files made with the group's
//!     alone and other's alone), and a directory needs none (one made with
//!     mode 0) — amended 2026-10-23, slice 8 fix round.
//!   5 `tty.ftruncate` (derived) — `ttyprobe trunc <hex>`: `ftruncate` (77)
//!     cuts a 12,411-byte file to 100 (blocks freed, bytes kept, offset
//!     unmoved), grows it to 5000 (zeros), and refuses a negative length, a
//!     read-only open file and a pipe with `-EINVAL`.
//!   6 `tty.edit` (derived, keys) — an interactive `ash` on the console
//!     (`sh -c 'exec </dev/console; …; exec sh -i +m'`, its output still the
//!     tee) edits its line in raw mode (busybox's lineedit: `poll(-1)`, then
//!     one byte at a time). Once its prompt shows, the vehicle prints its
//!     edit-ready marker and the script types `cho $W:z`, Home, `e`, End,
//!     left, left, `-`, right, `y`, Enter: the command run is `echo
//!     $W-:yz`, so the line prints `<W>-:yz`.
//!   7 `tty.vi` (derived, keys) — at the next prompt (the vi-ready marker)
//!     the script types `vi $F`; `$F` holds a long line and a short one,
//!     both from the nonce. Once vi has drawn its screen (`ESC [ r ; c H`
//!     through the tee) the vehicle prints its vi-open marker and the script
//!     types `dd`, `G`, `o`, `vi`, Esc, `:wq`, Enter. vi asks `access(F,
//!     W_OK)` before it will write, writes over the file and cuts it with
//!     `ftruncate`: the file read back through the VFS is the short line and
//!     `vi`, nothing after.
//!   8 `tty.cooked.after` (derived, keys) — at the next prompt (the
//!     cooked-ready marker) the script types `read v`, Enter, `abx`,
//!     Backspace, `c`, Enter, `echo $v$W`, Enter and `exit`, Enter: ash's
//!     `read` reads the console cooked again, the line discipline's erase
//!     included, so it prints `abc<W>`; the shell exits and the console's
//!     termios is cooked (K).
//!
//! A group that prints no marker within `GROUP_MS`, or keys that do not
//! come within `KEYS_MS`, by the PM timer, stop the session: the rows still
//! owed fail with the reason. The plan always completes.
//!
//! Plan 8.

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
use kernel::console::{Console, Fd, FileObj, FileRef, CONSOLE};
use kernel::exec;
use kernel::fs::ext2::Ext2;
use kernel::fs::mount;
use kernel::fs::vfs::{self, InodeRef};
use kernel::ktap;
use kernel::pipe;
use kernel::proc::{self, Pid};
use kernel::syscall::{self, Errno};
use kernel::task::{self, Tid};
use kernel::{bootinfo, frame, gdt, heap, idt, irq, sched, serial, time};

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 64;
const PM_HZ: u64 = 3_579_545;

const PLAN: usize = 8;
const TCGETS_ID: &str = "tty.tcgets";
const TCSETS_ID: &str = "tty.tcsets";
const RAW_ID: &str = "tty.raw";
const ACCESS_ID: &str = "tty.access";
const TRUNC_ID: &str = "tty.ftruncate";
const EDIT_ID: &str = "tty.edit";
const VI_ID: &str = "tty.vi";
const COOKED_ID: &str = "tty.cooked.after";
const IDS: [&str; PLAN] = [TCGETS_ID, TCSETS_ID, RAW_ID, ACCESS_ID, TRUNC_ID, EDIT_ID, VI_ID, COOKED_ID];

/// How long a group may run before the session counts as stopped, by the
/// PM timer.
const GROUP_MS: u64 = 20_000;
/// How long the keys of one step of the script have to come and be acted
/// on, by the PM timer (ttyprobe's own wait for a key is 20 s).
const KEYS_MS: u64 = 40_000;
/// What the practice script and the rubric's `[[script]]` wait for before
/// they type (CONTRACT §D.7), one per step.
const RAW_READY: &str = "# sboot tty-raw-ready";
const EDIT_READY: &str = "# sboot tty-edit-ready";
const VI_READY: &str = "# sboot tty-vi-ready";
const VI_OPEN: &str = "# sboot tty-vi-open";
const COOKED_READY: &str = "# sboot tty-cooked-ready";

/// The learner's init (lab 28), and how long it has to print its hello line
/// and fork the shell, by the PM timer.
const INIT: &str = "/bin/init";
const INIT_MS: u64 = 10_000;

// errnos and flags, as literals: the vehicle builds at every cut.
const EAGAIN: Errno = 11;
const O_NONBLOCK: u32 = 0x800;

/// Linux's cooked termios (`tty_std_termios`) as `TCGETS` writes it:
/// c_iflag ICRNL|IXON, c_oflag OPOST|ONLCR, c_cflag B38400|CS8|CREAD|HUPCL,
/// c_lflag ISIG|ICANON|ECHO|ECHOE|ECHOK|ECHOCTL|ECHOKE|IEXTEN, c_line 0,
/// c_cc INIT_C_CC. The vehicle's own copy: CONTRACT §A.9, interface 18.
const COOKED: [u8; 36] = [
    0x00, 0x05, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0xbf, 0x04, 0x00, 0x00, 0x3b, 0x8a, 0x00, 0x00, 0x00, 0x03, 0x1c, 0x7f, 0x15, 0x04,
    0x00, 0x01, 0x00, 0x11, 0x13, 0x1a, 0x00, 0x12, 0x0f, 0x17, 0x16, 0x00, 0x00, 0x00,
];
/// What row 3's keys send in raw mode with ICRNL and ISIG off: `q`, CR, the
/// left arrow (`ESC [ D`), ctrl+c (0x03).
const RAW_KEYS: &str = "710d1b5b4403";

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

fn halt() -> ! {
    loop {
        // SAFETY: interrupts off, then wait: nothing runs after this.
        unsafe { asm!("cli", "hlt", options(nomem, nostack)) };
    }
}

fn note(args: core::fmt::Arguments) {
    quiet(|| ktap::note(args));
}

fn errno_text(e: Errno) -> String {
    let name = match e {
        2 => "ENOENT",
        3 => "ESRCH",
        4 => "EINTR",
        5 => "EIO",
        9 => "EBADF",
        11 => "EAGAIN",
        12 => "ENOMEM",
        13 => "EACCES",
        14 => "EFAULT",
        20 => "ENOTDIR",
        22 => "EINVAL",
        25 => "ENOTTY",
        32 => "EPIPE",
        38 => "ENOSYS",
        _ => "?",
    };
    format!("-{e} ({name})")
}

// ── Rows ─────────────────────────────────────────────────────────────────

struct Row {
    id: &'static str,
    failed: bool,
}

impl Row {
    fn new(id: &'static str) -> Row {
        Row { id, failed: false }
    }
    fn fail(&mut self, why: core::fmt::Arguments) {
        self.failed = true;
        note(format_args!("{}: {why}", self.id));
    }
    fn finish(mut self, n: usize) {
        if ktap::nonce().is_none() {
            self.fail(format_args!("derived from the run's nonce, and this run has none"));
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

/// Rows `from..=PLAN`, failed with `why`.
fn fail_rest(from: usize, why: &str) {
    for n in from..=PLAN {
        let mut r = Row::new(IDS[n - 1]);
        r.fail(format_args!("{why}"));
        r.finish(n);
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

fn status_text(s: Option<i32>) -> String {
    match s {
        None => String::from("still running"),
        Some(v) if v & 0x7f == 0 => format!("exit({})", (v >> 8) & 0xff),
        Some(v) => format!("killed by signal {} (status {v:#x})", v & 0x7f),
    }
}

/// A live process whose argv begins with `argv`.
fn live_with(argv: &[&str]) -> Option<Pid> {
    let mut out = None;
    quiet(|| {
        proc::each(|p| {
            if p.exit.is_none() && p.argv.len() >= argv.len() && p.argv.iter().zip(argv).all(|(a, b)| a == b) {
                out = Some(p.pid);
            }
        })
    });
    out
}

/// The live child of `init` running `sh` (argv[0]).
fn shell_of(init: Pid) -> Option<Pid> {
    let mut out = None;
    quiet(|| {
        proc::each(|p| {
            if p.ppid == init && p.exit.is_none() && p.argv.first().map(String::as_str) == Some("sh") {
                out = Some(p.pid);
            }
        })
    });
    out
}

// ── Mounting ─────────────────────────────────────────────────────────────

/// ext2 at `/`, nothing else: `/proc` and `/dev` are init's to mount
/// (lab 28).
fn mount_all() -> Result<(), String> {
    let drive: Arc<dyn BlockDevice> = Arc::new(quiet(Ata::probe).map_err(|e| format!("Ata::probe() returned {}", errno_text(e)))?);
    let fs = quiet(|| ata::partition2(drive))
        .and_then(Ext2::mount)
        .map_err(|e| format!("partition2 then Ext2::mount returned {}", errno_text(e)))?;
    let fs = Arc::new(fs);
    let root: InodeRef = quiet(|| fs.root()).map_err(|e| format!("Ext2::root() returned {}", errno_text(e)))?;
    mount::mount("/dev/hda2", "/", "ext2", root).map_err(|e| format!("mount::mount(\"/dev/hda2\", \"/\", \"ext2\", its root) returned {}", errno_text(e)))
}

// ── The words ────────────────────────────────────────────────────────────

/// The words of this run, all from its nonce.
struct Words {
    nonce: [u8; 32],
    tcgets: u64,
    tcsets: u64,
    raw: u64,
    access: u64,
    trunc: u64,
    /// `$W`, `$F` and the prompt of the interactive shell.
    w: String,
    f: String,
    prompt: String,
    /// `$F`'s two lines: a long one, then a short one.
    long: String,
    short: String,
}

fn words() -> Words {
    let n = ktap::nonce().unwrap_or([b'0'; 32]);
    let d = |id: &str| ktap::derive(&n, id);
    let mut long = String::new();
    for k in 0..10 {
        long.push_str(&format!("{:016x}", d(&format!("tty.long/{k}"))));
    }
    Words {
        nonce: n,
        tcgets: d("tty.tcgets"),
        tcsets: d("tty.tcsets"),
        raw: d("tty.raw"),
        access: d("tty.access"),
        trunc: d("tty.trunc"),
        w: format!("w{:08x}", d("tty.w") & 0xffff_ffff),
        f: format!("/tmp/v{:08x}", d("tty.f") & 0xffff_ffff),
        prompt: format!("p{:06x}> ", d("tty.prompt") & 0xff_ffff),
        long,
        short: format!("l{:08x}", d("tty.short") & 0xffff_ffff),
    }
}

fn hex(b: &[u8]) -> String {
    let mut s = String::new();
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    let mut n = 0;
    let mut at = 0;
    while let Some(p) = find(&hay[at..], needle) {
        n += 1;
        at += p + needle.len();
    }
    n
}

/// Whether `b` holds a cursor move, `ESC [ <row> ; <col> H`.
fn has_cursor_move(b: &[u8]) -> bool {
    let mut i = 0;
    while let Some(p) = find(&b[i..], b"\x1b[") {
        let s = &b[i + p + 2..];
        let r = s.iter().take_while(|c| c.is_ascii_digit()).count();
        if r > 0 && s.get(r) == Some(&b';') {
            let c = s[r + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
            if c > 0 && s.get(r + 1 + c) == Some(&b'H') {
                return true;
            }
        }
        i += p + 2;
    }
    false
}

// ── Lab 33 step 1, through a twin ────────────────────────────────────────

/// `Console::tcgets` (lab 33 step 1) arrives with lab 33. This twin lets
/// the file build against the cuts before it and the learner's template
/// (G1380's class): the inherent method wins once the cut has it, and
/// before that this one answers 36 zero bytes, so row 1 fails as it should.
#[allow(dead_code)]
trait TcgetsOf {
    fn tcgets(&self) -> [u8; 36];
}
impl TcgetsOf for Console {
    fn tcgets(&self) -> [u8; 36] {
        [0; 36]
    }
}

fn termios() -> [u8; 36] {
    quiet(|| CONSOLE.tcgets())
}

/// The console's termios against the cooked one, with what differs.
fn cooked_check(r: &mut Row, when: &str) {
    let t = termios();
    if t != COOKED {
        r.fail(format_args!(
            "{when}, Console::tcgets() is {}; the console is cooked again, {} (whatever set another termios set this one back)",
            hex(&t),
            hex(&COOKED)
        ));
    }
}

// ── The session ──────────────────────────────────────────────────────────

/// The tee's bytes as notes, a line each, ESC and CR spelled out.
fn forward(bytes: &[u8]) {
    for line in bytes.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
        let mut s = String::new();
        for &b in line {
            match b {
                0x1b => s.push_str("\\e"),
                b'\r' => s.push_str("\\r"),
                0x20..=0x7e | b'\t' => s.push(b as char),
                _ => s.push_str(&format!("\\x{b:02x}")),
            }
        }
        note(format_args!("| {s}"));
    }
}

/// The shell, its two pipes, and everything it has printed.
struct Sh {
    init: Pid,
    pid: Pid,
    input: FileRef,
    output: FileRef,
    out: Vec<u8>,
    /// Why the session stopped, once it has.
    stopped: Option<String>,
}

impl Sh {
    /// `/bin/init <nonce>` spawned first, so it is pid 1, with fd 0 the
    /// script pipe, fds 1 and 2 the tee and fd 3 the console (close-on-exec);
    /// then its hello line, and the child it forks running `sh`.
    fn start(w: &Words) -> Result<Sh, String> {
        let (ir, iw) = pipe::open(0).map_err(|e| format!("pipe::open(0) returned {}", errno_text(e)))?;
        let (or, ow) = pipe::open(0).map_err(|e| format!("pipe::open(0) returned {}", errno_text(e)))?;
        or.flags.store(or.flags.load(Ordering::SeqCst) | O_NONBLOCK, Ordering::SeqCst);
        let con = FileRef::open(FileObj::CONSOLE, 2).map_err(|e| format!("FileRef::open(the console) returned {}", errno_text(e)))?;
        let nonce = String::from_utf8_lossy(&w.nonce).into_owned();
        let fds = [(0, ir.clone(), false), (1, ow.clone(), false), (2, ow.clone(), false), (3, con.clone(), true)];
        let started = quiet(|| -> Result<(Pid, Vec<Fd>), String> {
            let pid = exec::spawn(INIT, &["init", &nonce], &[]).map_err(|e| format!("{INIT}: starting it returned {}", errno_text(e)))?;
            let Some(tid) = task_of(pid) else {
                return Err(format!("{INIT}: pid {} has no task", pid.0));
            };
            let back = task::with_table(|tt| {
                let mut back = Vec::new();
                if let Some(t) = tt.get_mut(tid) {
                    for (n, f, cloexec) in fds {
                        if let Ok(Some(old)) = t.fds.install(n, Fd { file: f, cloexec }) {
                            back.push(old);
                        }
                    }
                }
                back
            });
            Ok((pid, back))
        });
        drop((ir, ow, con));
        let init = match started {
            Ok((pid, back)) => {
                drop(back);
                pid
            }
            Err(why) => return Err(why),
        };
        let mut sh = Sh { init, pid: Pid(0), input: iw, output: or, out: Vec::new(), stopped: None };
        let mut c = Clock::new();
        loop {
            sh.pump();
            if sh.pid.0 == 0 {
                sh.pid = shell_of(init).unwrap_or(Pid(0));
            }
            let hello = sh.out.split(|&b| b == b'\n').rev().skip(1).any(|l| l.starts_with(b"init: "));
            if hello && sh.pid.0 != 0 {
                forward(&sh.out);
                return Ok(sh);
            }
            if exit_of(init).is_some() || c.ms() >= INIT_MS {
                forward(&sh.out);
                return Err(format!(
                    "{INIT} (pid {}) {}: {} within {INIT_MS} ms by the PM timer",
                    init.0,
                    status_text(exit_of(init)),
                    if hello { "it printed its hello line but no child of it ran `sh`" } else { "no `init: ` line came" }
                ));
            }
            sched::yield_now();
        }
    }

    /// Whatever the tee holds now, appended to `out`.
    fn pump(&mut self) -> Option<Errno> {
        let mut buf = [0u8; 1024];
        loop {
            match self.output.read(&mut buf) {
                Ok(0) => return Some(0),
                Ok(k) => self.out.extend_from_slice(&buf[..k]),
                Err(EAGAIN) => return None,
                Err(e) => return Some(e),
            }
        }
    }

    fn send(&mut self, text: &str) -> Result<(), String> {
        if let Some(why) = &self.stopped {
            return Err(why.clone());
        }
        if let Err(e) = self.input.write(text.as_bytes()) {
            let why = format!("writing the script to the shell's stdin returned {}", errno_text(e));
            self.stopped = Some(why.clone());
            return Err(why);
        }
        Ok(())
    }

    /// Write one group's script, then read until its marker, for at most
    /// `ms`; the group's output, or why the session stopped.
    fn group(&mut self, n: usize, script: &str, ms: u64) -> Result<Vec<u8>, String> {
        let from = self.out.len();
        self.send(&format!("{script}echo ':q{n}:'\n"))?;
        self.collect(n, from, ms)
    }

    /// Read until group `n`'s marker, which the script already holds.
    fn collect(&mut self, n: usize, from: usize, ms: u64) -> Result<Vec<u8>, String> {
        let marker = format!(":q{n}:\n");
        let mut c = Clock::new();
        loop {
            let seg = &self.out[from..];
            let at = if seg.starts_with(marker.as_bytes()) {
                Some(0)
            } else {
                seg.windows(marker.len() + 1).position(|w| w[0] == b'\n' && &w[1..] == marker.as_bytes()).map(|p| p + 1)
            };
            if let Some(at) = at {
                let got = seg[..at].to_vec();
                forward(&got);
                return Ok(got);
            }
            match self.pump() {
                Some(0) => {
                    let why = format!("the shell closed its output during group {n} ({}), after {} bytes of it", status_text(exit_of(self.pid)), self.out.len() - from);
                    forward(&self.out[from..]);
                    self.stopped = Some(why.clone());
                    return Err(why);
                }
                Some(e) => {
                    let why = format!("a read of the shell's output returned {}", errno_text(e));
                    self.stopped = Some(why.clone());
                    return Err(why);
                }
                None => {}
            }
            if c.ms() >= ms {
                let why = format!("group {n} printed no marker within {ms} ms by the PM timer ({} bytes of it); the session stopped there", self.out.len() - from);
                forward(&self.out[from..]);
                self.stopped = Some(why.clone());
                return Err(why);
            }
            sched::yield_now();
        }
    }

    /// Pump the tee until `done` holds, for at most `ms`; whether it did.
    fn until(&mut self, ms: u64, mut done: impl FnMut(&Sh) -> bool) -> bool {
        let mut c = Clock::new();
        loop {
            self.pump();
            if done(self) {
                return true;
            }
            if c.ms() >= ms {
                return false;
            }
            sched::yield_now();
        }
    }
}

/// The group's output against the bytes expected.
fn expect(r: &mut Row, n: usize, got: &Result<Vec<u8>, String>, want: &str, what: &str) {
    match got {
        Err(why) => r.fail(format_args!("{why}")),
        Ok(g) if g.as_slice() != want.as_bytes() => r.fail(format_args!(
            "group {n} printed {:?}; on Linux the same commands print {:?} — {what}",
            String::from_utf8_lossy(g),
            want
        )),
        Ok(_) => {}
    }
}

/// One ttyprobe group's output against the line expected.
fn probe(sh: &mut Sh, n: usize, mode: &str, nonce: u64, want: &str, what: &str) -> Row {
    let g = sh.group(n, &format!("ttyprobe {mode} {nonce:016x}\n"), GROUP_MS);
    let mut r = Row::new(IDS[n - 1]);
    expect(&mut r, n, &g, want, what);
    r
}

/// Rows 1-8. Returns early, after failing what is left, when the session
/// cannot go on.
fn session(w: &Words) {
    // Row 1, the kernel's half first: no program has touched the termios.
    let k_tcgets = termios();
    let mut sh = match Sh::start(w) {
        Ok(sh) => sh,
        Err(why) => return fail_rest(1, &format!("no shell ran: {why}")),
    };
    note(format_args!("session: init is pid {}, and /bin/sh, its child, pid {}; the script's words are derived from the nonce", sh.init.0, sh.pid.0));

    let mut r = probe(
        &mut sh,
        1,
        "tcgets",
        w.tcgets,
        &format!("ttyprobe tcgets {} {:016x}\n", hex(&COOKED), w.tcgets),
        "ioctl(TCGETS) on the console writes its termios, Linux's 36-byte struct termios (c_iflag, c_oflag, c_cflag, c_lflag, c_line, c_cc[19]) with the cooked defaults, and no byte past it",
    );
    if k_tcgets != COOKED {
        r.fail(format_args!(
            "before any program ran, Console::tcgets() was {}; the console starts cooked, Linux's tty_std_termios: {}",
            hex(&k_tcgets),
            hex(&COOKED)
        ));
    }
    r.finish(1);

    let mut r = probe(
        &mut sh,
        2,
        "tcsets",
        w.tcsets,
        &format!("ttyprobe tcsets ok {:016x}\n", w.tcsets),
        "ioctl(TCSETS) replaces the console's termios with the 36 bytes given, every one of them, so TCGETS reads back what was set",
    );
    cooked_check(&mut r, "after `ttyprobe tcsets` set the first termios back");
    r.finish(2);

    if !raw_row(&mut sh, w) {
        return fail_rest(4, &format!("not run: {}", sh.stopped.clone().unwrap_or_default()));
    }

    probe(
        &mut sh,
        4,
        "access",
        w.access,
        &format!("ttyprobe access ok {:016x}\n", w.access),
        "access (21) answers for root: F_OK, R_OK and W_OK for anything there, X_OK for any directory (mode 0 too) and for a file with any of its three execute bits set, the group's or other's alone included (-EACCES otherwise), -ENOENT for a missing name, -EINVAL for a mode above 7",
    )
    .finish(4);

    probe(
        &mut sh,
        5,
        "trunc",
        w.trunc,
        &format!("ttyprobe trunc ok {:016x}\n", w.trunc),
        "ftruncate (77) sets a regular file's length: a cut frees the blocks past it and keeps the bytes before, a growth reads as zeros, the offset does not move, and a negative length, a file not open for writing or a pipe is -EINVAL",
    )
    .finish(5);

    // `$F` for row 7: a long line, then a short one.
    let g = sh.group(6, &format!("printf '%s\\n%s\\n' {} {} > {}\n", w.long, w.short, w.f), GROUP_MS);
    if let Err(why) = g {
        return fail_rest(6, &format!("not run: {why}"));
    }
    if let Ok(g) = &g {
        if !g.is_empty() {
            return fail_rest(6, &format!("not run: writing {} with printf printed {:?}", w.f, String::from_utf8_lossy(g)));
        }
    }
    interactive(&mut sh, w);
}

/// Row 3: raw keys. False when the session cannot go on.
fn raw_row(sh: &mut Sh, w: &Words) -> bool {
    let n = 3;
    let mut r = Row::new(RAW_ID);
    let from = sh.out.len();
    let ready = format!("ttyprobe raw ready {:016x}\n", w.raw);
    if let Err(why) = sh.send(&format!("ttyprobe raw {:016x}\necho ':q{n}:'\n", w.raw)) {
        r.fail(format_args!("{why}"));
        r.finish(n);
        return false;
    }
    if !sh.until(GROUP_MS, |s| find(&s.out[from..], ready.as_bytes()).is_some() || find(&s.out[from..], b":q3:\n").is_some()) {
        forward(&sh.out[from..]);
        r.fail(format_args!("`ttyprobe raw` printed no ready line within {GROUP_MS} ms by the PM timer"));
        sh.stopped = Some(String::from("row 3's ttyprobe never got ready"));
        r.finish(n);
        return false;
    }
    quiet(|| kernel::kprintln!("{RAW_READY}"));
    let g = sh.collect(n, from, KEYS_MS);
    let want = format!("{ready}ttyprobe raw {RAW_KEYS} {:016x}\n", w.raw);
    match &g {
        Ok(got) if got.as_slice() != want.as_bytes() => {
            let text = String::from_utf8_lossy(got);
            let keys = text.lines().find_map(|l| l.strip_prefix("ttyprobe raw ").filter(|k| !k.starts_with("ready")).map(|k| String::from(k.split(' ').next().unwrap_or(""))));
            match keys {
                Some(k) => r.fail(format_args!(
                    "with ICANON, ECHO, ISIG and ICRNL off and VMIN 1, the keys q, Enter, left, ctrl+c read as {k:?}; on Linux they are {RAW_KEYS:?}: each byte as it comes, Enter a CR (0x0d, ICRNL off), the left arrow ESC [ D, ctrl+c the byte 0x03 (ISIG off: no SIGINT)"
                )),
                None => r.fail(format_args!(
                    "group {n} printed {text:?}; on Linux it prints {want:?} — ttyprobe ended without its result line (ctrl+c with ISIG off is a byte, not SIGINT)"
                )),
            }
        }
        Ok(_) => {}
        Err(why) => r.fail(format_args!("{why}")),
    }
    cooked_check(&mut r, "after `ttyprobe raw` set its first termios back");
    // Nothing typed is left over.
    match FileRef::open(FileObj::CONSOLE, O_NONBLOCK) {
        Ok(con) => {
            let mut buf = [0u8; 64];
            match con.read(&mut buf) {
                Err(EAGAIN) => {}
                Ok(k) => r.fail(format_args!(
                    "after row 3 a non-blocking read of the console returned {:?}: ttyprobe read every key typed, so nothing is waiting (-EAGAIN)",
                    String::from_utf8_lossy(&buf[..k])
                )),
                Err(e) => r.fail(format_args!("a non-blocking read of the console after row 3 returned {}; nothing is waiting, so -EAGAIN", errno_text(e))),
            }
        }
        Err(e) => r.fail(format_args!("FileRef::open(the console, O_NONBLOCK) returned {}", errno_text(e))),
    }
    r.finish(n);
    g.is_ok()
}

/// The bytes of `path`, read through the VFS.
fn read_file(path: &str) -> Result<Vec<u8>, Errno> {
    let ino = quiet(|| vfs::walk(&vfs::root()?, path))?;
    let mut out = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        let k = quiet(|| ino.read_at(out.len() as u64, &mut buf))?;
        if k == 0 {
            return Ok(out);
        }
        out.extend_from_slice(&buf[..k]);
    }
}

/// Rows 6-8: the interactive shell on the console.
fn interactive(sh: &mut Sh, w: &Words) {
    let from = sh.out.len();
    let p = w.prompt.as_bytes();
    let cmd = format!("sh -c 'exec </dev/console; export W={} F={} PS1=\"{}\"; exec sh -i +m'\necho ':q7:'\n", w.w, w.f, w.prompt);
    if let Err(why) = sh.send(&cmd) {
        return fail_rest(6, &format!("not run: {why}"));
    }
    let up = sh.until(KEYS_MS, |s| live_with(&["sh", "-i"]).is_some() && count(&s.out[from..], p) >= 1);
    if !up {
        forward(&sh.out[from..]);
        let why = if count(&sh.out[from..], p) >= 1 {
            format!(
                "the interactive shell on the console (`exec sh -i +m` with stdin /dev/console) printed its prompt {:?} and then ended: its line editor waits for each key with poll(-1), and a poll that returns at once with nothing ready reads as the end of its input",
                w.prompt
            )
        } else {
            format!(
                "the interactive shell on the console (`exec sh -i +m` with stdin /dev/console) showed no prompt {:?} within {KEYS_MS} ms by the PM timer: busybox's line editor reads TCGETS, sets its raw termios with TCSETS, then prints the prompt",
                w.prompt
            )
        };
        return fail_rest(6, &why);
    }
    let inner = live_with(&["sh", "-i"]).unwrap_or(Pid(0));
    note(format_args!("the interactive shell is pid {}; its prompt is {:?}", inner.0, w.prompt));
    quiet(|| kernel::kprintln!("{EDIT_READY}"));

    // Row 6: the line, edited.
    let mut r = Row::new(EDIT_ID);
    let out6 = format!("{}-:yz\n", w.w);
    let mut at6 = 0;
    let done = sh.until(KEYS_MS, |s| match find(&s.out[from..], out6.as_bytes()) {
        Some(at) => count(&s.out[from + at..], p) >= 1,
        None => false,
    });
    if let Some(at) = find(&sh.out[from..], out6.as_bytes()) {
        at6 = from + at;
    }
    forward(&sh.out[from..]);
    if !done {
        r.fail(format_args!(
            "the keys `cho $W:z`, Home, `e`, End, left, left, `-`, right, `y`, Enter did not make the shell print {out6:?} and a prompt within {KEYS_MS} ms: in raw mode each key reaches the line editor as it is typed, Home ESC [ 1 ~, End ESC [ 4 ~, left ESC [ D, right ESC [ C, and it waits for each with poll(-1)"
        ));
        r.finish(6);
        sh.stopped = Some(String::from("row 6's line never ran"));
        return fail_rest(7, "not run: the interactive shell did not run row 6's line");
    }
    r.finish(6);
    let from7 = sh.out.len().max(at6);
    quiet(|| kernel::kprintln!("{VI_READY}"));

    // Row 7: vi.
    let mut r = Row::new(VI_ID);
    let f = w.f.as_bytes();
    let opened = sh.until(KEYS_MS, |s| live_with(&["vi"]).is_some() && find(&s.out[from7..], f).is_some() && has_cursor_move(&s.out[from7..]));
    if !opened {
        forward(&sh.out[from7..]);
        let why = if live_with(&["vi"]).is_none() {
            format!("`vi $F` did not run within {KEYS_MS} ms")
        } else {
            format!("vi ran but drew no screen naming {} with a cursor move (ESC [ row ; col H) within {KEYS_MS} ms", w.f)
        };
        r.fail(format_args!("{why}"));
        r.finish(7);
        sh.stopped = Some(String::from("row 7's vi never opened"));
        return fail_rest(8, "not run: vi did not open");
    }
    let vi = live_with(&["vi"]).unwrap_or(Pid(0));
    quiet(|| kernel::kprintln!("{VI_OPEN}"));
    let base = count(&sh.out[from7..], p);
    let closed = sh.until(KEYS_MS, |s| live_with(&["vi"]).is_none() && count(&s.out[from7..], p) > base);
    let shown = sh.out[from7..].to_vec();
    forward(&shown);
    let want = format!("{}\nvi\n", w.short);
    if !closed {
        r.fail(format_args!(
            "vi (pid {}) {} and the shell showed no prompt within {KEYS_MS} ms of `dd`, `G`, `o`, `vi`, Esc, `:wq`, Enter",
            vi.0,
            if live_with(&["vi"]).is_some() { "is still running" } else { "ended" }
        ));
    }
    if find(&shown, b"[Readonly]").is_some() {
        r.fail(format_args!("vi opened {} [Readonly]: it asks access(F, W_OK) first, and for root a file that is there is writable (access 21 answers 0)", w.f));
    }
    match read_file(&w.f) {
        Ok(got) if got == want.as_bytes() => {}
        Ok(got) => {
            let hint = if got.starts_with(want.as_bytes()) {
                " — the new bytes are right, and the old file's tail is still after them: vi writes over the file, then cuts it to its length with ftruncate (77)"
            } else {
                ""
            };
            r.fail(format_args!("after `:wq`, {} holds {:?} ({} bytes); vi saves {:?}{hint}", w.f, String::from_utf8_lossy(&got), got.len(), want));
        }
        Err(e) => r.fail(format_args!("reading {} back through the VFS returned {}", w.f, errno_text(e))),
    }
    r.finish(7);
    if !closed {
        sh.stopped = Some(String::from("row 7's vi did not end"));
        return fail_rest(8, "not run: vi did not end");
    }
    let from8 = sh.out.len();
    quiet(|| kernel::kprintln!("{COOKED_READY}"));

    // Row 8: cooked again.
    let mut r = Row::new(COOKED_ID);
    let out8 = format!("abc{}\n", w.w);
    let done = sh.until(KEYS_MS, |s| find(&s.out[from8..], out8.as_bytes()).is_some() && live_with(&["sh", "-i"]).is_none());
    let printed = find(&sh.out[from8..], out8.as_bytes()).is_some();
    if !printed {
        forward(&sh.out[from8..]);
        r.fail(format_args!(
            "`read v` with abx, Backspace, c, Enter typed, then `echo $v$W`, printed no {out8:?} within {KEYS_MS} ms: ash's `read` reads the console cooked, a byte at a time, after a poll(-1) for each byte that must wait until the line is ready, and the line discipline's Backspace erases the x"
        ));
    } else if !done {
        r.fail(format_args!("the interactive shell (pid {}) did not exit within {KEYS_MS} ms of `exit`", inner.0));
    }
    if done {
        cooked_check(&mut r, "after the interactive shell exited");
    }
    if !done {
        r.finish(8);
        return;
    }
    let _ = sh.collect(7, from8, GROUP_MS);
    r.finish(8);
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
    match mount_all() {
        Err(why) => fail_rest(1, &format!("not run: the file systems did not mount: {why}")),
        Ok(()) => session(&words()),
    }
    irq_off();
    ktap::end();
    halt()
}
