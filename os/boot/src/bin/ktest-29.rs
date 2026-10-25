//! Lab 29's vehicle (`29-your-shell`): the learner's own shell, user-mode
//! Rust on the course's crate (`userland/sh.rs`, `user/rust/`), installed as
//! `/bin/ush`. Lab 29 adds no kernel interface (CONTRACT §B.2-29): everything
//! the shell calls is labs 09–27's, and this file only starts init, types
//! into the shells and watches.
//!
//! The disk is `ktest-29.img` as the course's image writer made it (CONTRACT
//! §A.7): `/bin/init` is the learner's init (lab 28), `/bin/sh` the course's
//! shell, and from lab 29 `/bin/ush` the learner's shell and `/bin/mark` the
//! course program that ends each group of the script (`mark N` prints
//! `mark N <cwd>`). The vehicle mounts ext2 at `/` and the boot archive at
//! `/boot` (init mounts `/proc` and `/dev`), then starts `/bin/init <nonce>`
//! through the learner's exec as pid 1, with the console as fd 0 and two
//! pipes this file drains as fds 1 and 2 — the TEE (OUT and ERR), forwarded
//! to the serial line with `# ` at the start of every line, as `ktest-28`
//! does. init starts the course shell; everything below is typed through
//! `kbd::push` (shifted keys with the shift key's make and break codes, in
//! chunks the console's reader takes before the next is pushed).
//!
//! THE SCRIPT is lab 23's, trimmed to lab 29's grammar (RATIFY Q6 as amended
//! 2026-10-23: words, `/bin/<name>`, `>`, `<`, one `|`, `$?`, the builtins
//! `cd`, `echo` and `exit`; no quoting, no `$NAME`). Six groups, each ending
//! with `mark N`; the values marked * are drawn from the nonce:
//!   1 `ls`, `mark 1`
//!   2 `hi <nonce> 1`, `ls /proc`, `mark 2`
//!   3 `cat /n*`, `echo $?`, `x*`, `echo $?`, `mark 3`
//!   4 `echo a* b* > /tmp/u*`, `cat /tmp/u*`, `echo c* > /tmp/u*`,
//!     `cat < /tmp/u*`, `cat /proc/sboot/nonce > /dev/null`, `mark 4`
//!   5 `producer <nonce> 1 chain | consumer <nonce> 1 chain 3 4`, `mark 5`
//!   6 `cd <dir*>`, `mark 6`
//! It runs twice in one session. First through the course shell — the
//! ORACLE — whose answers this file checks against what it knows they must
//! be (a group the course shell gets wrong is the kernel's fault, and says
//! so); then `cd /`, `ush` typed at the course shell's prompt, and the same
//! script through the learner's shell, which ends with `exit` (the course
//! shell's `echo $?` after it must print 0). Each typed line waits for the
//! next prompt (`$ `), bounded by the PM timer (`STEP_MS` a line), never by
//! the learner's clock; a shell that ends, or a line that never comes back
//! to a prompt, stops the session there, and the rows it did not reach say
//! what did not happen.
//!
//! The rows, printed after the session in this order. Rows 1–6 compare what
//! the learner's shell printed on OUT for each line of their group with what
//! the course shell printed for the same line in the same run (programs'
//! notes — `# …` lines: pids and read counts that differ from run to run —
//! and, after `ls /proc`, the pid lines left out); ERR is not compared (each
//! shell words its own messages), but a `panicked at` line on it is quoted.
//!   1 `ush.command.runs` (plain): the prompt, a one-word command run from
//!     `/bin` (fork, exec, wait), and one with an argument.
//!   2 `ush.args.passed` (derived): `hi` gets the nonce and its index as
//!     typed; `ls /proc` lists `/proc`.
//!   3 `ush.status.shown` (derived): `echo $?` prints 1 after a failing
//!     command and 127 after a name not found.
//!   4 `ush.redirect.files` (derived): `>` creates and empties the file it
//!     names, `<` reads it, `echo` joins its words with one space, and
//!     `/dev/null` takes output.
//!   5 `ush.pipe.two` (derived): a pipeline carries the nonce's stream from
//!     `producer` to `consumer` whole, ends at its end, and leaves no pipe
//!     end open in the consumer (consumer's own rows, compared).
//!   6 `ush.cd.exit` (derived): `cd` moves the shell itself (the next
//!     command starts in the drawn directory), and `exit` ends the shell with
//!     status 0.
//!   7 `ush.oracle.kernel` (derived): the course shell's run of the script
//!     gave every answer it must, and the learner's shell got at least one
//!     group the same; red names the group when the kernel, not the
//!     learner's shell, is wrong, or says the learner's shell got none
//!     (amended 2026-10-23, slice 8 fix round: a shell that never ran, or
//!     matched no group, scores 0 of 7, not this row alone).
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
use alloc::vec;
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
/// The six groups' row ids, in the script's order.
const GROUP_IDS: [&str; 6] = ["ush.command.runs", "ush.args.passed", "ush.status.shown", "ush.redirect.files", "ush.pipe.two", "ush.cd.exit"];
const ORACLE_ID: &str = "ush.oracle.kernel";

/// init, and the name it is started under.
const INIT: &str = "/bin/init";
/// The learner's shell, as typed at the course shell's prompt (`/bin/ush`).
const USH: &str = "ush";
/// How long one typed line may take to come back to a prompt, and a shell
/// to be back in its `read` after one, by the PM timer.
const STEP_MS: u64 = 15_000;
/// The most scancodes pushed before the console's reader has taken them
/// (the ring holds 64).
const CHUNK: usize = 32;
/// fcntl's `O_NONBLOCK`, on the tee's read ends.
const O_NONBLOCK: u32 = 0x800;
/// The directories `cd` is drawn from: ext2's own, so no mount is crossed.
const DIRS: [&str; 6] = ["/t", "/a", "/a/b", "/a/b/c", "/tmp", "/bin"];

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

fn find(pid: Pid) -> Option<Rec> {
    procs().into_iter().find(|r| r.pid == pid)
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

/// The scancodes that type `line` and Enter: a shifted symbol is the left
/// shift key's make code, the key's, and shift's break code (kbd's
/// `translate` follows them).
fn codes(line: &str) -> Vec<u8> {
    const SHIFTED: [(u8, u8); 5] = [(b'>', 0x34), (b'<', 0x33), (b'|', 0x2b), (b'$', 0x05), (b'?', 0x35)];
    let mut out = Vec::new();
    for c in line.bytes().chain([b'\n']) {
        match SHIFTED.iter().find(|&&(k, _)| k == c) {
            Some(&(_, sc)) => out.extend_from_slice(&[0x2a, sc, 0xaa]),
            None => out.extend(make_code(c)),
        }
    }
    out
}

/// Task `t` is alive and off the CPU and the run queue: asleep (a shell
/// in its `read` of the console, after a prompt).
fn asleep(t: Tid) -> bool {
    quiet(|| task::with_table(|tt| tt.current != t && !tt.ready.contains(&t) && tt.get(t).is_some_and(|x| x.state != task::State::Dead)))
}

/// Process `pid` has ended (exited, or already collected).
fn ended(pid: Pid) -> bool {
    find(pid).is_none_or(|r| r.exit.is_some())
}

// ── The script ───────────────────────────────────────────────────────────

/// What a line's OUT must be under the course shell.
enum Want {
    /// Each of these lines is on OUT (others may be too).
    Has(Vec<String>),
    /// OUT is exactly these lines (notes aside: see `norm`).
    Is(Vec<String>),
}

struct Line {
    text: String,
    want: Want,
    /// The line lists `/proc`: its pid lines are left out of the comparison.
    pids: bool,
}

fn line(text: &str, want: Want) -> Line {
    Line { text: String::from(text), want, pids: false }
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| String::from(*s)).collect()
}

/// The words the script draws from the nonce.
struct Words {
    nonce: [u8; 32],
    file: String,
    wa: String,
    wb: String,
    wc: String,
    nofile: String,
    noname: String,
    dir: &'static str,
}

fn words(nonce: [u8; 32]) -> Words {
    let d = |id: &str| ktap::derive(&nonce, id);
    Words {
        nonce,
        file: format!("/tmp/u{:08x}", d("ush.redirect.files/file") as u32),
        wa: format!("a{:08x}", d("ush.redirect.files/a") as u32),
        wb: format!("b{:08x}", d("ush.redirect.files/b") as u32),
        wc: format!("c{:08x}", d("ush.redirect.files/c") as u32),
        nofile: format!("/n{:08x}", d("ush.status.shown/file") as u32),
        noname: format!("x{:08x}", d("ush.status.shown/name") as u32),
        dir: DIRS[(d("ush.cd.exit") % DIRS.len() as u64) as usize],
    }
}

fn nonce_text(w: &Words) -> String {
    String::from_utf8_lossy(&w.nonce).into_owned()
}

fn mark(n: usize, cwd: &str) -> Line {
    line(&format!("mark {n}"), Want::Is(vec![format!("mark {n} {cwd}")]))
}

/// The six groups, both shells' script.
fn script(w: &Words) -> Vec<Vec<Line>> {
    let n = nonce_text(w);
    let none = || Want::Is(Vec::new());
    vec![
        vec![line("ls", Want::Has(strs(&["a", "bin", "boot", "dev", "proc", "t", "tmp"]))), mark(1, "/")],
        vec![
            line(
                &format!("hi {n} 1"),
                Want::Has(vec![
                    format!("hi: cs=0x2b rpl=3 nonce={n}"),
                    format!("# derive={:016x}", ktap::derive(&w.nonce, "ring3.cs.rpl")),
                    String::from("ok 1 - ring3.cs.rpl"),
                ]),
            ),
            Line { pids: true, ..line("ls /proc", Want::Has(strs(&["1", "meminfo", "mounts", "sboot", "uptime"]))) },
            mark(2, "/"),
        ],
        vec![
            line(&format!("cat {}", w.nofile), none()),
            line("echo $?", Want::Is(strs(&["1"]))),
            line(&w.noname, none()),
            line("echo $?", Want::Is(strs(&["127"]))),
            mark(3, "/"),
        ],
        vec![
            line(&format!("echo {} {} > {}", w.wa, w.wb, w.file), none()),
            line(&format!("cat {}", w.file), Want::Is(vec![format!("{} {}", w.wa, w.wb)])),
            line(&format!("echo {} > {}", w.wc, w.file), none()),
            line(&format!("cat < {}", w.file), Want::Is(vec![w.wc.clone()])),
            line("cat /proc/sboot/nonce > /dev/null", none()),
            mark(4, "/"),
        ],
        vec![
            line(
                &format!("producer {n} 1 chain | consumer {n} 1 chain 3 4"),
                Want::Has(strs(&["ok 1 - pipe.chain.checksum", "ok 2 - pipe.eof.last_writer", "ok 3 - pipe.cloexec"])),
            ),
            mark(5, "/"),
        ],
        vec![line(&format!("cd {}", w.dir), none()), mark(6, w.dir)],
    ]
}

/// OUT's lines as the rows compare them: a trailing prompt dropped, and
/// two kinds of line left out — notes (`# …`: `hi`'s pid, `consumer`'s read
/// counts, which differ from run to run whatever the shell) and, when `pids`,
/// lines that are only digits (`/proc`'s pids: the two shells' processes
/// have different pids).
fn norm(out: &[u8], pids: bool) -> Vec<&[u8]> {
    let out = out.strip_suffix(b"$ ").unwrap_or(out);
    let out = out.strip_suffix(b"\n").unwrap_or(out);
    if out.is_empty() {
        return Vec::new();
    }
    out.split(|&b| b == b'\n').filter(|l| !(pids && !l.is_empty() && l.iter().all(u8::is_ascii_digit)) && !l.starts_with(b"# ")).collect()
}

fn shown(bytes: &[u8]) -> String {
    let b = bytes.strip_suffix(b"$ ").unwrap_or(bytes);
    format!("{:?}", String::from_utf8_lossy(b))
}

/// Whether the course shell's OUT for `l` is what it must be.
fn check(l: &Line, out: &[u8]) -> Result<(), String> {
    match &l.want {
        Want::Has(want) => {
            let lines: Vec<&[u8]> = out.split(|&b| b == b'\n').collect();
            match want.iter().find(|w| !lines.contains(&w.as_bytes())) {
                None => Ok(()),
                Some(w) => Err(format!("`{}` printed {} on fd 1, with no line `{w}`", l.text, shown(out))),
            }
        }
        Want::Is(want) => {
            let got = norm(out, l.pids);
            if got.len() == want.len() && got.iter().zip(want).all(|(g, w)| *g == w.as_bytes()) {
                Ok(())
            } else if want.is_empty() {
                Err(format!("`{}` printed {} on fd 1, where nothing belongs", l.text, shown(out)))
            } else {
                Err(format!("`{}` printed {} on fd 1, not the line(s) {want:?}", l.text, shown(out)))
            }
        }
    }
}

// ── The session ──────────────────────────────────────────────────────────

/// What one typed line left: OUT and ERR from the moment it was typed to
/// the next prompt.
struct Seg {
    out: Vec<u8>,
    err: Vec<u8>,
}

/// One shell's run of the script: each group's segments, as far as it got,
/// and where and why it stopped (the group, `None` before the first).
#[derive(Default)]
struct Pass {
    segs: Vec<Vec<Seg>>,
    stop: Option<(Option<usize>, String)>,
}

/// What the session saw, for the rows.
#[derive(Default)]
struct Seen {
    oracle: Pass,
    ush: Pass,
    /// `exit` in the learner's shell: it ended and the course shell's
    /// `echo $?` printed 0, or why not.
    exit: Option<Result<(), String>>,
}

/// A shell being typed at: its process and its task.
#[derive(Clone, Copy)]
struct Sh {
    pid: Pid,
    tid: Tid,
}

/// Why a typed line did not come back.
enum Lost {
    /// The shell that should be at the next prompt ended.
    Ended(String),
    /// No prompt (or no `read`) within the bound, or init ended.
    Stuck(String),
}

impl Lost {
    fn why(self) -> String {
        match self {
            Lost::Ended(s) | Lost::Stuck(s) => s,
        }
    }
}

struct Session {
    out: Tee,
    err: Tee,
    init: Pid,
}

impl Session {
    fn pump(&mut self) {
        self.out.pump();
        self.err.pump();
    }
    /// Pumps the tee and yields until `done` holds, init ends, or `ms` pass.
    fn wait(&mut self, ms: u64, what: &str, mut done: impl FnMut(&mut Session) -> bool) -> Result<(), String> {
        let mut c = Clock::new();
        loop {
            self.pump();
            if done(self) {
                return Ok(());
            }
            if let Some(v) = find(self.init).and_then(|r| r.exit) {
                self.pump();
                let last = self.err.all.split(|&b| b == b'\n').filter(|l| !l.is_empty()).last().map(String::from_utf8_lossy);
                return Err(match last {
                    Some(l) => format!("init (pid {}) ended with {} before {what}; the last line it or its children wrote on fd 2: {l}", self.init.0, status_text(v)),
                    None => format!("init (pid {}) ended with {} before {what}, and wrote nothing on fd 2", self.init.0, status_text(v)),
                });
            }
            if c.ms() >= ms {
                return Err(format!("{what} did not happen within {ms} ms"));
            }
            sched::yield_now();
        }
    }
    /// The line of ERR written since byte `from` that says most about an
    /// end: the first `panicked at` line, else the last line.
    fn err_since(&self, from: usize) -> Option<String> {
        let tail = &self.err.all[from.min(self.err.all.len())..];
        let lines: Vec<&[u8]> = tail.split(|&b| b == b'\n').filter(|l| !l.is_empty()).collect();
        let pick = lines.iter().find(|l| l.windows(11).any(|w| w == b"panicked at")).or(lines.last());
        pick.map(|l| String::from_utf8_lossy(l).into_owned())
    }
    /// Waits until `sh` is asleep (back in its `read`) or has ended; true
    /// when it is asleep.
    fn settle(&mut self, sh: Sh, what: &str) -> Result<bool, String> {
        self.wait(STEP_MS, what, |_| asleep(sh.tid) || ended(sh.pid))?;
        Ok(!ended(sh.pid))
    }
    /// Types `text` and Enter to `reader`, a chunk at a time: each chunk is
    /// pushed only once the reader has taken the last one into its line.
    fn type_to(&mut self, reader: Sh, text: &str) -> Result<(), Lost> {
        let codes = codes(text);
        for (i, chunk) in codes.chunks(CHUNK).enumerate() {
            if i > 0 {
                sched::yield_now();
                match self.settle(reader, &format!("the console's reader taking `{text}`")) {
                    Ok(true) => {}
                    Ok(false) => return Err(Lost::Ended(format!("the shell (pid {}) ended while `{text}` was being typed", reader.pid.0))),
                    Err(why) => return Err(Lost::Stuck(why)),
                }
            }
            quiet(|| chunk.iter().for_each(|&sc| kbd::push(sc)));
        }
        Ok(())
    }
    /// Types `text` at `reader`'s prompt, waits for the next prompt and for
    /// `after` (the shell that should print it) to be back in its `read`, and
    /// returns what came on OUT and ERR in between.
    fn line(&mut self, reader: Sh, after: Sh, text: &str) -> Result<Seg, Lost> {
        let (o, e) = (self.out.all.len(), self.err.all.len());
        self.type_to(reader, text)?;
        let bound = format!("a prompt after `{text}`");
        self.wait(STEP_MS, &bound, |s| s.out.take_prompt()).map_err(Lost::Stuck)?;
        let alive = self.settle(after, &bound).map_err(Lost::Stuck)?;
        let seg = Seg { out: self.out.all[o..].to_vec(), err: self.err.all[e..].to_vec() };
        if !alive {
            let said = self.err_since(e).map_or(String::from("nothing on fd 2"), |l| format!("it wrote on fd 2: {l}"));
            return Err(Lost::Ended(format!("the shell (pid {}) ended while running `{text}`; {said}", after.pid.0)));
        }
        Ok(seg)
    }
}

/// The live process running `name` (argv[0]) whose parent is `parent`.
fn running(name: &str, parent: Option<Pid>) -> Option<Sh> {
    let r = procs().into_iter().find(|r| r.exit.is_none() && r.argv.first().map(String::as_str) == Some(name) && parent.is_none_or(|p| r.ppid == p))?;
    let tid = quiet(|| task_of(r.pid))?;
    Some(Sh { pid: r.pid, tid })
}

/// One shell's run of the whole script into `pass`; stops at the first line
/// that does not come back.
fn run_script(s: &mut Session, sh: Sh, script: &[Vec<Line>], pass: &mut Pass) -> Result<(), ()> {
    for (g, group) in script.iter().enumerate() {
        pass.segs.push(Vec::new());
        for l in group {
            match s.line(sh, sh, &l.text) {
                Ok(seg) => pass.segs[g].push(seg),
                Err(lost) => {
                    pass.stop = Some((Some(g), lost.why()));
                    return Err(());
                }
            }
        }
    }
    Ok(())
}

/// Starts init and drives the session; fills `seen` as far as it gets.
fn session(w: &Words, script: &[Vec<Line>], seen: &mut Seen) {
    let fail = |seen: &mut Seen, why: String| {
        seen.oracle.stop = Some((None, why.clone()));
        seen.ush.stop = Some((None, format!("the course shell never ran the script: {why}")));
    };
    let (out, out_w, err, err_w) = match (Tee::new(), Tee::new()) {
        (Ok((o, ow)), Ok((e, ew))) => (o, ow, e, ew),
        (Err(why), _) | (_, Err(why)) => return fail(seen, why),
    };
    let nonce = nonce_text(w);
    let init = start_with(INIT, || exec::spawn(INIT, &["init", &nonce], &[]), &[(1, &out_w), (2, &err_w)]);
    drop(out_w);
    drop(err_w);
    let init = match init {
        Ok(p) => p,
        Err(why) => return fail(seen, why),
    };
    let mut s = Session { out, err, init };

    // The course shell init starts, at its first prompt.
    if let Err(why) = s.wait(STEP_MS, "the course shell's first prompt", |s| s.out.take_prompt()) {
        return fail(seen, why);
    }
    let Some(course) = running("sh", Some(init)) else {
        return fail(seen, format!("at the first prompt no process with parent {} was running `sh`", init.0));
    };
    match s.settle(course, "the course shell's first read") {
        Ok(true) => {}
        Ok(false) => return fail(seen, String::from("the course shell ended at its first prompt")),
        Err(why) => return fail(seen, why),
    }

    // The oracle: the script through the course shell, then back to `/`.
    if run_script(&mut s, course, script, &mut seen.oracle).is_err() {
        let why = seen.oracle.stop.as_ref().map_or(String::new(), |(_, w)| w.clone());
        seen.ush.stop = Some((None, format!("the course shell's run of the script stopped: {why}")));
        return;
    }
    if let Err(lost) = s.line(course, course, "cd /") {
        let why = lost.why();
        seen.oracle.stop = Some((Some(script.len() - 1), why.clone()));
        seen.ush.stop = Some((None, format!("the course shell's `cd /` did not come back: {why}")));
        return;
    }

    // The learner's shell, by name, at the course shell's prompt.
    let e = s.err.all.len();
    let first = s.type_to(course, USH).map_err(Lost::why).and_then(|()| s.wait(STEP_MS, "your shell's first prompt", |s| s.out.take_prompt()));
    if let Err(why) = first {
        seen.ush.stop = Some((Some(0), why));
        return;
    }
    let ush = match running(USH, Some(course.pid)) {
        Some(sh) => sh,
        None => {
            let said = s.err_since(e).map_or(String::from("nothing on fd 2"), |l| format!("on fd 2: {l}"));
            seen.ush.stop = Some((Some(0), format!("after `{USH}` was typed at the course shell, no process running `{USH}` was waiting for a line; {said}")));
            return;
        }
    };
    match s.settle(ush, "your shell's first read") {
        Ok(true) => {}
        Ok(false) => {
            let said = s.err_since(e).map_or(String::from("nothing on fd 2"), |l| format!("it wrote on fd 2: {l}"));
            seen.ush.stop = Some((Some(0), format!("your shell (pid {}) ended at its first prompt; {said}", ush.pid.0)));
            return;
        }
        Err(why) => {
            seen.ush.stop = Some((Some(0), why));
            return;
        }
    }
    note(format_args!("{}: pid {} runs {USH}; its parent is the course shell, pid {}", GROUP_IDS[0], ush.pid.0, course.pid.0));
    if run_script(&mut s, ush, script, &mut seen.ush).is_err() {
        return;
    }

    // `exit`: the learner's shell ends, the course shell's prompt comes back.
    let last = script.len() - 1;
    seen.exit = Some(match s.line(ush, course, "exit") {
        Err(lost) => Err(format!("after `exit`: {}", lost.why())),
        Ok(_) if !ended(ush.pid) => Err(format!("after `exit` your shell (pid {}) was still running", ush.pid.0)),
        Ok(_) => match s.line(course, course, "echo $?") {
            Err(lost) => {
                seen.ush.stop = Some((Some(last), lost.why()));
                Err(String::from("the course shell's `echo $?` after `exit` did not come back"))
            }
            Ok(seg) if norm(&seg.out, false) == [b"0".as_slice()] => Ok(()),
            Ok(seg) => Err(format!("after `exit` the course shell's `echo $?` printed {}: your shell's exit status was not 0", shown(&seg.out))),
        },
    });
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
    fn finish(self) {
        quiet(|| match self.why {
            None => kernel::kprintln!("ok {} - {}", self.n, self.id),
            Some(why) => {
                ktap::note(format_args!("{}: {why}", self.id));
                kernel::kprintln!("not ok {} - {}", self.n, self.id);
            }
        });
    }
}

/// The course shell's run of group `g`: complete and right, or why not.
fn oracle_group(script: &[Vec<Line>], seen: &Seen, g: usize) -> Result<(), String> {
    let segs = seen.oracle.segs.get(g).map_or(&[][..], Vec::as_slice);
    for (l, seg) in script[g].iter().zip(segs) {
        check(l, &seg.out)?;
    }
    if segs.len() < script[g].len() {
        let why = seen.oracle.stop.as_ref().map_or(String::new(), |(_, w)| w.clone());
        return Err(match seen.oracle.stop {
            Some((Some(sg), _)) if sg == g => why,
            _ => format!("the course shell's run did not get as far as this group: {why}"),
        });
    }
    Ok(())
}

/// The first `panicked at` line in `err`.
fn panicked(err: &[u8]) -> Option<String> {
    let l = err.split(|&b| b == b'\n').find(|l| l.windows(11).any(|w| w == b"panicked at"))?;
    Some(String::from_utf8_lossy(l).into_owned())
}

fn rows(script: &[Vec<Line>], seen: &Seen) {
    // The first of rows 1-6's reasons, and whether any of them was ok.
    let mut first_why: Option<String> = None;
    let mut any_ok = false;
    for (g, id) in GROUP_IDS.iter().enumerate() {
        let mut r = Row::new(g + 1, id);
        if let Err(why) = oracle_group(script, seen, g) {
            r.fail(format!("the course shell's own run of this group went wrong on this kernel, so the kernel is what is wrong here, not your shell: {why}"));
        }
        let segs = seen.ush.segs.get(g).map_or(&[][..], Vec::as_slice);
        let oracle = seen.oracle.segs.get(g).map_or(&[][..], Vec::as_slice);
        for (i, ((l, mine), theirs)) in script[g].iter().zip(segs).zip(oracle).enumerate() {
            if norm(&mine.out, l.pids) != norm(&theirs.out, l.pids) {
                // The first panic on ERR up to this line: a child that died
                // a line earlier is often why this one differs.
                let also = script[g].iter().zip(&segs[..=i]).find_map(|(at, seg)| Some((at, panicked(&seg.err)?)));
                let also = match also {
                    None => String::new(),
                    Some((at, p)) if core::ptr::eq(at, l) => format!("; on fd 2: {p}"),
                    Some((at, p)) => format!("; on fd 2, after `{}`: {p}", at.text),
                };
                r.fail(format!("`{}` printed {} on fd 1; the course shell printed {} for the same line{also}", l.text, shown(&mine.out), shown(&theirs.out)));
            }
        }
        if segs.len() < script[g].len() {
            let why = seen.ush.stop.as_ref().map_or(String::new(), |(_, w)| w.clone());
            r.fail(match seen.ush.stop {
                Some((Some(sg), _)) if sg == g => why,
                _ => format!("the session did not get as far as this group: {why}"),
            });
        }
        if g == GROUP_IDS.len() - 1 {
            match &seen.exit {
                Some(Ok(())) => {}
                Some(Err(why)) => r.fail(why.clone()),
                None => r.fail(String::from("the session did not get as far as `exit`")),
            }
        }
        match &r.why {
            None => any_ok = true,
            Some(why) if first_why.is_none() => first_why = Some(format!("group {}: {why}", g + 1)),
            Some(_) => {}
        }
        r.finish();
    }
    let mut r = Row::new(PLAN, ORACLE_ID);
    for g in 0..GROUP_IDS.len() {
        if let Err(why) = oracle_group(script, seen, g) {
            let first = script[g].first().map_or("", |l| l.text.as_str());
            r.fail(format!("the course shell, on this kernel, got group {} (`{first}` …) wrong: {why}", g + 1));
        }
    }
    if !any_ok {
        let why = first_why.unwrap_or_default();
        r.fail(format!("your shell got no group of the script to the course shell's answers, so this run shows nothing of it working on this kernel; {why}"));
    }
    r.finish();
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
    match ktap::nonce() {
        None => {
            irq_off();
            let why = "this run carries no nonce, and the script is drawn from it";
            for (n, id) in GROUP_IDS.into_iter().chain([ORACLE_ID]).enumerate() {
                ktap::note(format_args!("{id}: {why}"));
                kernel::kprintln!("not ok {} - {id}", n + 1);
            }
        }
        Some(nonce) => {
            let w = words(nonce);
            let script = script(&w);
            let mut seen = Seen::default();
            match mount_disk() {
                Err(why) => {
                    let why = format!("the file systems did not mount: {why}");
                    seen.oracle.stop = Some((None, why.clone()));
                    seen.ush.stop = Some((None, why));
                }
                Ok(()) => session(&w, &script, &mut seen),
            }
            irq_off();
            kernel::kprintln!();
            rows(&script, &seen);
        }
    }
    ktap::end();
    halt()
}
