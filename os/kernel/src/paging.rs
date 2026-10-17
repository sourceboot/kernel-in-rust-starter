//! Lab 08 — page tables, the HHDM, CR3. This file is yours.
//!
//! An `AddressSpace` owns a PML4 frame. `new_kernel` builds a fresh one whose
//! upper half (entries 256–511) is the running PML4's, so the kernel image
//! and the HHDM stay mapped; the lower half — the user half — starts empty,
//! and `map` builds it from frames.
//!
//! Fill in the bodies; keep the signatures and the types — later labs and
//! the course's tests use them.

use crate::addr::{PhysAddr, VirtAddr};
use crate::frame::Frame;

/// Page-table entry bits (SDM Vol. 3A §4.5); `flags: u64` throughout.
pub mod flags {
    pub const PRESENT: u64 = 1;
    pub const WRITE: u64 = 1 << 1;
    pub const USER: u64 = 1 << 2;
    /// PS: in a PDPT or PD entry, this entry maps a 1 GiB or 2 MiB page.
    pub const HUGE: u64 = 1 << 7;
    /// Bit 9 is ignored by the hardware; the course reserves it for lab 22's
    /// copy-on-write mark.
    pub const COW: u64 = 1 << 9;
    pub const NX: u64 = 1 << 63;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Pte(pub u64);

impl Pte {
    pub fn is_present(self) -> bool {
        todo!("lab 08 step 1 — Pte::is_present")
    }
    pub fn addr(self) -> PhysAddr {
        todo!("lab 08 step 1 — Pte::addr: the frame address bits")
    }
    pub fn flags(self) -> u64 {
        todo!("lab 08 step 1 — Pte::flags: every bit but the address")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageSize {
    Size4K,
    Size2M,
    Size1G,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapError {
    /// The page is already mapped.
    AlreadyMapped,
    /// A huge page covers the address where a table would go.
    HugePage,
    /// No frame for a page table.
    NoMemory,
}

/// The HHDM: every physical address, mapped at `pa + offset`.
pub fn hhdm(pa: PhysAddr) -> VirtAddr {
    let _ = pa;
    todo!("lab 08 step 1 — hhdm: the BootInfo's offset")
}

pub struct AddressSpace {
    pml4: Frame,
}

impl AddressSpace {
    /// A fresh PML4: the user half empty, the kernel half the running one's.
    pub fn new_kernel() -> Self {
        todo!("lab 08 step 2 — new_kernel: a zeroed PML4, the upper half copied from CR3's")
    }

    /// The physical address of this space's PML4 (what `activate` loads).
    pub fn root(&self) -> PhysAddr {
        self.pml4.0
    }

    /// Map the 4 KiB page at `va` to `f` with `flags` (PRESENT is implied).
    pub fn map(&mut self, va: VirtAddr, f: Frame, flags: u64) -> Result<(), MapError> {
        let _ = (va, f, flags);
        todo!("lab 08 step 3 — map: walk four levels, making tables as needed, write the leaf")
    }

    /// Unmap the 4 KiB page at `va`; returns the frame it mapped.
    pub fn unmap(&mut self, va: VirtAddr) -> Option<Frame> {
        let _ = va;
        todo!("lab 08 step 4 — unmap: clear the leaf, flush the TLB entry")
    }

    /// Where `va` lands, and the size of the page that maps it.
    pub fn translate(&self, va: VirtAddr) -> Option<(PhysAddr, PageSize)> {
        let _ = va;
        todo!("lab 08 step 4 — translate: walk, stopping at a huge page")
    }

    /// Load this space into CR3.
    pub fn activate(&self) {
        todo!("lab 08 step 5 — activate: mov cr3")
    }

    /// The 4 KiB leaf entry for `va`, if every table on the way exists.
    pub fn entry_mut(&mut self, va: VirtAddr) -> Option<&mut Pte> {
        let _ = va;
        todo!("lab 08 step 4 — entry_mut: the leaf entry")
    }
}
