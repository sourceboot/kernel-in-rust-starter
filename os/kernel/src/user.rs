//! Lab 09 — ring 3: entering it, coming back from it, and touching its
//! memory safely. This file is yours.
//!
//! The way down is `iretq`, through a frame that names the user selectors
//! (RPL 3). The way up is a trap (the CPU switches to the TSS's `rsp0`) or a
//! `syscall` (your entry switches by itself). Until lab 14, a program that
//! ends returns to whoever started it: `run` saves the caller's context and
//! enters the program, and returns when something calls `leave`.
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests call them.

use crate::addr::VirtAddr;
use crate::paging::AddressSpace;
use crate::syscall::Errno;

/// Enter ring 3 at `entry` with stack `rsp`, in `aspace`: every general
/// register zero, interrupts on, SSE ready. Never returns.
pub fn enter(entry: VirtAddr, rsp: VirtAddr, aspace: &AddressSpace) -> ! {
    let _ = (entry, rsp, aspace);
    todo!("lab 09 step 4 — enter: activate the space, ready SSE (CR0/CR4, fninit, MXCSR), iretq with UCODE/UDATA and IF set")
}

/// Run a program to its end and return the status `leave` was called with.
pub fn run(entry: VirtAddr, rsp: VirtAddr, aspace: &AddressSpace) -> i64 {
    let _ = (entry, rsp, aspace);
    todo!("lab 09 step 2 — run: fds 0-2 on the console, save the callee-saved registers and the stack, enter; restore CR3 and the data segments after")
}

/// End the running program: `run` returns `status`.
pub fn leave(status: i64) -> ! {
    let _ = status;
    todo!("lab 09 step 2 — leave: put back what run saved and return there with status")
}

/// `Ok(())` when `[va, va + len)` may be read (or, with `write`, written)
/// as user memory, else `-EFAULT`: the same test the copies make, for a
/// caller that must answer before it moves anything (`sys_write` checks the
/// whole buffer first). A kernel-half address is refused at any length.
pub fn access_ok(va: VirtAddr, len: usize, write: bool) -> Result<(), Errno> {
    let _ = (va, len, write);
    todo!("lab 09 step 6 — access_ok: the user half, then USER (and writable, if asked) at every level of every page")
}

/// Copy `dst.len()` bytes from user address `src`, or `-EFAULT`.
pub fn copy_from_user(dst: &mut [u8], src: VirtAddr) -> Result<(), Errno> {
    let _ = (dst, src);
    todo!("lab 09 step 6 — copy_from_user: every page in the user half and USER at every level, then copy")
}

/// Copy `src` to user address `dst`, or `-EFAULT`.
pub fn copy_to_user(dst: VirtAddr, src: &[u8]) -> Result<(), Errno> {
    let _ = (dst, src);
    todo!("lab 09 step 6 — copy_to_user: as copy_from_user, and writable at every level")
}
