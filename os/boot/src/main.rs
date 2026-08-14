//! The lab binary: boot via the shim, hand the machine to gfx::main.

#![no_std]
#![no_main]

mod shim;

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    kernel::kernel_main(shim::boot_info())
}
