//! Lab 28's vehicle (`28-init`): the learner's own `init`, user-mode Rust on
//! the course's crate (`userland/init.rs`, `user/rust/`), started as pid 1.
//! Lab 28 adds no kernel interface (CONTRACT §B.2-28): everything init calls
//! is labs 09–27's, and this file only starts it, types into the shell it
//! starts, and watches.
//!
//! The disk is `ktest-28.img` as the course's image writer made it (CONTRACT
//! §A.7): from lab 28 its `/bin/init` is the learner's (xtask builds
//! `userland/` for a vehicle of lab 28 on); `/bin/sh` is the course's shell.
//! The vehicle mounts only ext2 at `/` (from `/dev/hda2`) and the boot
//! archive at `/boot` — never `/proc` or `/dev`: those are init's to mount.
//! Then it starts `/bin/init <nonce>` through the learner's exec, first, so
//! it is pid 1, with the console as its fd 0 (`proc::start`'s three standard
//! fds) and two pipes this file drains as its fds 1 and 2 — the TEE (OUT and
//! ERR). There is no course `init` in this vehicle: what init starts, and
//! what those start, inherit OUT and ERR from it, so everything they print
//! comes through the tee. This file keeps it (the rows read it) and forwards
//! it to the serial line with `# ` at the start of every line, so a
//! program's own KTAP lines (`forker`'s `ok 1 - fork.wait.codes`) are
//! comments here. A typed line is echoed by the learner's console, not by
//! this file.
//!
//! The session, each step bounded by the PM timer (`STEP_MS`, `ORPHAN_MS`),
//! never by the learner's clock: init's key line on OUT; the shell's first
//! prompt (`$ `); then, one line at a time after each prompt, typed through
//! `kbd::push` (make codes only, a whole line at once, under the ring's 64):
//! `echo <w1>`, `cat /proc/mounts`, `cat /<n1>`, `forker <nonce> 1 hold`,
//! and `exit` at once, while the grandchild `forker` leaves behind is held.
//! The HOLD is a third pipe whose only write end this file keeps: just before
//! `forker` is typed, its read end goes into the first shell's table as fd 9
//! (`HOLD_FD`), in place of whatever was there, so `forker` inherits it from
//! the shell, and init, which forked that shell earlier, cannot close it or
//! put another file there (fix round 3, 2026-10-29: round 2 gave it to init,
//! and an init that closed or reused its fd 9 let the orphan run unheld).
//! `hold` makes the grandchild read its fd 9 before anything else, and that
//! read returns only when this file closes the hold's write end, so the orphan's life is
//! this file's to end, by the PM timer, whatever the guest CPU's speed (fix
//! round 2, 2026-10-29: the 200,000 yields it ran before took 2-4 s at
//! ambient load and over 20 s under a starved host). The hold is released
//! once the new shell has prompted, or once `STEP_MS` has passed without
//! one; then the wait for the orphan to be collected (`ORPHAN_MS` from when
//! it is first seen ended, which for a held orphan is the release; when no
//! prompt came while the orphan was held, the same wait also watches for a
//! late one), and `echo <w2>`. `w1`, `n1` and `w2` are drawn from the nonce. If init ends,
//! or a step does not happen within its bound, the session stops there: the
//! rows it did not reach say what did not happen, each on its own line, and
//! the run ends as usual.
//!
//! The seven rows, printed after the session in this order:
//!   1 `init.nonce.line` (derived) — OUT carries exactly one line `init: key
//!     <16 hex>`, the digits `derive(nonce, "init.nonce.line")`. (The digits
//!     are compared whole, so an unpadded `{:x}` is caught only on a run
//!     whose key starts with a zero digit, one in 16. A wrong key is
//!     reported as wrong; the right one is never printed.)
//!   2 `init.mounts.own` (plain) — neither `/proc` nor `/dev` is mounted
//!     when init is started (read in the same interrupts-off block as the
//!     exec, before init runs), and at the end the mount table holds `/proc`
//!     as "proc" and `/dev` as "devfs" exactly once each (this file mounted
//!     neither, so a user program did, through `mount`; which user program
//!     is not seen), `cat /proc/mounts` in the shell lists both, and
//!     syscall 165's work for a second `/proc` at the end is `-EBUSY`.
//!   3 `init.shell.started` (derived) — at the first prompt a process
//!     running `sh` (argv[0] `sh`, or a path ending in `/sh`), not pid 1,
//!     whose parent is pid 1, is alive; and `echo <w1>` typed into it prints
//!     `<w1>` on OUT: fd 0 is the console and fd 1 the tee, inherited.
//!   4 `init.fds.inherited` (derived) — `cat /<n1>` prints `cat: /<n1>:
//!     ENOENT` on ERR and nothing of it on OUT: fd 2 reached init's child's
//!     child, unchanged.
//!   5 `init.orphans.reaped` (derived) — `forker <nonce> 1 hold` names the
//!     grandchild it leaves behind (`fork.orphan.to_init: pid C forked G`);
//!     G, whose argv carries the nonce, is seen with parent pid 1 (the
//!     kernel hands an orphan to init, lab 14), and then its record is gone
//!     within `ORPHAN_MS` of its end (the hold's release) — so, as far as this file can
//!     see, init's `wait4` collected it (the kernel's own process table is
//!     trusted: a kernel that drops a record unasked is not caught here).
//!     When init itself has ended first, the orphan has no init to collect
//!     it and only the first two are asked (rows 6 and 7 are red then).
//!   6 `init.shell.respawn` (derived) — after `exit`, the shell's record is
//!     gone (init reaped it); a new process running `sh` prompts within
//!     `STEP_MS`, while the orphan is held, so an init that waits for every
//!     child cannot (one whose prompt comes only after the release is red by
//!     name); that shell's pid, or its parent's, is the next one after the
//!     orphan's, so init's first new process after `exit` is the shell or
//!     the one that starts it (nothing else forks between the orphan and
//!     `exit`; a child that exits at once before the shell is red); `echo <w2>` typed into it prints `<w2>`; and after it at
//!     most one process runs `sh` (a shell's builtin runs in a child that
//!     keeps the name `sh`, so a second shell beside the first can count 3). When the orphan was not held (a
//!     kernel whose fork did not pass fd 9 on: its read fails and it yields
//!     200,000 times, as before) and it ended while the shell was gone and before init's
//!     next child, that order is red as well, as in fix round 1.
//!   7 `init.pid1.cmdline` (derived) — at the end pid 1 is alive and its
//!     argv in the kernel's process record is exactly `init <nonce>`, as
//!     this file started it. (The record, not `/proc/1/cmdline`: procfs is
//!     lab 22's and is not read here.)
//!
//! Plan 7.

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
use kernel::fs::ext2::Ext2;
use kernel::fs::mount;
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
const NONCE_ID: &str = "init.nonce.line";
const MOUNTS_ID: &str = "init.mounts.own";
const SHELL_ID: &str = "init.shell.started";
const FDS_ID: &str = "init.fds.inherited";
const ORPHAN_ID: &str = "init.orphans.reaped";
const RESPAWN_ID: &str = "init.shell.respawn";
const CMDLINE_ID: &str = "init.pid1.cmdline";

/// The program, and the name it is started under (argv[0]).
const INIT: &str = "/bin/init";
/// How long one step of the session may take, by the PM timer: init's key
/// line, a prompt, a command's answer, a respawned shell.
const STEP_MS: u64 = 15_000;
/// How long an ended orphan may stay uncollected, from when this file first
/// sees it ended. A held orphan ends once the hold is released: its read
/// returns end of file and it exits at once (`user/forker/forker.c`,
/// `hold`), so this is the release's bound too. (Before fix round 2 it was
/// 20 s from `forker`'s end, for 200,000 yields whose length was the guest
/// CPU's.)
const ORPHAN_MS: u64 = 10_000;
/// An orphan still running (not a zombie) is also given this long from
/// `forker`'s end, round 1's bound: an orphan whose fd 9 is not the hold
/// (a fork that did not pass it on) yields 200,000 times instead, at the
/// guest CPU's pace, as before.
const ORPHAN_RUN_MS: u64 = 20_000;
/// The fd the hold's read end is installed at in the first shell, just
/// before `forker` is typed; `forker`'s `hold` reads it (`HOLD_FD` there).
const HOLD_FD: usize = 9;
/// fcntl's `O_NONBLOCK`, on the tee's read ends.
const O_NONBLOCK: u32 = 0x800;

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
        14 => "EFAULT",
        16 => "EBUSY",
        19 => "ENODEV",
        20 => "ENOTDIR",
        22 => "EINVAL",
        38 => "ENOSYS",
        _ => "?",
    };
    format!("-{e} ({name})")
}

fn status_text(v: i32) -> String {
    if v & 0x7f == 0 {
        format!("exit({})", (v >> 8) & 0xff)
    } else {
        format!("killed by signal {} (status {v:#x})", v & 0x7f)
    }
}

// ── Processes ────────────────────────────────────────────────────────────

/// One process record, as `proc::each` shows it.
struct Rec {
    pid: Pid,
    ppid: Pid,
    exit: Option<i32>,
    argv: Vec<String>,
}

fn procs() -> Vec<Rec> {
    let mut out = Vec::new();
    quiet(|| proc::each(|p| out.push(Rec { pid: p.pid, ppid: p.ppid, exit: p.exit, argv: p.argv.clone() })));
    out
}

/// One record, copying only that one: the session's waits poll this, and
/// each poll runs with interrupts off, so a poll that copied the whole
/// table would take CPU from the processes being waited for (the orphan's
/// 200,000 yields each give way to one poll).
fn find(pid: Pid) -> Option<Rec> {
    let mut out = None;
    quiet(|| {
        proc::each(|p| {
            if p.pid == pid {
                out = Some(Rec { pid: p.pid, ppid: p.ppid, exit: p.exit, argv: p.argv.clone() });
            }
        })
    });
    out
}

/// Whether `parent` has a child with a pid above `above`, copying nothing.
fn child_above(parent: Pid, above: u64) -> bool {
    let mut any = false;
    quiet(|| proc::each(|p| any |= p.ppid == parent && p.pid.0 > above));
    any
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

/// argv[0] names the shell: `sh`, or a path ending in `/sh` (C's habit of
/// passing the path as argv[0]).
fn is_sh(argv: &[String]) -> bool {
    argv.first().is_some_and(|a| a == "sh" || a.ends_with("/sh"))
}

/// The live processes running `sh`.
fn shells() -> Vec<Rec> {
    procs().into_iter().filter(|r| r.exit.is_none() && is_sh(&r.argv)).collect()
}

/// Pids and argvs, for a message.
fn listed(rs: &[Rec]) -> String {
    let each: Vec<String> = rs.iter().map(|r| format!("pid {} {:?}", r.pid.0, r.argv)).collect();
    each.join(", ")
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

/// `f` installed as fd `n` of a running process, in place of whatever was
/// there (dropped outside the table's lock, as `start_with`'s are). False
/// when `pid` has no task.
fn install_in(pid: Pid, n: usize, f: &FileRef) -> bool {
    let d = Fd { file: f.clone(), cloexec: false };
    let (ok, back) = quiet(|| match task_of(pid) {
        Some(tid) => task::with_table(move |tt| match tt.get_mut(tid) {
            Some(t) => (true, t.fds.install(n, d).ok().flatten()),
            None => (false, Some(d)),
        }),
        None => (false, Some(d)),
    });
    drop(back);
    ok
}

// ── Mounting: ext2 at `/` and the archive at `/boot`, nothing else ───────

fn mount_disk() -> Result<(), String> {
    let drive: Arc<dyn BlockDevice> = Arc::new(quiet(Ata::probe).map_err(|e| format!("Ata::probe() returned {}", errno_text(e)))?);
    let fs = quiet(|| ata::partition2(drive))
        .and_then(Ext2::mount)
        .map_err(|e| format!("partition2 then Ext2::mount returned {}", errno_text(e)))?;
    let fs = Arc::new(fs);
    let root: InodeRef = quiet(|| fs.root()).map_err(|e| format!("Ext2::root() returned {}", errno_text(e)))?;
    mount::mount("/dev/hda2", "/", "ext2", root).map_err(|e| format!("mount::mount(\"/dev/hda2\", \"/\", \"ext2\", its root) returned {}", errno_text(e)))?;
    let arch: InodeRef = Arc::new(Archive::new(&ARCHIVE).map_err(|e| format!("Archive::new over the boot archive returned {}", errno_text(e)))?);
    mount::mount("boot.archive", "/boot", "archive", arch)
        .map_err(|e| format!("mount::mount(\"boot.archive\", \"/boot\", \"archive\", its root) returned {}", errno_text(e)))
}

// ── The tee ──────────────────────────────────────────────────────────────

/// One pipe this file drains: everything read (`all`), and the part of the
/// last line not yet forwarded (`pend`).
struct Tee {
    r: FileRef,
    all: Vec<u8>,
    pend: Vec<u8>,
}

impl Tee {
    fn new() -> Result<(Tee, FileRef), String> {
        let (r, w) = pipe::open(0).map_err(|e| format!("pipe::open(0) returned {}", errno_text(e)))?;
        r.flags.store(r.flags.load(Ordering::SeqCst) | O_NONBLOCK, Ordering::SeqCst);
        Ok((Tee { r, all: Vec::new(), pend: Vec::new() }, w))
    }
    /// Takes what is waiting and forwards every whole line as `# <line>`.
    fn pump(&mut self) {
        let mut buf = [0u8; 512];
        while let Ok(n @ 1..) = self.r.read(&mut buf) {
            self.all.extend_from_slice(&buf[..n]);
            self.pend.extend_from_slice(&buf[..n]);
        }
        while let Some(i) = self.pend.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pend.drain(..=i).collect();
            forward(&line);
        }
    }
    /// A prompt is waiting (`$ ` with nothing after it): forward it, and
    /// start the next line afresh.
    fn take_prompt(&mut self) -> bool {
        if self.pend.ends_with(b"$ ") {
            let p: Vec<u8> = core::mem::take(&mut self.pend);
            let mut line = p;
            line.push(b'\n');
            forward(&line);
            true
        } else {
            false
        }
    }
}

/// `# ` and the bytes, to the serial line.
fn forward(line: &[u8]) {
    quiet(|| {
        serial::write_str("# ");
        for chunk in line.utf8_chunks() {
            serial::write_str(chunk.valid());
            for &b in chunk.invalid() {
                serial::write_byte(b);
            }
        }
    });
}

/// The set-1 make code that types `c` with no modifier (kbd's map).
fn make_code(c: u8) -> Option<u8> {
    const ROWS: [(u8, &[u8]); 4] = [(0x02, b"1234567890-="), (0x10, b"qwertyuiop[]"), (0x1e, b"asdfghjkl;'`"), (0x2b, b"\\zxcvbnm,./")];
    match c {
        b' ' => Some(0x39),
        b'\n' => Some(0x1c),
        _ => ROWS.iter().find_map(|&(first, row)| row.iter().position(|&k| k == c).map(|i| first + i as u8)),
    }
}

/// Types `line` and Enter: its make codes, all at once (under the ring's 64).
fn type_line(line: &str) {
    let codes: Vec<u8> = line.bytes().chain([b'\n']).filter_map(make_code).collect();
    quiet(|| codes.iter().for_each(|&sc| kbd::push(sc)));
}

// ── The session ──────────────────────────────────────────────────────────

/// What the session saw, for the rows.
#[derive(Default)]
struct Seen {
    init: Option<Pid>,
    /// Why the session stopped early, if it did.
    stop: Option<String>,
    /// What was mounted on `/proc` or `/dev` as init was started, before it
    /// ran: `(target, fstype)`.
    premounted: Vec<(String, String)>,
    /// The first shell: its pid and its parent's, at the first prompt.
    shell: Option<(Pid, Pid)>,
    /// When no process but pid 1 runs `sh` at the first prompt: what ran.
    no_shell: Option<String>,
    /// OUT from the moment `echo <w1>` was typed to the next prompt.
    echo1: Option<Vec<u8>>,
    /// OUT between typing `cat /proc/mounts` and the next prompt.
    mounts_out: Option<Vec<u8>>,
    /// OUT and ERR between typing `cat /<n1>` and the next prompt.
    cat_out: Option<(Vec<u8>, Vec<u8>)>,
    /// The orphan: its pid, whether it was seen with parent 1 and the
    /// nonce in its argv, whether its record went within the bound.
    forker_ran: bool,
    orphan: Option<Pid>,
    orphan_ppid1: bool,
    orphan_named: bool,
    orphan_gone: Option<Result<(), String>>,
    /// Init ended while the orphan was still there: no init to collect it.
    orphan_init_ended: bool,
    /// The respawn: the old shell gone, and whether the orphan was still
    /// there at that moment; at init's first new child, whether the orphan
    /// had been collected by then; the new shell, its echo, and the shells
    /// alive after it.
    old_gone: bool,
    orphan_at_old_gone: Option<bool>,
    orphan_before_respawn: Option<bool>,
    /// The highest pid when `exit` was typed (the orphan's): the new shell,
    /// init's first new child, is the next.
    high: u64,
    /// The pid whose fd `HOLD_FD` the hold's read end went into.
    hold_in: Option<Pid>,
    /// The orphan was still running when the hold was released.
    orphan_held: bool,
    /// Ms after the release when the orphan's record went, if it did.
    orphan_gone_ms: Option<u64>,
    /// No prompt came while the orphan was held, and one came this many ms
    /// after the release.
    late_shell: Option<u64>,
    new_shell: Option<Pid>,
    new_shell_ppid: Option<Pid>,
    new_shell_why: Option<String>,
    echo2: Option<Vec<u8>>,
    shells_after: Option<Vec<Rec>>,
}

struct Session {
    out: Tee,
    err: Tee,
    init: Pid,
    /// The hold's write end; `None` once released (dropping it is the
    /// release: the pipe's readers see end of file).
    hold: Option<FileRef>,
    /// The hold's read end, until it is installed in the first shell.
    hold_r: Option<FileRef>,
}

impl Session {
    fn pump(&mut self) {
        self.out.pump();
        self.err.pump();
    }
    /// Pumps the tee and yields until `done` holds, init ends, or `ms` pass.
    fn wait(&mut self, ms: u64, what: &str, mut done: impl FnMut(&mut Session) -> bool) -> Result<(), String> {
        let mut c = Clock::new();
        let from = self.err.all.len();
        loop {
            self.pump();
            if done(self) {
                return Ok(());
            }
            if let Some(v) = find(self.init).and_then(|r| r.exit) {
                self.pump();
                let last = self.err.all[from..].split(|&b| b == b'\n').filter(|l| !l.is_empty()).last().map(String::from_utf8_lossy);
                return Err(match last {
                    Some(l) => format!("init (pid {}) ended with {} before {what}; the last line it or its children wrote on fd 2 meanwhile: {l}", self.init.0, status_text(v)),
                    None => format!("init (pid {}) ended with {} before {what}, and nothing came on fd 2 meanwhile", self.init.0, status_text(v)),
                });
            }
            if c.ms() >= ms {
                // What came on fd 2 while waiting: a prompt (fd 1 pointed
                // elsewhere), or the last whole line (an exec refused, …).
                let new = &self.err.all[from..];
                let last = new.split(|&b| b == b'\n').filter(|l| !l.is_empty()).last().map(String::from_utf8_lossy);
                return Err(if new.ends_with(b"$ ") {
                    format!("{what} did not happen within {ms} ms; a prompt `$ ` came on fd 2 instead")
                } else if let Some(l) = last {
                    format!("{what} did not happen within {ms} ms; the last line on fd 2 meanwhile: {l}")
                } else {
                    format!("{what} did not happen within {ms} ms")
                });
            }
            sched::yield_now();
        }
    }
    /// Waits for the shell's next prompt.
    fn prompt(&mut self, what: &str) -> Result<(), String> {
        self.wait(STEP_MS, what, |s| s.out.take_prompt())
    }
    /// The last whole line on fd 2 since byte `from`, for a message.
    fn err_since(&self, from: usize) -> Option<String> {
        self.err.all[from..].split(|&b| b == b'\n').filter(|l| !l.is_empty()).last().map(|l| String::from_utf8_lossy(l).into_owned())
    }
    /// Types `line` at a prompt, waits for the next one, and returns what
    /// came on OUT and ERR in between.
    fn command(&mut self, line: &str) -> Result<(Vec<u8>, Vec<u8>), String> {
        let (o, e) = (self.out.all.len(), self.err.all.len());
        type_line(line);
        self.prompt(&format!("the prompt after `{line}`"))?;
        let out = self.out.all[o..].to_vec();
        let err = self.err.all[e..].to_vec();
        Ok((out, err))
    }
}

struct Words {
    nonce: [u8; 32],
    key: String,
    w1: String,
    n1: String,
    w2: String,
}

fn words(nonce: [u8; 32]) -> Words {
    let d = |id: &str| ktap::derive(&nonce, id);
    Words {
        nonce,
        key: format!("init: key {:016x}", d(NONCE_ID)),
        w1: format!("w{:08x}", d(SHELL_ID) as u32),
        n1: format!("n{:08x}", d(FDS_ID) as u32),
        w2: format!("r{:08x}", d(RESPAWN_ID) as u32),
    }
}

fn nonce_text(w: &Words) -> String {
    String::from_utf8_lossy(&w.nonce).into_owned()
}

/// Starts init and drives the session; fills `seen` as far as it gets, and
/// `out_all`/`err_all` with everything OUT and ERR carried.
fn session(w: &Words, seen: &mut Seen, out_all: &mut Vec<u8>, err_all: &mut Vec<u8>) -> Result<(), String> {
    let (out, out_w) = Tee::new()?;
    let (err, err_w) = Tee::new()?;
    let (hold_r, hold_w) = pipe::open(0).map_err(|e| format!("pipe::open(0) for the hold returned {}", errno_text(e)))?;
    let nonce = nonce_text(w);
    let premounted = &mut seen.premounted;
    // The mount table is read in the same interrupts-off block as the exec
    // (`start_with`), so init has not run yet: whatever is on `/proc` or
    // `/dev` now, the kernel put there.
    let start = || {
        let r = exec::spawn(INIT, &["init", &nonce], &[]);
        for m in mount::table() {
            if m.target == "/proc" || m.target == "/dev" {
                premounted.push((String::from(m.target.as_str()), String::from(m.fstype.as_str())));
            }
        }
        r
    };
    let init = start_with(INIT, start, &[(1, &out_w), (2, &err_w)]);
    drop(out_w);
    drop(err_w);
    let init = init?;
    seen.init = Some(init);
    if init != Pid(1) {
        note(format_args!("{CMDLINE_ID}: {INIT} started as pid {}, not 1: something was started before it", init.0));
    }
    let mut s = Session { out, err, init, hold: Some(hold_w), hold_r: Some(hold_r) };
    let r = drive(&mut s, w, seen);
    s.pump();
    *out_all = core::mem::take(&mut s.out.all);
    *err_all = core::mem::take(&mut s.err.all);
    r
}

/// The session proper. Init's key line is not waited for (row 1 reads OUT
/// at the end); the first wait is the shell's first prompt.
fn drive(s: &mut Session, w: &Words, seen: &mut Seen) -> Result<(), String> {
    let init = s.init;
    let nonce = nonce_text(w);
    s.prompt("the shell's first prompt")?;
    let sh = shells().into_iter().find(|r| r.pid != init);
    seen.shell = sh.as_ref().map(|r| (r.pid, r.ppid));
    match sh {
        Some(r) => note(format_args!("{SHELL_ID}: at the first prompt, pid {} runs sh; its parent is {}", r.pid.0, r.ppid.0)),
        None => {
            let all = procs();
            let one_sh = all.iter().find(|r| r.pid == init).is_some_and(|p| is_sh(&p.argv));
            let one_argv = all.iter().find(|r| r.pid == init).map(|p| p.argv.clone()).unwrap_or_default();
            let kids: Vec<Rec> = all.into_iter().filter(|r| r.ppid == init && r.pid != init && r.exit.is_none()).collect();
            seen.no_shell = Some(match () {
                _ if one_sh => format!("at the first prompt pid 1 itself was running {one_argv:?}: init forks, and only the child execs /bin/sh"),
                _ if kids.is_empty() => String::from("at the first prompt no process but pid 1 was alive, and none ran `sh`"),
                _ => format!("at the first prompt no process had argv[0] `sh`; pid 1's live children: {}", listed(&kids)),
            });
        }
    }

    let (o, _) = s.command(&format!("echo {}", w.w1))?;
    seen.echo1 = Some(o);
    let (o, _) = s.command("cat /proc/mounts")?;
    seen.mounts_out = Some(o);
    let (o, e) = s.command(&format!("cat /{}", w.n1))?;
    seen.cat_out = Some((o, e));

    // Step 4: the orphan, held (the file's header). The shell is asleep at
    // its prompt; its fd 9 becomes the hold's read end, which `forker` and
    // its grandchild inherit. With no shell found, pid 1 gets it (an init
    // that is itself the shell).
    let into = seen.shell.map_or(init, |(p, _)| p);
    if let Some(r) = s.hold_r.take() {
        seen.hold_in = install_in(into, HOLD_FD, &r).then_some(into);
    }
    if seen.hold_in.is_none() {
        note(format_args!("{ORPHAN_ID}: pid {} had no task to take the hold; the orphan runs unheld", into.0));
    }
    let (o, _) = s.command(&format!("forker {nonce} 1 hold"))?;
    seen.forker_ran = true;
    let g = o
        .split(|&b| b == b'\n')
        .find_map(|l| {
            let l = core::str::from_utf8(l).ok()?;
            let rest = l.split("fork.orphan.to_init: pid ").nth(1)?;
            let g = rest.split(" forked ").nth(1)?.split(' ').next()?;
            g.parse::<u64>().ok()
        })
        .map(Pid);
    seen.orphan = g;
    let mut fc = Clock::new();
    let watch = |seen: &mut Seen, g: Pid| -> Option<Rec> {
        let r = find(g)?;
        seen.orphan_ppid1 |= r.ppid == Pid(1);
        seen.orphan_named |= r.argv.iter().any(|a| a.as_bytes() == w.nonce);
        Some(r)
    };

    // Step 5: `exit` at once, while the orphan is held, and a new shell
    // within STEP_MS. Nothing forks between the orphan and `exit` (the
    // orphan waits in its read; the shell's `exit` is a builtin), so init's
    // first new child gets the pid after the orphan's. The order kept from
    // fix round 1 (init's first new child before or after the orphan went)
    // still matters when the orphan was not held.
    let old = seen.shell.map(|(p, _)| p);
    let high = old.map_or(0, |p| p.0).max(g.map_or(0, |p| p.0));
    seen.high = high;
    let from_err = s.err.all.len();
    let mut next_ms: Option<u64> = None;
    let mut ac = Clock::new();
    type_line("exit");
    let mut respawned = s.wait(STEP_MS, "a new shell's prompt after `exit`", |s| {
        // Every clock is read on every pass: the PM timer wraps every 4.7 s.
        fc.ms();
        let a = ac.ms();
        let orphan_there = g.map(|g| watch(seen, g).is_some());
        if let Some(old) = old {
            if !seen.old_gone && find(old).is_none() {
                seen.old_gone = true;
                seen.orphan_at_old_gone = orphan_there;
            }
        }
        if seen.orphan_before_respawn.is_none() && child_above(init, high) {
            seen.orphan_before_respawn = orphan_there.map(|there| !there);
            next_ms = Some(a);
        }
        s.out.take_prompt()
    });

    // The release, prompt or no prompt. Then ORPHAN_MS for the orphan to
    // end and be collected; when no prompt came while it was held, the same
    // wait watches for a late one (an init that starts the next shell only
    // once its last child has ended).
    seen.orphan_held = g.and_then(find).is_some_and(|r| r.exit.is_none());
    let init_alive = find(init).is_some_and(|r| r.exit.is_none());
    let late_wanted = respawned.is_err() && init_alive;
    let prompt_held = respawned.is_ok();
    drop(s.hold.take());
    let mut oc = Clock::new();
    let mut still: Option<Rec> = None;
    let mut ended_at: Option<u64> = None;
    loop {
        s.pump();
        let now = oc.ms();
        let since_forker = fc.ms();
        if find(init).is_some_and(|r| r.exit.is_some()) {
            seen.orphan_init_ended = g.is_some();
            if let Some(g) = g {
                watch(seen, g);
            }
            break;
        }
        if late_wanted && seen.late_shell.is_none() && s.out.take_prompt() {
            seen.late_shell = Some(now);
        }
        let there = g.and_then(|g| watch(seen, g));
        if seen.orphan_held && there.is_none() && seen.orphan_gone_ms.is_none() {
            seen.orphan_gone_ms = Some(now);
        }
        if there.is_none() && (!late_wanted || seen.late_shell.is_some()) {
            break;
        }
        let give_up = match there.as_ref().map(|r| r.exit.is_some()) {
            // Gone: only a late prompt is still awaited.
            None => now >= ORPHAN_MS,
            // Ended: ORPHAN_MS from when it is first seen ended.
            Some(true) => now >= *ended_at.get_or_insert(now) + ORPHAN_MS,
            // Still running (not held): round 1's bound from `forker`'s end.
            Some(false) => now >= ORPHAN_MS && since_forker >= ORPHAN_RUN_MS,
        };
        if give_up {
            still = there;
            break;
        }
        sched::yield_now();
    }
    if seen.late_shell.is_some() {
        respawned = Ok(());
    }
    if respawned.is_ok() {
        if let Some(old) = old {
            seen.old_gone |= find(old).is_none();
        }
        let ns = shells().into_iter().find(|r| Some(r.pid) != old && r.pid != init);
        seen.new_shell = ns.as_ref().map(|r| r.pid);
        seen.new_shell_ppid = ns.map(|r| r.ppid);
        match seen.new_shell {
            Some(p) => note(format_args!("{RESPAWN_ID}: after `exit`, pid {} runs sh", p.0)),
            None => {
                seen.new_shell_why = Some(match s.err_since(from_err) {
                    Some(l) => format!("after `exit` a prompt came, but no new process running `sh` was alive at it; the last line on fd 2 since `exit`: {l}"),
                    None => String::from("after `exit` a prompt came, but no new process running `sh` was alive at it"),
                })
            }
        }
    }
    if let Some(g) = g {
        seen.orphan_gone = Some(match &still {
            Some(r) if !seen.orphan_init_ended => Err(match r.exit {
                Some(v) => format!("pid {} ended with {} and was still not collected {ORPHAN_MS} ms later: its parent is {}", g.0, status_text(v), r.ppid.0),
                None => format!("pid {} was still running {ORPHAN_RUN_MS} ms after `forker` ended: parent {}", g.0, r.ppid.0),
            }),
            _ => Ok(()),
        });
        if still.is_none() && !seen.orphan_init_ended {
            note(format_args!(
                "{ORPHAN_ID}: pid {}{}{}{}; its record is gone{}",
                g.0,
                if seen.orphan_named { ", the nonce in its argv" } else { "" },
                if seen.orphan_ppid1 { ", was seen with parent 1" } else { "" },
                match (seen.orphan_held, prompt_held) {
                    (true, true) => ", was still running at the new shell's prompt",
                    (true, false) => ", was still running when it was let end, with no new prompt yet",
                    (false, _) => ", ended by itself, not held",
                },
                match (seen.orphan_gone_ms, next_ms) {
                    (Some(at), _) => format!(", {at} ms after that"),
                    (None, Some(m)) => format!(" before init's next child came, {m} ms after `exit`"),
                    (None, None) => String::new(),
                }
            ));
        }
    }

    respawned?;
    let (o, _) = s.command(&format!("echo {}", w.w2))?;
    seen.echo2 = Some(o);
    seen.shells_after = Some(shells());
    Ok(())
}

// ── The rows ─────────────────────────────────────────────────────────────

struct Row {
    n: usize,
    id: &'static str,
    why: Option<String>,
}

impl Row {
    fn new(n: usize, id: &'static str) -> Row {
        Row { n, id, why: None }
    }
    fn fail(&mut self, why: String) {
        if self.why.is_none() {
            self.why = Some(why);
        }
    }
    fn check(&mut self, ok: bool, why: impl FnOnce() -> String) {
        if !ok {
            self.fail(why());
        }
    }
    /// Prints the row. A failing row of a session that stopped early also
    /// says why it stopped, so its line stands alone.
    fn finish(self, stop: &Option<String>) {
        quiet(|| match self.why {
            None => kernel::kprintln!("ok {} - {}", self.n, self.id),
            Some(why) => {
                match stop {
                    Some(s) if !why.starts_with("the session did not get as far") => ktap::note(format_args!("{}: {why}; the session stopped: {s}", self.id)),
                    _ => ktap::note(format_args!("{}: {why}", self.id)),
                }
                kernel::kprintln!("not ok {} - {}", self.n, self.id);
            }
        });
    }
}

/// `line` (no newline) is one of `bytes`' lines.
fn has_line(bytes: &[u8], line: &str) -> bool {
    bytes.split(|&b| b == b'\n').any(|l| l == line.as_bytes())
}

fn shown(bytes: &[u8]) -> String {
    format!("{:?}", String::from_utf8_lossy(bytes))
}

fn rows(w: &Words, seen: &Seen, out: &[u8], err: &[u8]) {
    let stopped = |what: &str| match &seen.stop {
        Some(why) => format!("the session did not get as far as {what}: {why}"),
        None => format!("the session did not get as far as {what}"),
    };

    // 1: the key line.
    let mut r = Row::new(1, NONCE_ID);
    let n = out.split(|&b| b == b'\n').filter(|l| *l == w.key.as_bytes()).count();
    if n != 1 {
        let theirs: Vec<String> =
            out.split(|&b| b == b'\n').filter(|l| l.starts_with(b"init: key")).map(|l| String::from_utf8_lossy(l).into_owned()).collect();
        let on_err = err.split(|&b| b == b'\n').any(|l| l.starts_with(b"init: key"));
        r.fail(match (n, theirs.is_empty(), &seen.stop) {
            (0, true, _) if on_err => String::from("no line `init: key <16 hex digits>` on fd 1; the key line came on fd 2"),
            (0, true, _) => String::from("no line `init: key <16 hex digits>` on fd 1"),
            (0, false, _) => format!("init printed {theirs:?}, not the key for this run's nonce"),
            _ => format!("init printed its key line {n} times; once is the line"),
        });
    }
    r.finish(&seen.stop);

    // 2: the mounts.
    let mut r = Row::new(2, MOUNTS_ID);
    for (target, fstype) in &seen.premounted {
        r.check(false, || format!("{target} was already mounted (\"{fstype}\") when init was started, before it ran: the kernel mounts only what a program asks for"));
    }
    let table = mount::table();
    for (fstype, target) in [("proc", "/proc"), ("devfs", "/dev")] {
        let k = table.iter().filter(|m| m.target == target).count();
        let typed = table.iter().filter(|m| m.target == target && m.fstype == fstype).count();
        r.check(k == 1 && typed == 1, || {
            format!("the mount table has {k} mount(s) on {target} ({typed} of type \"{fstype}\"); init mounts \"{fstype}\" there once")
        });
    }
    match &seen.mounts_out {
        Some(o) => {
            for (fstype, target) in [("proc", "/proc"), ("devfs", "/dev")] {
                let listed = o.split(|&b| b == b'\n').any(|l| {
                    let f: Vec<&[u8]> = l.split(|&b| b == b' ').collect();
                    f.len() >= 3 && f[1] == target.as_bytes() && f[2] == fstype.as_bytes()
                });
                r.check(listed, || format!("`cat /proc/mounts` in the shell printed {}, with no `{target} {fstype}` line", shown(o)));
            }
        }
        None => r.fail(stopped("`cat /proc/mounts`")),
    }
    let again = mount::sys_mount("proc", "/proc", "proc");
    r.check(again == Err(16), || match again {
        Ok(()) => String::from("a second mount of \"proc\" on /proc succeeded; /proc was not mounted"),
        Err(e) => format!("a second mount of \"proc\" on /proc returned {}, not -16 (EBUSY)", errno_text(e)),
    });
    r.finish(&seen.stop);

    // 3: the shell init started.
    let mut r = Row::new(3, SHELL_ID);
    match seen.shell {
        None => match &seen.no_shell {
            Some(why) => r.fail(why.clone()),
            None => r.fail(stopped("the first prompt")),
        },
        Some((pid, ppid)) => r.check(ppid == Pid(1), || format!("the shell (pid {}) has parent {}, not 1: init forks it and execs /bin/sh in that child", pid.0, ppid.0)),
    }
    match &seen.echo1 {
        Some(o) => r.check(has_line(o, &w.w1), || format!("`echo {}` printed {} on fd 1", w.w1, shown(o))),
        None => r.fail(stopped(&format!("`echo {}`", w.w1))),
    }
    r.finish(&seen.stop);

    // 4: fd 2, inherited.
    let mut r = Row::new(4, FDS_ID);
    let want = format!("cat: /{}: ENOENT", w.n1);
    match &seen.cat_out {
        Some((o, e)) => {
            r.check(has_line(e, &want), || format!("`cat /{}` printed {} on fd 2, not the line `{want}`", w.n1, shown(e)));
            r.check(!has_line(o, &want), || format!("`cat /{}`'s error line came on fd 1, not fd 2", w.n1));
        }
        None => r.fail(stopped(&format!("`cat /{}`", w.n1))),
    }
    r.finish(&seen.stop);

    // 5: the orphan.
    let mut r = Row::new(5, ORPHAN_ID);
    match (seen.orphan, &seen.orphan_gone) {
        (None, _) if seen.forker_ran => r.fail(String::from("`forker` did not name the grandchild it leaves behind")),
        (None, _) => r.fail(stopped("`forker`")),
        (Some(_), None) => r.fail(stopped("the wait for the orphan")),
        (Some(g), Some(gone)) => {
            r.check(seen.orphan_named, || format!("pid {} was never seen with the nonce in its argv", g.0));
            r.check(seen.orphan_ppid1, || format!("pid {} was never seen with parent 1", g.0));
            // Init ended first (rows 6 and 7 say so): no init was left to
            // collect it, so only the hand-over is asked.
            if let Err(why) = gone {
                r.fail(why.clone());
            }
        }
    }
    r.finish(&seen.stop);

    // 6: the respawn.
    let mut r = Row::new(6, RESPAWN_ID);
    if let Some(m) = seen.late_shell {
        let held = match seen.orphan {
            Some(g) if seen.orphan_held => format!(" while pid {}, also init's child, still ran", g.0),
            _ => String::new(),
        };
        let after = match (seen.orphan, seen.orphan_gone_ms) {
            (Some(g), Some(at)) if seen.orphan_held && at <= m => format!(", after pid {} had ended", g.0),
            _ => String::new(),
        };
        r.fail(format!(
            "no new shell came in the {STEP_MS} ms after `exit`{held}; one came {} ms after it{after}: when the shell ends, init starts the next one at once, whatever its other children are doing",
            STEP_MS + m
        ));
    }
    if let (Some(g), Some(true), Some(true)) = (seen.orphan, seen.orphan_at_old_gone, seen.orphan_before_respawn) {
        r.fail(format!(
            "init started its next child only after pid {}, also its child and still running when the shell ended, had ended and been collected: when the shell ends, init starts the next one at once, whatever its other children are doing",
            g.0
        ));
    }
    match &seen.echo2 {
        None => match &seen.new_shell_why {
            Some(why) => r.fail(why.clone()),
            None => r.fail(stopped("a new shell")),
        },
        Some(o) => {
            if let Some((old, _)) = seen.shell {
                r.check(seen.old_gone, || format!("the shell that ran `exit` (pid {}) was never collected", old.0));
            }
            match &seen.new_shell_why {
                Some(why) => r.fail(why.clone()),
                None => r.check(seen.new_shell.is_some(), || String::from("after `exit` no new process running `sh` was alive")),
            }
            if let (Some(p), Some(_)) = (seen.new_shell, seen.orphan) {
                let first = seen.high + 1;
                r.check(p.0 == first || seen.new_shell_ppid.is_some_and(|q| q.0 == first), || {
                    format!(
                        "after `exit`, init started another process before the new shell (pid {}, not pid {}): when the shell ends, the next process init starts is the shell",
                        p.0,
                        seen.high + 1
                    )
                });
            }
            if let Some(after) = &seen.shells_after {
                r.check(after.len() <= 1, || {
                    format!(
                        "after `echo {}`, more than one process was running `sh`, {} of them ({}); init keeps one shell, and starts the next when it ends",
                        w.w2,
                        after.len(),
                        listed(after)
                    )
                });
            }
            r.check(has_line(o, &w.w2), || format!("`echo {}` at the new prompt printed {} on fd 1", w.w2, shown(o)));
        }
    }
    r.finish(&seen.stop);

    // 7: pid 1.
    let mut r = Row::new(7, CMDLINE_ID);
    match find(Pid(1)) {
        None => r.fail(String::from("there is no pid 1")),
        Some(p) => {
            let want = ["init", &nonce_text(w)].map(String::from);
            r.check(p.exit.is_none(), || format!("pid 1 ended with {}; init never ends", status_text(p.exit.unwrap_or(0))));
            r.check(seen.init == Some(Pid(1)), || format!("pid 1 is not the {INIT} this file started"));
            r.check(p.argv[..] == want[..], || format!("pid 1's argv is {:?}, not {want:?}", p.argv));
        }
    }
    r.finish(&seen.stop);
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
    let mut seen = Seen::default();
    let mut out = Vec::new();
    let mut err = Vec::new();
    match ktap::nonce() {
        None => seen.stop = Some(String::from("this run carries no nonce, and every row is drawn from it")),
        Some(nonce) => {
            let w = words(nonce);
            if let Err(why) = mount_disk() {
                seen.stop = Some(format!("the file systems did not mount: {why}"));
            } else {
                let r = session(&w, &mut seen, &mut out, &mut err);
                if let Err(why) = r {
                    note(format_args!("init: the session stopped: {why}"));
                    seen.stop = Some(why);
                }
            }
            irq_off();
            kernel::kprintln!();
            rows(&w, &seen, &out, &err);
            ktap::end();
            halt()
        }
    }
    // No nonce: every row says so.
    irq_off();
    let why = seen.stop.unwrap_or_default();
    for (n, id) in [NONCE_ID, MOUNTS_ID, SHELL_ID, FDS_ID, ORPHAN_ID, RESPAWN_ID, CMDLINE_ID].into_iter().enumerate() {
        ktap::note(format_args!("{id}: {why}"));
        kernel::kprintln!("not ok {} - {id}", n + 1);
    }
    ktap::end();
    halt()
}
