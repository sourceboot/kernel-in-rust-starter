//! Lab 05 — the page-fault seam. This file is yours.
//!
//! `idt::dispatch` calls `fault` for every `#PF` with CR2 and the error code
//! the CPU pushed. In lab 05 nothing can be resolved yet, so `fault`
//! classifies the fault and returns `Kill`; `dispatch` reports it and halts.
//! Later labs give `fault` arms behind the same signature.
//!
//! Fill in the body; keep the signature and the types — later labs and the
//! course's tests use them.

use crate::addr::VirtAddr;
use crate::idt::TrapFrame;

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

/// The `#PF` error-code bits (SDM Vol. 3A §4.7).
pub mod pf {
    pub const PRESENT: u64 = 1 << 0;
    pub const WRITE: u64 = 1 << 1;
    pub const USER: u64 = 1 << 2;
}

/// Decide what a page fault at `addr` means.
pub fn fault(addr: VirtAddr, error: u64, tf: &TrapFrame) -> FaultOutcome {
    let _ = (addr, error, tf);
    todo!("lab 05 step 5 — fault: classify by the error code and the address, return Kill")
}
