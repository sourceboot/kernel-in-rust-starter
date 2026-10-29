//! Lab 09 — ring 3: entering it, coming back from it, and touching its
//! memory safely. This file is yours.
//!
//! The way down is `iretq`, through a frame that names the user selectors
//! (RPL 3). The way up is a trap (the CPU switches to the TSS's `rsp0`) or a
//! `syscall` (your entry switches by itself). Until lab 12 gives the kernel
//! processes (and, after it, for a program run without one), a program that
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
    todo!("lab 09 task 4 — enter: activate the space, ready SSE (CR0/CR4, fninit, MXCSR), iretq with UCODE/UDATA and IF set")
}

/// Run a program to its end and return the status `leave` was called with.
pub fn run(entry: VirtAddr, rsp: VirtAddr, aspace: &AddressSpace) -> i64 {
    let _ = (entry, rsp, aspace);
    todo!("lab 09 task 2 — run: fds 0-2 on the console, save the callee-saved registers and the stack, enter; restore CR3 and the data segments after")
}

/// End the running program: `run` returns `status`.
pub fn leave(status: i64) -> ! {
    let _ = status;
    todo!("lab 09 task 2 — leave: put back what run saved and return there with status")
}

/// `Ok(())` when `[va, va + len)` may be read (or, with `write`, written)
/// as user memory, else `-EFAULT`: the same test the copies make, for a
/// caller that must answer before it moves anything (`sys_write` checks the
/// whole buffer first). A kernel-half address is refused at any length.
pub fn access_ok(va: VirtAddr, len: usize, write: bool) -> Result<(), Errno> {
    let _ = (va, len, write);
    todo!("lab 09 task 6 — access_ok: the user half, then USER (and writable, if asked) at every level of every page")
}

/// Copy `dst.len()` bytes from user address `src`, or `-EFAULT`.
pub fn copy_from_user(dst: &mut [u8], src: VirtAddr) -> Result<(), Errno> {
    let _ = (dst, src);
    todo!("lab 09 task 6 — copy_from_user: every page in the user half and USER at every level, then copy")
}

/// Copy `src` to user address `dst`, or `-EFAULT`.
pub fn copy_to_user(dst: VirtAddr, src: &[u8]) -> Result<(), Errno> {
    let _ = (dst, src);
    todo!("lab 09 task 6 — copy_to_user: as copy_from_user, and writable at every level")
}

/// Lab 31 step 2: the way back from every syscall acts on what is pending
/// (`proc::take_signal`), and a wait a signal ended (`-ERESTARTSYS`) starts
/// again. Lab 32 step 2 grows it: after a handler that wait restarts or
/// answers `-EINTR`, and a signal with a handler gets its frame below the
/// program's stack, the frame `f` returning into the handler. `nr` is the
/// syscall's number.
pub fn signal_return(f: &mut crate::syscall::SyscallFrame, nr: u64) {
    let _ = (f, nr);
    todo!("lab 31 step 2 — signal_return: what is pending acted on, an interrupted wait started again (lab 32: the frame pushed and the handler entered)")
}

/// Lab 32 step 1: the same, on the way back from an interrupt that came
/// from ring 3.
pub fn signal_trap(tf: &mut crate::idt::TrapFrame) {
    let _ = tf;
    todo!("lab 32 step 1 — signal_trap: the frame pushed and the handler entered, from an interrupt")
}

/// Lab 32 step 3: `rt_sigreturn` (15). The registers, flags, FPU state and
/// mask the frame saved come back, and the program carries on where the
/// signal found it. Never returns.
pub fn sigreturn(f: &crate::syscall::SyscallFrame) -> ! {
    let _ = f;
    todo!("lab 32 step 3 — sigreturn: everything the frame saved restored, back through iretq")
}
