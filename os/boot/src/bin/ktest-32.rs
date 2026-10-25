//! Lab 32's vehicle (`32-handlers`): signal handlers, through busybox's
//! `ash` and the course's `sigregs` (`user/sigregs/sigregs.c`, layout musl),
//! on the learner's kernel.
//!
//! The disk is `ktest-32.img` as the course's image writer made it (CONTRACT
//! §A.7): lab 31's, plus `/bin/sigregs` (`HANDLER_PROGRAMS`, from lab 32).
//! The vehicle mounts ext2 at `/` from `/dev/hda2` through the learner's
//! `Ata`, `partition2` and `Ext2::mount`; `/proc` and `/dev` are init's.
//!
//! It starts the learner's `/bin/init <nonce>` through the learner's exec,
//! first, so it is pid 1, as ktest-31 does: fd 0 a pipe this file writes the
//! script into, fds 1 and 2 a second pipe it drains (the tee, forwarded as
//! `# | <line>`), and fd 3 the console, close-on-exec, so init holds the
//! console open and nothing it execs does. init forks and execs `/bin/sh`
//! (busybox's `ash`, non-interactive: its stdin is a pipe). Each command
//! group ends with `echo ':q<N>:'`, and the group's output is what came
//! through the tee before that marker. Every word in the script is derived
//! from the run's nonce (`ktap::derive`).
//!
//! The rows, printed in this order:
//!
//!   1 `sig.trap.usr1` (derived) — `trap '…' USR1; kill -USR1 $$`: the
//!     handler ash installed runs, then the next command.
//!   2 `sig.chld.wakes.wait` (derived) — `sleep 2 & wait`: ash's `wait`
//!     sleeps in `rt_sigsuspend` until SIGCHLD, which the kernel sends the
//!     parent when a child ends. Its own bound, `CHLD_MS`: a wait that never
//!     returns ends the run here (the rest of the rows fail, with that reason).
//!   3 `sig.int.ctrl_c` (derived) — an interactive shell that opened the
//!     console as fd 3 (`sh -c 'exec 3</dev/console; exec sh -i +m'`: the
//!     redirection is the child's own, so the session's shell, which `ash`
//!     would otherwise open it in around the fork, never holds the console)
//!     runs `sleep 30`; the vehicle prints `# sboot sigint-ready`, the
//!     script types `discard` and Enter, then ctrl+c. SIGINT reaches every
//!     process holding the console but init: `sleep` dies, the shell is back
//!     at its prompt with `$?` 130, init and the session's shell (which does
//!     not hold it) live on, and the keys typed before ctrl+c are gone (a
//!     non-blocking read of the console then finds nothing).
//!   4 `sig.regs.restored` (derived) — `sigregs regs <hex>`: a handler that
//!     overwrites every register runs from the timer interrupt, and after
//!     `rt_sigreturn` every general register (rcx and r11 included) and xmm1
//!     hold what they held; it prints a fold of them, derived from the nonce.
//!   5 `sig.restart.wait4` (derived) — `sigregs restart <hex>`: a SIGUSR1
//!     handler with `SA_RESTART` interrupts `wait4`, which starts again and
//!     returns the child's status (derived).
//!   6 `sig.sigsuspend.eintr` (derived) — `sigregs suspend <hex>`:
//!     `rt_sigsuspend` returns -1/EINTR once the handler ran, and the mask it
//!     replaced is back.
//!
//! A group that prints no marker within `GROUP_MS` by the PM timer stops the
//! session: the rows still owed fail with the reason. The plan always
//! completes.
//!
//! Plan 6.

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
use kernel::console::{Fd, FileObj, FileRef};
use kernel::exec;
use kernel::fs::ext2::Ext2;
use kernel::fs::mount;
use kernel::fs::vfs::InodeRef;
use kernel::ktap;
use kernel::pipe;
use kernel::proc::{self, Pid};
use kernel::syscall::{self, Errno};
use kernel::task::{self, Tid};
use kernel::{bootinfo, frame, gdt, heap, idt, irq, sched, serial, time};

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 64;
const PM_HZ: u64 = 3_579_545;

const PLAN: usize = 6;
const TRAP_ID: &str = "sig.trap.usr1";
const CHLD_ID: &str = "sig.chld.wakes.wait";
const INT_ID: &str = "sig.int.ctrl_c";
const REGS_ID: &str = "sig.regs.restored";
const RESTART_ID: &str = "sig.restart.wait4";
const SUSPEND_ID: &str = "sig.sigsuspend.eintr";
const IDS: [&str; PLAN] = [TRAP_ID, CHLD_ID, INT_ID, REGS_ID, RESTART_ID, SUSPEND_ID];

/// How long a group may run before the session counts as stopped, by the
/// PM timer.
const GROUP_MS: u64 = 20_000;
/// Row 2's own bound: `sleep 2 & wait` returns in about 2 s when SIGCHLD
/// is delivered, and never when it is not.
const CHLD_MS: u64 = 10_000;
/// Row 3: how long the interactive shell and its `sleep 30` have to appear,
/// and how long ctrl+c has to end the `sleep`, by the PM timer.
const INT_MS: u64 = 15_000;
/// What the practice script and the rubric's `[[script]]` wait for before
/// they type (CONTRACT §D.7).
const SIGINT_READY: &str = "# sboot sigint-ready";

/// The learner's init (lab 28), and how long it has to print its hello line
/// and fork the shell, by the PM timer.
const INIT: &str = "/bin/init";
const INIT_MS: u64 = 10_000;

// errnos, as literals: the vehicle builds at every cut.
const EAGAIN: Errno = 11;
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
        14 => "EFAULT",
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
    w: [String; 12],
    regs: u64,
    restart: u64,
    suspend: u64,
}

fn words() -> Words {
    let n = ktap::nonce().unwrap_or([b'0'; 32]);
    let d = |id: &str| ktap::derive(&n, id);
    Words {
        nonce: n,
        w: core::array::from_fn(|k| format!("s{:08x}", d(&format!("sig.word/{k}")) & 0xffff_ffff)),
        regs: d("sig.regs"),
        restart: d("sig.restart"),
        suspend: d("sig.suspend"),
    }
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// What `sigregs regs <nonce>` prints when every register came back:
/// sigregs.c's fold of the fifteen values and xmm1's two halves.
fn regs_fold(nonce: u64) -> u64 {
    let mut fold = nonce;
    for i in 0..15 {
        fold = splitmix64(fold ^ splitmix64(nonce.wrapping_add(i)));
    }
    let x0 = splitmix64(nonce ^ 0x786d_6d31);
    let x1 = splitmix64(x0);
    fold = splitmix64(fold ^ x0);
    splitmix64(fold ^ x1)
}

// ── The session ──────────────────────────────────────────────────────────

fn forward(bytes: &[u8]) {
    for line in bytes.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
        note(format_args!("| {}", String::from_utf8_lossy(line)));
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

/// Rows 1-6. Returns early, after failing what is left, when the session
/// cannot go on.
fn session(w: &Words) {
    let [w0, w1, w2, w3, w4, w5, w6, w7, w8, w9, w10, w11] = &w.w;
    let mut sh = match Sh::start(w) {
        Ok(sh) => sh,
        Err(why) => return fail_rest(1, &format!("no shell ran: {why}")),
    };
    note(format_args!("session: init is pid {}, and /bin/sh, its child, pid {}; the script's words are derived from the nonce", sh.init.0, sh.pid.0));

    // Row 1: a handler ash installed runs.
    let g = sh.group(1, &format!("trap 'echo {w0}' USR1\nkill -USR1 $$\necho {w1}\n"), GROUP_MS);
    let mut r = Row::new(TRAP_ID);
    expect(&mut r, 1, &g, &format!("{w0}\n{w1}\n"), "`kill -USR1 $$` sends ash its own SIGUSR1, whose handler (the trap) runs before the next command, on a frame below its stack, and comes back through rt_sigreturn");
    r.finish(1);

    // Row 2: SIGCHLD wakes `wait`.
    let g = sh.group(2, &format!("sleep 2 & wait\necho \"{w2}=$?\"\n"), CHLD_MS);
    let mut r = Row::new(CHLD_ID);
    if g.is_err() && sh.stopped.as_deref().is_some_and(|s| s.contains("printed no marker")) {
        note(format_args!("SIGCHLD was not delivered: wait did not return"));
        r.fail(format_args!(
            "`sleep 2 & wait` did not return within {CHLD_MS} ms: ash's `wait` sleeps in rt_sigsuspend (130) until SIGCHLD, which the kernel sends a parent when its child ends; the run ends here"
        ));
        r.finish(2);
        return fail_rest(3, "not run: the session ended at row 2 (SIGCHLD was not delivered: wait did not return)");
    }
    expect(&mut r, 2, &g, &format!("{w2}=0\n"), "`wait` returns 0 once the background `sleep 2` has ended");
    r.finish(2);

    // Row 3: ctrl+c.
    int_row(&mut sh, w, w5, w6);

    // Rows 4-6: sigregs.
    let g = sh.group(4, &format!("sigregs regs {:016x}\n", w.regs), GROUP_MS);
    let mut r = Row::new(REGS_ID);
    expect(
        &mut r,
        4,
        &g,
        &format!("sigregs regs {:016x}\n", regs_fold(w.regs)),
        "the frame saves every register and xmm1 where the handler can see them (rt_sigframe: siginfo, ucontext, fpstate), and rt_sigreturn puts each back — rcx and r11 too, so not through sysretq",
    );
    r.finish(4);

    let g = sh.group(5, &format!("sigregs restart {:016x}\n", w.restart), GROUP_MS);
    let mut r = Row::new(RESTART_ID);
    expect(
        &mut r,
        5,
        &g,
        &format!("sigregs restart pid matched status {}\n", w.restart & 0x7f),
        "a wait4 a signal interrupted starts again when the handler's action has SA_RESTART (rax back to 61, rip back on the `syscall`)",
    );
    r.finish(5);

    let g = sh.group(6, &format!("sigregs suspend {:016x}\n", w.suspend), GROUP_MS);
    let mut r = Row::new(SUSPEND_ID);
    expect(
        &mut r,
        6,
        &g,
        &format!("sigregs suspend EINTR {:016x}\n", w.suspend),
        "rt_sigsuspend (130) sleeps with the mask it is given, returns -EINTR once a handler has run, and the mask it replaced comes back",
    );
    r.finish(6);
    let _ = (w3, w4, w7, w8, w9, w10, w11);
}

/// Row 3: ctrl+c at an interactive shell's `sleep 30`.
fn int_row(sh: &mut Sh, w: &Words, w5: &str, w6: &str) {
    let _ = w;
    let mut r = Row::new(INT_ID);
    let n = 3;
    let from = sh.out.len();
    let fail = |r: &mut Row, sh: &Sh, why: String| {
        forward(&sh.out[from..]);
        r.fail(format_args!("{why}"));
    };
    // Not `sh -i +m 3</dev/console`: ash opens a command's redirections in
    // itself around the fork, so the session's shell would hold the console
    // while it waits, and ctrl+c would end it too.
    if let Err(why) = sh.send("sh -c 'exec 3</dev/console; exec sh -i +m'\n") {
        r.fail(format_args!("{why}"));
        return r.finish(n);
    }
    if !sh.until(INT_MS, |_| live_with(&["sh", "-i"]).is_some()) {
        fail(&mut r, sh, format!("the interactive shell (`sh -c 'exec 3</dev/console; exec sh -i +m'`) was not running within {INT_MS} ms by the PM timer"));
        sh.stopped = Some(String::from("the interactive shell of row 3 never ran"));
        return r.finish(n);
    }
    let inner = live_with(&["sh", "-i"]).unwrap_or(Pid(0));
    let _ = sh.send(&format!("echo {w5}; sleep 30\n"));
    if !sh.until(INT_MS, |_| live_with(&["sleep", "30"]).is_some()) {
        fail(&mut r, sh, format!("the interactive shell (pid {}) ran no `sleep 30` within {INT_MS} ms by the PM timer", inner.0));
        sh.stopped = Some(String::from("row 3's `sleep 30` never ran"));
        return r.finish(n);
    }
    let sleeper = live_with(&["sleep", "30"]).unwrap_or(Pid(0));
    quiet(|| kernel::kprintln!("{SIGINT_READY}"));
    let ended = sh.until(INT_MS, |_| exit_of(sleeper).is_some() || task_of(sleeper).is_none() || live_with(&["sleep", "30"]).is_none());
    if !ended {
        fail(
            &mut r,
            sh,
            format!(
                "ctrl+c did not end `sleep 30` (pid {}) within {INT_MS} ms: SIGINT goes to every process holding the console open but init — the interactive shell and its `sleep` inherited it as fd 3",
                sleeper.0
            ),
        );
        sh.stopped = Some(String::from("row 3's `sleep 30` outlived ctrl+c"));
        return r.finish(n);
    }
    let _ = sh.send(&format!("echo \"{w6}=$?\"; exit 0\n"));
    let marker = format!("{w6}=");
    let got_rc = sh.until(INT_MS, |s| s.out[from..].windows(marker.len()).any(|x| x == marker.as_bytes()) && live_with(&["sh", "-i"]).is_none());
    let text = String::from_utf8_lossy(&sh.out[from..]).into_owned();
    let rc = text.lines().find_map(|l| l.split(&marker).nth(1)).map(|v| String::from(v.trim()));
    if !got_rc {
        fail(&mut r, sh, format!("after ctrl+c the interactive shell printed no `{w6}=$?` line and did not exit within {INT_MS} ms: it should be back at its prompt"));
        sh.stopped = Some(String::from("row 3's shell did not come back from ctrl+c"));
        return r.finish(n);
    }
    if !text.contains(w5) {
        r.fail(format_args!("the interactive shell never printed `{w5}`, the command before `sleep 30`"));
    }
    if rc.as_deref() != Some("130") {
        r.fail(format_args!(
            "after ctrl+c, `$?` at the interactive shell is {:?}; a `sleep` killed by SIGINT is 128 + 2 = 130, and the shell is back at its prompt",
            rc
        ));
    }
    if exit_of(sh.pid).is_some() || task_of(sh.pid).is_none() {
        r.fail(format_args!(
            "the session's shell (pid {}) {} after ctrl+c: it does not hold the console open, so SIGINT is not its to get",
            sh.pid.0,
            status_text(exit_of(sh.pid))
        ));
    }
    if exit_of(sh.init).is_some() {
        r.fail(format_args!("init (pid {}) {} after ctrl+c: it holds the console too, and pid 1 is never sent SIGINT", sh.init.0, status_text(exit_of(sh.init))));
    }
    // The keys typed before ctrl+c (`discard`, Enter) are gone.
    match FileRef::open(FileObj::CONSOLE, O_NONBLOCK) {
        Ok(con) => {
            let mut buf = [0u8; 64];
            match con.read(&mut buf) {
                Err(EAGAIN) => {}
                Ok(k) => r.fail(format_args!(
                    "after ctrl+c a non-blocking read of the console returned {:?}: the keys typed before ctrl+c are thrown away (ISIG), so it finds nothing (-EAGAIN)",
                    String::from_utf8_lossy(&buf[..k])
                )),
                Err(e) => r.fail(format_args!("a non-blocking read of the console after ctrl+c returned {}; nothing is waiting, so -EAGAIN", errno_text(e))),
            }
        }
        Err(e) => r.fail(format_args!("FileRef::open(the console, O_NONBLOCK) returned {}", errno_text(e))),
    }
    forward(&sh.out[from..]);
    r.finish(n);
    // The outer shell carries on with the next group once the inner one ended.
    let from = sh.out.len();
    if sh.send("echo ':q3:'\n").is_ok() {
        let _ = sh.collect(3, from, GROUP_MS);
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
    match mount_all() {
        Err(why) => fail_rest(1, &format!("not run: the file systems did not mount: {why}")),
        Ok(()) => session(&words()),
    }
    irq_off();
    ktap::end();
    halt()
}
