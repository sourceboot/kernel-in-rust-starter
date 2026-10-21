//! Lab 09 — the system-call boundary. This file is yours.
//!
//! A user program asks the kernel for something with the `syscall`
//! instruction: the number in `rax`, up to six arguments in `rdi rsi rdx r10
//! r8 r9`, the answer back in `rax` — Linux's x86-64 convention, so Linux's
//! numbers. `syscall` does not switch stacks: the entry's first job is to
//! leave the user's stack for a kernel one. `sysret` is the way back.
//!
//! The dispatch is one `match` over the number. Every later lab adds arms
//! to it and never reorders them; a number with no arm yet is `-ENOSYS`,
//! counted, never a kill.
//!
//! Fill in the bodies; keep the signatures, the types and the constants —
//! later labs and the course's tests use them.

use crate::addr::VirtAddr;

/// Linux's errno numbers. A syscall that fails returns `-errno`, always in
/// `[-4095, -1]`.
pub mod errno {
    pub const EPERM: i64 = 1;
    pub const ENOENT: i64 = 2;
    pub const EIO: i64 = 5;
    pub const ENOEXEC: i64 = 8;
    pub const EBADF: i64 = 9;
    pub const ECHILD: i64 = 10;
    pub const EAGAIN: i64 = 11;
    pub const ENOMEM: i64 = 12;
    pub const EFAULT: i64 = 14;
    pub const EEXIST: i64 = 17;
    pub const EXDEV: i64 = 18;
    pub const ENODEV: i64 = 19;
    pub const ENOTDIR: i64 = 20;
    pub const EISDIR: i64 = 21;
    pub const EINVAL: i64 = 22;
    pub const ENFILE: i64 = 23;
    pub const EMFILE: i64 = 24;
    pub const ENOTTY: i64 = 25;
    pub const ESPIPE: i64 = 29;
    pub const EROFS: i64 = 30;
    pub const EPIPE: i64 = 32;
    pub const ENOSYS: i64 = 38;
    pub const ENOTEMPTY: i64 = 39;
}

/// A refusal: the positive errno number; the syscall returns its negation.
pub type Errno = i64;

/// The numbers lab 09 answers — x86-64 Linux's.
pub mod nr {
    pub const READ: u64 = 0;
    pub const WRITE: u64 = 1;
    pub const CLOSE: u64 = 3;
    pub const IOCTL: u64 = 16;
    pub const GETPID: u64 = 39;
    pub const EXIT: u64 = 60;
}

/// Everything the entry saves, lowest address first: the user's registers
/// as they were at the `syscall` instruction. `rip` is what the CPU left in
/// `rcx`, `rflags` what it left in `r11`, `rsp` the user's stack pointer.
/// `rax` holds the number on the way in and the answer on the way out.
#[repr(C)]
pub struct SyscallFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub r9: u64,
    pub r8: u64,
    pub r10: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rax: u64,
    pub rip: u64,
    pub rflags: u64,
    pub rsp: u64,
}

/// Program STAR, LSTAR and SFMASK, and set EFER.SCE.
pub fn init() {
    todo!("lab 09 step 1 — init: STAR (kernel 0x08, user base 0x18), LSTAR = your naked entry, SFMASK (IF, TF, DF at least), EFER.SCE")
}

/// The kernel stack your entry switches to (its top, 16-aligned) — the
/// same stack the TSS's `rsp0` names.
pub fn set_kernel_stack(top: VirtAddr) {
    let _ = top;
    todo!("lab 09 step 1 — set_kernel_stack: remember the top for the entry to load")
}

/// Bumped by `dispatch`'s `_` arm, once per unknown number.
static ENOSYS_COUNT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// How many syscalls named a number with no arm. (Given: the counter is
/// here; step 5's `_` arm is what moves it.)
pub fn enosys_count() -> u64 {
    ENOSYS_COUNT.load(core::sync::atomic::Ordering::SeqCst)
}

/// One `match` over the number; `-errno` for a refusal.
pub fn dispatch(nr: u64, a: [u64; 6], f: &mut SyscallFrame) -> i64 {
    let _ = (nr, a, f);
    todo!("lab 09 step 5 — dispatch: read, write, close, ioctl, getpid, exit; `_` counts and returns -ENOSYS")
}
