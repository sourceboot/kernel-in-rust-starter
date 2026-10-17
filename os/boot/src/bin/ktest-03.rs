//! Lab 03's vehicle: header, nonce, plan, then a deliberate `bug_on!` as the
//! run's last act. Lab 03's rows read the panic line and the register dump
//! that follow (CONTRACT §A.6); there is no `# sboot end` in this run — the
//! dump's last line (`  cr3=…`) is where it settles. Plan `1..3` counts those
//! three rows; no `ok` line is printed here.

#![no_std]
#![no_main]

#[path = "../shim.rs"]
mod shim;

use core::arch::asm;

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    kernel::serial::init();
    kernel::ktap::begin(3);
    let (cr0, cr4): (u64, u64);
    // SAFETY: reading CR0 and CR4 is side-effect free at CPL 0.
    unsafe {
        asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack, preserves_flags));
        asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
    }
    kernel::ktap::note(format_args!(
        "cr0=0x{cr0:016x} wp={} cr4=0x{cr4:016x} smep={} smap={}",
        cr0 >> 16 & 1,
        cr4 >> 20 & 1,
        cr4 >> 21 & 1
    ));
    // CR0.PE is set in long mode, so this always fires; the condition is a
    // run-time value only so the call is the real macro path.
    kernel::bug_on!(cr0 & 1 == 1, "the cockpit's deliberate bug_on");
    kernel::hcf()
}
