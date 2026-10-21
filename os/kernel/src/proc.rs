//! Lab 12 — processes: a program in an address space of its own, with a
//! record of what it may touch. This file is yours.
//!
//! A process is a task (lab 11) plus what makes it a program: an address
//! space whose user half is its alone, the list of the ranges in that half
//! it may use (the VMA list), its argv, and how it ended. `spawn_user`
//! builds one from a flat image (layout `flat`, user/README.md: one PT_LOAD
//! at file offset 0 placed at `BASE`) — the text read-only, a `STACK_PAGES`
//! stack ending at `STACK_TOP` with an unmapped guard page below it,
//! the initial stack the System V psABI describes — and starts a task that
//! enters ring 3 in it. The address space lives in the task (`Task::aspace`),
//! because that is what the scheduler loads into CR3; the record keeps its root.
//!
//! A process ends two ways: it asks (`exit`, syscall 60) or it faults
//! (`kill`, from `mm::fault`'s user-mode arm or from the #UD and #GP
//! handlers `init` installs) — the kill line first, then the same end. Either way its
//! user half goes back to the frame pool, its status is recorded in Linux's
//! `wait4` encoding, and its task ends; the record stays, for `each`.
//! Touch the table only with interrupts off: a syscall or a fault that found
//! it held would spin on it forever.
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests use them.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::addr::{PhysAddr, VirtAddr};
use crate::mm::Kill;
use crate::syscall::{Errno, SyscallFrame};
use crate::task::Tid;

/// Where a flat image's one PT_LOAD is placed.
pub const BASE: u64 = 0x100_0000;
/// The top of the user stack (its last page ends here).
pub const STACK_TOP: u64 = 0x7fff_ffff_f000;
/// The stack's size in pages; the page below it stays unmapped (the guard).
pub const STACK_PAGES: u64 = 16;

/// A VMA's protection: Linux's `PROT_*` bits.
pub mod prot {
    pub const READ: u8 = 1;
    pub const WRITE: u8 = 2;
    pub const EXEC: u8 = 4;
}

/// `E2BIG`: argv does not fit the top stack page. `syscall::errno` has no
/// name for it, so `spawn_user` uses this one.
const E2BIG: Errno = 7;

/// A process id. 1 is the first process spawned; ids are never reused.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Pid(pub u64);

/// A path, as a string. Every process's cwd is `/` until lab 23, which
/// keeps it absolute with no `.` or `..`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Path(pub String);

/// What a VMA holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VmaKind {
    Text,
    Data,
    Stack,
}

/// One range of the user half a process may use: `[start, end)`, both
/// page-aligned, with Linux's `PROT_*` bits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Vma {
    pub start: VirtAddr,
    pub end: VirtAddr,
    pub prot: u8,
    pub kind: VmaKind,
}

/// A process's VMAs, sorted by start, never overlapping (interface 19).
#[derive(Clone, Default, Debug)]
pub struct VmaList(Vec<Vma>);

impl VmaList {
    pub const fn new() -> VmaList {
        VmaList(Vec::new())
    }

    /// The VMAs, lowest first.
    pub fn iter(&self) -> core::slice::Iter<'_, Vma> {
        self.0.iter()
    }

    /// Add `v`, keeping the list sorted. `-EINVAL` for an empty or unaligned
    /// range, `-EEXIST` when it overlaps a VMA already there.
    pub fn insert(&mut self, v: Vma) -> Result<(), Errno> {
        let _ = v;
        todo!("lab 12 step 3 — VmaList::insert: sorted by start, refusing an overlap")
    }

    /// The VMA holding `va`, if one does.
    pub fn find(&self, va: VirtAddr) -> Option<&Vma> {
        let _ = va;
        todo!("lab 12 step 8 — VmaList::find: the VMA whose [start, end) holds va")
    }

    /// Whether every byte of `[start, start + len)` lies in VMAs that allow
    /// every bit of `need`. An empty range is covered.
    pub fn covers(&self, start: VirtAddr, len: usize, need: u8) -> bool {
        let _ = (start, len, need);
        todo!("lab 12 step 7 — VmaList::covers: every byte of the range in a VMA allowing `need`")
    }
}

/// A process. Its address space is its task's (`Task::aspace`); `root` is
/// that space's PML4, recorded when the process was built. Add private
/// fields as you need them (where its task first enters ring 3, say).
pub struct Process {
    pub pid: Pid,
    /// The process that spawned it; `Pid(0)` when the kernel did.
    pub ppid: Pid,
    pub task: Tid,
    pub root: PhysAddr,
    pub vmas: VmaList,
    pub cwd: Path,
    pub argv: Vec<String>,
    /// How it ended, as `wait4` reports it: `code << 8` for an exit, the
    /// signal number (11 SIGSEGV, 4 SIGILL) for a kill. `None` while it runs.
    pub exit: Option<i32>,
    pub children: Vec<Pid>,
}

/// Route #UD (vector 6) and #GP (13) here through `idt::set_handler`: from
/// ring 3 (the frame's CS RPL is 3) with a process running, each ends that
/// process (kill line reason `ud`/`gp`, `addr` 0, `rip` from the frame); any
/// other #UD or #GP is a kernel bug and halts. Call once, after `idt::init`.
pub fn init() {
    todo!("lab 12 step 1 — init: #UD and #GP from ring 3 kill the running process")
}

/// The running process: the one whose task is on the CPU and has not ended.
/// `None` in a kernel task (the boot flow, a vehicle's helpers).
pub fn current() -> Option<Pid> {
    todo!("lab 12 step 4 — current: the process whose task is on the CPU, if any")
}

/// Run `f` on the running process's record, with interrupts off; `None` in
/// a kernel task. `f` may end the process (`exit` never returns), so the
/// table's lock must not be held while it runs.
pub fn with_current<R>(f: impl FnOnce(&mut Process) -> R) -> Option<R> {
    let _ = f;
    todo!("lab 12 step 9 — with_current: the running process's record, for exit")
}

/// Call `f` on every process, in spawn order (interface 17). Interrupts are
/// off while it runs.
pub fn each(f: impl FnMut(&Process)) {
    let _ = f;
    todo!("lab 12 step 5 — each: every record, in spawn order")
}

/// Lab 23: `path` made absolute: itself when it starts with `/` (or is
/// empty), else the running process's cwd joined to it (`/` when no process
/// is running). Every syscall that takes a path walks this from `/`.
pub fn absolute(path: &str) -> String {
    let _ = path;
    todo!("lab 23 step 4 — absolute: the cwd joined to a relative path")
}

/// Lab 23, syscall 80: the cwd becomes the directory `path` names, kept
/// absolute with `.` and `..` folded out. `-ENOENT`, `-ENOTDIR` as Linux.
pub fn chdir(path: &str) -> Result<(), Errno> {
    let _ = path;
    todo!("lab 23 step 5 — chdir: walk it, a directory, the folded absolute path as the cwd")
}

/// Lab 23, syscall 81: the cwd becomes the directory open on `fd`, by the
/// path it was opened by. `-EBADF`, `-ENOTDIR` as Linux.
pub fn fchdir(fd: u64) -> Result<(), Errno> {
    let _ = fd;
    todo!("lab 23 step 5 — fchdir: the open file's path, when it is a directory")
}

/// Lab 23, syscall 79's answer: the running process's cwd (`/` when no
/// process is running).
pub fn getcwd() -> String {
    todo!("lab 23 step 5 — getcwd: the running process's cwd")
}

/// For `user::access_ok` (S5): whether the running process's VMAs cover
/// `[va, va + len)` with every bit of `need` (`prot::*`); `None` in a kernel
/// task, which has no VMAs to ask.
pub fn vmas_allow(va: VirtAddr, len: usize, need: u8) -> Option<bool> {
    let _ = (va, len, need);
    todo!("lab 12 step 6 — vmas_allow: ask the running process's VMA list")
}

/// Start `image` (layout `flat`) as a new process with `argv`; returns its
/// pid. It is ready to run, not running: it gets the CPU in its turn.
/// `-ENOEXEC` for an image that is not flat, `-ENOMEM` when the frame pool
/// runs out (everything taken so far goes back), `-E2BIG` (7) when argv
/// does not fit the top stack page.
pub fn spawn_user(image: &[u8], argv: &[&str]) -> Result<Pid, Errno> {
    let _ = (image, argv);
    todo!("lab 12 step 2 — spawn_user: an address space, the image and the stack mapped, the initial stack, a task that enters ring 3")
}

/// `wait4`'s options: return 0 at once when no child has ended yet.
pub const WNOHANG: u32 = 1;

/// Make a copy of the calling process (`fork`, syscall 57) and return the
/// copy's pid; the copy returns 0 from the same syscall, at the same
/// instruction with the same registers (`f`, as the syscall entry saved
/// them). Every page of every VMA is copied into a fresh frame; the fd
/// table is copied (the open files are shared); its parent is the caller.
pub fn fork(parent: &mut Process, f: &SyscallFrame) -> Result<Pid, Errno> {
    let _ = (parent, f);
    todo!("lab 14 step 4 — fork: copy the pages, the fds and the FPU state; a kernel stack whose first return is the caller's syscall return, rax = 0")
}

/// Wait for a child to end (`wait4`, syscall 61) and collect it: its record
/// goes, its status is written to `status` (unless null), its pid returned.
/// `pid` −1 takes any child. `-ECHILD` at once when there is no such child;
/// with `WNOHANG`, 0 at once when none has ended; otherwise sleep until a
/// child's `exit` wakes the caller, and look again.
pub fn wait4(p: &mut Process, pid: i64, status: VirtAddr, opts: u32) -> Result<Pid, Errno> {
    let _ = (p, pid, status, opts);
    todo!("lab 14 step 5 — wait4: collect an ended child, -ECHILD when there is none, sleep on your own record otherwise")
}

/// End the running process with `code` (`exit`, syscall 60): its status is
/// `(code & 0xff) << 8`, as `wait4` reports an exit. Lab 14 adds the zombie
/// and the wakeup of the parent behind the same signature.
pub fn exit(p: &mut Process, code: i32) -> ! {
    let _ = (p, code);
    todo!("lab 12 step 10 — exit: the user half back to the frame pool, the status recorded, the task ended")
}

/// The kill line, `kill: pid=<n> reason=<reason> rip=0x<hex> addr=0x<hex>`.
pub struct KillLine {
    pub pid: Pid,
    pub kill: Kill,
}

impl fmt::Display for KillLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = f;
        todo!("lab 12 step 12 — KillLine: the kill line above, the addresses in lowercase hex with 0x")
    }
}

/// End the running process `pid` for the fault `k` describes: the kill line on
/// the console, then the same end as `exit`, with the signal `wait4` reports
/// (4 = SIGILL for `ud`, 11 = SIGSEGV for the rest). Called from the fault
/// path, on the process's own task.
pub fn kill(pid: Pid, k: Kill) -> ! {
    let _ = (pid, k);
    todo!("lab 12 step 11 — kill: print the line, then end the process as exit does")
}
