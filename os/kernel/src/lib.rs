//! The kernel crate's root. COURSE-OWNED: the module list below is fixed for
//! the whole course (CONTRACT §B.1) — every module a lab will ask you to
//! write is declared here from day one, and each ships as a stub until its
//! lab. You edit the learner modules; this file, `ktap.rs`, `fwcfg.rs`,
//! `bootinfo.rs`, `handshake.rs`, `exit.rs`, `addr.rs`, `sync.rs` and all of
//! `boot/` are the course's and are compared before every grade.

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

// ── Course-owned ─────────────────────────────────────────────────────────
pub mod addr;
pub mod bootinfo;
pub mod exit;
pub mod fwcfg;
pub mod handshake;
pub mod ktap;
pub mod sync;

// ── Provided (lab 01's raw material; yours to read and edit) ─────────────
pub mod fb;

// ── Learner modules, in lab order ────────────────────────────────────────
pub mod gfx; // 01
pub mod serial; // 02
pub mod panic; // 03
pub mod gdt; // 04
pub mod idt; // 05
pub mod mm; // 05 (the fault seam), 12, 22, 23
pub mod irq; // 06
pub mod kbd; // 06
pub mod time; // 06
pub mod frame; // 07
pub mod paging; // 08
pub mod console; // 09
pub mod syscall; // 09
pub mod user; // 09
pub mod heap; // 10
pub mod sched; // 11
pub mod task; // 11
pub mod proc; // 12, 14
pub mod elf; // 13
pub mod exec; // 13
pub mod pipe; // 15
pub mod blk {
    //! The block layer (lab 16): the device and the buffer cache.
    pub mod ata;
    pub mod cache;
}
pub mod fs {
    //! The file-system layer: the VFS trait and the boot archive (13), ext2
    //! (17, 18), the log (18, 19), the mount walk, /proc and /dev (20).
    pub mod archive;
    pub mod devfs;
    pub mod ext2;
    pub mod log;
    pub mod mount;
    pub mod procfs;
    pub mod vfs;
}
pub mod cow; // 22

use core::sync::atomic::{AtomicU32, Ordering};

use bootinfo::BootInfo;

/// The kernel's C-ABI entry — the other half of the BootInfo seam. The
/// provided shim enters here with a filled BootInfo.
///
/// # Safety
/// `bi` must point to a live, correctly-filled BootInfo.
#[no_mangle]
pub unsafe extern "C" fn kernel_main(bi: *const BootInfo) -> ! {
    // SAFETY: the caller guarantees `bi` is live and filled.
    let bi = unsafe { &*bi };
    if bi.magic != bootinfo::BOOT_MAGIC {
        hcf();
    }
    // Lab 00's boot handshake (handshake.rs, provided): two fixed lines before
    // your code runs. The first proves the provided kernel came up at all; the
    // second only prints when the framebuffer handoff checks out, so its absence
    // means the display path — lab 01's raw material — is what's broken.
    handshake::line("[scaffold] boot: kernel_main reached");
    if let Some(mut fb) = fb::Fb::new(&bi.fb) {
        handshake::line("[scaffold] boot: framebuffer handed over");
        gfx::main(&mut fb);
    }
    hcf()
}

/// How many times the panic path has been entered: 0 → your `panic::report`
/// (lab 03); 1 → a panic inside that path, reported here without the
/// `panic:` shape; more → halt silently.
static PANICS: AtomicU32 = AtomicU32::new(0);

#[panic_handler]
fn on_panic(info: &core::panic::PanicInfo) -> ! {
    let (file, line) = info.location().map_or(("?", 0), |l| (l.file(), l.line()));
    match PANICS.fetch_add(1, Ordering::SeqCst) {
        0 => panic::report(file, line, format_args!("{}", info.message())),
        1 => {
            print(format_args!("# a panic inside the panic path, at {file}:{line} — {}\n", info.message()));
            hcf()
        }
        _ => hcf(),
    }
}

/// `kprint!`'s back end: format straight onto the wire through your
/// `serial::write_str`, no buffer, no lock — so an interrupt handler can
/// print while the code it interrupted is printing (the lines may interleave;
/// nothing deadlocks).
pub fn print(args: core::fmt::Arguments) {
    struct Wire;
    impl core::fmt::Write for Wire {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            serial::write_str(s);
            Ok(())
        }
    }
    let _ = core::fmt::Write::write_fmt(&mut Wire, args);
}

/// `print!` for the kernel, over `serial::write_str`.
#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => { $crate::print(format_args!($($arg)*)) };
}

/// `println!` for the kernel (`\n` becomes CRLF on the wire).
#[macro_export]
macro_rules! kprintln {
    () => { $crate::print(format_args!("\n")) };
    ($($arg:tt)*) => { $crate::print(format_args!("{}\n", format_args!($($arg)*))) };
}

/// Halt and catch fire: interrupts may still wake the CPU, so loop on `hlt`.
pub fn hcf() -> ! {
    loop {
        // SAFETY: `hlt` waits for the next interrupt; it touches no memory.
        unsafe { core::arch::asm!("hlt") };
    }
}
