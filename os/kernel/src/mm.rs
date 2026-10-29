//! Lab 05 — the page-fault seam. This file is yours.
//!
//! `idt::dispatch` calls `fault` for every `#PF` with CR2 and the error code
//! the CPU pushed, before any handler registered for vector 14. In lab 05
//! nothing can be resolved yet, so `fault` classifies the fault and returns
//! `Kill`, and remembers the last `Kill` it returned for `last_kill`;
//! `dispatch` reports it and halts.
//! Later labs give `fault` arms behind the same signature.
//!
//! Fill in the body; keep the signature and the types — later labs and the
//! course's tests use them.

use crate::addr::VirtAddr;
use crate::console::FileRef;
use crate::idt::TrapFrame;
use crate::proc::Process;
use crate::syscall::Errno;

/// Why a fault kills — the `reason=` of the kill line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KillReason {
    PfUnmapped,
    PfWriteRo,
    PfKernelAddr,
    Gp,
    Ud,
}

impl KillReason {
    pub fn as_str(self) -> &'static str {
        match self {
            KillReason::PfUnmapped => "pf-unmapped",
            KillReason::PfWriteRo => "pf-write-ro",
            KillReason::PfKernelAddr => "pf-kernel-addr",
            KillReason::Gp => "gp",
            KillReason::Ud => "ud",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Kill {
    pub reason: KillReason,
    pub rip: u64,
    pub addr: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaultOutcome {
    Resolved,
    Kill(Kill),
}

/// The `#PF` error-code bits (SDM Vol. 3A, Page-Fault Exceptions).
pub mod pf {
    pub const PRESENT: u64 = 1 << 0;
    pub const WRITE: u64 = 1 << 1;
    pub const USER: u64 = 1 << 2;
}

/// Decide what a page fault at `addr` means.
pub fn fault(addr: VirtAddr, error: u64, tf: &TrapFrame) -> FaultOutcome {
    let _ = (addr, error, tf);
    todo!("lab 05 task 5 — fault: classify by the error code and the address, remember the Kill for last_kill, return it")
}

/// The last `Kill` that `fault` returned, if it has returned one.
pub fn last_kill() -> Option<Kill> {
    todo!("lab 05 task 5 — last_kill: the Kill that fault returned last")
}

// ── Lab 25: the mmap family ──────────────────────────────────────────────

/// Lab 25, syscall 9: `len` bytes of fresh memory with `prot`, placed below
/// the top of the mmap region (CONTRACT §A.5). Private and anonymous only:
/// a `file` is `-ENODEV` until lab 26; `hint` is never a reason to refuse.
/// Nothing is mapped until a page is touched.
pub fn mmap(p: &mut Process, hint: VirtAddr, len: usize, prot: u8, flags: u32, file: Option<FileRef>, off: u64) -> Result<VirtAddr, Errno> {
    let _ = (p, hint, len, prot, flags, file, off);
    todo!("lab 25 step 6 — mmap: an anonymous VMA in a free gap below the mmap top, nothing mapped yet")
}

/// Lab 25, syscall 11: `[addr, addr + len)` no longer mapped — whatever VMAs
/// it overlaps shrink, split or go, and every frame mapped there drops a
/// reference. Nothing mapped there is not an error.
pub fn munmap(p: &mut Process, addr: VirtAddr, len: usize) -> Result<(), Errno> {
    let _ = (p, addr, len);
    todo!("lab 25 step 7 — munmap: carve the range out of the VMAs and free every frame mapped in it")
}

/// Lab 25, syscall 25: the anonymous mapping at `[old, old + old_len)`
/// resized to `new_len`: shrunk in place, grown in place when the pages above
/// it are free, otherwise moved whole (with `MREMAP_MAYMOVE`, flag 1;
/// `-ENOMEM` without it). A move takes the frames with it.
pub fn mremap(p: &mut Process, old: VirtAddr, old_len: usize, new_len: usize, flags: u32) -> Result<VirtAddr, Errno> {
    let _ = (p, old, old_len, new_len, flags);
    todo!("lab 25 step 9 — mremap: shrink, grow in place, or move the mapping's pages to a new gap")
}
