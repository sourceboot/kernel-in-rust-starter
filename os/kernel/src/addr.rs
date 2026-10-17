//! Physical and virtual addresses as two types the compiler keeps apart.
//! Course-owned (proposed CONTRACT §B.1 amendment, 2026-10-16: §B.2 names
//! `PhysAddr`, `VirtAddr` and `PhysRange` from lab 04 on but no module holds
//! them). Shipped whole; no lab asks you to write it.

/// A physical address: what goes into a page-table entry or CR3.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
#[repr(transparent)]
pub struct PhysAddr(pub u64);

/// A virtual address: what an instruction dereferences.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
#[repr(transparent)]
pub struct VirtAddr(pub u64);

/// A half-open physical range `[start, end)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PhysRange {
    pub start: PhysAddr,
    pub end: PhysAddr,
}

pub const PAGE_SIZE: u64 = 4096;

impl PhysAddr {
    pub const fn align_down(self) -> PhysAddr {
        PhysAddr(self.0 & !(PAGE_SIZE - 1))
    }
    pub const fn align_up(self) -> PhysAddr {
        PhysAddr((self.0 + PAGE_SIZE - 1) & !(PAGE_SIZE - 1))
    }
}

impl VirtAddr {
    pub const fn as_ptr<T>(self) -> *const T {
        self.0 as *const T
    }
    pub const fn as_mut_ptr<T>(self) -> *mut T {
        self.0 as *mut T
    }
    /// The user half is everything below the canonical hole.
    pub const fn is_user(self) -> bool {
        self.0 < 0x0000_8000_0000_0000
    }
}

impl PhysRange {
    pub const fn contains(&self, pa: PhysAddr) -> bool {
        pa.0 >= self.start.0 && pa.0 < self.end.0
    }
}
