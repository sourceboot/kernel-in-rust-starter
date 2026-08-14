#![no_std]

pub mod bootinfo;
pub mod exit;
pub mod fb;
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
    if let Some(mut fb) = fb::Fb::new(&bi.fb) {
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
