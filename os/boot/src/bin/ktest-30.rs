//! Lab 30's vehicle (`30-busybox`): the real shell. busybox 1.37.0, built by
//! the course (`user/build.sh`, layout `busybox`; `user/busybox/README.md`)
//! and run unmodified on the learner's kernel: `/bin/sh` is busybox's `ash`,
//! and every applet name in `/bin` is a hard link to `/bin/busybox`.
//!
//! The disk is `ktest-30.img` as the course's image writer made it (CONTRACT
//! §A.7, from lab 30): `/bin/busybox` and its 18 applet names, one inode with
//! 19 links; the course programs an applet name shadows (`ls`, `cat`, `rm`,
//! `mv`, `mkdir`, `sh`) are not in `/bin`; `/bin/init` is the learner's
//! (lab 28). The vehicle mounts only ext2 at `/` from `/dev/hda2`, through
//! the learner's `Ata`, `partition2` and `Ext2::mount`: `/proc` and `/dev`
//! are init's to mount.
//!
//! Step 1 first: before any program runs, the vehicle asks the learner's
//! `Stat::lstat` (lab 30 step 1, the kernel half of syscall 6) about
//! `/bin/busybox` and each applet name, and the VFS (lab 17's walk) about the
//! same names: one inode, `st_nlink` 19, a regular file of busybox's size.
//!
//! Lab 30's kernel work is two steps: `Stat::lstat` (1) and syscall 6's
//! arm, `sys_lstat` (2), which hands busybox the 144-byte `struct stat` of
//! step 1's answer. Without step 2 every `ls`, `cp`, `mv` and `rm` prints
//! "Function not implemented" and rows 2 and 7-9 say so.
//!
//! Then it starts the learner's `/bin/init <nonce>` through the learner's
//! exec, first, so it is pid 1. Its fd 0 is a pipe this file writes the
//! script into, one command group at a time; its fds 1 and 2 are a second
//! pipe this file drains (the TEE), forwarding every line to the serial line
//! as `# | <line>`. init says hello, mounts `/proc` and `/dev`, and forks
//! and execs `/bin/sh` — busybox's `ash` — which inherits all three fds:
//! this file writes nothing into ash but through init's own fds. The session
//! waits (`INIT_MS`) for init's `init: ` line and for a child of init running
//! `sh`. Each group ends with `echo ':q<N>:'`; the
//! group's output is what came through the tee before that marker, and it
//! must equal, byte for byte, what busybox prints on Linux for the same
//! input. Every word in the script is derived from the run's nonce
//! (`ktap::derive`), so no output can be written down in advance. ash runs
//! non-interactive: its stdin is a pipe, so `isatty` (`TIOCGWINSZ`) is
//! `-ENOTTY`, and nothing is echoed.
//!
//! The rows, printed in this order:
//!
//!   1 `bb.links.one_inode` (K, plain) — `Stat::lstat` and the VFS walk
//!     agree on `/bin/busybox` and every applet name: one inode number, a
//!     regular file, `st_nlink` = 1 + the applet count, busybox's size.
//!   2 `bb.cd` (derived) — `mkdir` and `cd` into this run's directory, `cd`
//!     into a subdirectory, a file written there by a relative name, `cd ..`
//!     and the file read back through `s1/`; `cd` to a missing name refused
//!     (status, no output on stdout); `ls` lists the cwd's one entry.
//!   3 `bb.redirect` (derived) — `>`, `>>`, `<`, `2>` and `exec 3>`/`>&3`
//!     (ash's builtin redirection moves fd 1 with `fcntl(F_DUPFD_CLOEXEC)`):
//!     the files hold what was written and `cat`'s ENOENT goes to `e1`.
//!   4 `bb.pipeline` (derived) — `/bin/printf … | sort | head -n 3` (three
//!     processes) and `cat | grep | wc -c`.
//!   5 `bb.subst` (derived) — `$(…)` around `wc -l <`, a pipeline and `head`.
//!   6 `bb.for` (derived) — a `for` over three words, a `for` writing three
//!     files read back through `sort -r`, a `while` counting with `$((…))`.
//!   7 `bb.ls.proc` (derived) — `ls -l /proc` (busybox's `ls -l` calls
//!     `lstat` on every name): `/proc`'s names with their types, init's pid
//!     and the shell's among them; `ls -l f1 /bin/sh`: `f1`'s size and one link,
//!     `/bin/sh` with 19 links and busybox's size.
//!   8 `bb.applets` (derived) — `cp`, `sed`, `mv`, `rm`; `rm /bin/yes`
//!     removes one name of busybox's inode and leaves the rest: `ls -l
//!     /bin/busybox` shows 18 links, and the shell still runs busybox
//!     (the file's bytes are intact, `wc -c`); then `exit <derived>`, and
//!     init, which reaps it, reports `init: sh (pid P) ended, exit <that
//!     number>`.
//!   9 `bb.enosys.exact` (K, plain) — in each group
//!     `syscall::enosys_count()` moved by exactly the calls of the stubs
//!     the kernel leaves: `sendfile` (40) always, and `access` (21) until
//!     lab 33 arms it (CONTRACT §A.4; `ENOSYS_WANT`, counted by the
//!     re-trace). Which applies is asked of the kernel before the session:
//!     `syscall::dispatch(21, …)` with a null path answers `-ENOSYS` only
//!     while `access` has no arm (`ENOSYS_WANT_ACCESS` after it). A failure
//!     names the group and both counts.
//!
//! A session that stops — no marker within `GROUP_MS` of the group's
//! script, by the PM timer — fails the rows still owed with the reason, and
//! the vehicle ends: the plan always completes.
//!
//! Plan 9.

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
use kernel::fs::ext2::Ext2;
use kernel::fs::mount;
use kernel::fs::vfs::{self, InodeRef, Stat};
use kernel::ktap;
use kernel::pipe;
use kernel::proc::{self, Pid};
use kernel::syscall::{self, Errno, SyscallFrame};
use kernel::task::{self, Tid};
use kernel::{bootinfo, frame, gdt, heap, idt, irq, sched, serial, time};

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 64;
const PM_HZ: u64 = 3_579_545;

const PLAN: usize = 9;
const LINKS_ID: &str = "bb.links.one_inode";
const CD_ID: &str = "bb.cd";
const REDIRECT_ID: &str = "bb.redirect";
const PIPELINE_ID: &str = "bb.pipeline";
const SUBST_ID: &str = "bb.subst";
const FOR_ID: &str = "bb.for";
const LS_ID: &str = "bb.ls.proc";
const APPLETS_ID: &str = "bb.applets";
const ENOSYS_ID: &str = "bb.enosys.exact";

/// busybox's applet names, each a hard link to `/bin/busybox` from lab 30
/// (`user/busybox/applets`, which the image writer reads).
const APPLETS: [&str; 18] =
    ["ash", "cat", "cp", "grep", "head", "kill", "ls", "mkdir", "mv", "printf", "rm", "sed", "sh", "sleep", "sort", "vi", "wc", "yes"];

/// What `syscall::enosys_count()` moves by in each group, in order (the
/// session's script, below): the calls the course leaves `-ENOSYS` at lab
/// 30, counted on the reference by the re-trace (2026-10-23; CONTRACT
/// §B.2-30 as built). `sendfile` (40): busybox's copy loop tries it once
/// per file `cat` or `cp` copies, and falls back to read/write — groups 1,
/// 2, 3 and 5 copy 1, 2, 1 and 3 files, group 7 two (`cat f4`, `cp`).
/// `access(path, W_OK)` (21): `rm` asks it once per name it removes before
/// it checks `isatty(0)` — group 7 removes six (`f2`, `s1/f5`, `/bin/yes`,
/// `s1`'s one file, `s1`, `t_a`). `uname` (63) is not called.
const ENOSYS_WANT: [u64; 7] = [1, 2, 1, 0, 3, 0, 8];
/// The same once `access` (21) has its arm (lab 33): group 7's six calls
/// are answered, and `sendfile`'s two are what is left.
const ENOSYS_WANT_ACCESS: [u64; 7] = [1, 2, 1, 0, 3, 0, 2];

/// Whether the kernel answers `access` (21): a call with a null path is
/// `-ENOSYS` while it has no arm, `-EFAULT` (or any other answer) once it
/// has. Asked before the session, through the dispatcher, as a program's
/// call would be.
fn access_armed() -> bool {
    // SAFETY: an all-zero frame is a valid SyscallFrame (plain integers);
    // `access` reads only its two arguments.
    let mut f: SyscallFrame = unsafe { core::mem::zeroed() };
    quiet(|| syscall::dispatch(21, [0; 6], &mut f)) != -38
}

/// How long a group may run before the session counts as stopped, by the
/// PM timer.
const GROUP_MS: u64 = 20_000;

/// The learner's init (lab 28), and how long it has to print its hello line
/// and fork the shell, by the PM timer.
const INIT: &str = "/bin/init";
const INIT_MS: u64 = 10_000;

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
        5 => "EIO",
        8 => "ENOEXEC",
        9 => "EBADF",
        11 => "EAGAIN",
        12 => "ENOMEM",
        14 => "EFAULT",
        20 => "ENOTDIR",
        22 => "EINVAL",
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
        if ktap::nonce().is_none() && n > 1 && self.id != ENOSYS_ID {
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

/// `start`, then `fds` installed in the new process's table before it runs
/// (ktest-23's).
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

// ── Step 1: the links, through the learner's lstat ───────────────────────

/// `Stat::lstat` (lab 30 step 1, the kernel half of syscall 6: the stat of
/// what a path names without following a final symlink) arrives with lab
/// 30. This twin lets the file build against the cuts before it and the
/// learner's template (G1380's class): the inherent function wins once the
/// cut has it, and before that this one answers `-ENOSYS`, so step 1 fails
/// as it should.
#[allow(dead_code)]
trait LstatOf {
    fn lstat(path: &str) -> Result<Stat, Errno>;
}
impl LstatOf for Stat {
    fn lstat(_: &str) -> Result<Stat, Errno> {
        Err(38)
    }
}

fn lstat(path: &str) -> Result<Stat, Errno> {
    quiet(|| Stat::lstat(path))
}

fn walk_stat(path: &str) -> Result<Stat, Errno> {
    quiet(|| vfs::walk(&vfs::root()?, path).map(|i| i.stat()))
}

/// Row 1. Returns busybox's size for the shell rows (0 when unknown).
fn links_row(n: usize) -> u64 {
    let mut r = Row::new(LINKS_ID);
    let want_links = 1 + APPLETS.len() as u32;
    let bb = match (lstat("/bin/busybox"), walk_stat("/bin/busybox")) {
        (Ok(l), Ok(w)) => {
            if (l.ino, l.mode, l.nlink, l.size) != (w.ino, w.mode, w.nlink, w.size) {
                r.fail(format_args!(
                    "lstat(\"/bin/busybox\") gave ino {} mode {:#o} nlink {} size {}; the walk's stat ino {} mode {:#o} nlink {} size {}",
                    l.ino, l.mode, l.nlink, l.size, w.ino, w.mode, w.nlink, w.size
                ));
            }
            if !w.is_reg() || w.nlink != want_links {
                r.fail(format_args!("/bin/busybox: mode {:#o}, {} links; expected a regular file with {want_links}", w.mode, w.nlink));
            }
            Some(w)
        }
        (l, w) => {
            r.fail(format_args!(
                "lstat(\"/bin/busybox\") {}; the walk {}",
                l.map(|_| String::from("answered")).unwrap_or_else(errno_text),
                w.map(|_| String::from("answered")).unwrap_or_else(errno_text)
            ));
            None
        }
    };
    let mut same = 0;
    if let Some(bb) = &bb {
        for a in APPLETS {
            let path = format!("/bin/{a}");
            match lstat(&path) {
                Ok(s) if (s.ino, s.nlink, s.size) == (bb.ino, bb.nlink, bb.size) => same += 1,
                Ok(s) => r.fail(format_args!("lstat(\"{path}\"): ino {} nlink {} size {}; /bin/busybox is ino {} nlink {} size {}", s.ino, s.nlink, s.size, bb.ino, bb.nlink, bb.size)),
                Err(e) => r.fail(format_args!("lstat(\"{path}\") returned {}", errno_text(e))),
            }
        }
    }
    note(format_args!(
        "{LINKS_ID}: /bin/busybox ino {} nlink {} size {}; {same} of {} applet names are that inode",
        bb.as_ref().map_or(0, |s| s.ino),
        bb.as_ref().map_or(0, |s| s.nlink),
        bb.as_ref().map_or(0, |s| s.size),
        APPLETS.len()
    ));
    r.finish(n);
    bb.map_or(0, |s| s.size)
}

// ── The session ──────────────────────────────────────────────────────────

const O_NONBLOCK: u32 = 0x800;

/// The shell, its two pipes, and everything it has printed.
struct Sh {
    /// The learner's init (pid 1), and the shell it started.
    init: Pid,
    pid: Pid,
    input: Option<FileRef>,
    output: FileRef,
    out: Vec<u8>,
    /// Why the session stopped, once it has.
    stopped: Option<String>,
}

/// The words of this run, all from its nonce.
struct Words {
    nonce: [u8; 32],
    dir: String,
    w: [String; 4],
    nums: [String; 5],
    count: u64,
    exit: u64,
}

fn words() -> Words {
    let n = ktap::nonce().unwrap_or([b'0'; 32]);
    let d = |id: &str| ktap::derive(&n, id);
    Words {
        nonce: n,
        dir: format!("/tmp/b{:08x}", d("bb.dir") & 0xffff_ffff),
        w: core::array::from_fn(|k| format!("w{:010x}", d(&format!("bb.word/{k}")) & 0xff_ffff_ffff)),
        nums: core::array::from_fn(|k| format!("n{:06x}", d(&format!("bb.sort/{k}")) & 0xff_ffff)),
        count: 3 + d("bb.for") % 9,
        exit: 1 + d("bb.exit") % 100,
    }
}

fn forward(bytes: &[u8]) {
    for line in bytes.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
        note(format_args!("| {}", String::from_utf8_lossy(line)));
    }
}

impl Sh {
    /// `/bin/init <nonce>` started first, so it is pid 1, with fd 0 the
    /// script pipe and fds 1 and 2 the tee; then its hello line, and the
    /// child it forks running `sh` — which inherits all three.
    fn start(w: &Words) -> Result<Sh, String> {
        let (ir, iw) = pipe::open(0).map_err(|e| format!("pipe::open(0) returned {}", errno_text(e)))?;
        let (or, ow) = pipe::open(0).map_err(|e| format!("pipe::open(0) returned {}", errno_text(e)))?;
        or.flags.store(or.flags.load(Ordering::SeqCst) | O_NONBLOCK, Ordering::SeqCst);
        let nonce = String::from_utf8_lossy(&w.nonce).into_owned();
        let init = start_with(INIT, || exec::spawn(INIT, &["init", &nonce], &[]), &[(0, &ir), (1, &ow), (2, &ow)]);
        drop((ir, ow));
        let init = init?;
        let mut sh = Sh { init, pid: Pid(0), input: Some(iw), output: or, out: Vec::new(), stopped: None };
        let mut buf = alloc::vec![0u8; 2048];
        let mut c = Clock::new();
        loop {
            if let Ok(k) = sh.output.read(&mut buf) {
                sh.out.extend_from_slice(&buf[..k]);
            }
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

    /// Write one group's script, then read until its marker; the group's
    /// output, or why the session stopped.
    fn group(&mut self, n: usize, script: &str) -> Result<Vec<u8>, String> {
        if let Some(why) = &self.stopped {
            return Err(why.clone());
        }
        let marker = format!(":q{n}:\n");
        let text = format!("{script}echo ':q{n}:'\n");
        let from = self.out.len();
        let Some(input) = self.input.as_ref() else {
            return Err(String::from("the shell's input is closed"));
        };
        if let Err(e) = input.write(text.as_bytes()) {
            let why = format!("writing group {n}'s script to the shell's stdin returned {}", errno_text(e));
            self.stopped = Some(why.clone());
            return Err(why);
        }
        let mut buf = alloc::vec![0u8; 2048];
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
            match self.output.read(&mut buf) {
                Ok(0) => {
                    let why = format!("the shell closed its output during group {n} ({}), after {} bytes of it", status_text(exit_of(self.pid)), self.out.len() - from);
                    forward(&self.out[from..]);
                    self.stopped = Some(why.clone());
                    return Err(why);
                }
                Ok(k) => self.out.extend_from_slice(&buf[..k]),
                Err(11) => {
                    if c.ms() >= GROUP_MS {
                        let why = format!("group {n} printed no marker within {GROUP_MS} ms by the PM timer ({} bytes of it); the session stopped there", self.out.len() - from);
                        forward(&self.out[from..]);
                        self.stopped = Some(why.clone());
                        return Err(why);
                    }
                    sched::yield_now();
                }
                Err(e) => {
                    let why = format!("a read of the shell's output returned {}", errno_text(e));
                    self.stopped = Some(why.clone());
                    return Err(why);
                }
            }
        }
    }

    /// The last group: `exit <code>`; how init says the shell ended (its
    /// `init: sh (pid P) ended, <how>; …` line: init reaped it), or `None`.
    /// The input stays open, so the shell init starts next waits on it.
    fn finish(&mut self, code: u64) -> Option<String> {
        if self.stopped.is_some() {
            return None;
        }
        if let Some(input) = self.input.as_ref() {
            let _ = input.write(format!("exit {code}\n").as_bytes());
        }
        let from = self.out.len();
        let head = format!("init: sh (pid {}) ended, ", self.pid.0);
        let mut c = Clock::new();
        let mut buf = alloc::vec![0u8; 2048];
        loop {
            if let Ok(k) = self.output.read(&mut buf) {
                self.out.extend_from_slice(&buf[..k]);
            }
            let text = String::from_utf8_lossy(&self.out[from..]).into_owned();
            if let Some(line) = text.split_inclusive('\n').filter(|l| l.ends_with('\n')).find(|l| l.starts_with(&head)) {
                forward(&self.out[from..]);
                return Some(String::from(line[head.len()..].trim_end().split(';').next().unwrap_or("")));
            }
            if c.ms() >= 2_000 {
                forward(&self.out[from..]);
                return None;
            }
            sched::yield_now();
        }
    }
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

/// The group's output against the bytes expected.
fn expect(r: &mut Row, n: usize, got: &Result<Vec<u8>, String>, want: &str) {
    match got {
        Err(why) => r.fail(format_args!("{why}")),
        Ok(g) if g.as_slice() != want.as_bytes() => r.fail(format_args!(
            "group {n} printed {:?}; Linux's busybox prints {:?}",
            String::from_utf8_lossy(g),
            want
        )),
        Ok(_) => {}
    }
}

/// A busybox line that says a call answered `-ENOSYS` names its cause.
fn enosys_hint(r: &mut Row, out: &[u8]) {
    if out.windows(24).any(|w| w == b"Function not implemented") {
        r.fail(format_args!(
            "a call answered -ENOSYS (\"Function not implemented\"): `ls`, `cp`, `mv` and `rm` ask about every name with lstat, syscall 6 (lab 30 step 2, its arm)"
        ));
    }
}

fn lines(b: &[u8]) -> Vec<&str> {
    core::str::from_utf8(b).unwrap_or("").lines().collect()
}

/// One `ls -l` line: (type character, links, size, name), or `None`.
fn ls_fields(line: &str) -> Option<(char, u64, u64, &str)> {
    let f: Vec<&str> = line.split_whitespace().collect();
    if f.len() < 9 {
        return None;
    }
    Some((f[0].chars().next()?, f[1].parse().ok()?, f[4].parse().ok()?, f[f.len() - 1]))
}

/// The seven groups and their rows; returns each group's enosys delta.
fn session(w: &Words, bb_size: u64, base: usize) -> Vec<Option<u64>> {
    let mut deltas = Vec::new();
    let [w0, w1, w2, w3] = &w.w;
    let d = &w.dir;
    let mut sh = match Sh::start(w) {
        Ok(sh) => sh,
        Err(why) => {
            for (k, id) in [CD_ID, REDIRECT_ID, PIPELINE_ID, SUBST_ID, FOR_ID, LS_ID, APPLETS_ID].iter().enumerate() {
                let mut r = Row::new(id);
                r.fail(format_args!("no shell ran: {why}"));
                r.finish(base + k);
            }
            return deltas;
        }
    };
    note(format_args!("session: init is pid {}, and /bin/sh, its child, pid {}; the script's words are derived from the nonce", sh.init.0, sh.pid.0));
    let mut run = |sh: &mut Sh, n: usize, script: String| {
        let e0 = syscall::enosys_count();
        let got = sh.group(n, &script);
        deltas.push(got.is_ok().then(|| syscall::enosys_count() - e0));
        got
    };

    // Group 1: cd.
    let g = run(
        &mut sh,
        1,
        format!(
            "mkdir {d} && cd {d} && echo \"cwd=$PWD\"\n\
             mkdir s1 && cd s1 && printf '%s\\n' {w0} > in_s1 && cd .. && cat s1/in_s1\n\
             cd nosuch_{w1} 2>/dev/null || echo 'cd refused'\n\
             ls\n"
        ),
    );
    let mut r = Row::new(CD_ID);
    expect(&mut r, 1, &g, &format!("cwd={d}\n{w0}\ncd refused\ns1\n"));
    r.finish(base);

    // Group 2: redirection.
    let g = run(
        &mut sh,
        2,
        format!(
            "printf '%s\\n' {w1} > f1\n\
             printf '%s\\n' {w2} >> f1\n\
             cat < f1\n\
             cat nosuch_{w0} 2> e1; echo \"rc=$?\"\n\
             wc -l < e1\n\
             exec 3> f3; echo {w0} >&3; exec 3>&-\n\
             cat f3\n"
        ),
    );
    let mut r = Row::new(REDIRECT_ID);
    expect(&mut r, 2, &g, &format!("{w1}\n{w2}\nrc=1\n1\n{w0}\n"));
    r.finish(base + 1);

    // Group 3: pipelines.
    let mut sorted = w.nums.clone();
    sorted.sort();
    let g = run(
        &mut sh,
        3,
        format!(
            "/bin/printf '%s\\n' {} | sort | head -n 3\n\
             cat f1 | grep {w2} | wc -c\n",
            w.nums.join(" ")
        ),
    );
    let mut r = Row::new(PIPELINE_ID);
    expect(&mut r, 3, &g, &format!("{}\n{}\n{}\n{}\n", sorted[0], sorted[1], sorted[2], w2.len() + 1));
    r.finish(base + 2);

    // Group 4: $(…).
    let g = run(
        &mut sh,
        4,
        format!(
            "x=$(wc -l < f1); echo \"lines=$x\"\n\
             echo \"len=$(printf '%s' {w0}{w1} | wc -c)\"\n\
             echo \"first=$(head -n 1 f1)\"\n"
        ),
    );
    let mut r = Row::new(SUBST_ID);
    expect(&mut r, 4, &g, &format!("lines=2\nlen={}\nfirst={w1}\n", w0.len() + w1.len()));
    r.finish(base + 3);

    // Group 5: loops.
    let g = run(
        &mut sh,
        5,
        format!(
            "for i in {w0} {w1} {w2}; do printf '%s-' \"$i\"; done; echo\n\
             for f in a b c; do echo \"$f.{w2}\" > \"t_$f\"; done; cat t_a t_b t_c | sort -r\n\
             i=0; while [ $i -lt {} ]; do i=$((i+1)); done; echo \"i=$i\"\n",
            w.count
        ),
    );
    let mut r = Row::new(FOR_ID);
    expect(&mut r, 5, &g, &format!("{w0}-{w1}-{w2}-\nc.{w2}\nb.{w2}\na.{w2}\ni={}\n", w.count));
    r.finish(base + 4);

    // Group 6: ls -l, through lstat.
    let g = run(&mut sh, 6, String::from("ls -l /proc\nls -l f1 /bin/sh\n"));
    let mut r = Row::new(LS_ID);
    match &g {
        Err(why) => r.fail(format_args!("{why}")),
        Ok(out) => {
            enosys_hint(&mut r, out);
            let ls = lines(out);
            let f1_size = (w1.len() + w2.len() + 2) as u64;
            let shell = format!("{}", sh.pid.0);
            let init = format!("{}", sh.init.0);
            let mut want: Vec<(&str, char)> =
                alloc::vec![("meminfo", '-'), ("mounts", '-'), ("sboot", 'd'), ("uptime", '-'), (init.as_str(), 'd'), (shell.as_str(), 'd')];
            let mut rest = Vec::new();
            for l in &ls {
                match ls_fields(l) {
                    Some((t, _, _, name)) => {
                        if let Some(k) = want.iter().position(|(n, _)| *n == name) {
                            if want[k].1 != t {
                                r.fail(format_args!("`ls -l /proc` gave {name} the type {t:?}, expected {:?}: {l:?}", want[k].1));
                            }
                            want.remove(k);
                        } else {
                            rest.push((t, name, l));
                        }
                    }
                    None if l.starts_with("total ") => {}
                    None => r.fail(format_args!("an `ls -l` line with fewer than nine fields: {l:?}")),
                }
            }
            if !want.is_empty() {
                r.fail(format_args!("`ls -l /proc` lacks {:?}; it printed {:?}", want.iter().map(|w| w.0).collect::<Vec<_>>(), ls));
            }
            match ls_fields(ls.iter().rev().nth(1).copied().unwrap_or("")) {
                Some(('-', 19, s, "/bin/sh")) if s == bb_size => {}
                _ => r.fail(format_args!("`ls -l f1 /bin/sh`'s first line should show /bin/sh with 19 links and {bb_size} bytes: {:?}", ls.iter().rev().nth(1))),
            }
            match ls_fields(ls.last().copied().unwrap_or("")) {
                Some(('-', 1, s, "f1")) if s == f1_size => {}
                _ => r.fail(format_args!("`ls -l f1 /bin/sh`'s last line should show f1 with 1 link and {f1_size} bytes: {:?}", ls.last())),
            }
            note(format_args!("{LS_ID}: {} lines; init's pid {} and the shell's {} listed in /proc", ls.len(), sh.init.0, sh.pid.0));
        }
    }
    r.finish(base + 5);

    // Group 7: the other applets, and one name of busybox removed.
    let g = run(
        &mut sh,
        7,
        format!(
            "cp f1 f2 && sed 's/{w1}/{w3}/' f2 > f4 && cat f4\n\
             mv f4 s1/f5 && ls s1\n\
             rm f2 s1/f5 && ls s1\n\
             rm /bin/yes && ls -l /bin/busybox\n\
             wc -c < /bin/sh\n\
             rm -r s1 t_a && ls\n"
        ),
    );
    let exit = sh.finish(w.exit);
    let mut r = Row::new(APPLETS_ID);
    match &g {
        Err(why) => r.fail(format_args!("{why}")),
        Ok(out) => {
            enosys_hint(&mut r, out);
            let ls = lines(out);
            let head = format!("{w3}\n{w2}\nf5\nin_s1\nin_s1\n");
            if !out.starts_with(head.as_bytes()) {
                r.fail(format_args!("group 7 began {:?}; expected {head:?}", String::from_utf8_lossy(&out[..out.len().min(head.len() + 16)])));
            }
            match ls.get(5).and_then(|l| ls_fields(l)) {
                Some(('-', 18, s, "/bin/busybox")) if s == bb_size => {}
                _ => r.fail(format_args!("after `rm /bin/yes`, `ls -l /bin/busybox` should show 18 links and {bb_size} bytes: {:?}", ls.get(5))),
            }
            let tail: Vec<&str> = ls.iter().skip(6).copied().collect();
            let size = format!("{bb_size}");
            let want = [size.as_str(), "e1", "f1", "f3", "t_b", "t_c"];
            if tail != want {
                r.fail(format_args!("group 7 ended {tail:?}; expected {want:?}"));
            }
        }
    }
    let want_exit = format!("exit {}", w.exit);
    if exit.as_deref() != Some(want_exit.as_str()) {
        r.fail(format_args!(
            "after `exit {}`, init (pid {}) should print `init: sh (pid {}) ended, {want_exit}; …`; it said {:?}",
            w.exit, sh.init.0, sh.pid.0, exit
        ));
    }
    r.finish(base + 6);
    deltas
}

fn enosys_row(n: usize, deltas: &[Option<u64>], armed: bool) {
    let mut r = Row::new(ENOSYS_ID);
    let (want_all, stubs) = if armed { (ENOSYS_WANT_ACCESS, "the calls to sendfile (40), with access (21) armed,") } else { (ENOSYS_WANT, "the calls to sendfile (40) and access (21)") };
    let got: Vec<String> = deltas.iter().map(|d| d.map_or(String::from("-"), |d| format!("{d}"))).collect();
    note(format_args!("{ENOSYS_ID}: enosys_count() moved by {} in groups 1-7; {stubs} account for {:?}", got.join(" "), want_all));
    if deltas.len() != want_all.len() || deltas.iter().any(|d| d.is_none()) {
        r.fail(format_args!("the session did not run all {} groups", want_all.len()));
    }
    for (k, (d, want)) in deltas.iter().zip(want_all).enumerate() {
        if let Some(d) = d {
            if *d != want {
                r.fail(format_args!("group {}: {d} calls answered -ENOSYS; {stubs} account for {want}", k + 1));
            }
        }
    }
    r.finish(n);
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
        Err(why) => {
            for (k, id) in [LINKS_ID, CD_ID, REDIRECT_ID, PIPELINE_ID, SUBST_ID, FOR_ID, LS_ID, APPLETS_ID, ENOSYS_ID].iter().enumerate() {
                let mut r = Row::new(id);
                r.fail(format_args!("not run: the file systems did not mount: {why}"));
                r.finish(k + 1);
            }
        }
        Ok(()) => {
            let bb_size = links_row(1);
            let w = words();
            let armed = access_armed();
            let deltas = session(&w, bb_size, 2);
            enosys_row(9, &deltas, armed);
        }
    }
    irq_off();
    ktap::end();
    halt()
}
