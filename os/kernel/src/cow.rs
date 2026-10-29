//! Lab 24 — copy-on-write. This file is yours.
//!
//! Lab 14's `fork` copied every page of the caller into fresh frames, so a
//! process that has touched 48 MiB cannot fork twice in 128 MiB. Here the
//! child gets the parent's frames instead: `share` maps a page of one space
//! into another, and when the page is writable it becomes read-only in both,
//! marked `COW` (bit 9, one the CPU ignores), with one more reference on its
//! frame (lab 07's counts). Nothing is copied until somebody writes.
//!
//! A write to a `COW` page faults — `WRITE` is clear — and `mm::fault` asks
//! `write_fault` first. The faulting process gets a page of its own: a copy
//! in a fresh frame, mapped writable, with one reference on the old frame
//! dropped — or, when it holds the only reference left (the other side
//! exited, or already copied), the same frame made writable again. The
//! kernel's own writes into user memory fault the same way, because CR0.WP
//! stays set (CONTRACT §A.5): `wait4`'s status, `read`'s buffer.
//!
//! A page that was never writable (the program's text) is shared read-only
//! and never marked `COW`: a write to it is still a kill (`pf-write-ro`).
//!
//! A page-table entry the kernel changes in the tables the CPU is running on
//! is stale in the TLB until `invlpg` drops it: the parent's own entries
//! change under it while it forks, and the faulting process's entry changes
//! in the fault.
//!
//! Fill in the bodies; keep the signatures — `proc::fork`, `mm::fault` and
//! the course's tests use them. Add private helpers as you need them.

use crate::addr::VirtAddr;
use crate::paging::AddressSpace;
use crate::syscall::Errno;

/// Step 1. Share the 4 KiB page at `va` of `src` into `dst`: the same frame,
/// one more reference on it. A writable (or already copy-on-write) page
/// becomes `COW` and read-only in both spaces; a read-only page is mapped
/// read-only in `dst` and is never `COW`. USER and NX carry over. Nothing
/// mapped at `va` in `src`: nothing to do. `-ENOMEM` when `dst` has no
/// frame for a page table (nothing changed in `src`, no reference taken).
/// When `src` is the space the CPU is running on, the changed entry is
/// flushed (`invlpg`) before `share` returns: each call leaves its own page
/// flushed.
pub fn share(src: &mut AddressSpace, dst: &mut AddressSpace, va: VirtAddr) -> Result<(), Errno> {
    let _ = (src, dst, va);
    todo!("lab 24 task 1 — share: one frame mapped in both spaces, copy-on-write when writable")
}

/// Step 2. The fault arm: `true` when the fault at `addr` (error code
/// `error`) was a write to a present `COW` page at a user address and is
/// resolved — the running tables now map a writable page there, holding what
/// the shared one held. `false` for every other fault, and when no frame is
/// left for the copy (the fault then kills as any write to a read-only page).
/// Runs inside the #PF handler, interrupts off. The kernel never touches
/// user memory while it holds the task table or the process table (`wait4`
/// lets the process table go before it stores the status), so reaching the
/// entry through the running process's record is as safe as through CR3.
pub fn write_fault(addr: VirtAddr, error: u64) -> bool {
    let _ = (addr, error);
    todo!("lab 24 task 2 — write_fault: a write to a COW page gets a page of its own")
}
