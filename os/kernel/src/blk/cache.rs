//! Lab 16 — the buffer cache: the disk's blocks kept in memory. This file
//! is yours.
//!
//! A `Cache` sits on one `BlockDevice` with a fixed number of 4 KiB
//! buffers. `get(n)` hands out block `n` as a `Buf`, pinned: from the
//! buffer already holding it if there is one (a hit, counted by `hits`),
//! else read from the device into a free buffer, or into the one used
//! longest ago that nobody holds and that has no unwritten change. A pinned
//! buffer is never reused for another block; `release` unpins it. Two
//! `get`s of one block share one buffer, so a change made through one is
//! seen through the other.
//!
//! A change is not written when it is made. `write(&buf)` marks the buffer
//! dirty; `sync()` writes every dirty buffer to the device, in the cache's
//! order, and a buffer is clean again once its write succeeds. A dirty
//! buffer is never reused for another block before then — so what reaches
//! the disk, and when, is decided by whoever calls `sync` (a later lab's
//! log depends on that). A write the device refuses leaves it dirty, and
//! `sync` still tries every other dirty buffer, then answers `Ok` or the
//! errno of the first write that failed: a later lab refuses writes on
//! purpose, and `sync` must report that, never panic and never hide it.
//!
//! A read the device fails is the caller's error: `get` hands back the
//! device's errno, and the buffer it was filling holds no block afterwards.
//! A `get` when every buffer is pinned or dirty is a bug in the kernel, not
//! a result: `panic!` with the block's number.
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests use them. Add private fields and helpers as you need them.

extern crate alloc;

use alloc::sync::Arc;

use crate::blk::ata::{BlockDevice, BLOCK_SIZE};
use crate::sync::SpinGuard;
use crate::syscall::Errno;

/// A block, pinned in the cache until it is released (add the buffer it
/// stands for).
pub struct Buf {}

impl Buf {
    /// The block this buffer holds.
    pub fn block(&self) -> u64 {
        todo!("lab 16 step 11 — Buf::block: the block this buffer holds")
    }

    /// Its bytes, to read or change (a change reaches the disk through
    /// `Cache::write` and `Cache::sync`).
    pub fn data(&self) -> SpinGuard<'_, [u8; BLOCK_SIZE]> {
        todo!("lab 16 step 12 — Buf::data: the buffer's bytes, behind its lock")
    }
}

/// A buffer cache of `nbuf` blocks over one device (add the device, the
/// buffers and their lock, and the hit count).
pub struct Cache {}

impl Cache {
    /// An empty cache of `nbuf` buffers (at least one) over `dev`.
    pub fn new(dev: Arc<dyn BlockDevice>, nbuf: usize) -> Cache {
        let _ = (dev, nbuf);
        todo!("lab 16 step 8 — Cache::new: an empty cache over the device")
    }

    /// Block `n`, pinned: from its buffer if the cache holds it (a hit),
    /// else read from the device — whose errno, if the read fails, is the
    /// answer.
    pub fn get(&self, n: u64) -> Result<Buf, Errno> {
        let _ = n;
        todo!("lab 16 step 10 — Cache::get: a hit if a buffer holds block n, else read it into a buffer nobody needs")
    }

    /// Unpin `b`.
    pub fn release(&self, b: Buf) {
        let _ = b;
        todo!("lab 16 step 13 — Cache::release: unpin the buffer")
    }

    /// Mark `b` changed: `sync` writes it.
    pub fn write(&self, b: &Buf) {
        let _ = b;
        todo!("lab 16 step 14 — Cache::write: mark the buffer dirty")
    }

    /// Write every dirty buffer to the device, in the cache's order. Every
    /// dirty buffer is tried, even after one fails; a write the device
    /// refuses leaves its buffer dirty, for the next `sync`. The answer is
    /// `Ok` when every write landed, else the errno of the first that did
    /// not (in the cache's order).
    pub fn sync(&self) -> Result<(), Errno> {
        todo!("lab 16 step 15 — Cache::sync: write the dirty buffers, in the cache's order; answer the first errno")
    }

    /// `get`s served from a buffer, without a device read, since `new`.
    pub fn hits(&self) -> u64 {
        todo!("lab 16 step 9 — Cache::hits: count every get that needed no device read")
    }
}
