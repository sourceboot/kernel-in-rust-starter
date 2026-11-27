//! The run vehicle (sprint 12 #188, `sboot run`; docs/sboot-run-2026-11-17.md).
//! Not a test: no KTAP, no rows, no script. It boots the learner's kernel the
//! way ktest-33 does (the same prelude, ext2 at / from /dev/hda2) and starts
//! the learner's `/bin/init <nonce>` as pid 1 with fds 0-2 on the console
//! (`proc::start`'s own open), then idles while the learner types. The keys
//! reach the PS/2 keyboard from the host (xtask `run-lab`, QMP send-key); the
//! console writes to the serial line, which xtask copies to the terminal.
//!
//! The nonce is the run's practice nonce from fw_cfg, as a test boot gets one:
//! with it the learner's init prints its own `init: key …` line (lab 28's
//! task) instead of `init: no key (argv[1] is not a nonce)`. A run with no
//! nonce on fw_cfg (`cargo xtask` by hand, without one) starts init with no
//! argument.
//!
//! Silent on the way to init, so the terminal shows the learner's programs
//! and nothing of ours; a line only when the boot cannot get there. Built for
//! labs 28-33 only (`cargo xtask run-lab`): an earlier lab's kernel has no
//! exec or file system for it to call.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../shim.rs"]
mod shim;
#[path = "../pmtime.rs"]
mod pmtime;
use pmtime::pm_step;

use alloc::sync::Arc;
use core::arch::asm;
use core::sync::atomic::Ordering;

use kernel::addr::VirtAddr;
use kernel::blk::ata::{self, Ata, BlockDevice};
use kernel::exec;
use kernel::fs::ext2::Ext2;
use kernel::fs::mount;
use kernel::fs::vfs::InodeRef;
use kernel::ktap;
use kernel::{bootinfo, frame, gdt, heap, idt, irq, kprintln, proc, sched, serial, syscall, time};

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 64;
const PM_HZ: u64 = 3_579_545;
const INIT: &str = "/bin/init";

fn pm() -> u32 {
    let v: u32;
    // SAFETY: a 32-bit read of the ACPI PM timer port; no memory is touched.
    unsafe { asm!("in eax, dx", out("eax") v, in("dx") 0x608u16, options(nomem, nostack, preserves_flags)) };
    v & 0xff_ffff
}

/// Milliseconds since `new`, from the 24-bit PM timer (ktest-33's).
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
        self.acc += pm_step(&mut self.last, now) as u64;
        self.acc * 1000 / PM_HZ
    }
}

fn ticks() -> u64 {
    time::TICKS.load(Ordering::Relaxed)
}

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
/// across a read (ktest-33's `quiet`).
fn quiet<R>(f: impl FnOnce() -> R) -> R {
    let on = irq_off();
    let r = f();
    irq_restore(on);
    r
}

fn sse_on() {
    let mxcsr: u32 = 0x1F80;
    // SAFETY: as ktest-33's sse_on: CR0.EM off, CR0.MP on, CR4.OSFXSR and
    // OSXMMEXCPT on, then a clean x87 and MXCSR state.
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

/// ktest-33's prelude, unchanged.
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

fn mount_root() -> Result<(), &'static str> {
    let drive: Arc<dyn BlockDevice> = Arc::new(quiet(Ata::probe).map_err(|_| "Ata::probe failed")?);
    let fs = quiet(|| ata::partition2(drive)).and_then(Ext2::mount).map_err(|_| "partition2 then Ext2::mount failed")?;
    let fs = Arc::new(fs);
    let root: InodeRef = quiet(|| fs.root()).map_err(|_| "Ext2::root failed")?;
    mount::mount("/dev/hda2", "/", "ext2", root).map_err(|_| "mount::mount of / failed")
}

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    serial::init();
    prelude();
    match mount_root() {
        Err(why) => kprintln!("sboot run: your kernel booted, but its file system did not mount ({why}), so /bin/init never started"),
        Ok(()) => {
            let nonce = ktap::nonce();
            let key = nonce.as_ref().and_then(|n| core::str::from_utf8(n).ok());
            let started = match key {
                Some(k) => quiet(|| exec::spawn(INIT, &["init", k], &[])),
                None => quiet(|| exec::spawn(INIT, &["init"], &[])),
            };
            if let Err(e) = started {
                kprintln!("sboot run: your kernel booted, but {INIT} did not start (errno {e:?})");
            }
        }
    }
    loop {
        sched::yield_now();
        // SAFETY: wait for the next interrupt with IF set.
        unsafe { asm!("sti", "hlt", options(nomem, nostack)) };
    }
}
