//! Lab 27's vehicle (`27-the-port`): a real program, unmodified, on the
//! learner's kernel — `/bin/doomgeneric` (upstream doomgeneric, the `linuxvt`
//! backend, static musl; `user/README.md`) with Freedoom's data file. Lab 27
//! adds no kernel interface (CONTRACT §B.2-27): everything the program calls
//! is labs 09–26's, and this file only starts it and watches.
//!
//! The disk is `ktest-27.img` as the course's image writer made it (CONTRACT
//! §A.7): partition 2 carries the W4 programs in `/bin` and the 28,795,076-byte
//! `/port/game.wad` (through the double-indirect block). The vehicle mounts
//! ext2 at `/` from `/dev/hda2`, calls the learner's `devfs::probe` and mounts
//! a `DevFs::new()` at `/dev` (ktest-26's order: `fb0` and `input/event0`
//! exist only after `probe`), then starts
//! `/bin/doomgeneric -iwad /port/game.wad` with the learner's exec and the
//! console as its fds 0, 1 and 2, so the engine's start-up lines reach the
//! serial line as it prints them.
//!
//! Then it watches the screen, by the ACPI PM timer — never by the
//! learner's clock or `nanosleep` — and stops once the program has drawn a
//! picture and holds it: every `SAMPLE_MS` it reads a 40 × 25 grid of the
//! framebuffer, and it stops when the grid differs from the screen at the
//! exec, at least a quarter of it is not black, and it has not changed for
//! `HOLD_MS`. It names no pixel and no colour of the title (the rubric's fb
//! row does; the vehicle must not hand that answer out). A loading screen
//! is black and a frame being drawn is not stable, so the first stable,
//! non-black picture is the title (the program draws nothing before it).
//! The stop: interrupts off, its one row, `# sboot end`, `hlt`. Nothing runs
//! after that, so the screen the grader dumps is the picture it stopped on.
//! If no picture holds within `BOUND_MS` (100 s) it stops there, and the fb
//! row reads whatever is on the screen: red for a kernel that never gets
//! the program to its title. (2026-10-22, decided over the 4.5 s fixed
//! stop: under 12–20 busy loops at load 12–27 the title first appeared
//! 4.2–8.4 s after the exec, and a fixed 4.5 s stop caught a black screen
//! 10 of 10 times; the title then stayed up 9–12 s.)
//!
//! The rubric's two rows are read from outside this file:
//!   - `port.startup.lines` (serial, plain) — substrings of what the program
//!     printed: the WAD it added and the start-up steps that read it.
//!   - `port.title.frame` (fb, plain) — exact colours at pixels in flat
//!     regions of the title picture's upper rows (`DG_DrawFrame` copies a frame
//!     line by line, top first).
//!
//! The one KTAP row, printed at the stop:
//!   1 `port.survives` (K, plain) — at the stop the program is still running
//!     (it never exits on its own: its loop is `while (1)`), nothing killed it
//!     (`mm::last_kill` did not move), and no syscall it made was `-ENOSYS`
//!     (`syscall::enosys_count` did not move). Each failure names the cause.
//!
//! Plan 1.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../shim.rs"]
mod shim;

use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use core::arch::asm;
use core::sync::atomic::Ordering;

use kernel::addr::VirtAddr;
use kernel::blk::ata::{self, Ata, BlockDevice};
use kernel::exec;
use kernel::fs::devfs::{self, DevFs};
use kernel::fs::ext2::Ext2;
use kernel::fs::mount;
use kernel::fs::vfs::InodeRef;
use kernel::ktap;
use kernel::proc::{self, Pid};
use kernel::syscall::{self, Errno};
use kernel::{bootinfo, frame, gdt, heap, idt, irq, mm, sched, serial, time};

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 64;
const PM_HZ: u64 = 3_579_545;

const PLAN: usize = 1;
const SURVIVES_ID: &str = "port.survives";

/// The program and its arguments.
const PROGRAM: &str = "/bin/doomgeneric";
const ARGV: [&str; 3] = ["doomgeneric", "-iwad", "/port/game.wad"];
/// The longest the vehicle waits for a held picture after the exec, by the
/// PM timer: 3 × the slowest stop of the slowest honest variant under 24
/// busy loops (slice 7's `h-read-twice`, block reads 2× slower: 32.05 s
/// after the exec at load 29–40, so 96.15 s), taken to the next 5 s
/// (2026-10-22, by TPM ruling over the 12-loop figure of 45 s: this runs on
/// the learner's machine, and the long wait falls only on a kernel that
/// never draws the title). The rubric's `timeout_secs` must exceed it with
/// the boot.
const BOUND_MS: u64 = 100_000;
/// How often the grid is read, and how long it must stay the same.
const SAMPLE_MS: u64 = 50;
const HOLD_MS: u64 = 300;
/// The grid: `GRID_W` × `GRID_H` points spread over the whole screen.
const GRID_W: u64 = 40;
const GRID_H: u64 = 25;
// The title lights 260 of these 1,000 points against the stop's 250; that
// holds only because the screen mode and the data file are fixed.

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
        19 => "ENODEV",
        20 => "ENOTDIR",
        22 => "EINVAL",
        38 => "ENOSYS",
        _ => "?",
    };
    format!("-{e} ({name})")
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

fn status_text(v: i32) -> String {
    if v & 0x7f == 0 {
        format!("ended exit({})", (v >> 8) & 0xff)
    } else {
        format!("was killed by signal {} (status {v:#x})", v & 0x7f)
    }
}

// ── The screen, sampled ──────────────────────────────────────────────────

/// A summary of the screen: a hash of the grid's pixels and how many of
/// them are not black. `None` when the bootloader gave no 32-bit
/// framebuffer.
fn sample() -> Option<(u64, u64)> {
    let fb = bootinfo::get().map(|bi| bi.fb).filter(|f| f.bpp == 32 && f.addr != 0 && f.width >= GRID_W && f.height >= GRID_H)?;
    let (mut h, mut lit) = (0xcbf2_9ce4_8422_2325u64, 0u64);
    for gy in 0..GRID_H {
        let y = gy * fb.height / GRID_H + fb.height / (2 * GRID_H);
        for gx in 0..GRID_W {
            let x = gx * fb.width / GRID_W + fb.width / (2 * GRID_W);
            // SAFETY: the bootloader's framebuffer is mapped for the whole
            // run, and (x, y) is inside it.
            let px = unsafe { ((fb.addr + y * fb.pitch + x * 4) as *const u32).read_volatile() } & 0x00ff_ffff;
            h = (h ^ px as u64).wrapping_mul(0x0100_0000_01b3);
            lit += (px != 0) as u64;
        }
    }
    Some((h, lit))
}

// ── Mounting (ktest-26's) ────────────────────────────────────────────────

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

/// `/dev` as the learner's `DevFs`, after `devfs::probe` (lab 26 step 2).
fn mount_dev() -> Result<(), String> {
    quiet(devfs::probe);
    let dev: InodeRef = Arc::new(DevFs::new());
    mount::mount("devfs", "/dev", "devfs", dev).map_err(|e| format!("mount::mount(\"devfs\", \"/dev\", \"devfs\", a DevFs) returned {}", errno_text(e)))
}

// ── The run ──────────────────────────────────────────────────────────────

/// Start the program, wait until it holds a picture on the screen (or
/// `BOUND_MS`), and say whether it is still running, unkilled and served.
/// Returns the row's failure, if any.
/// Ends with interrupts OFF: nothing may run between this and the stop.
fn port() -> Option<String> {
    let k0 = mm::last_kill();
    let e0 = syscall::enosys_count();
    let f0 = frame::stats().used;
    quiet(|| ktap::note(format_args!("port: exec {}", ARGV.join(" "))));
    let at_exec = sample().map(|(h, _)| h);
    let mut c = Clock::new();
    let pid = match exec::spawn(PROGRAM, &ARGV, &[]) {
        Ok(pid) => pid,
        Err(e) => {
            irq_off();
            return Some(format!("exec of {PROGRAM} returned {}", errno_text(e)));
        }
    };
    let started = c.ms();
    let mut c = Clock::new();
    let (mut next, mut held): (u64, Option<(u64, u64)>) = (0, None);
    let mut shown = None;
    loop {
        let now = c.ms();
        if now >= BOUND_MS || exit_of(pid).is_some() {
            break;
        }
        if now >= next {
            next = now + SAMPLE_MS;
            match sample() {
                Some((h, lit)) if Some(h) != at_exec && lit * 4 >= GRID_W * GRID_H => match held {
                    Some((h0, since)) if h0 == h => {
                        if now - since >= HOLD_MS {
                            shown = Some(since);
                            break;
                        }
                    }
                    _ => held = Some((h, now)),
                },
                _ => held = None,
            }
        }
        sched::yield_now();
    }
    let ms = c.ms();
    irq_off();
    match shown {
        Some(t) => ktap::note(format_args!("port: a picture held on the screen from {t} ms after the exec; stopped at {ms} ms")),
        None => ktap::note(format_args!("port: no picture held on the screen within {ms} ms of the exec (the bound is {BOUND_MS} ms)")),
    }
    let (k1, e1, f1) = (mm::last_kill(), syscall::enosys_count(), frame::stats().used);
    ktap::note(format_args!(
        "{SURVIVES_ID}: {PROGRAM} pid {} ran {ms} ms after its exec ({started} ms to start); enosys_count() {e0} -> {e1}; frames in use {f0} -> {f1}",
        pid.0
    ));
    let mut why = String::new();
    if let Some(v) = exit_of(pid) {
        why += &format!("{PROGRAM} {} {ms} ms after its exec; it runs until it is stopped", status_text(v));
    }
    if k1 != k0 {
        if let Some(k) = k1 {
            if !why.is_empty() {
                why += "; ";
            }
            why += &format!("a fault killed a process: reason {} at rip {:#x}, address {:#x}", k.reason.as_str(), k.rip, k.addr);
        }
    }
    if e1 != e0 {
        if !why.is_empty() {
            why += "; ";
        }
        why += &format!("{} of its system calls returned -ENOSYS (enosys_count() {e0} -> {e1}): every call it makes is a row of the syscall table at or before lab 26", e1 - e0);
    }
    (!why.is_empty()).then_some(why)
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
    let why = match mount_root().and_then(|()| mount_dev()) {
        Err(why) => {
            irq_off();
            Some(format!("not run: the file systems did not mount: {why}"))
        }
        Ok(()) => port(),
    };
    // Interrupts are off from here on: the program's last frame stays on the
    // screen. A line it was part-way through ends first.
    kernel::kprintln!();
    match why {
        None => kernel::kprintln!("ok 1 - {SURVIVES_ID}"),
        Some(why) => {
            ktap::note(format_args!("{SURVIVES_ID}: {why}"));
            kernel::kprintln!("not ok 1 - {SURVIVES_ID}");
        }
    }
    ktap::end();
    halt()
}
