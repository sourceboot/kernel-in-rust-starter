//! Lab 13 — exec: a file becomes the running program. This file is yours.
//!
//! `execve(path, argv, envp)` finds the file through the VFS, loads it into
//! a fresh address space with the ELF loader, writes the initial stack a
//! Linux program expects, and only then — when nothing can fail any more —
//! swaps the new program in for the caller's: same process, same pid, same
//! open files, new memory. A failed `execve` leaves the caller exactly as it
//! was, which is why the new image is built beside the old one, never over
//! it.
//!
//! The initial stack (the x86-64 psABI), from `STACK_TOP`
//! down: 16 random bytes (`AT_RANDOM` points at them), the path
//! (`AT_EXECFN`), the envp strings, the argv strings, padding, then upward
//! from `rsp`: `argc`, the argv pointers and a 0, the envp pointers and a 0,
//! the auxv pairs ending with `AT_NULL`. `rsp % 16 == 0`. The program enters
//! at `e_entry` with every general register zero (`rdx = 0`: no exit handler
//! to register), interrupts on, in ring 3 (`user::enter`).
//!
//! `spawn` is the same work for a process that does not exist yet — the
//! kernel starting a program, as the course's tests do before lab 14 gives
//! the kernel `fork`: the image is built the same way, and your `proc`
//! makes a process around it.
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests use them.

use crate::addr::VirtAddr;
use crate::elf::ReadAt;
use crate::paging::AddressSpace;
use crate::proc::{Pid, Process, VmaList};
use crate::syscall::Errno;

/// `E2BIG`: the strings and the vector do not fit the initial stack.
pub const E2BIG: Errno = 7;
/// `EACCES`: the path names something that is not a regular file.
pub const EACCES: Errno = 13;
/// How much of the stack the strings and the vector may take.
pub const ARG_MAX: u64 = 32 * 1024;

/// The auxv keys (Linux's `AT_*`).
pub mod at {
    pub const AT_NULL: u64 = 0;
    pub const AT_PHDR: u64 = 3;
    pub const AT_PHENT: u64 = 4;
    pub const AT_PHNUM: u64 = 5;
    pub const AT_PAGESZ: u64 = 6;
    pub const AT_ENTRY: u64 = 9;
    pub const AT_UID: u64 = 11;
    pub const AT_EUID: u64 = 12;
    pub const AT_GID: u64 = 13;
    pub const AT_EGID: u64 = 14;
    pub const AT_SECURE: u64 = 23;
    pub const AT_RANDOM: u64 = 25;
    pub const AT_EXECFN: u64 = 31;
}

/// A program ready to enter: its own address space with the segments and
/// the stack mapped, its VMAs, where it starts and its initial `rsp`.
pub struct Image {
    pub aspace: AddressSpace,
    pub vmas: VmaList,
    pub entry: VirtAddr,
    pub rsp: VirtAddr,
}

/// The image of the ELF executable `r` with this initial stack, in a fresh
/// address space: the loader's segments and VMAs, `STACK_PAGES` pages under
/// `STACK_TOP` (an unmapped guard page below them), the initial stack (lesson: The initial stack).
/// `execfn` is what `AT_EXECFN` names. On an error everything it took goes
/// back to the frame pool. Call with interrupts off (it takes the frame
/// pool's lock).
pub fn build(r: &dyn ReadAt, execfn: &str, argv: &[&str], envp: &[&str]) -> Result<Image, Errno> {
    let _ = (r, execfn, argv, envp);
    todo!("lab 13 step 8 — build: a fresh address space, elf::load, the stack pages and their VMA, the initial stack (lesson: The initial stack)")
}

/// Replace the running process `p`'s program with the executable at `path`,
/// run with `argv` and `envp`. Returns only on failure, with the errno —
/// `-ENOENT`, `-EACCES`, `-ENOEXEC`, `-ENOMEM`, `-E2BIG` — and `p` exactly
/// as it was. On success the old address space is freed, the process keeps
/// its pid and its open files, and the CPU enters the new program. Called on
/// `p`'s own task, with interrupts off.
pub fn execve(p: &mut Process, path: &str, argv: &[&str], envp: &[&str]) -> Errno {
    let _ = (p, path, argv, envp);
    todo!("lab 13 step 12 — execve: build the new image beside the old one; only when it is whole, swap it in and enter it")
}

/// Start the executable at `path` as a new process with `argv` and `envp`;
/// returns its pid. It is ready to run, not running. The same errors as
/// `execve`, and on an error no process exists and every frame taken went
/// back.
pub fn spawn(path: &str, argv: &[&str], envp: &[&str]) -> Result<Pid, Errno> {
    let _ = (path, argv, envp);
    todo!("lab 13 step 5 — spawn: the file through the VFS, its image, and a process around it")
}

/// Syscall 59: copy the path, argv and envp in from the caller's memory —
/// all of it, before the old image goes — and do what `execve` does.
/// Returns the errno; on success it does not return. The copies are dropped
/// before the new program is entered (a stack that is never returned to
/// frees nothing).
pub fn sys_execve(path: VirtAddr, argv: VirtAddr, envp: VirtAddr) -> Errno {
    let _ = (path, argv, envp);
    todo!("lab 13 step 10 — sys_execve: copy the path and both vectors in, then what execve does")
}
