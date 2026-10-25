//! Lab 31's vehicle (`31-signals`): `kill`, dispositions and the default
//! actions, through busybox's `ash`, on the learner's kernel.
//!
//! The disk is `ktest-31.img` as the course's image writer made it (CONTRACT
//! §A.7): lab 30's, unchanged (`sigregs` is lab 32's, `HANDLER_PROGRAMS`).
//! The vehicle mounts ext2 at `/` from `/dev/hda2` through the learner's
//! `Ata`, `partition2` and `Ext2::mount`; `/proc` and `/dev` are init's.
//!
//! It starts the learner's `/bin/init <nonce>` through the learner's exec,
//! first, so it is pid 1, as ktest-30 does: fd 0 a pipe this file writes the
//! script into, fds 1 and 2 a second pipe it drains (the tee, forwarded as
//! `# | <line>`). init forks and execs `/bin/sh` (busybox's `ash`,
//! non-interactive: its stdin is a pipe). Each command group ends with
//! `echo ':q<N>:'`, and the group's output is what came through the tee
//! before that marker. Every word in the script is derived from the run's
//! nonce (`ktap::derive`).
//!
//! Before init first runs (it is spawned, not yet scheduled), the vehicle
//! calls `kill` (62) through `syscall::dispatch` — lab 31 step 1, the only
//! row no program takes part in.
//!
//! No row needs a handler to run: lab 31 acts on what is pending on every
//! way back from a syscall (an ignored signal goes, a default action ends
//! the process) and skips a handler; lab 32 runs them. ash catches SIGCHLD
//! and its traps, so nothing here waits for one (no `wait`).
//!
//! The rows, printed in this order:
//!
//!   1 `sig.kill` (K, plain) — `kill(init, 0)` = 0 (signal 0 only asks);
//!     `kill(init, 65)` = `-EINVAL`; `kill(0, 15)`, `kill(-1, 15)` and
//!     `kill(99999, 0)` = `-ESRCH` (no process groups: a pid ≤ 0 names none).
//!   2 `sig.ignore` (derived) — `sh -c 'trap "" USR1; kill -USR1 $$; echo
//!     …'`: SIG_IGN, so the shell lives on to run the next command and exits
//!     0 (a child shell, so a signal not dropped ends it, not the session).
//!   3 `sig.default.terminate` (derived) — `sh -c 'kill -SEGV $$'` and
//!     `sh -c 'kill -USR1 $$'`: the default action ends each shell, its
//!     status the signal (`$?` 139 and 138; ash names the signal).
//!   4 `sig.default.ignore` (derived) — SIGCHLD, SIGWINCH and SIGURG to a
//!     background `sleep 30 &`: their default is to ignore, so they neither
//!     end it nor end its sleep, and a second later it is still there.
//!   5 `sig.kill.other` (derived) — a second background `sleep 30 &`, a
//!     second to fall asleep, then `kill $!` sends it SIGTERM: the signal
//!     ends its sleep at once (`sleep_until_intr`), and it ends, killed by
//!     signal 15, long before its 30 s are up. Its own process, so row 4's
//!     outcome does not decide it.
//!   6 `sig.pipe` (derived) — `yes | head -n 1` in a subshell that reports
//!     yes's status: 141 (SIGPIPE's default kills it); with SIGPIPE ignored,
//!     yes gets `-EPIPE` and says `yes: : Broken pipe`, status 1 (busybox
//!     1.37.0's own text: `bb_perror_nomsg_and_die` passes `"%s"`, whose
//!     first byte is not NUL, to `bb_verror_msg`, so the `: ` stays).
//!   7 `sig.intr.read` (derived) — `sleep 30 | cat &`: `cat` (`$!`, the
//!     pipeline's last process) is asleep reading a pipe no one writes to;
//!     `kill -<sig> $!`, the signal drawn from the nonce among five whose
//!     default action terminates (HUP, USR1, USR2, ALRM, TERM): the signal
//!     ends the pipe read's sleep at once (`sched::sleep_intr`), and `cat`
//!     ends, killed by that signal, long before the writer's 30 s are up.
//!   8 `sig.intr.wait` (derived) — `sh -c 'sleep 30;:' &`: the child shell
//!     (`$!`) runs `sleep 30` in the foreground, so it is asleep in `wait4`
//!     (the `:` after it keeps ash from exec'ing `sleep` in its place);
//!     `kill -<sig> $!` with a second drawn signal ends `wait4`'s sleep, and
//!     the shell ends, killed by it, not 30 s later when its child exits.
//!     (Added 2026-10-23, slice 8 fix round: rows 7 and 8 grade the two
//!     sleeps a signal ends that row 5's `nanosleep` does not reach.)
//!   9 `sig.intr.write` (derived) — `yes | sleep 30 &`: `sleep` never
//!     reads, so `yes` fills the pipe's 4096 bytes and is asleep in `write`
//!     waiting for room. `$!` is `sleep`, so the writer is found as the
//!     shell's live child running `yes`; a second after the job starts it
//!     must still run and a writer must have waited for room since the group
//!     began (`pipe::full_waits`). Then `kill -<sig> <its pid>`, a third
//!     drawn signal: the signal ends the write's sleep at once, and `yes`
//!     ends, killed by that signal.
//!  10 `sig.intr.console` (derived) — `cat </dev/console &` (the redirect
//!     replaces the `/dev/null` ash gives a background job's stdin without
//!     job control): no key is typed, so `cat` is asleep in the console's
//!     cooked read waiting for a line, and a second later it still runs;
//!     `kill -<sig> $!`, a fourth drawn signal, ends that sleep at once, and
//!     `cat` ends, killed by it. Rows 9 and 10 run in two halves (`:q9:` then
//!     `:q9b:`), so the process is seen asleep before the signal is sent.
//!     (Added 2026-10-23, slice 8 fix round 2: the pipe write's and the
//!     console read's sleeps.)
//!
//! A group that prints no marker within `GROUP_MS` by the PM timer stops the
//! session: the rows still owed fail with the reason. The plan always
//! completes.
//!
//! Plan 10.

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
use kernel::fs::vfs::InodeRef;
use kernel::ktap;
use kernel::pipe;
use kernel::proc::{self, Pid};
use kernel::syscall::{self, Errno, SyscallFrame};
use kernel::task::{self, Tid};
use kernel::{bootinfo, frame, gdt, heap, idt, irq, sched, serial, time};

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 64;
const PM_HZ: u64 = 3_579_545;

const PLAN: usize = 10;
const KILL_ID: &str = "sig.kill";
const IGN_ID: &str = "sig.ignore";
const TERM_ID: &str = "sig.default.terminate";
const IGNDFL_ID: &str = "sig.default.ignore";
const OTHER_ID: &str = "sig.kill.other";
const PIPE_ID: &str = "sig.pipe";
const READ_ID: &str = "sig.intr.read";
const WAIT_ID: &str = "sig.intr.wait";
const WRITE_ID: &str = "sig.intr.write";
const CONSOLE_ID: &str = "sig.intr.console";
const IDS: [&str; PLAN] = [KILL_ID, IGN_ID, TERM_ID, IGNDFL_ID, OTHER_ID, PIPE_ID, READ_ID, WAIT_ID, WRITE_ID, CONSOLE_ID];

/// Rows 7 to 10 send one of these, drawn from the nonce: signals whose
/// default action ends the process, with the name `kill -l` gives each.
const TERMINATING: [(u64, &str); 5] = [(1, "HUP"), (10, "USR1"), (12, "USR2"), (14, "ALRM"), (15, "TERM")];

/// How long a group may run before the session counts as stopped, by the
/// PM timer.
const GROUP_MS: u64 = 20_000;
/// Rows 5 and 7 to 10: how long the process has to end once `kill` sent it
/// its signal (a sleep a signal does not end runs 30 s).
const KILL_MS: u64 = 5_000;

/// The learner's init (lab 28), and how long it has to print its hello line
/// and fork the shell, by the PM timer.
const INIT: &str = "/bin/init";
const INIT_MS: u64 = 10_000;

// syscall numbers and errnos, as literals: the vehicle builds at every cut,
// and `syscall::nr::KILL` arrives with lab 31.
const NR_KILL: u64 = 62;
const EINVAL: i64 = 22;
const ESRCH: i64 = 3;
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
        if ktap::nonce().is_none() && self.id != KILL_ID {
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

// ── Row 1: kill, through the dispatcher ──────────────────────────────────

fn kill(pid: i64, sig: u64) -> i64 {
    // SAFETY: every field of the frame is a u64; all zero is a valid frame,
    // and `kill` reads none of it.
    let mut f: SyscallFrame = unsafe { core::mem::zeroed() };
    syscall::dispatch(NR_KILL, [pid as u64, sig, 0, 0, 0, 0], &mut f)
}

/// Row 1, called with interrupts off between init's spawn and its first
/// run, so no signal it might be sent can reach it before it starts.
fn kill_row(init: Pid) {
    let mut r = Row::new(KILL_ID);
    let me = init.0 as i64;
    let cases: [(&str, i64, u64, i64); 5] = [
        ("signal 0 to a live process only asks whether it is there", me, 0, 0),
        ("a signal number above 64 is refused", me, 65, -EINVAL),
        ("pid 0 names no process (there are no process groups)", 0, 15, -ESRCH),
        ("pid -1 names no process (there are no process groups)", -1, 15, -ESRCH),
        ("no process has pid 99999", 99_999, 0, -ESRCH),
    ];
    let mut got = Vec::new();
    for (why, pid, sig, want) in cases {
        let v = kill(pid, sig);
        got.push(format!("kill({pid}, {sig}) = {v}"));
        if v != want {
            let want_text = if want == 0 { String::from("0") } else { errno_text(-want) };
            let v_text = if v < 0 { errno_text(-v) } else { format!("{v}") };
            r.fail(format_args!("kill({pid}, {sig}) returned {v_text}; {why}, so it is {want_text} (syscall 62, lab 31 step 1)"));
        }
    }
    note(format_args!("{KILL_ID}: init is pid {}: {}", init.0, got.join(", ")));
    r.finish(1);
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
    w: [String; 21],
    /// Rows 7 to 10's signals, from `TERMINATING`.
    sig: [(u64, &'static str); 4],
}

fn words() -> Words {
    let n = ktap::nonce().unwrap_or([b'0'; 32]);
    Words {
        nonce: n,
        w: core::array::from_fn(|k| format!("s{:08x}", ktap::derive(&n, &format!("sig.word/{k}")) & 0xffff_ffff)),
        sig: core::array::from_fn(|k| TERMINATING[(ktap::derive(&n, &format!("sig.intr/{k}")) % TERMINATING.len() as u64) as usize]),
    }
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
    /// script pipe and fds 1 and 2 the tee; row 1 before it first runs; then its hello line, and the child it
    /// forks running `sh`.
    fn start(w: &Words) -> Result<Sh, String> {
        let (ir, iw) = pipe::open(0).map_err(|e| format!("pipe::open(0) returned {}", errno_text(e)))?;
        let (or, ow) = pipe::open(0).map_err(|e| format!("pipe::open(0) returned {}", errno_text(e)))?;
        or.flags.store(or.flags.load(Ordering::SeqCst) | O_NONBLOCK, Ordering::SeqCst);
        let nonce = String::from_utf8_lossy(&w.nonce).into_owned();
        let fds = [(0, ir.clone(), false), (1, ow.clone(), false), (2, ow.clone(), false)];
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
            kill_row(pid);
            Ok((pid, back))
        });
        drop((ir, ow));
        let init = match started {
            Ok((pid, back)) => {
                drop(back);
                pid
            }
            Err(why) => {
                let mut r = Row::new(KILL_ID);
                r.fail(format_args!("no init ran: {why}"));
                r.finish(1);
                return Err(why);
            }
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
    fn group(&mut self, n: impl core::fmt::Display, script: &str, ms: u64) -> Result<Vec<u8>, String> {
        let from = self.out.len();
        self.send(&format!("{script}echo ':q{n}:'\n"))?;
        self.collect(n, from, ms)
    }

    /// Read until group `n`'s marker, which the script already holds.
    fn collect(&mut self, n: impl core::fmt::Display, from: usize, ms: u64) -> Result<Vec<u8>, String> {
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
}

/// The group's output against the bytes expected.
fn expect(r: &mut Row, n: impl core::fmt::Display, got: &Result<Vec<u8>, String>, want: &str, what: &str) {
    match got {
        Err(why) => r.fail(format_args!("{why}")),
        Ok(g) if g.as_slice() != want.as_bytes() => r.fail(format_args!(
            "group {n} printed {:?}; Linux's busybox prints {:?} — {what}",
            String::from_utf8_lossy(g),
            want
        )),
        Ok(_) => {}
    }
}

/// Rows 2-10. Returns early, after failing what is left, when the session
/// cannot go on.
fn session(w: &Words) {
    let [w0, w1, w2, w3, w4, w5, w6, w7, w8, w9, w10, w11, w12, w13, w14, w15, w16, w17, w18, w19, w20] = &w.w;
    let mut sh = match Sh::start(w) {
        Ok(sh) => sh,
        Err(why) => return fail_rest(2, &format!("no shell ran: {why}")),
    };
    note(format_args!("session: init is pid {}, and /bin/sh, its child, pid {}; the script's words are derived from the nonce", sh.init.0, sh.pid.0));

    // Row 2: SIG_IGN.
    // In a child shell, so that a kernel that does not drop the signal ends
    // that shell and not the session's.
    let g = sh.group(2, &format!("sh -c 'trap \"\" USR1; kill -USR1 $$; echo {w0}'; echo \"{w10}=$?\"\n"), GROUP_MS);
    let mut r = Row::new(IGN_ID);
    expect(&mut r, 2, &g, &format!("{w0}\n{w10}=0\n"), "a signal whose action is SIG_IGN (`trap '' USR1`) is dropped, so the shell that sent it to itself runs the next command and exits 0");
    if exit_of(sh.pid).is_some() {
        r.fail(format_args!("the shell (pid {}) {}: SIGUSR1 was ignored, so it should still run", sh.pid.0, status_text(exit_of(sh.pid))));
    }
    r.finish(2);

    // Row 3: default actions that end a process.
    let g = sh.group(
        3,
        &format!("sh -c 'kill -SEGV $$; echo {w1}'; echo \"{w2}=$?\"\nsh -c 'kill -USR1 $$; echo {w1}'; echo \"{w3}=$?\"\n"),
        GROUP_MS,
    );
    let mut r = Row::new(TERM_ID);
    expect(
        &mut r,
        3,
        &g,
        &format!("Segmentation fault\n{w2}=139\nUser defined signal 1\n{w3}=138\n"),
        "SIGSEGV's and SIGUSR1's default action ends the process before its next instruction, with the signal in its status (`$?` = 128 + 11 = 139, 128 + 10 = 138), and ash names the signal",
    );
    r.finish(3);

    // Row 4: default actions that do nothing.
    let g = sh.group(4, &format!("sleep 30 & echo \"{w4}=$!\"\nkill -CHLD $!; kill -WINCH $!; kill -URG $!\nsleep 1\necho {w5}\n"), GROUP_MS);
    let mut r = Row::new(IGNDFL_ID);
    let text = g.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
    let bg = text.lines().find_map(|l| l.strip_prefix(&format!("{w4}=")).and_then(|v| v.trim().parse::<u64>().ok())).map(Pid);
    match (&g, bg) {
        (Err(why), _) => r.fail(format_args!("{why}")),
        (Ok(_), None) => r.fail(format_args!(
            "group 4 printed {text:?}: no `{w4}=<pid>` line for the background `sleep 30 &`"
        )),
        (Ok(_), Some(p)) => {
            if text != format!("{w4}={}\n{w5}\n", p.0) {
                r.fail(format_args!("group 4 printed {text:?}; Linux's busybox prints \"{w4}={}\\n{w5}\\n\" — `kill` of a sleeping process prints nothing", p.0));
            }
            if exit_of(p).is_some() {
                r.fail(format_args!(
                    "`sleep 30` (pid {}) {} a second after SIGCHLD, SIGWINCH and SIGURG: their default action is to do nothing, so they end neither the process nor its sleep",
                    p.0,
                    status_text(exit_of(p))
                ));
            } else if task_of(p).is_none() {
                r.fail(format_args!(
                    "`sleep 30` (pid {}, the `$!` of `sleep 30 &`) is gone a second after SIGCHLD, SIGWINCH and SIGURG (it ended and the shell collected it): their default action is to do nothing, so they end neither the process nor its sleep",
                    p.0
                ));
            }
        }
    }
    r.finish(4);

    // Row 5: a signal to another process, asleep: its own `sleep 30`, so
    // that what row 4 did to its process does not decide this row.
    let g = sh.group(5, &format!("sleep 30 & echo \"{w9}=$!\"\nsleep 1\nkill $!\necho {w6}\n"), GROUP_MS);
    let mut r = Row::new(OTHER_ID);
    let text = g.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
    let bg = text.lines().find_map(|l| l.strip_prefix(&format!("{w9}=")).and_then(|v| v.trim().parse::<u64>().ok())).map(Pid);
    match (&g, bg) {
        (Err(why), _) => r.fail(format_args!("{why}")),
        (Ok(_), None) => r.fail(format_args!(
            "group 5 printed {text:?}: no `{w9}=<pid>` line for the background `sleep 30 &`"
        )),
        (Ok(_), Some(p)) => {
            if text != format!("{w9}={}\n{w6}\n", p.0) {
                r.fail(format_args!("group 5 printed {text:?}; Linux's busybox prints \"{w9}={}\\n{w6}\\n\" — `kill` of a sleeping process prints nothing", p.0));
            }
            let mut c = Clock::new();
            while exit_of(p).is_none() && task_of(p).is_some() && c.ms() < KILL_MS {
                sh.pump();
                sched::yield_now();
            }
            match exit_of(p) {
                Some(15) => {}
                None if task_of(p).is_none() => r.fail(format_args!(
                    "`sleep 30` (pid {}) ended and the shell collected it before its status could be read: SIGTERM's default action ends it, killed by signal 15",
                    p.0
                )),
                s => r.fail(format_args!(
                    "`sleep 30` (pid {}) {} {KILL_MS} ms after `kill` sent it SIGTERM: the signal ends its sleep at once, and its default action ends it, killed by signal 15",
                    p.0,
                    status_text(s)
                )),
            }
        }
    }
    r.finish(5);

    // Row 6: SIGPIPE.
    let g = sh.group(
        6,
        &format!("(yes; echo \"{w7}=$?\" >&2) | head -n 1\n(trap '' PIPE; yes; echo \"{w8}=$?\" >&2) | head -n 1\n"),
        GROUP_MS,
    );
    let mut r = Row::new(PIPE_ID);
    expect(
        &mut r,
        6,
        &g,
        // `yes: : Broken pipe` is busybox 1.37.0's text (libbb/perror_msg.c,
        // libbb/verror_msg.c), read from its source, not measured on Linux.
        &format!("y\n{w7}=141\ny\nyes: : Broken pipe\n{w8}=1\n"),
        "a write to a pipe no one reads sends the writer SIGPIPE, whose default kills it (128 + 13 = 141); ignored, the write returns -EPIPE and `yes` says so and exits 1",
    );
    r.finish(6);

    // Row 7: a signal ends a pipe read's sleep. `cat` reads a pipe whose
    // only writer is `sleep 30`, which writes nothing.
    killed_row(&mut sh, 7, READ_ID, "sleep 30 | cat", ("`cat`", "asleep reading a pipe nothing is written to"), "a blocked pipe read", w11, w12, w.sig[0]);

    // Row 8: a signal ends `wait4`'s sleep. The child shell waits for its
    // foreground `sleep 30`; the `:` after it keeps ash from exec'ing it.
    killed_row(&mut sh, 8, WAIT_ID, "sh -c 'sleep 30;:'", ("the shell", "asleep in `wait4` for its `sleep 30`"), "a `wait4` that is waiting for a child", w13, w14, w.sig[1]);

    // Row 9: a signal ends a pipe write's sleep. `sleep 30` holds the read
    // end and never reads, so `yes` fills the pipe and waits for room.
    staged_row(
        &mut sh,
        9,
        WRITE_ID,
        "yes | sleep 30",
        Target::Writer,
        ("`yes`", "asleep writing to a pipe whose 4096 bytes are full and whose reader never reads"),
        "its reader never reads, so it fills the pipe's 4096 bytes and waits in `write` for room until a signal comes",
        "a write that is waiting for room in a pipe",
        [w15, w16, w17],
        w.sig[2],
    );

    // Row 10: a signal ends a console read's sleep. No key is typed, so the
    // cooked read waits for a line that never comes.
    staged_row(
        &mut sh,
        10,
        CONSOLE_ID,
        "cat </dev/console",
        Target::Last,
        ("`cat`", "asleep reading the console, where no key is typed"),
        "no key is typed, so its read of the console waits for a line until a signal comes",
        "a read of the console that is waiting for a line",
        [w18, w19, w20],
        w.sig[3],
    );
}

/// Whom rows 9 and 10 send their signal to.
enum Target {
    /// `$!`, the job's last process.
    Last,
    /// The job's live process running `yes`, a child of the shell (`$!`
    /// is the pipeline's last process, the reader).
    Writer,
}

/// The shell's live child running `yes`.
fn writer_of(shell: Pid) -> Option<Pid> {
    let mut out = None;
    quiet(|| {
        proc::each(|p| {
            if p.ppid == shell && p.exit.is_none() && p.argv.first().map(String::as_str) == Some("yes") {
                out = Some(p.pid);
            }
        })
    });
    out
}

/// Rows 9 and 10, in two halves. First `<job> &` and a second for it to
/// fall asleep: the process must still run then (row 9: and a writer must
/// have waited for room in a full pipe since the group began). Then `kill
/// -<sig> <pid>`, and the process must end, killed by `sig`, within
/// `KILL_MS`.
#[allow(clippy::too_many_arguments)]
fn staged_row(sh: &mut Sh, n: usize, id: &'static str, job: &str, target: Target, who: (&str, &str), waits_why: &str, sleep: &str, w: [&str; 3], sig: (u64, &str)) {
    let (num, name) = sig;
    let (who, doing) = who;
    let [wpid, wup, wdone] = w;
    let mut r = Row::new(id);
    let waits = pipe::full_waits();
    let g = sh.group(n, &format!("{job} & echo \"{wpid}=$!\"\nsleep 1\necho {wup}\n"), GROUP_MS);
    let text = g.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
    let last = text.lines().find_map(|l| l.strip_prefix(&format!("{wpid}=")).and_then(|v| v.trim().parse::<u64>().ok())).map(Pid);
    let last = match (&g, last) {
        (Err(why), _) => return fail_one(r, n, format_args!("{why}")),
        (Ok(_), None) => return fail_one(r, n, format_args!("group {n} printed {text:?}: no `{wpid}=<pid>` line for the background `{job} &`")),
        (Ok(_), Some(p)) => p,
    };
    if text != format!("{wpid}={}\n{wup}\n", last.0) {
        r.fail(format_args!("group {n} printed {text:?}; Linux's busybox prints \"{wpid}={}\\n{wup}\\n\"", last.0));
    }
    let p = match target {
        Target::Last => last,
        Target::Writer => match writer_of(sh.pid) {
            Some(p) => p,
            None => {
                return fail_one(r, n, format_args!(
                    "a second after `{job} &` started, before any signal was sent, no child of the shell (pid {}) running `yes` was left: its reader never reads, so `yes` fills the pipe and waits for room in `write`",
                    sh.pid.0
                ))
            }
        },
    };
    if exit_of(p).is_some() || task_of(p).is_none() {
        let s = if task_of(p).is_none() { String::from("was gone") } else { status_text(exit_of(p)) };
        return fail_one(r, n, format_args!(
            "{who} (pid {}) {s} a second after `{job} &` started, before any signal was sent: {waits_why}",
            p.0
        ));
    }
    if matches!(target, Target::Writer) && pipe::full_waits() == waits {
        r.fail(format_args!(
            "{who} (pid {}) still ran a second after `{job} &` started, but no writer had waited for room in a full pipe since then: a write that finds the pipe's 4096 bytes full, with a reader open, waits for room (lab 15)",
            p.0
        ));
    }
    let g = sh.group(format_args!("{n}b"), &format!("kill -{num} {}\necho {wdone}\n", p.0), GROUP_MS);
    expect(&mut r, format_args!("{n}b"), &g, &format!("{wdone}\n"), "`kill` of a sleeping process prints nothing");
    if g.is_err() {
        return r.finish(n);
    }
    let mut c = Clock::new();
    while exit_of(p).is_none() && task_of(p).is_some() && c.ms() < KILL_MS {
        sh.pump();
        sched::yield_now();
    }
    match exit_of(p) {
        Some(s) if s as u64 == num => {
            if !r.failed {
                note(format_args!("{id}: {who} (pid {}) still ran a second after `{job} &` started; `kill -{num}` ended it, killed by signal {num}", p.0));
            }
        }
        None if task_of(p).is_none() => r.fail(format_args!(
            "{who} (pid {}), {doing}, ended and the shell collected it before its status could be read: SIG{name}'s default action ends it, killed by signal {num}",
            p.0
        )),
        s => r.fail(format_args!(
            "{who} (pid {}), {doing}, {} {KILL_MS} ms after `kill -{num}` sent it SIG{name}: a signal ends {sleep} at once, and SIG{name}'s default action ends the process, killed by signal {num}",
            p.0,
            status_text(s)
        )),
    }
    r.finish(n);
}

/// Row `n` failed with `why`, and finished.
fn fail_one(mut r: Row, n: usize, why: core::fmt::Arguments) {
    r.fail(why);
    r.finish(n);
}

/// Rows 7 and 8: `<job> &`, a second for it to fall asleep, `kill -<sig>
/// $!`; then `$!` must end, killed by `sig`, within `KILL_MS`.
#[allow(clippy::too_many_arguments)]
fn killed_row(sh: &mut Sh, n: usize, id: &'static str, job: &str, who: (&str, &str), sleep: &str, wpid: &str, wdone: &str, sig: (u64, &str)) {
    let (num, name) = sig;
    let (who, doing) = who;
    let g = sh.group(n, &format!("{job} & echo \"{wpid}=$!\"\nsleep 1\nkill -{num} $!\necho {wdone}\n"), GROUP_MS);
    let mut r = Row::new(id);
    let text = g.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
    let bg = text.lines().find_map(|l| l.strip_prefix(&format!("{wpid}=")).and_then(|v| v.trim().parse::<u64>().ok())).map(Pid);
    match (&g, bg) {
        (Err(why), _) => r.fail(format_args!("{why}")),
        (Ok(_), None) => r.fail(format_args!("group {n} printed {text:?}: no `{wpid}=<pid>` line for the background `{job} &`")),
        (Ok(_), Some(p)) => {
            if text != format!("{wpid}={}\n{wdone}\n", p.0) {
                r.fail(format_args!("group {n} printed {text:?}; Linux's busybox prints \"{wpid}={}\\n{wdone}\\n\" — `kill` of a sleeping process prints nothing", p.0));
            }
            let mut c = Clock::new();
            while exit_of(p).is_none() && task_of(p).is_some() && c.ms() < KILL_MS {
                sh.pump();
                sched::yield_now();
            }
            match exit_of(p) {
                Some(s) if s as u64 == num => {}
                None if task_of(p).is_none() => r.fail(format_args!(
                    "{who} (pid {}), {doing}, ended and the shell collected it before its status could be read: SIG{name}'s default action ends it, killed by signal {num}",
                    p.0
                )),
                s => r.fail(format_args!(
                    "{who} (pid {}), {doing}, {} {KILL_MS} ms after `kill -{num}` sent it SIG{name}: a signal ends {sleep} at once, and SIG{name}'s default action ends the process, killed by signal {num}",
                    p.0,
                    status_text(s)
                )),
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
        Err(why) => fail_rest(1, &format!("not run: the file systems did not mount: {why}")),
        Ok(()) => session(&words()),
    }
    irq_off();
    ktap::end();
    halt()
}
