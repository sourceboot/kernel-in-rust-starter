#![no_std]

pub mod bootinfo;
pub mod exit;
pub mod fb;
pub mod handshake;
pub mod ktap;
pub mod gfx;
pub mod serial;

use bootinfo::BootInfo;

/// The kernel's C-ABI entry — the other half of the BootInfo seam. Any
/// bootloader (the provided shim today, the learner's own in Arc B) enters
/// here with a filled BootInfo.
///
/// # Safety
/// `bi` must point to a live, correctly-filled BootInfo.
#[no_mangle]
pub unsafe extern "C" fn kernel_main(bi: *const BootInfo) -> ! {
    let bi = &*bi;
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

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    hcf()
}

pub fn hcf() -> ! {
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}
