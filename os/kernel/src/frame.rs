//! Lab 07 — the frame allocator. This file is yours.
//!
//! Every 4 KiB physical frame the bootloader marks usable (memmap kind 0 —
//! Rust for Kernels lab 01 spelled usable as 1; this course's BootInfo
//! carries Limine's numbering), at or above 1 MiB, outside the kernel's image, gets a
//! reference count. `alloc` hands out a zeroed frame with count 1; `inc`
//! adds a reference; `free` drops one and the frame returns to the pool at 0.
//!
//! Fill in the bodies; keep the signatures and the types — later labs and
//! the course's tests use them.

use crate::addr::{PhysAddr, PhysRange};
use crate::bootinfo::MemRegion;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Frame(pub PhysAddr);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stats {
    pub free: usize,
    pub used: usize,
    pub total: usize,
}

/// Build the pool from the memory map. Call once.
pub fn init(memmap: &[MemRegion], kernel: PhysRange) {
    let _ = (memmap, kernel);
    todo!("lab 07 task 1 — init: count the usable frames, place the counts, mark them free")
}

/// A zeroed frame with reference count 1, or `None` when none is free.
pub fn alloc() -> Option<Frame> {
    todo!("lab 07 task 2 — alloc: find a free frame, count 1, zero it")
}

/// Drop one reference; at 0 the frame is free again.
pub fn free(f: Frame) {
    let _ = f;
    todo!("lab 07 task 3 — free: drop one reference")
}

/// Add one reference to an allocated frame.
pub fn inc(f: Frame) {
    let _ = f;
    todo!("lab 07 task 3 — inc: add one reference")
}

/// The frame's reference count (0 = free, or not a frame of the pool).
pub fn refs(f: Frame) -> u32 {
    let _ = f;
    todo!("lab 07 task 3 — refs: the frame's count")
}

/// free, used and total under the lock; all zeros before `init` has run.
pub fn stats() -> Stats {
    todo!("lab 07 task 4 — stats: free, used, total")
}
