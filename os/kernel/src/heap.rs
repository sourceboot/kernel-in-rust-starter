//! Lab 10 — the heap. This file is yours.
//!
//! `Box`, `Vec`, `String` and `Arc` live in the `alloc` crate, and every one
//! of them asks the `#[global_allocator]` below for memory. Write it as a
//! linked list of free blocks in address order, kept inside the free memory
//! itself; when nothing fits, take frames from `frame::alloc` and map them
//! at the heap's top through `paging`. The heap lives in the top 512 GiB
//! (PML4 entry 511), the slot every address space shares. An allocation
//! that arrives before `init` places the heap at `START`: from this lab the
//! console opens a file through `Arc::new`, and lab 09's tests never call
//! `init`.
//!
//! One CPU, one lock: interrupts stay off while the heap's lock is held,
//! and a lock found held on entry is a deadlock — refuse the allocation
//! (return null) and name it, never wait.
//!
//! Fill in the bodies; keep the signatures, the types and the static — later
//! labs and the course's tests use them.

use core::alloc::{GlobalAlloc, Layout};

use crate::addr::VirtAddr;
use crate::sync::SpinLock;

/// Where the heap starts when an allocation arrives before `init`.
const START: u64 = 0xffff_ffff_c000_0000;

/// The heap's state, behind the lock: yours to design.
struct Heap {}

/// The heap behind its lock: what `#[global_allocator]` names.
pub struct LockedHeap {
    heap: SpinLock<Heap>,
}

/// The heap's lock, held: interrupts stay off until it drops.
pub struct HeapGuard<'a> {
    _heap: &'a LockedHeap,
}

impl LockedHeap {
    pub const fn new() -> LockedHeap {
        LockedHeap { heap: SpinLock::new(Heap {}) }
    }

    /// Take the heap's lock with interrupts off; `None` when it is already
    /// held (on this one CPU, waiting would never end).
    pub fn lock(&self) -> Option<HeapGuard<'_>> {
        let _ = &self.heap;
        todo!("lab 10 step 4 — LockedHeap::lock: interrupts off, then try the lock; held means deadlock: None")
    }
}

impl Default for LockedHeap {
    fn default() -> LockedHeap {
        LockedHeap::new()
    }
}

// SAFETY: yours to uphold — `alloc` returns null or a block no other live
// allocation overlaps, of at least `layout.size()` bytes at a multiple of
// `layout.align()`; `dealloc` takes back what `alloc` handed out.
unsafe impl GlobalAlloc for LockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = (layout, START);
        // (The `if` keeps this stub's message: an `alloc` that can never
        // return is compiled to a bare `ud2` where `Box::new` calls it, and
        // the `todo!` never prints. Your body returns, so it needs none.)
        if core::hint::black_box(true) {
            todo!("lab 10 step 2 — alloc: the first free block that fits, split; grow by frames when none does (step 3)")
        }
        core::ptr::null_mut()
    }

    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        let _ = (p, layout);
        todo!("lab 10 step 2 — dealloc: back into the list in address order, merged with the blocks it touches")
    }
}

#[global_allocator]
pub static HEAP: LockedHeap = LockedHeap::new();

/// Place the heap at `start` (page-aligned, in the top 512 GiB) and map
/// `initial_pages` frames there.
pub fn init(start: VirtAddr, initial_pages: usize) {
    let _ = (start, initial_pages);
    todo!("lab 10 step 1 — init: map initial_pages frames at start through paging, and free them into the list")
}
