//! Lab 26's vehicle (`26-devices`): the framebuffer and the keyboard as a
//! Linux program finds them, `/dev/fb0` and `/dev/input/event0`, driven by
//! two static musl programs (`user/build.sh`, layout `musl`) run unmodified
//! on the learner's kernel: `/bin/fbtest` and `/bin/evtest`.
//!
//! The disk is `ktest-26.img` as the course's image writer made it (CONTRACT
//! §A.7). The vehicle mounts ext2 at `/` from `/dev/hda2` (the learner's
//! `Ata`, `partition2`, `Ext2::mount`), calls the learner's `devfs::probe`
//! and mounts a `DevFs::new()` at `/dev`, then starts each program with the
//! learner's exec, its fds 1 and 2 a pipe this file drains. Every row is
//! PLAIN (CONTRACT §A.1-5): what a row expects is the machine's own
//! framebuffer geometry and the fixed keys of the rubric's `[[script]]`.
//!
//! Before anything runs, the vehicle asks the learner's `kbd::to_keycode`
//! (lab 26 step 1) about every set-1 scancode, 0 to 0xff, and prints row 1.
//! Each program run's rows print as soon as it ends (rows 2 and 3 as soon
//! as `fbtest fill` has drawn, before it unmaps), so a later task's panic
//! leaves the rows before it standing; nothing here pushes a key until
//! rows 1 to 7 are printed, so `feed` (step 8) is first called after them.
//!
//! The ten KTAP rows, printed in this order, and one `fb` row read from
//! outside:
//!
//!   1 `input.keycode.table` (plain) — `kbd::to_keycode` gives each of the
//!     256 scancodes its `KEY_*` code: a make code 0x01 to 0x58 but 0x54 is
//!     its own number, a break code (bit 7 set) its make code's, and every
//!     other byte `None`.
//!   2 `fb0.vscreeninfo` (plain) — `fbtest fill`'s `var` line is the
//!     bootloader's framebuffer (BootInfo `fb`): `var <width> <height>
//!     <width> <height> 32 r 16/8 g 8/8 b 0/8 t 0/0`, the pixel the kernel
//!     itself plots (`0x00RRGGBB`), as `FBIOGET_VSCREENINFO` (0x4600) gave it.
//!   3 `fb0.fscreeninfo` (plain) — its `fix` line: `smem` = pitch × height,
//!     `line` = the pitch, `type` 0 (packed pixels), `visual` 2 (true
//!     colour), from `FBIOGET_FSCREENINFO` (0x4602); and `other -25`: a
//!     request the device does not serve (`FBIOPAN_DISPLAY`) is `-ENOTTY`.
//!     The id is not graded.
//!   4 `fb0.mmap.fill` (plain) — `fbtest fill` maps `/dev/fb0` `MAP_SHARED`,
//!     fills the screen with (0x18, 0x2c, 0x48), reads the rectangle's
//!     middle back through its mapping (`fill 00182c48`), and waits on its
//!     fd 0, a pipe this file holds, with the mapping in place. This file
//!     reads that pixel through the HHDM until it is `0x00182c48` or
//!     `SHOW_MS` has passed. Once it is there, and the program has had
//!     `LOOK_MS` to run, the program must still be running with no `munmap`
//!     line printed, and its own address space must still map the page that
//!     holds that pixel (`mapped_at`: the start of its one VMA at least as
//!     long as the screen, plus the pixel's offset, a present leaf entry by
//!     the learner's `AddressSpace::entry_mut`); else the row fails: what
//!     this file saw is not the screen while the mapping was in place. Then
//!     it closes the pipe; the program unmaps and exits 0 (`munmap 0`), and
//!     the pixel must still be there. What that proves: a store through the
//!     mapping is on the screen while the mapping is in place, not only once
//!     it is unmapped, whatever the kernel does with the program's fd 0 or
//!     however long its `munmap` takes to return (a kernel that copies a
//!     buffer of its own to the screen while it is mapped, on a timer, a
//!     syscall or a yield, passes; which frames the page maps, and frames in
//!     use, are not graded).
//!   5 `fb0.mmap.fork` (plain) — `fbtest` does the same, then forks; the
//!     CHILD draws a (0xe8, 0x70, 0x2c) rectangle from 3/4 of the width and
//!     height through the mapping it inherited and waits on fd 0; the
//!     parent reads the rectangle back through its own mapping (`private
//!     <n> child 00e8702c`; `MAP_PRIVATE`'s answer is not graded) and waits
//!     too. This file reads the rectangle's middle through the HHDM, as for
//!     row 4, with both mappings in place (the parent still running with no
//!     `munmap` line, a child of it still running, and each one's address
//!     space still mapping the page that holds that pixel), then closes the
//!     pipe; the child exits 0, and the parent unmaps and exits 0 (`munmap 0`).
//!   - `fb0.mmap.frame` (an `fb` row of the rubric, not KTAP) — the grader
//!     samples that rectangle at (1088, 680) on QEMU's 1280 × 800:
//!     `0xE8702C`. This file draws nothing; it notes what it reads there
//!     through the HHDM, and the frames in use before and after `fbtest`
//!     (device frames are never the pool's: counting or freeing one stops
//!     the kernel). Row 5 is what keeps a screen painted some other way
//!     (by the kernel, with no working mapping) from scoring.
//!   6 `input.event0.caps` (plain) — `evtest probe` finds `event0` in
//!     `/dev/input`, `EVIOCGBIT(0)` answers 8 bytes with `EV_SYN` and
//!     `EV_KEY`, `EVIOCGBIT(EV_KEY)` 96 bytes with `KEY_ENTER`, `KEY_A` and
//!     `KEY_UP`, every code 1 to 88 but 84 (`lack 0`), and none of 0, 84 or
//!     256 and up (`spare 0`); `EVIOCGRAB` takes the device, a second
//!     reader's grab is `-EBUSY` and its release `-EINVAL`; with that
//!     reader still open the holder lets go, a third open takes the grab
//!     and closes, a fourth takes it and lets go, and the holder takes it
//!     back (`follow 0`); once the fourth, a reader that let go, has closed,
//!     the second's grab is still `-EBUSY` (`kept -16`: `caps event0 ev
//!     8:03 key 96:111 lack 0 spare 0 grab 0 busy -16 steal -22 follow 0
//!     kept -16`), and the holder's grab goes back once (`end release 0
//!     again -22`).
//!   7 `input.poll.files` (plain) — `evtest probe`'s `files ok`: `poll` (7)
//!     gives 0 at once and after 30 ms on the empty device; a pipe's ends
//!     (empty: `POLLOUT` only; a byte: `POLLIN`; no writer: `POLLHUP`; no
//!     reader: the write end `POLLOUT` and `POLLERR`); `POLLNVAL` for an fd
//!     not open and nothing for a negative one.
//!   8 `input.keys.codes` (plain, the script's keys) — once this file
//!     prints `# sboot keys-ready` the script presses k, e, y, up and
//!     escape, and `evtest` reads exactly `keys 37+ . 37- . 18+ . 18- . 21+
//!     . 21- . 103+ . 103- . 1+ . 1- . time ok` — each key's press and
//!     release, each followed by `SYN_REPORT`, timestamped by
//!     `CLOCK_REALTIME`; none of those keys reached the console while the
//!     device was grabbed; and with `evtest` gone a key reaches the
//!     console's ring again.
//!   9 `input.nonblock.eagain` (plain) — `evtest` clears `O_NONBLOCK`
//!     before it prints `ready` and reads once right after it, before the
//!     script types: that read sleeps until the first key and returns it
//!     (`block ok`). Last, a read of the empty device opened `O_NONBLOCK`
//!     is `-EAGAIN` (`eagain ok`); if it sleeps instead, this file wakes it
//!     with one more key after `UNBLOCK_MS`, so only this row goes red. A
//!     read that is never woken holds `evtest` until `RUN_MS`.
//!  10 `input.poll` (plain) — `POLLIN` and `POLLOUT` on a console holding
//!     a typed line (this file types `zq` and enter into the ring first),
//!     0 once it is read; then `evtest` writes `# sboot poll-ready` to
//!     /dev/console and polls, and the rubric's `[[script]]` types F12
//!     once that line is on the serial port (the engine reads it there:
//!     `evtest`'s stdout is this file's pipe, read only when this file
//!     runs, so it cannot carry the cue). Its `poll(3000)` must wake for
//!     the press within 2 s with `POLLIN`, and `POLLIN` must come before
//!     each of the four events, 0 after them (`poll ok`). The key comes
//!     through the real interrupt, as the script's five do, so the wake is
//!     graded whether or not this file gets the CPU while `evtest` polls
//!     (G1820). If `evtest` never prints its `poll` line this file prints
//!     `# sboot poll-ready` itself, last, so the script runs out.
//!
//! Plan 10. A program still running after `RUN_MS` by the PM timer fails
//! the rows whose lines it had not printed; the vehicle then goes on.

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
use kernel::fs::devfs::{self, DevFs};
use kernel::fs::ext2::Ext2;
use kernel::fs::mount;
use kernel::fs::vfs::InodeRef;
use kernel::ktap;
use kernel::pipe;
use kernel::proc::{self, Pid};
use kernel::syscall::{self, Errno};
use kernel::task::{self, Tid};
use kernel::paging::flags;
use kernel::{bootinfo, frame, gdt, heap, idt, irq, kbd, sched, serial, time};

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 64;
const PM_HZ: u64 = 3_579_545;
const O_NONBLOCK: u32 = 0x800;

const PLAN: usize = 10;
const KEYCODE_ID: &str = "input.keycode.table";
const VAR_ID: &str = "fb0.vscreeninfo";
const FIX_ID: &str = "fb0.fscreeninfo";
const FILL_ID: &str = "fb0.mmap.fill";
const FORK_ID: &str = "fb0.mmap.fork";
const CAPS_ID: &str = "input.event0.caps";
const FILES_ID: &str = "input.poll.files";
const KEYS_ID: &str = "input.keys.codes";
const EAGAIN_ID: &str = "input.nonblock.eagain";
const POLL_ID: &str = "input.poll";

/// How long one program may run, by the PM timer.
const RUN_MS: u64 = 15_000;
/// How long `evtest` may stay in its last read before this file wakes it.
const UNBLOCK_MS: u64 = 2_000;
/// The keys the rubric's `[[script]]` sends (k, e, y, up, escape), as
/// `evtest` prints them: press, `SYN_REPORT`, release, `SYN_REPORT`.
const KEYS_LINE: &str = "keys 37+ . 37- . 18+ . 18- . 21+ . 21- . 103+ . 103- . 1+ . 1- . time ok";
const CAPS_LINE: &str = "caps event0 ev 8:03 key 96:111 lack 0 spare 0 grab 0 busy -16 steal -22 follow 0 kept -16";
const END_LINE: &str = "end release 0 again -22";
/// `fbtest fill`'s line: the fill colour read back through its mapping.
const FILL_LINE: &str = "fill 00182c48";
/// What `fbtest` prints once it has unmapped.
const MUNMAP_LINE: &str = "munmap 0";
/// The fill and the rectangle colours as the screen holds them, and
/// `fbtest`'s line after the fork, past its `private <n>`.
const FILL_PX: u32 = 0x0018_2c48;
const RECT_PX: u32 = 0x00e8_702c;
const FORK_TAIL: &str = "child 00e8702c";
/// How long, by the PM timer, a store through fb0's mapping may take to
/// reach the screen while the program holds the mapping. A kernel that
/// maps a buffer of its own and copies it to the screen every 50 ticks, as
/// Linux's DRM fbdev emulation runs its deferred I/O every HZ/20, passes;
/// one that copies it only at `munmap` does not. Measured 2026-10-22
/// (fx26b; tests/honest/kernel-in-rust/26-devices/cases/periodic-flush):
/// that kernel showed the colour 60-64 ms after fbtest's line at load 7-8
/// and 113-142 ms at load 29-30, and was red on rows 4 and 5 when this
/// file read the screen once (`SHOW_MS` 0).
const SHOW_MS: u64 = 2_000;
/// Once the colour is on the screen, how long, by the PM timer, the program
/// is let run before this file asks whether it still holds its mapping: a
/// settle only, so a program that had already unmapped, or was about to say
/// so, gets the CPU to print `munmap` or end (sk26-re2-forger-2). The guard
/// itself is a property read after it, not this window: the program's own
/// address space must still map the pixel's page (`mapped_at`), however
/// long its `munmap` takes to return (sk26-re3-1: a `munmap` that copied the
/// buffer and then held the program 300 ms outlived a 50 ms window). A
/// program that holds its mapping is asleep on fd 0 all the while, so the
/// wait costs it nothing.
const LOOK_MS: u64 = 50;
/// What this file types into the console's ring before `evtest` runs:
/// z, q, enter, each pressed and released.
const LINE_KEYS: [u8; 6] = [0x2c, 0xac, 0x10, 0x90, 0x1c, 0x9c];
/// F12's make code: the key that wakes a read that slept (the script types
/// the poll's F12 itself).
const F12_MAKE: u8 = 0x58;

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

// ── One row's recorder (ktap's shapes, printed in this file's order) ─────

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
        note(why);
    }
    fn finish(self, n: usize) {
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

/// One program started by this file: its pid, the read end of
/// its output pipe and what has come out of it, and the write end of its
/// fd 0 while this file holds it open.
struct Run {
    path: &'static str,
    pid: Pid,
    r: FileRef,
    out: Vec<u8>,
    open: bool,
    err: Option<Errno>,
    hold: Option<FileRef>,
}

/// Whether `out` holds a whole line whose first word is `w`.
fn has_line(out: &[u8], w: &str) -> bool {
    let out = String::from_utf8_lossy(out);
    out.lines().zip(out.split_inclusive('\n')).any(|(l, raw)| raw.ends_with('\n') && l.split(' ').next() == Some(w))
}

/// `path` with `argv`, its fds 1 and 2 a pipe this file reads, and, when
/// `hold`, its fd 0 an empty pipe whose write end this file keeps (a read
/// of it waits until this file lets it go).
fn launch(path: &'static str, argv: &[&str], hold: bool) -> Result<Run, String> {
    let (r, w) = pipe::open(0).map_err(|e| format!("pipe::open(0) returned {}", errno_text(e)))?;
    r.flags.store(r.flags.load(Ordering::SeqCst) | O_NONBLOCK, Ordering::SeqCst);
    let (input, held) = if hold {
        let (ir, iw) = pipe::open(0).map_err(|e| format!("pipe::open(0) returned {}", errno_text(e)))?;
        (Some(ir), Some(iw))
    } else {
        (None, None)
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
    Ok(Run { path, pid, r, out: Vec::new(), open: true, err: None, hold: held })
}

/// Read every run's pipe until each is at end of file or `RUN_MS` has
/// passed; then give them up to half a second to be recorded as ended.
/// The first time a run that this file holds prints a whole line starting
/// with `at.0`, `at.1` is called with it, and then its fd 0 is let go.
/// Returns how long the reading took, by the PM timer.
fn drain_at(runs: &mut [Run], mut at: Option<(&str, &mut dyn FnMut(&mut Run))>) -> u64 {
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
            if let Some((word, f)) = at.as_mut() {
                if run.hold.is_some() && has_line(&run.out, word) {
                    f(run);
                    run.hold = None;
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
    for run in runs.iter_mut() {
        run.hold = None;
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
    let open = if run.open { String::from(", its output still open when its run was over") } else { String::new() };
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


// ── Step 1: kbd::to_keycode ───────────────────────────────────────────────

/// The `KEY_*` code set-1 scancode `sc` must give (linux/input-event-
/// codes.h: a make code 0x01 to 0x58 is its own KEY_ number, but for 0x54,
/// a number no key has; a break code, bit 7 set, is its make code's key;
/// every other byte, 0xE0 among them, is no key).
fn keycode_of(sc: u8) -> Option<u16> {
    match sc & 0x7f {
        0x54 => None,
        m @ 0x01..=0x58 => Some(m as u16),
        _ => None,
    }
}

/// Row 1, printed at once: every scancode, 0 to 0xff.
fn keycode_row() {
    let mut row = Row::new(KEYCODE_ID);
    let mut wrong = 0;
    for sc in 0..=0xffu8 {
        let (got, want) = (kbd::to_keycode(sc), keycode_of(sc));
        if got != want {
            wrong += 1;
            if wrong <= 4 {
                row.fail(format_args!("{KEYCODE_ID}: kbd::to_keycode({sc:#04x}) returned {got:?}; expected {want:?}"));
            }
        }
    }
    if wrong > 4 {
        row.fail(format_args!("{KEYCODE_ID}: {wrong} of the 256 scancodes answered wrong in all"));
    }
    row.finish(1);
}

// ── The programs' lines ──────────────────────────────────────────────────

/// The line of `out` that starts with `word` and a space, if one does.
fn line_of<'a>(out: &'a str, word: &str) -> Option<&'a str> {
    out.lines().find(|l| l.split(' ').next() == Some(word))
}

/// `what`'s line `word` is `want`; else why not, in one sentence.
fn expect_line(run: &Run, word: &str, want: &str) -> Option<String> {
    let out = String::from_utf8_lossy(&run.out);
    match line_of(&out, word) {
        Some(l) if l == want => None,
        Some(l) => Some(format!("{} printed {l:?}; expected {want:?}", run.path)),
        None => Some(format!(
            "{} printed no `{word}` line; expected {want:?}{}",
            run.path,
            ended(run).map(|e| format!(" ({e})")).unwrap_or_default()
        )),
    }
}

// ── Rows 1-2 and the fb row: /bin/fbtest ─────────────────────────────────

/// The rectangle's middle on a `w` × `h` screen, as `fbtest` draws it.
fn probe_point(w: u64, h: u64) -> (u64, u64) {
    (w / 4 * 3 + w / 5 / 2, h / 4 * 3 + h / 5 / 2)
}

/// `path argv` started and drained, or why it did not start.
fn run_one(path: &'static str, argv: &[&str]) -> Result<(Run, u64), String> {
    run_at(path, argv, None)
}

/// `path argv` with its fd 0 held, drained; `at` as `drain_at`'s.
fn run_at(path: &'static str, argv: &[&str], at: Option<(&str, &mut dyn FnMut(&mut Run))>) -> Result<(Run, u64), String> {
    let run = launch(path, argv, at.is_some())?;
    let mut runs = [run];
    let ms = drain_at(&mut runs, at);
    let [run] = runs;
    Ok((run, ms))
}

/// The screen's pixel at (x, y), read through the HHDM.
fn screen_at(fb: &bootinfo::FramebufferInfo, x: u64, y: u64) -> u32 {
    // SAFETY: the bootloader's framebuffer is mapped for the whole run, and
    // (x, y) is inside it.
    unsafe { ((fb.addr + y * fb.pitch + x * 4) as *const u32).read_volatile() }
}

/// (x, y) read through the HHDM until it holds `want` or `SHOW_MS` has
/// passed: `Ok` after how many ms, or `Err` with the last pixel read.
fn shows(fb: &bootinfo::FramebufferInfo, x: u64, y: u64, want: u32) -> Result<u64, u32> {
    let mut c = Clock::new();
    loop {
        let px = screen_at(fb, x, y);
        if px == want {
            return Ok(c.ms());
        }
        if c.ms() >= SHOW_MS {
            return Err(px);
        }
        sched::yield_now();
    }
}

/// Whether a child of `pid` is still running.
fn child_running(pid: Pid) -> bool {
    let mut found = false;
    quiet(|| {
        proc::each(|p| {
            if p.ppid == pid && p.exit.is_none() {
                found = true;
            }
        })
    });
    found
}

/// The leaf entry, in `pid`'s own address space, for byte `off` of its
/// mapping of the screen: the start of its one VMA at least `len` bytes long
/// (none other of `fbtest`'s is), plus `off`, walked with the learner's
/// `AddressSpace::entry_mut` (which creates nothing) on the process's own
/// task, and present. Else what was seen instead, as the end of a sentence
/// whose subject is the process. Which frame it maps is not asked: a kernel
/// that maps a buffer of its own and copies it to the screen while it is
/// mapped passes (Q10).
fn mapped_at(pid: Pid, len: u64, off: u64) -> Result<(), String> {
    let mut task: Option<Tid> = None;
    let mut starts = [0u64; 4];
    let mut n = 0usize;
    quiet(|| {
        proc::each(|p| {
            if p.pid == pid {
                task = Some(p.task);
                for v in p.vmas.iter() {
                    if v.end.0.saturating_sub(v.start.0) >= len && n < starts.len() {
                        starts[n] = v.start.0;
                        n += 1;
                    }
                }
            }
        })
    });
    let Some(tid) = task else {
        return Err(String::from("was no longer a process"));
    };
    if n == 0 {
        return Err(String::from("no longer had a mapping as long as the screen in its address space"));
    }
    let mut seen = String::new();
    for &start in &starts[..n] {
        let va = start + off;
        let e = quiet(|| task::with_table(|tt| tt.get_mut(tid).and_then(|t| t.aspace.as_mut()).and_then(|a| a.entry_mut(VirtAddr(va)).map(|e| e.0))));
        match e {
            Some(e) if e & flags::PRESENT != 0 => return Ok(()),
            Some(e) => seen = format!("had no page mapped at {va:#x} in its address space (its entry there is {e:#x}), where its mapping of /dev/fb0 holds that pixel"),
            None => seen = format!("had no page mapped at {va:#x} in its address space (no page table on the way), where its mapping of /dev/fb0 holds that pixel"),
        }
    }
    Err(seen)
}

/// A child of `pid` that is still running, if one is.
fn child_of(pid: Pid) -> Option<Pid> {
    let mut found = None;
    quiet(|| {
        proc::each(|p| {
            if p.ppid == pid && p.exit.is_none() && found.is_none() {
                found = Some(p.pid);
            }
        })
    });
    found
}

/// Called once the screen holds the colour: `run` gets `LOOK_MS` to run,
/// its pipe is read again, and unless it is still running with no `munmap`
/// line printed and its address space still maps byte `off` of its `len`-byte
/// mapping (and, with `child`, a child of it still running whose address
/// space maps it too), what was seen instead, as the end of a sentence. The
/// claim is the guard: a row that cannot show the mapping was still in
/// place fails.
fn let_go_seen(run: &mut Run, child: bool, len: u64, off: u64) -> Option<String> {
    let pid = run.pid;
    let _ = yield_until(LOOK_MS, || exit_of(pid).is_some());
    let mut buf = alloc::vec![0u8; 4096];
    while run.open {
        match run.r.read(&mut buf) {
            Ok(0) => run.open = false,
            Ok(n) => run.out.extend_from_slice(&buf[..n]),
            Err(11) => break,
            Err(e) => {
                run.err = Some(e);
                run.open = false;
            }
        }
    }
    let out = String::from_utf8_lossy(&run.out);
    if let Some(l) = line_of(&out, "munmap") {
        return Some(format!("had already printed {l:?}"));
    }
    if exit_of(pid).is_some() || !run.open {
        return Some(format!("had already ended, having printed {out:?}"));
    }
    if child && !child_running(pid) {
        return Some(String::from("was running with no child of it still running"));
    }
    if let Err(why) = mapped_at(pid, len, off) {
        return Some(why);
    }
    if child {
        match child_of(pid) {
            None => return Some(String::from("was running with no child of it still running")),
            Some(c) => {
                if let Err(why) = mapped_at(c, len, off) {
                    return Some(format!("was running, and its child (pid {}) {why}", c.0));
                }
            }
        }
    }
    None
}

/// Rows 2 and 3 off `fbtest fill`'s `var` and `fix` lines.
fn geometry_rows(run: &Run, fb: &bootinfo::FramebufferInfo) {
    let (w, h, pitch) = (fb.width, fb.height, fb.pitch);
    let (mut var, mut fix) = (Row::new(VAR_ID), Row::new(FIX_ID));
    let want_var = format!("var {w} {h} {w} {h} 32 r 16/8 g 8/8 b 0/8 t 0/0");
    if let Some(why) = expect_line(run, "var", &want_var) {
        var.fail(format_args!("{VAR_ID}: {why}"));
    }
    // The id (the third word) is the kernel's to name.
    let want_fix = format!("smem {} line {pitch} type 0 visual 2 other -25", pitch * h);
    let out = String::from_utf8_lossy(&run.out);
    match line_of(&out, "fix").map(|l| l.splitn(3, ' ').nth(2).unwrap_or("")) {
        Some(rest) if rest == want_fix => {}
        Some(rest) => fix.fail(format_args!("{FIX_ID}: /bin/fbtest printed `fix <id> {rest}`; expected `fix <id> {want_fix}`")),
        None => fix.fail(format_args!(
            "{FIX_ID}: /bin/fbtest printed no `fix` line; expected `fix <id> {want_fix}`{}",
            ended(run).map(|e| format!(" ({e})")).unwrap_or_default()
        )),
    }
    var.finish(2);
    fix.finish(3);
}

/// What the screen showed while the mapping was in place, as a row's
/// sentence when it is wrong: `held` names who held it, `live` is from
/// `shows`, `after` is the pixel once the program was done.
fn shown_wrong(held: &str, at: (u64, u64), live: Option<Result<u64, u32>>, want: u32, after: u32) -> Option<String> {
    let (x, y) = at;
    match live {
        None | Some(Ok(_)) => None,
        Some(Err(px)) if after == want => Some(format!(
            "while {held}, the screen at ({x}, {y}) held {px:#010x}, not the {want:#010x} stored through the mapping; the screen held it only once the program was done with the mapping"
        )),
        Some(Err(px)) => Some(format!(
            "while {held}, the screen at ({x}, {y}) held {px:#010x}, not the {want:#010x} stored through the mapping, and {after:#010x} once the program was done"
        )),
    }
}

/// Rows 2 to 5: `fbtest fill`, then `fbtest`.
fn fb_rows() {
    let fb = bootinfo::get().map(|bi| bi.fb);
    let (mut fill, mut fork) = (Row::new(FILL_ID), Row::new(FORK_ID));
    let Some(fb) = fb.filter(|f| f.bpp == 32 && f.addr != 0) else {
        for (n, id) in [(2, VAR_ID), (3, FIX_ID), (4, FILL_ID), (5, FORK_ID)] {
            let mut row = Row::new(id);
            row.fail(format_args!("{id}: not run: the bootloader gave no 32-bit framebuffer"));
            row.finish(n);
        }
        return;
    };
    let (x, y) = probe_point(fb.width, fb.height);
    // The mapping fbtest makes, and where that pixel is in it.
    let (len, off) = (fb.pitch * fb.height, y * fb.pitch + x * 4);
    // Rows 2 and 3 print once fbtest has drawn, before it unmaps; the
    // screen is read while it holds the mapping.
    let mut geometry = false;
    let mut live: Option<Result<u64, u32>> = None;
    let mut gone: Option<String> = None;
    let mut at_fill = |run: &mut Run| {
        geometry_rows(run, &fb);
        geometry = true;
        let seen = shows(&fb, x, y, FILL_PX);
        if seen.is_ok() {
            gone = let_go_seen(run, false, len, off);
        }
        live = Some(seen);
    };
    match run_at("/bin/fbtest", &["fbtest", "fill"], Some(("fill", &mut at_fill))) {
        Err(why) => {
            for (n, id) in [(2, VAR_ID), (3, FIX_ID)] {
                let mut row = Row::new(id);
                row.fail(format_args!("{id}: /bin/fbtest did not start: {why}"));
                row.finish(n);
            }
            geometry = true;
            fill.fail(format_args!("{FILL_ID}: /bin/fbtest did not start: {why}"));
        }
        Ok((run, _)) => {
            if !geometry {
                geometry_rows(&run, &fb);
                geometry = true;
            }
            let after = screen_at(&fb, x, y);
            if let Some(why) = expect_line(&run, "fill", FILL_LINE) {
                fill.fail(format_args!("{FILL_ID}: {why}"));
            } else if let Some(why) = shown_wrong("`/bin/fbtest fill` held its shared mapping of /dev/fb0", (x, y), live, FILL_PX, after) {
                fill.fail(format_args!("{FILL_ID}: {why}"));
            } else if let Some(seen) = &gone {
                fill.fail(format_args!(
                    "{FILL_ID}: when the screen at ({x}, {y}) held the {FILL_PX:#010x} stored through the mapping, `/bin/fbtest fill` {seen}, so it was not seen there while the program held its shared mapping of /dev/fb0"
                ));
            } else if let Some(why) = expect_line(&run, "munmap", MUNMAP_LINE) {
                fill.fail(format_args!("{FILL_ID}: {why}"));
            } else if let Some(e) = ended(&run) {
                fill.fail(format_args!("{FILL_ID}: {e}"));
            }
            if after != FILL_PX {
                fill.fail(format_args!(
                    "{FILL_ID}: after `/bin/fbtest fill` the screen at ({x}, {y}) holds {after:#010x}, not the {FILL_PX:#010x} it stored through its shared mapping of /dev/fb0"
                ));
            }
            if let (Some(Ok(_)), None) = (live, &gone) {
                note(format_args!("{FILL_ID}: the fill was on the screen while `/bin/fbtest fill` held its mapping"));
            }
        }
    }
    let _ = geometry;
    fill.finish(4);
    let f0 = frame::stats().used;
    let mut live: Option<Result<u64, u32>> = None;
    let mut gone: Option<String> = None;
    let mut at_fork = |run: &mut Run| {
        let seen = shows(&fb, x, y, RECT_PX);
        if seen.is_ok() {
            gone = let_go_seen(run, true, len, off);
        }
        live = Some(seen);
    };
    match run_at("/bin/fbtest", &["fbtest"], Some(("private", &mut at_fork))) {
        Err(why) => fork.fail(format_args!("{FORK_ID}: /bin/fbtest did not start: {why}")),
        Ok((run, _)) => {
            let f1 = frame::stats().used;
            let after = screen_at(&fb, x, y);
            let out = String::from_utf8_lossy(&run.out);
            let line = line_of(&out, "private");
            let tail = line.map(|l| l.splitn(3, ' ').nth(2).unwrap_or(""));
            if tail != Some(FORK_TAIL) {
                fork.fail(format_args!(
                    "{FORK_ID}: /bin/fbtest printed {}; expected `private <n> {FORK_TAIL}`{}",
                    line.map_or(String::from("no `private` line"), |l| format!("{l:?}")),
                    ended(&run).map(|e| format!(" ({e})")).unwrap_or_default()
                ));
            } else if let Some(why) = shown_wrong("`/bin/fbtest` and its forked child held their shared mappings of /dev/fb0", (x, y), live, RECT_PX, after) {
                fork.fail(format_args!("{FORK_ID}: {why}"));
            } else if let Some(seen) = &gone {
                fork.fail(format_args!(
                    "{FORK_ID}: when the screen at ({x}, {y}) held the {RECT_PX:#010x} the child stored through its mapping, `/bin/fbtest` {seen}, so it was not seen there while both held their shared mappings of /dev/fb0"
                ));
            } else if let Some(why) = expect_line(&run, "munmap", MUNMAP_LINE) {
                fork.fail(format_args!("{FORK_ID}: {why}"));
            } else if let Some(e) = ended(&run) {
                fork.fail(format_args!("{FORK_ID}: {e}"));
            }
            note(format_args!(
                "fb0.mmap.frame: the screen at ({x}, {y}) holds {after:#010x} (the fb row wants 0x00e8702c); fbtest: {:?}; {}; frames in use {f0} before fbtest, {f1} after{}",
                line.unwrap_or("no `private` line"),
                ended(&run).unwrap_or_else(|| String::from("fbtest exited 0")),
                match (live, &gone) {
                    (Some(Ok(_)), None) => String::from("; the rectangle was on the screen while both held their mappings"),
                    _ => String::new(),
                }
            ));
        }
    }
    fork.finish(5);
}

// ── Rows 6-10: /bin/evtest and the script's keys ─────────────────────────

/// Read `run`'s pipe until it is at end of file or `RUN_MS` has passed,
/// acting on its lines as they come: `ready` → `# sboot keys-ready`;
/// `poll` printed and `evtest` still there after `UNBLOCK_MS` → one more
/// key, to wake a read that slept. If `evtest` ends before `ready`, the
/// keys-ready line is printed anyway so the script runs out.
/// Returns (ms, whether this file had to wake it).
fn drive(run: &mut Run) -> (u64, bool) {
    let mut buf = alloc::vec![0u8; 4096];
    let mut c = Clock::new();
    let (mut ready, mut woke) = (false, false);
    let mut poll_at: Option<u64> = None;
    loop {
        match run.r.read(&mut buf) {
            Ok(0) => run.open = false,
            Ok(n) => run.out.extend_from_slice(&buf[..n]),
            Err(11) => {}
            Err(e) => {
                run.err = Some(e);
                run.open = false;
            }
        }
        let has = |w: &str| has_line(&run.out, w);
        if !ready && (has("ready") || !run.open) {
            ready = true;
            quiet(|| kernel::kprintln!("# sboot keys-ready"));
        }
        if poll_at.is_none() && has("poll") {
            poll_at = Some(c.ms());
        }
        if !woke && run.open && poll_at.is_some_and(|t| c.ms() >= t + UNBLOCK_MS) {
            woke = true;
            quiet(|| kbd::push(F12_MAKE));
        }
        if !run.open || c.ms() >= RUN_MS {
            break;
        }
        sched::yield_now();
    }
    let ms = c.ms();
    let _ = yield_until(500, || exit_of(run.pid).is_some());
    (ms, woke)
}

/// Rows 6 and 7: `evtest probe`, before any key is pushed.
fn probe_rows() {
    let (mut caps, mut files) = (Row::new(CAPS_ID), Row::new(FILES_ID));
    match run_one("/bin/evtest", &["evtest", "probe"]) {
        Err(why) => {
            caps.fail(format_args!("{CAPS_ID}: /bin/evtest did not start: {why}"));
            files.fail(format_args!("{FILES_ID}: /bin/evtest did not start: {why}"));
        }
        Ok((run, _)) => {
            for (word, want) in [("caps", CAPS_LINE), ("end", END_LINE)] {
                if let Some(why) = expect_line(&run, word, want) {
                    caps.fail(format_args!("{CAPS_ID}: {why}"));
                }
            }
            if let Some(why) = expect_line(&run, "files", "files ok") {
                files.fail(format_args!("{FILES_ID}: {why}"));
            }
        }
    }
    caps.finish(6);
    files.finish(7);
}

/// Rows 8 to 10: `evtest` and the script's keys.
fn input_rows() {
    let (mut keys, mut eagain, mut poll) = (Row::new(KEYS_ID), Row::new(EAGAIN_ID), Row::new(POLL_ID));
    // A line for the console to hold when evtest asks: z, q, enter.
    while kbd::pop().is_some() {}
    quiet(|| LINE_KEYS.iter().for_each(|&sc| kbd::push(sc)));
    match launch("/bin/evtest", &["evtest"], false) {
        Err(why) => {
            quiet(|| kernel::kprintln!("# sboot keys-ready"));
            quiet(|| kernel::kprintln!("# sboot poll-ready"));
            for (row, id) in [(&mut keys, KEYS_ID), (&mut eagain, EAGAIN_ID), (&mut poll, POLL_ID)] {
                row.fail(format_args!("{id}: /bin/evtest did not start: {why}"));
            }
        }
        Ok(mut run) => {
            let (_, woke) = drive(&mut run);
            if let Some(why) = expect_line(&run, "keys", KEYS_LINE) {
                keys.fail(format_args!("{KEYS_ID}: {why}"));
            }
            // The grabbed keys went to the device alone.
            let mut leaked = Vec::new();
            while let Some(sc) = kbd::pop() {
                leaked.push(format!("{sc:#04x}"));
            }
            if !leaked.is_empty() {
                // Say "held" only when its caps line says the grab was taken
                // and taken back; otherwise say what that line showed.
                let out = String::from_utf8_lossy(&run.out);
                let field = |w: &str| line_of(&out, "caps").and_then(|l| l.split(' ').skip_while(|&t| t != w).nth(1).map(String::from));
                let while_ = match (field("grab"), field("follow"), field("kept")) {
                    (Some(g), _, _) if g != "0" => format!("while /bin/evtest ran, after its EVIOCGRAB 1 was refused (`grab {g}` in its caps line)"),
                    (Some(_), Some(f), _) if f != "0" => format!("while /bin/evtest ran, after its caps line showed `follow {f}`, not `follow 0`"),
                    (Some(_), Some(_), Some(k)) if k != "-16" => format!("while /bin/evtest ran, after its caps line showed `kept {k}`, not `kept -16`"),
                    (Some(_), Some(_), Some(_)) => String::from("while /bin/evtest held EVIOCGRAB"),
                    _ => String::from("while /bin/evtest ran, which printed no caps line"),
                };
                keys.fail(format_args!(
                    "{KEYS_ID}: scancodes {} reached the console's ring {while_}; a grabbed device's keys are the grabber's alone",
                    leaked.join(" ")
                ));
            }
            // With evtest gone, a key is the console's again.
            if exit_of(run.pid).is_none() {
                let out = String::from_utf8_lossy(&run.out);
                let last = out.lines().last().unwrap_or("");
                keys.fail(format_args!(
                    "{KEYS_ID}: /bin/evtest was still running at the end of its run, its last line {last:?}, so whether a key reaches the console once it lets go and ends could not be seen"
                ));
            } else {
                quiet(|| kbd::push(0x1e));
                let back = kbd::pop();
                while kbd::pop().is_some() {}
                if back != Some(0x1e) {
                    keys.fail(format_args!(
                        "{KEYS_ID}: after /bin/evtest ended, kbd::push(0x1e) then kbd::pop() gave {back:?}, not Some(0x1e): the grab, or the device's hold on the keys, outlived the last open file"
                    ));
                }
            }
            let out = String::from_utf8_lossy(&run.out);
            if line_of(&out, "ready").is_some() && line_of(&out, "block").is_none() && line_of(&out, "block:").is_none() {
                eagain.fail(format_args!(
                    "{EAGAIN_ID}: /bin/evtest printed `ready` and then read event0 without O_NONBLOCK, before any key was typed; that read was still asleep when its run was over, though the keys were typed after `# sboot keys-ready`"
                ));
            } else if let Some(why) = expect_line(&run, "block", "block ok") {
                let printed = line_of(&out, "block:").unwrap_or("");
                if printed.is_empty() {
                    eagain.fail(format_args!("{EAGAIN_ID}: {why}"));
                } else {
                    eagain.fail(format_args!("{EAGAIN_ID}: /bin/evtest printed {printed:?}; a read without O_NONBLOCK sleeps until there is an event"));
                }
            }
            if let Some(why) = expect_line(&run, "eagain", "eagain ok") {
                eagain.fail(format_args!("{EAGAIN_ID}: {why}"));
            }
            if woke {
                eagain.fail(format_args!(
                    "{EAGAIN_ID}: /bin/evtest was still in its last read after its poll line, until a key was pushed to wake it; a read of an empty O_NONBLOCK device must return -EAGAIN at once"
                ));
            }
            if let Some(why) = expect_line(&run, "poll", "poll ok") {
                poll.fail(format_args!("{POLL_ID}: {why}"));
            }
            note(format_args!("{KEYS_ID}: {}", ended(&run).unwrap_or_else(|| String::from("/bin/evtest exited 0"))));
            // evtest writes the script's cue itself, just before its poll; one
            // that never got there leaves the script waiting, so the line is
            // printed here, after the last look at the keys (the F12 it brings
            // lands in a ring nothing reads any more).
            if line_of(&out, "poll").is_none() {
                quiet(|| kernel::kprintln!("# sboot poll-ready"));
            }
        }
    }
    keys.finish(8);
    eagain.finish(9);
    poll.finish(10);
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


/// Rows 2 to 10 when the file systems did not mount.
fn not_run(why: &str) {
    quiet(|| kernel::kprintln!("# sboot keys-ready"));
    quiet(|| kernel::kprintln!("# sboot poll-ready"));
    for (n, id) in [VAR_ID, FIX_ID, FILL_ID, FORK_ID, CAPS_ID, FILES_ID, KEYS_ID, EAGAIN_ID, POLL_ID].into_iter().enumerate() {
        let mut r = Row::new(id);
        r.fail(format_args!("{id}: not run: {why}"));
        r.finish(n + 2);
    }
}

/// `/dev` as the learner's `DevFs`, after `devfs::probe` (lab 26 step 2).
fn mount_dev() -> Result<(), String> {
    quiet(devfs::probe);
    let dev: InodeRef = Arc::new(DevFs::new());
    mount::mount("devfs", "/dev", "devfs", dev).map_err(|e| format!("mount::mount(\"devfs\", \"/dev\", \"devfs\", a DevFs) returned {}", errno_text(e)))
}

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    serial::init();
    ktap::begin(PLAN);
    prelude();
    // Step 1, before anything runs: the key codes.
    keycode_row();
    match mount_root().and_then(|()| mount_dev()) {
        Err(why) => not_run(&format!("the file systems did not mount: {why}")),
        Ok(()) => {
            fb_rows();
            probe_rows();
            input_rows();
        }
    }
    sched::yield_now();
    quiet(ktap::end);
    halt()
}
