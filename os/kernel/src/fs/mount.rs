//! Lab 22 — `fs::mount`: what a mount is beyond the root `vfs` walks into.
//! This file is yours.
//!
//! `vfs` (lab 13) keeps the table the walk reads: a path and a root, and
//! since lab 22 the walk crosses into a mount at every component. This
//! module keeps what that table does not hold, the three things lab 22 adds
//! around it:
//!
//! - **device numbers.** Every file system a kernel serves answers one
//!   `st_dev` for every file in it, so `stat` can tell two file systems
//!   apart. One with no device under it takes an *anonymous* number, as
//!   Linux's do: `anon_dev()` hands out 1, 2, 3, … (major 0). ext2, `/proc`
//!   and `/dev` each take one when they are made; the boot archive (lab 13)
//!   answers 0.
//! - **the mount records** `/proc/mounts` lists: where, mounted from what
//!   (`source`, any text), and the file system's type — in mount order.
//!   `mount` checks the mount point, adds the root to `vfs`'s table and
//!   records the line; a root put in with `vfs::mount` alone is walked into
//!   but not listed.
//! - **syscall 165**, `mount(source, target, fstype, flags, data)`: the types
//!   a kernel can make from nothing — `"proc"` and `"devfs"`. `"ext2"` and
//!   `"archive"` need a drive or the archive's bytes, which only whoever
//!   booted the kernel holds, so they are mounted through `mount` and the
//!   syscall answers `-ENODEV` for them, as for any other type.
//!
//! The records are touched with interrupts off, like every shared table.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::fs::vfs::InodeRef;
use crate::syscall::Errno;

/// One mount, as `/proc/mounts` lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    /// What it was mounted from (any text: `/proc/mounts`' first field).
    pub source: String,
    /// The mount point: absolute, no trailing `/` (or `/` itself).
    pub target: String,
    /// `"ext2"`, `"archive"`, `"proc"`, `"devfs"`.
    pub fstype: String,
}

static NEXT_DEV: AtomicU64 = AtomicU64::new(1);

/// A new anonymous device number (major 0, minor 1, 2, …): what a file
/// system with no device under it answers as `st_dev`. Given: one counter.
pub fn anon_dev() -> u64 {
    NEXT_DEV.fetch_add(1, Ordering::SeqCst)
}

/// Mount the file system whose root is `root` at `target`, and record the
/// line `/proc/mounts` shows for it. Any target but `/` must be a directory
/// that exists (the walk's errno; `-ENOTDIR` for a file); then `vfs::mount`'s
/// refusals (`-EBUSY`, `-ENOMEM`); `-EINVAL` for a relative target.
pub fn mount(source: &str, target: &str, fstype: &str, root: InodeRef) -> Result<(), Errno> {
    let _ = (source, target, fstype, root);
    todo!("lab 22 step 3 — mount: check the mount point, vfs::mount the root, record the line")
}

/// Every mount `mount` recorded, in mount order.
pub fn table() -> Vec<Mount> {
    todo!("lab 22 step 8 — table: the mount records, in mount order")
}

/// Syscall 165's work (the strings already copied in; `flags` and `data`
/// are ignored): a new `/proc` for `"proc"`, a new `/dev` for `"devfs"`,
/// `-ENODEV` for any other type; then `mount`'s checks.
pub fn sys_mount(source: &str, target: &str, fstype: &str) -> Result<(), Errno> {
    let _ = (source, target, fstype);
    todo!("lab 22 step 9 — sys_mount: make a /proc or a /dev for the type, then mount it")
}
