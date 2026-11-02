//! Lab 31's vehicle (`31-signals`): `kill`, dispositions and the default
//! actions, through busybox's `ash`, on the learner's kernel.
//!
//! The disk is `ktest-31.img` as the course's image writer made it (CONTRACT
//! §A.7): lab 30's, plus `/bin/sigprobe` (`SIGNAL_PROGRAMS`, from lab 31), the
//! course's C program that makes the raw signal calls ash never makes
//! (`sigregs` is lab 32's, `HANDLER_PROGRAMS`).
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
//! calls `kill` (62) through `syscall::dispatch` — lab 31 task 1, the only
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
//!     …; trap - USR1; kill -USR1 $$; echo …'`: SIG_IGN, so the shell lives
//!     on to run the next command; then, its action back to SIG_DFL, the
//!     same signal ends it, `$?` 138 (a child shell, so a signal not dropped
//!     ends it, not the session). The second half is what tells a dropped
//!     signal from one never delivered (added 2026-10-29, slice 9 (#47): a
//!     kernel whose `kill` only marks the signal pending, nothing taken,
//!     scored this row).
//!   3 `sig.default.terminate` (derived) — `sh -c 'kill -SEGV $$'` and
//!     `sh -c 'kill -USR1 $$'`: the default action ends each shell, its
//!     status the signal (`$?` 139 and 138; ash names the signal).
//!   4 `sig.default.ignore` (derived) — first a child shell sends itself
//!     SIGCHLD, SIGWINCH and SIGURG, prints a word, then sends itself
//!     SIGUSR1: the three do nothing and the fourth ends it, `$?` 138, so
//!     the three were taken and dropped, not left undelivered (added
//!     2026-10-29, slice 9 (#47), as row 2's second half). Then `sigprobe
//!     dfl <w>` sets SIGCHLD's, SIGWINCH's and SIGURG's action to SIG_DFL
//!     and sends each to itself: the default of each is to do nothing, so
//!     it prints `<w> dfl`, `$?` 0. ash catches SIGCHLD, so only a program
//!     whose action is SIG_DFL meets its default. (Fix round 2, 2026-10-29:
//!     the lens L31r2's F3, a kernel whose SIGCHLD default ended the
//!     process scored full marks. The `sleep 30 &` this row sent SIGWINCH
//!     and SIGURG to went with it: row 13's sleeping process is sent both.)
//!   5 `sig.kill.other` (derived) — a `sleep 30` of its own, in the
//!     background, left to fall asleep, then `kill -15` sends it SIGTERM: the signal
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
//!     shell's live child running `yes`; once it is asleep (off the CPU, out
//!     of `TaskTable::ready`, not `Dead`: ktest-15's test) or a writer has
//!     waited for room since the group began (`pipe::full_waits`, lab 15's
//!     counter, which no row grades), and `SETTLE_TICKS` more, it must still
//!     run. Then `kill -<sig> <its pid>`, a third
//!     drawn signal: the signal ends the write's sleep at once, and `yes`
//!     ends, killed by that signal.
//!  10 `sig.intr.console` (derived) — `cat </dev/console &` (the redirect
//!     replaces the `/dev/null` ash gives a background job's stdin without
//!     job control): no key is typed, so `cat` is asleep in the console's
//!     cooked read waiting for a line, and it still runs `SETTLE_TICKS`
//!     after it runs `cat`;
//!     `kill -<sig> $!`, a fourth drawn signal, ends that sleep at once, and
//!     `cat` ends, killed by it. Rows 5 and 7 to 11 run in two halves (`:q9:`
//!     then `:q9b:`), so the process is seen asleep before the signal is
//!     sent.
//!     (Added 2026-10-23, slice 8 fix round 2: the pipe write's and the
//!     console read's sleeps.)
//!  11 `sig.kill.uncatchable` (derived) — `sh -c 'trap "" KILL; trap "echo
//!     …" STOP; kill -STOP $$; sleep 30 & wait; echo …' &`: the child shell
//!     asks for SIGKILL to be ignored and SIGSTOP caught, both of which
//!     `rt_sigaction` refuses (`-EINVAL`, ignored by ash), sends itself
//!     SIGSTOP (its default does nothing here: there is no stopped state, so
//!     a trap that runs is a handler SIGSTOP was given), then waits for its
//!     `sleep 30`, and ash's `wait` blocks every signal (`sigfillset`, then
//!     `rt_sigprocmask(SIG_SETMASK)`) around its `rt_sigsuspend`. Once it
//!     has its `sleep 30`, and `SETTLE_TICKS` more, it must still run and
//!     have printed nothing; then
//!     `kill -9` must end it, killed by signal 9, within `KILL_TICKS`. Before lab
//!     32 `rt_sigsuspend` is `-ENOSYS`, so ash's `wait` loops with the full
//!     mask set: a mask that kept SIGKILL blocks it there. From lab 32 the
//!     wait sleeps under the mask it had before, so only the ignore and the
//!     STOP trap are seen. (Added 2026-10-29, slice 9 (#47): the TL's
//!     finding 2, nothing graded CONTRACT §A.4 rows 13/14's "SIGKILL and
//!     SIGSTOP can be neither caught nor blocked".) A third group, the same
//!     on every cut: `sigprobe uncatch <w>` blocks SIGKILL, SIGSTOP and
//!     SIGUSR2 with `SIG_BLOCK`, reads the mask back, asks for SIGKILL
//!     ignored and SIGSTOP caught, prints the three answers (`800 -22 -22`:
//!     the mask holds SIGUSR2 alone) and sends itself SIGKILL, which ends
//!     it, `$?` 137. (Added 2026-10-29, slice 9 fix round 1: the lens L31r's
//!     F2, `SIG_BLOCK` keeping 9 and 19, and an `rt_sigaction` that answers 0
//!     for them, scored full marks.)
//!  12 `sig.mask.hold` (derived) — `sigprobe order <w>`: SIGHUP, SIGUSR1
//!     and SIGUSR2 blocked (`SIG_BLOCK`), SIGHUP's action SIG_IGN; it sends
//!     itself SIGUSR2, SIGUSR1 and SIGHUP, prints `<w> held`, and unblocks
//!     the three. Held until then, all three are taken on the way back from
//!     that one call, lowest number first: SIGHUP dropped, SIGUSR1 ends it
//!     (`$?` 138) before its next line. Then `sigprobe queue <w>`: SIGWINCH
//!     blocked, sent and unblocked, which drops it (its default does
//!     nothing); then SIGUSR2 blocked, its action SIG_IGN, sent, its action
//!     set back to SIG_DFL, a line printed, SIGUSR2 unblocked: blocked, it
//!     was kept though its action ignored it, so the unblock ends the
//!     program, `$?` 140. (Added
//!     2026-10-29, slice 9 fix round 1: the lenses' finding that no row saw
//!     the mask hold a signal, nor the order signals are taken in.)
//!  13 `sig.ignore.sleep` (derived) — `sigprobe nap <w>`: SIGUSR1's action
//!     SIG_IGN, and its child sends it SIGUSR1, SIGWINCH and SIGURG while it
//!     is in a 600 ms `nanosleep`: none of the three acts, so they end
//!     neither the process nor its sleep, which returns 0 (busybox's `sleep` hides an early end, since it sleeps again for
//!     what is left). It prints `<w> 0`, `$?` 0, and the group takes at
//!     least `NAP_FLOOR` ticks of IRQ 0, the one 600 ms sleep (fix round 3).
//!  14 `sig.kill.process` (derived) — `sigprobe send <w>`, the calls row 1
//!     makes from the kernel made from a process: `kill(<a zombie child>,
//!     0)` = 0, `kill(0, 0)`, `kill(-1, 0)` and `kill(0, 15)` = `-ESRCH`,
//!     `$?` 0. Last in the plan, so a `kill(0, …)` or `kill(-1, …)` that
//!     reached a process costs no other row.
//!
//! Rows 5 and 7 to 11 start their job as `( <job> & echo …=$!; exec sleep
//! 30 ) &`: the subshell becomes `sleep 30`, which never waits for a child,
//! so the process the row signals stays in the table with its status until
//! the row has read it. From lab 32 ash is sent SIGCHLD and reaps its
//! children at once; while ash was the parent, that reap raced the row's
//! read (measured 2026-10-29, slice 9: on the full reference, rows 10 and 11
//! each lost a boot to it).
//!
//! No script sleeps. Rows 5 and 7 to 11 echo their marker at once; the
//! vehicle then waits for the job's `<w>=<pid>` line, for its process to
//! run what it was started for (rows 8 and 11: to have a `sleep 30` of its
//! own; row 9: for its writer to be asleep), and `SETTLE_TICKS`
//! ticks of IRQ 0 more, each wait bounded by `SETTLE_MS` by the PM timer.
//! After the `kill`, the process must end within `KILL_TICKS` ticks of IRQ
//! 0, under a `KILL_MS` ceiling by the PM timer (fix round 3: a sleep that
//! looked for a signal every 3 s scored full marks under a PM-timer bound
//! alone).
//! Row 13's group is bounded by `NAP_TICKS` ticks under a `NAP_MS` ceiling.
//! A count of the guest's own ticks slows with the guest: under macOS's
//! background QoS its ticks ran about 38 times slower than real time, and
//! the `sleep 1`s these rows had took the reference to 379.87 s, where
//! default QoS took 16.55 s (fix round 2, 2026-10-29).
//!
//! A group that prints no marker within `GROUP_MS` by the PM timer (row 13:
//! `NAP_TICKS` ticks of IRQ 0, or `NAP_MS`) stops the session: the rows
//! still owed fail with the reason. The plan always completes.
//!
//! Plan 14.

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
use kernel::task::{self, State, Tid};
use kernel::{bootinfo, frame, gdt, heap, idt, irq, sched, serial, time};

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 64;
const PM_HZ: u64 = 3_579_545;

const PLAN: usize = 14;
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
const UNCATCH_ID: &str = "sig.kill.uncatchable";
const HOLD_ID: &str = "sig.mask.hold";
const NAP_ID: &str = "sig.ignore.sleep";
const SEND_ID: &str = "sig.kill.process";
const IDS: [&str; PLAN] = [KILL_ID, IGN_ID, TERM_ID, IGNDFL_ID, OTHER_ID, PIPE_ID, READ_ID, WAIT_ID, WRITE_ID, CONSOLE_ID, UNCATCH_ID, HOLD_ID, NAP_ID, SEND_ID];

/// The course's C program rows 11 to 14 run (`os/user/sigprobe`).
const PROBE: &str = "sigprobe";

/// Rows 7 to 10 send one of these, drawn from the nonce: signals whose
/// default action ends the process, with the name `kill -l` gives each.
const TERMINATING: [(u64, &str); 5] = [(1, "HUP"), (10, "USR1"), (12, "USR2"), (14, "ALRM"), (15, "TERM")];

/// How long a group may run before the session counts as stopped, by the
/// PM timer.
const GROUP_MS: u64 = 20_000;
/// Rows 5, 7 to 10 and 11: once the `kill` line is written to the shell, the
/// process must end within `KILL_TICKS` ticks of IRQ 0, since a signal ends
/// its sleep at once (fix round 3). The vehicle looks for the end for `KILL_MS` by the
/// PM timer, so an end that comes later than `KILL_TICKS` is named as late,
/// and a sleep a signal does not end (it runs 30 s) as still running.
/// Counted in the guest's ticks, as `SETTLE_TICKS`; under macOS's
/// background QoS `KILL_MS` is reached first.
const KILL_MS: u64 = 5_000;
const KILL_TICKS: u64 = 500;
/// Rows 5 and 7 to 11: once the job's process runs what it was started
/// for, how long it is left before it is looked at, in ticks of IRQ 0 (about
/// 1000 a second). Counted in the guest's ticks, not by the PM timer, so a
/// host that slows the guest slows the wait with it (macOS's background
/// QoS ran the ticks about 38 times slower than real time, fix round 2,
/// 2026-10-29). `SETTLE_MS` is each wait's ceiling by the PM timer.
const SETTLE_TICKS: u64 = 100;
const SETTLE_MS: u64 = 20_000;
/// Row 13's bound: `sigprobe nap` sleeps 600 ms, once on a kernel that
/// lets the sleep run its course and up to five times, so 600 to 3000
/// ticks of IRQ 0. `NAP_MS` is the ceiling, by the PM timer, for a kernel
/// whose timer stops.
const NAP_TICKS: u64 = 5_000;
const NAP_MS: u64 = 60_000;
/// Row 13: the fewest ticks of IRQ 0 its group can take, the parent's one
/// 600 ms `nanosleep` at lab 06's 1000 Hz (fix round 3: a `nanosleep` that
/// returned 0 at once was blamed on the child's `kill`).
const NAP_FLOOR: u64 = 600;

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

/// The parent of `pid`, while it is in the table.
fn parent_of(pid: Pid) -> Option<Pid> {
    let mut out = None;
    quiet(|| {
        proc::each(|p| {
            if p.pid == pid {
                out = Some(p.ppid);
            }
        })
    });
    out
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

/// The live child of `init` running the shell: argv[0] `sh`, or a path
/// ending in `/sh` (C's habit of passing the path as argv[0]; CONTRACT
/// §B.2-28, as ktest-28's `is_sh`).
fn shell_of(init: Pid) -> Option<Pid> {
    let mut out = None;
    quiet(|| {
        proc::each(|p| {
            if p.ppid == init && p.exit.is_none() && p.argv.first().is_some_and(|a| a == "sh" || a.ends_with("/sh")) {
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
            r.fail(format_args!("kill({pid}, {sig}) returned {v_text}; {why}, so it is {want_text} (syscall 62, lab 31 task 1)"));
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
    w: [String; 41],
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
        self.group_ticks(n, script, ms, None)
    }

    /// `group`, also stopped after `cap` ticks of IRQ 0 when one is given.
    fn group_ticks(&mut self, n: impl core::fmt::Display, script: &str, ms: u64, cap: Option<u64>) -> Result<Vec<u8>, String> {
        let from = self.out.len();
        self.send(&format!("{script}echo ':q{n}:'\n"))?;
        self.collect(n, from, ms, cap)
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

    /// Pump the tee for `k` ticks of IRQ 0, or `ms` by the PM timer,
    /// whichever ends first.
    fn hold(&mut self, k: u64, ms: u64) {
        let mut c = Clock::new();
        let t0 = ticks();
        while ticks().wrapping_sub(t0) < k && c.ms() < ms {
            self.pump();
            sched::yield_now();
        }
    }

    /// Read until group `n`'s marker, which the script already holds.
    fn collect(&mut self, n: impl core::fmt::Display, from: usize, ms: u64, cap: Option<u64>) -> Result<Vec<u8>, String> {
        let marker = format!(":q{n}:\n");
        let mut c = Clock::new();
        let t0 = ticks();
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
            let late = cap.filter(|&k| ticks().wrapping_sub(t0) >= k);
            if late.is_some() || c.ms() >= ms {
                let within = match late {
                    Some(k) => format!("{k} ticks of IRQ 0"),
                    None => format!("{ms} ms by the PM timer"),
                };
                let why = format!("group {n} printed no marker within {within} ({} bytes of it); the session stopped there", self.out.len() - from);
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

/// Rows 2-14. Returns early, after failing what is left, when the session
/// cannot go on.
fn session(w: &Words) {
    let [w0, w1, w2, w3, w4, w5, w6, w7, w8, w9, w10, w11, w12, w13, w14, w15, w16, w17, w18, w19, w20, w21, w22, w23, w24, w25, w26, w27, w28, w29, w30, w31, w32, w33, w34, w35, w36, w37, w38, w39, w40] = &w.w;
    let mut sh = match Sh::start(w) {
        Ok(sh) => sh,
        Err(why) => return fail_rest(2, &format!("no shell ran: {why}")),
    };
    note(format_args!("session: init is pid {}, and /bin/sh, its child, pid {}; the script's words are derived from the nonce", sh.init.0, sh.pid.0));

    // Row 2: SIG_IGN.
    // In a child shell, so that a kernel that does not drop the signal ends
    // that shell and not the session's.
    let g = sh.group(2, &format!("sh -c 'trap \"\" USR1; kill -USR1 $$; echo {w0}; trap - USR1; kill -USR1 $$; echo {w0}'; echo \"{w10}=$?\"\n"), GROUP_MS);
    let mut r = Row::new(IGN_ID);
    expect(
        &mut r,
        2,
        &g,
        &format!("{w0}\nUser defined signal 1\n{w10}=138\n"),
        "a signal whose action is SIG_IGN (`trap '' USR1`) is dropped, so the shell that sent it to itself runs the next command; with its action back to SIG_DFL (`trap - USR1`) the same signal ends it, `$?` 128 + 10 = 138",
    );
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
    let g = sh.group(4, &format!("sh -c 'kill -CHLD $$; kill -WINCH $$; kill -URG $$; echo {w26}; kill -USR1 $$; echo {w26}'; echo \"{w27}=$?\"\n"), GROUP_MS);
    let mut r = Row::new(IGNDFL_ID);
    expect(
        &mut r,
        4,
        &g,
        &format!("{w26}\nUser defined signal 1\n{w27}=138\n"),
        "SIGCHLD's, SIGWINCH's and SIGURG's default action is to do nothing, so the shell that sent them to itself runs the next command; SIGUSR1's ends it, `$?` 138",
    );
    let what = "`sigprobe dfl` set SIGCHLD's, SIGWINCH's and SIGURG's action to SIG_DFL and sent each to itself";
    match probe(&mut sh, format_args!("4b"), "dfl", [w4, w5]) {
        Err(why) => r.fail(format_args!("{why}")),
        Ok((lines, st)) => {
            let want = format!("{w4} dfl");
            let by = match st {
                Some(145) => Some("SIGCHLD (17)"),
                Some(156) => Some("SIGWINCH (28)"),
                Some(151) => Some("SIGURG (23)"),
                _ => None,
            };
            if let Some(sig) = by {
                r.fail(format_args!("{what}: it printed {lines:?}, {}: {sig} ended it — its default is to do nothing, so taken, it is dropped", shell_status(st)));
            } else if lines != [want.clone()] || st != Some(0) {
                r.fail(format_args!("{what}: it printed {lines:?}, {}; Linux's answer is `{want}`, `$?` 0 — the default of each is to do nothing", shell_status(st)));
            }
        }
    }
    r.finish(4);

    // Row 5: a signal to another process, asleep: a `sleep 30` of its own.
    staged_row(
        &mut sh,
        5,
        OTHER_ID,
        "sleep 30",
        Target::Last,
        Ready::Runs("sleep"),
        ("`sleep 30`", "asleep in `nanosleep`"),
        "its `nanosleep` lasts 30 s unless a signal ends it",
        "a `nanosleep`",
        [w9, w28, w6],
        (15, "TERM"),
    );

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
    staged_row(
        &mut sh,
        7,
        READ_ID,
        "sleep 30 | cat",
        Target::Last,
        Ready::Runs("cat"),
        ("`cat`", "asleep reading a pipe nothing is written to"),
        "nothing is written to its pipe, so its read waits until a signal comes",
        "a blocked pipe read",
        [w11, w29, w12],
        w.sig[0],
    );

    // Row 8: a signal ends `wait4`'s sleep. The child shell waits for its
    // foreground `sleep 30`; the `:` after it keeps ash from exec'ing it.
    staged_row(
        &mut sh,
        8,
        WAIT_ID,
        "sh -c 'sleep 30;:'",
        Target::Last,
        Ready::Sleeper,
        ("the shell", "asleep in `wait4` for its `sleep 30`"),
        "its child sleeps 30 s, so its `wait4` waits until a signal comes",
        "a `wait4` that is waiting for a child",
        [w13, w30, w14],
        w.sig[1],
    );

    // Row 9: a signal ends a pipe write's sleep. `sleep 30` holds the read
    // end and never reads, so `yes` fills the pipe and waits for room.
    staged_row(
        &mut sh,
        9,
        WRITE_ID,
        "yes | sleep 30",
        Target::Writer,
        Ready::Writer,
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
        Ready::Runs("cat"),
        ("`cat`", "asleep reading the console, where no key is typed"),
        "no key is typed, so its read of the console waits for a line until a signal comes",
        "a read of the console that is waiting for a line",
        [w18, w19, w20],
        w.sig[3],
    );

    // Row 11: SIGKILL and SIGSTOP, which no action and no mask changes.
    uncatchable_row(&mut sh, [w21, w22, w23, w24, w25], [w31, w32]);

    // Row 12: the mask holds a signal until it is unblocked; then the
    // lowest number goes first.
    hold_row(&mut sh, [w33, w34], [w39, w40]);

    // Row 13: a signal the process would not act on ends no sleep.
    nap_row(&mut sh, [w35, w36]);

    // Row 14: `kill` from a process, and a blocked signal its action ignores.
    send_row(&mut sh, [w37, w38]);
}

/// One run of `sigprobe <mode> <word>` and its status: the lines it
/// printed (ash's own line for a signal that ended it among them), and the
/// `$?` the shell echoed after it, or why the session stopped.
fn probe(sh: &mut Sh, n: impl core::fmt::Display, mode: &str, w: [&str; 2]) -> Result<(Vec<String>, Option<u32>), String> {
    probe_ticks(sh, n, mode, w, GROUP_MS, None)
}

/// `probe`, its group bounded by `ms` by the PM timer and by `cap` ticks of
/// IRQ 0 when one is given.
fn probe_ticks(sh: &mut Sh, n: impl core::fmt::Display, mode: &str, w: [&str; 2], ms: u64, cap: Option<u64>) -> Result<(Vec<String>, Option<u32>), String> {
    let [wp, ws] = w;
    let g = sh.group_ticks(n, &format!("{PROBE} {mode} {wp}; echo \"{ws}=$?\"\n"), ms, cap)?;
    let text = String::from_utf8_lossy(&g).into_owned();
    let mut lines: Vec<String> = Vec::new();
    let mut status = None;
    for l in text.lines() {
        match l.strip_prefix(&format!("{ws}=")) {
            Some(v) => status = v.trim().parse::<u32>().ok(),
            None => lines.push(String::from(l)),
        }
    }
    Ok((lines, status))
}

/// `$?` in words.
fn shell_status(s: Option<u32>) -> String {
    match s {
        None => String::from("no status line"),
        Some(v) if v > 128 => format!("`$?` {v}, killed by signal {}", v - 128),
        Some(v) => format!("`$?` {v}, an exit"),
    }
}

/// Row 12.
fn hold_row(sh: &mut Sh, w: [&str; 2], wq: [&str; 2]) {
    let n = 12;
    let wp = w[0];
    let mut r = Row::new(HOLD_ID);
    let (lines, st) = match probe(sh, format_args!("{n}"), "order", w) {
        Ok(v) => v,
        Err(why) => return fail_one(r, n, format_args!("{why}")),
    };
    let held = format!("{wp} held");
    let what = "`sigprobe order` blocked SIGHUP, SIGUSR1 and SIGUSR2 (`rt_sigprocmask(SIG_BLOCK)`), set SIGHUP's action to SIG_IGN, sent itself SIGUSR2, SIGUSR1 and SIGHUP, printed its line, and unblocked the three";
    if lines.first() != Some(&held) {
        r.fail(format_args!(
            "{what}: it printed {lines:?}, {}, and never its `{held}` line — a signal the mask blocks stays pending, and ends nothing, until it is unblocked",
            shell_status(st)
        ));
    } else if st == Some(128 + 12) {
        r.fail(format_args!("{what}: it printed {lines:?}, {}: of the signals pending once they are unblocked, the lowest number is taken first, so SIGUSR1 (10) ends it, `$?` 138", shell_status(st)));
    } else if lines.iter().any(|l| l == &format!("{wp} survived")) {
        r.fail(format_args!(
            "{what}: it printed {lines:?}, {}: it ran on after the call that unblocked them — the three were held, so all are taken on the way back from that call (SIGHUP, ignored, dropped; then SIGUSR1, whose default ends it before its next line)",
            shell_status(st)
        ));
    } else if lines != [held.clone(), String::from("User defined signal 1")] || st != Some(138) {
        r.fail(format_args!("{what}: it printed {lines:?}, {}; Linux's answer is its `{held}` line, then ash's `User defined signal 1`, `$?` 138", shell_status(st)));
    }
    let (lines, st) = match probe(sh, format_args!("{n}b"), "queue", wq) {
        Ok(v) => v,
        Err(why) => return fail_one(r, n, format_args!("{why}")),
    };
    let queued = format!("{} queued", wq[0]);
    let what = "`sigprobe queue` blocked SIGWINCH, sent it to itself and unblocked it; then blocked SIGUSR2, set its action to SIG_IGN, sent it to itself, set the action back to SIG_DFL, printed its line and unblocked it";
    if st == Some(128 + 28) {
        r.fail(format_args!("{what}: it printed {lines:?}, {}: SIGWINCH, held by the mask and taken at its unblock, ended it — its default is to do nothing, so taken, it is dropped", shell_status(st)));
    } else if lines.first() != Some(&queued) {
        r.fail(format_args!("{what}: it printed {lines:?}, {}, and never its `{queued}` line — a blocked signal ends nothing until it is unblocked", shell_status(st)));
    } else if lines.iter().any(|l| l == &format!("{} survived", wq[0])) {
        r.fail(format_args!(
            "{what}: it printed {lines:?}, {}: it ran on after the unblock — a blocked signal is kept even while its action ignores it, since the action may change before the unblock, so SIGUSR2 ends it, `$?` 140",
            shell_status(st)
        ));
    } else if lines != [queued.clone(), String::from("User defined signal 2")] || st != Some(140) {
        r.fail(format_args!("{what}: it printed {lines:?}, {}; Linux's answer is its `{queued}` line, then ash's `User defined signal 2`, `$?` 140", shell_status(st)));
    }
    r.finish(n);
}

/// Row 13.
fn nap_row(sh: &mut Sh, w: [&str; 2]) {
    let n = 13;
    let wp = w[0];
    let mut r = Row::new(NAP_ID);
    let t0 = ticks();
    let (lines, st) = match probe_ticks(sh, n, "nap", w, NAP_MS, Some(NAP_TICKS)) {
        Ok(v) => v,
        Err(why) => return fail_one(r, n, format_args!("{why}")),
    };
    let dt = ticks().wrapping_sub(t0);
    let what = "`sigprobe nap` set SIGUSR1's action to SIG_IGN, and its child sent it SIGUSR1, SIGWINCH and SIGURG while it was in a 600 ms `nanosleep`";
    let want = format!("{wp} 0");
    let short = format!("its run took {dt} ticks of IRQ 0, fewer than the {NAP_FLOOR} of one 600 ms sleep, so a `nanosleep` that no signal cut short returned before its time was up");
    if st == Some(2) && dt < NAP_FLOOR {
        r.fail(format_args!("{what}: it printed {lines:?}, `$?` 2 — {short}, and the child had not sent the three signals by the fifth"));
    } else if st == Some(2) {
        r.fail(format_args!("{what}: it printed {lines:?}, `$?` 2 — its child had not sent the three signals within five 600 ms sleeps"));
    } else if st == Some(3) {
        r.fail(format_args!("{what}: it printed {lines:?}, `$?` 3 — one of its child's three `kill(<its parent>, <signal>)` calls did not answer 0, so the sleep was never tested against a signal"));
    } else if lines.len() == 1 && lines[0] != want && lines[0].starts_with(&format!("{wp} ")) {
        let v = lines[0][wp.len() + 1..].parse::<i64>().unwrap_or(0);
        let v_text = if v < 0 { errno_text(-v) } else { format!("{v}") };
        r.fail(format_args!(
            "{what}: its `nanosleep` returned {v_text}, where Linux sleeps the 600 ms out and returns 0 — a signal the process would not act on (SIG_IGN, or a default of doing nothing) ends no sleep",
        ));
    } else if lines != [want.clone()] || st != Some(0) {
        r.fail(format_args!("{what}: it printed {lines:?}, {}; Linux's answer is `{want}`, `$?` 0 — none of the three acts, so the sleep runs its course", shell_status(st)));
    } else if dt < NAP_FLOOR {
        r.fail(format_args!("{what}: it printed {lines:?}, `$?` 0, but {short}"));
    }
    r.finish(n);
}

/// Row 14.
fn send_row(sh: &mut Sh, w: [&str; 2]) {
    let n = 14;
    let wp = w[0];
    let mut r = Row::new(SEND_ID);
    let (lines, st) = match probe(sh, n, "send", w) {
        Ok(v) => v,
        Err(why) => return fail_one(r, n, format_args!("{why}")),
    };
    let what = "`sigprobe send`";
    let calls: [(&str, i64, &str); 4] = [
        ("kill(<its child, exited and not yet waited for>, 0)", 0, "a zombie still has its pid, which signal 0 finds"),
        ("kill(0, 0)", -ESRCH, "pid 0 names no process (there are no process groups)"),
        ("kill(-1, 0)", -ESRCH, "pid -1 names no process (there are no process groups)"),
        ("kill(0, 15)", -ESRCH, "pid 0 names no process, the caller included"),
    ];
    let first = lines.first().and_then(|l| l.strip_prefix(&format!("{wp} "))).map(|v| v.split(' ').map(|x| x.parse::<i64>().unwrap_or(i64::MIN)).collect::<Vec<_>>());
    match first {
        Some(got) if got.len() == 4 => {
            for ((call, want, why), v) in calls.iter().zip(got) {
                if v != *want {
                    let want_text = if *want == 0 { String::from("0") } else { errno_text(-want) };
                    let v_text = if v < 0 { errno_text(-v) } else { format!("{v}") };
                    r.fail(format_args!("{what}: {call} from a process returned {v_text}; {why}, so it is {want_text} (syscall 62)"));
                }
            }
            if lines.len() != 1 || st != Some(0) {
                r.fail(format_args!("{what} printed {lines:?}, {}; it prints one line and exits 0", shell_status(st)));
            }
        }
        _ if st == Some(128 + 15) => r.fail(format_args!(
            "{what} printed {lines:?}, {}: it called `kill(0, 15)` and was ended by it — pid 0 names no process here (there are no process groups), so it is -3 (ESRCH)",
            shell_status(st)
        )),
        _ => r.fail(format_args!("{what} printed {lines:?}, {}; with no process groups, the answer is `{wp} 0 -3 -3 -3`, `$?` 0", shell_status(st))),
    }
    r.finish(n);
}

/// The program `pid` runs now: its `argv[0]`.
fn argv0_of(pid: Pid) -> Option<String> {
    let mut out = None;
    quiet(|| {
        proc::each(|p| {
            if p.pid == pid && p.exit.is_none() {
                out = p.argv.first().cloned();
            }
        })
    });
    out
}

/// The first half of rows 5 and 7 to 11: `( <job> & echo <wpid>=$!; exec
/// sleep 30 ) &` and `echo <wup>`, no sleep. Then the `<wpid>=` line,
/// wherever it came (the subshell's echo may follow the marker); then until
/// `ready` holds for that pid or its process is gone; then `SETTLE_TICKS`
/// ticks of IRQ 0. The pid, and every line the group printed but its
/// marker, or why the row fails.
fn stage(sh: &mut Sh, n: usize, job: &str, wpid: &str, wup: &str, mut ready: impl FnMut(Pid) -> bool) -> Result<(Pid, Vec<String>), String> {
    let from = sh.out.len();
    let g = sh.group(n, &format!("( {job} & echo \"{wpid}=$!\"; exec sleep 30 ) &\necho {wup}\n"), GROUP_MS)?;
    let marker = format!(":q{n}:");
    let shown = from + g.len() + marker.len() + 1;
    let key = format!("{wpid}=");
    let pid_in = |out: &[u8]| String::from_utf8_lossy(&out[from..]).lines().find_map(|l| l.strip_prefix(&key).and_then(|v| v.trim().parse::<u64>().ok())).map(Pid);
    sh.until(SETTLE_MS, |s| pid_in(&s.out).is_some());
    let lines = |out: &[u8]| String::from_utf8_lossy(&out[from..]).lines().filter(|l| *l != marker).map(String::from).collect::<Vec<_>>();
    let Some(p) = pid_in(&sh.out) else {
        forward(&sh.out[shown..]);
        return Err(format!("group {n} printed {:?}: no `{wpid}=<pid>` line for the background `{job} &` within {SETTLE_MS} ms by the PM timer", lines(&sh.out)));
    };
    sh.until(SETTLE_MS, |_| ready(p) || exit_of(p).is_some() || task_of(p).is_none());
    sh.hold(SETTLE_TICKS, SETTLE_MS);
    forward(&sh.out[shown..]);
    Ok((p, lines(&sh.out)))
}

/// The two lines the first half prints, `<wpid>=<pid>` and `<wup>`, in
/// either order (the subshell's echo races the shell's).
fn staged_text(r: &mut Row, n: usize, lines: &[String], wpid: &str, p: Pid, wup: &str) {
    let mut got: Vec<&str> = lines.iter().map(String::as_str).collect();
    got.sort_unstable();
    let pl = format!("{wpid}={}", p.0);
    let mut want = [pl.as_str(), wup];
    want.sort_unstable();
    if got != want {
        r.fail(format_args!("group {n} printed {lines:?}; Linux's busybox prints `{pl}` and `{wup}`, in either order"));
    }
}

/// The shell's live child running `sleep`.
fn sleeper_of(shell: Pid) -> Option<Pid> {
    let mut out = None;
    quiet(|| {
        proc::each(|p| {
            if p.ppid == shell && p.exit.is_none() && p.argv.first().map(String::as_str) == Some("sleep") {
                out = Some(p.pid);
            }
        })
    });
    out
}

/// Row 11, in two halves as rows 9 and 10. A child shell asks for SIGKILL
/// to be ignored and for a trap on SIGSTOP, sends itself SIGSTOP, and waits
/// for a `sleep 30` with every signal it can name blocked (ash's `wait`).
/// Once it has its `sleep 30`, and `SETTLE_TICKS` more, it must still run
/// and have printed nothing; then `kill -9` must end it, killed by signal 9, within
/// `KILL_TICKS`.
fn uncatchable_row(sh: &mut Sh, w: [&str; 5], wp: [&str; 2]) {
    let n = 11;
    let [wpid, wtrap, wpast, wup, wdone] = w;
    let mut r = Row::new(UNCATCH_ID);
    let job = format!("sh -c 'trap \"\" KILL; trap \"echo {wtrap}\" STOP; kill -STOP $$; sleep 30 & wait; echo {wpast}'");
    let what = "the child shell that ran `trap '' KILL` and waits for its `sleep 30` (ash's `wait`, every signal blocked)";
    let (p, lines) = match stage(sh, n, &job, wpid, wup, |p| sleeper_of(p).is_some()) {
        Ok(v) => v,
        Err(why) => return fail_one(r, n, format_args!("{why}")),
    };
    if lines.iter().any(|l| l == wtrap) {
        r.fail(format_args!(
            "the child shell (pid {}) ran its trap on SIGSTOP after `kill -STOP $$`: SIGSTOP cannot be caught, so `rt_sigaction(19, <a handler>)` is -EINVAL and the action stays SIG_DFL, which does nothing here (there is no stopped state)",
            p.0
        ));
    } else {
        staged_text(&mut r, n, &lines, wpid, p, wup);
    }
    if exit_of(p).is_some() || task_of(p).is_none() {
        let s = if task_of(p).is_none() { String::from("was gone") } else { status_text(exit_of(p)) };
        return fail_one(r, n, format_args!(
            "{what} (pid {}) {s} {SETTLE_TICKS} ticks of IRQ 0 after it started, before SIGKILL was sent: SIGSTOP does nothing here (there is no stopped state), and its `wait` lasts as long as its `sleep 30`",
            p.0
        ));
    }
    if sleeper_of(p).is_none() {
        r.fail(format_args!("{what} (pid {}) had no `sleep 30` of its own {SETTLE_MS} ms by the PM timer after it started, so it never reached its `wait`", p.0));
    }
    let t0 = ticks();
    let g = sh.group(format_args!("{n}b"), &format!("kill -9 {}\necho {wdone}\n", p.0), GROUP_MS);
    expect(&mut r, format_args!("{n}b"), &g, &format!("{wdone}\n"), "`kill` of a waiting process prints nothing");
    if g.is_err() {
        return r.finish(n);
    }
    let dt = await_end(sh, p, t0);
    match exit_of(p) {
        Some(9) if dt > KILL_TICKS => r.fail(format_args!(
            "{what} (pid {}) ended, killed by signal 9, but {dt} ticks of IRQ 0 after `kill -9` was written to the shell, past {KILL_TICKS}: SIGKILL ends a wait at once, whatever the mask holds",
            p.0
        )),
        Some(9) => {
            if !r.failed {
                note(format_args!("{UNCATCH_ID}: the shell (pid {}) still ran {SETTLE_TICKS} ticks of IRQ 0 after it had its `sleep 30`; `kill -9` ended it {dt} ticks of IRQ 0 after it was written, killed by signal 9", p.0));
            }
        }
        None if task_of(p).is_none() => r.fail(format_args!(
            "{what} (pid {}) ended and was collected before its status could be read: SIGKILL ends it, killed by signal 9",
            p.0
        )),
        s => r.fail(format_args!(
            "{what} (pid {}) {} {KILL_MS} ms after `kill -9` sent it SIGKILL: no action and no mask keeps SIGKILL out (`rt_sigaction(9, <an action>)` is -EINVAL, and `rt_sigprocmask` never blocks it), so it ends the process, killed by signal 9 (status 137 to a shell)",
            p.0,
            status_text(s)
        )),
    }
    // The third group: the raw calls, from `sigprobe uncatch`.
    let (lines, st) = match probe(sh, n, "uncatch", wp) {
        Ok(v) => v,
        Err(why) => return fail_one(r, n, format_args!("{why}")),
    };
    let [w0, _] = wp;
    let pwhat = "`sigprobe uncatch` blocked SIGKILL, SIGSTOP and SIGUSR2 (`rt_sigprocmask(SIG_BLOCK)`), read the mask back, asked for SIGKILL ignored (`rt_sigaction(9, SIG_IGN)`) and SIGSTOP caught (`rt_sigaction(19, <a handler>)`), and sent itself SIGKILL";
    let got = lines.first().and_then(|l| l.strip_prefix(&format!("{w0} "))).map(|v| v.split(' ').map(String::from).collect::<Vec<_>>());
    match got {
        Some(f) if f.len() == 3 => {
            if f[0] != "800" {
                r.fail(format_args!("{pwhat}: the mask read back {:#x}, where Linux's is 0x800, SIGUSR2's bit alone — no mask holds SIGKILL or SIGSTOP, so `SIG_BLOCK` leaves both out", u64::from_str_radix(&f[0], 16).unwrap_or(0)));
            }
            for (v, call) in [(&f[1], "rt_sigaction(9, SIG_IGN)"), (&f[2], "rt_sigaction(19, <a handler>)")] {
                if v != "-22" {
                    r.fail(format_args!("{pwhat}: {call} returned {v}; no action changes SIGKILL or SIGSTOP, so it is -22 (EINVAL)"));
                }
            }
            if st != Some(137) || lines.len() != 2 || lines[1] != "Killed" {
                r.fail(format_args!("{pwhat}: it printed {lines:?}, {}; after its line Linux's answer is ash's `Killed`, `$?` 137 — SIGKILL ends it whatever its mask holds", shell_status(st)));
            }
        }
        _ => r.fail(format_args!("{pwhat}: it printed {lines:?}, {}; Linux's answer starts `{w0} 800 -22 -22`", shell_status(st))),
    }
    r.finish(n);
}

/// What rows 5 and 7 to 10 wait for before they look at the process.
enum Ready {
    /// `$!` runs this program.
    Runs(&'static str),
    /// `$!` has a live child running `sleep`.
    Sleeper,
    /// The job's writer runs `yes` and is asleep, or a writer has waited
    /// for room in a full pipe since the group began.
    Writer,
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

/// Rows 5 and 7 to 10, in two halves. First `( <job> & echo …=$!; exec
/// sleep 30 ) &`, then (`stage`) until it runs what it was started for
/// (`Ready`), and `SETTLE_TICKS` more: the process must still run then
/// (row 9: and be asleep, or a writer have waited for room in a full pipe
/// since the group began). Then `kill -<sig> <pid>`, and the process must
/// end, killed by `sig`, within `KILL_TICKS`. The subshell that starts the job
/// becomes `sleep 30`, which never waits for a child, so the process stays
/// in the table, its status readable, until the row has read it: from lab
/// 32 ash is sent SIGCHLD and reaps its own children at once, which raced
/// this read when ash was the parent (measured 2026-10-29, slice 9).
#[allow(clippy::too_many_arguments)]
fn staged_row(sh: &mut Sh, n: usize, id: &'static str, job: &str, target: Target, ready: Ready, who: (&str, &str), waits_why: &str, sleep: &str, w: [&str; 3], sig: (u64, &str)) {
    let (num, name) = sig;
    let (who, doing) = who;
    let [wpid, wup, wdone] = w;
    let mut r = Row::new(id);
    let waits = pipe::full_waits();
    let is_ready = |last: Pid| match ready {
        Ready::Runs(prog) => argv0_of(last).as_deref() == Some(prog),
        Ready::Sleeper => sleeper_of(last).is_some(),
        Ready::Writer => parent_of(last).and_then(writer_of).is_some_and(|w| asleep(w) || pipe::full_waits() != waits),
    };
    let (last, lines) = match stage(sh, n, job, wpid, wup, is_ready) {
        Ok(v) => v,
        Err(why) => return fail_one(r, n, format_args!("{why}")),
    };
    staged_text(&mut r, n, &lines, wpid, last, wup);
    let p = match target {
        Target::Last => last,
        Target::Writer => match parent_of(last).and_then(writer_of) {
            Some(p) => p,
            None => {
                return fail_one(r, n, format_args!(
                    "{SETTLE_TICKS} ticks of IRQ 0 after `{job} &` started, before any signal was sent, no process of that job running `yes` was left beside `sleep 30` (pid {}): its reader never reads, so `yes` fills the pipe and waits for room in `write`",
                    last.0
                ))
            }
        },
    };
    if exit_of(p).is_some() || task_of(p).is_none() {
        let s = if task_of(p).is_none() { String::from("was gone") } else { status_text(exit_of(p)) };
        return fail_one(r, n, format_args!(
            "{who} (pid {}) {s} {SETTLE_TICKS} ticks of IRQ 0 after `{job} &` started, before any signal was sent: {waits_why}",
            p.0
        ));
    }
    if matches!(target, Target::Writer) && !asleep(p) && pipe::full_waits() == waits {
        r.fail(format_args!(
            "{who} (pid {}) still ran {SETTLE_TICKS} ticks of IRQ 0 after `{job} &` started, but was not asleep: a write that finds the pipe's 4096 bytes full, with a reader open, waits for room (lab 15)",
            p.0
        ));
    }
    let t0 = ticks();
    let g = sh.group(format_args!("{n}b"), &format!("kill -{num} {}\necho {wdone}\n", p.0), GROUP_MS);
    expect(&mut r, format_args!("{n}b"), &g, &format!("{wdone}\n"), "`kill` of a sleeping process prints nothing");
    if g.is_err() {
        return r.finish(n);
    }
    let dt = await_end(sh, p, t0);
    match exit_of(p) {
        Some(s) if s as u64 == num && dt > KILL_TICKS => r.fail(format_args!(
            "{who} (pid {}), {doing}, ended, killed by signal {num}, but {dt} ticks of IRQ 0 after `kill -{num}` was written to the shell, past {KILL_TICKS}: a signal ends {sleep} at once, so the sleep must see it each time it wakes",
            p.0
        )),
        Some(s) if s as u64 == num => {
            if !r.failed {
                note(format_args!("{id}: {who} (pid {}) still ran {SETTLE_TICKS} ticks of IRQ 0 after `{job} &` started; `kill -{num}` ended it {dt} ticks of IRQ 0 after it was written, killed by signal {num}", p.0));
            }
        }
        None if task_of(p).is_none() => r.fail(format_args!(
            "{who} (pid {}), {doing}, ended and was collected before its status could be read: SIG{name}'s default action ends it, killed by signal {num}",
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

/// Pump the tee until process `p` has ended or is gone, for at most
/// `KILL_MS` by the PM timer; the ticks of IRQ 0 since `t0`.
fn await_end(sh: &mut Sh, p: Pid, t0: u64) -> u64 {
    let mut c = Clock::new();
    while exit_of(p).is_none() && task_of(p).is_some() && c.ms() < KILL_MS {
        sh.pump();
        sched::yield_now();
    }
    ticks().wrapping_sub(t0)
}

/// Process `pid`'s task is asleep: off the CPU, out of the run queue, and
/// not `Dead` (ktest-15's test).
fn asleep(pid: Pid) -> bool {
    let Some(tid) = task_of(pid) else { return false };
    quiet(|| task::with_table(|tt| tt.current != tid && !tt.ready.contains(&tid) && tt.get(tid).is_some_and(|t| t.state != State::Dead)))
}

/// Row `n` failed with `why`, and finished.
fn fail_one(mut r: Row, n: usize, why: core::fmt::Arguments) {
    r.fail(why);
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
