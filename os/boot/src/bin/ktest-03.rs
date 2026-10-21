//! Lab 03's vehicle: header, nonce, plan, then two `bug_on!`s as the run's last
//! act — one whose condition is false, which must not fire, then the deliberate
//! one. Lab 03's rows read the panic line and the register dump that follow
//! (CONTRACT §A.6); there is no `# sboot end` — the dump's last line (`  cr3=…`)
//! is where it settles. Plan `1..3` counts those rows; no `ok` line prints.
//!
//! What makes each row's needle exact (2026-10-18, BUILD-STATUS F1):
//! - `panic.line`: the whole line — `panic: `, this file and the second
//!   `bug_on!`'s line, its message — so `report` prints all four as given.
//! - `panic.dump.rip`: the course's linker script places the code of the
//!   `kernel::panic` module first and starts the rest of the text at
//!   0xffffffff80010000, so a `rip` taken in the panic code starts with
//!   `0xffffffff8000` on every build; a zero, a stale slot, or a return address
//!   in a non-inlined, non-tail helper outside `kernel::panic` does not.
//! - `panic.dump.cr2`: CR2 holds `CR2_MARK` (written here just before the
//!   `bug_on!`s; nothing faults in between), so the dump must read CR2 itself.

#![no_std]
#![no_main]

#[path = "../shim.rs"]
mod shim;

use core::arch::asm;

/// A canonical user-half value no fault in this run produces.
const CR2_MARK: u64 = 0x0000_5b00_7cf2_c0de;

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    kernel::serial::init();
    kernel::ktap::begin(3);
    let (cr0, cr4): (u64, u64);
    // SAFETY: reading CR0 and CR4 is side-effect free at CPL 0; writing CR2
    // changes nothing but the register (no fault is pending: there is no IDT).
    unsafe {
        asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack, preserves_flags));
        asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
        asm!("mov cr2, {}", in(reg) CR2_MARK, options(nomem, nostack, preserves_flags));
    }
    kernel::ktap::note(format_args!(
        "cr0=0x{cr0:016x} wp={} cr4=0x{cr4:016x} smep={} smap={}",
        cr0 >> 16 & 1,
        cr4 >> 20 & 1,
        cr4 >> 21 & 1
    ));
    let at = kernel::panic::dump_state as fn() as usize;
    kernel::ktap::note(format_args!(
        "the panic code is linked below 0xffffffff80010000 (dump_state at 0x{at:016x}); CR2 now holds 0x{CR2_MARK:016x}"
    ));
    // CR0.PE is set in long mode: at run time the first is false, the second true.
    kernel::bug_on!(cr0 & 1 == 0, "bug_on! fired with its condition false");
    kernel::bug_on!(cr0 & 1 == 1, "the cockpit's deliberate bug_on");
    kernel::hcf()
}
