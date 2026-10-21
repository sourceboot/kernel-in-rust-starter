//! The log (CONTRACT §A.7 "The log", §A.9 interface 16). `18-files-write`
//! wrote its interface, `Txn`, as a pass-through; `21-the-log` writes its
//! body: write-ahead logging and recovery. This file is yours.
//!
//! Every change ext2 makes to its metadata — a bitmap, the inode table, a
//! directory block, an indirect block, the superblock or the group
//! descriptor — happens inside one *operation*:
//!
//! ```text
//! log::begin_op();
//! let b = cache.get(n)?;  /* change b.data() */  log::log_write(&b);
//! …
//! log::end_op()?;          // or log::abort_op() when the work failed
//! ```
//!
//! `log_write` is called *instead of* `Cache::write` for a metadata block,
//! and it remembers the block for the operation: once, however many times it
//! is called for it (`logged()` lists them, in the order they were first
//! logged). One operation logs at most `MAXOPBLOCKS` blocks; more is a
//! kernel bug, and `log_write` panics, so an operation that could need more
//! is split by its caller (a long `write_at` is several operations).
//! Operations nest: only the end of the outermost one commits, and what it
//! commits is every block logged since the outermost `begin_op`.
//!
//! **`21-the-log`: write-ahead.** A logged block is not marked dirty: the log
//! keeps it pinned in the cache (a `Buf` of its own), so it is never written
//! home, and never evicted, before the commit. The end of the outermost
//! operation, with a `Log` attached (`attach_log`, `Ext2::mount`'s), runs
//! the commit, in this order, each write straight to the log's device:
//!
//! 1. the cache's dirty buffers — the operation's *data* blocks — reach the
//!    disk (`Cache::sync`): data before the metadata that points at it;
//! 2. logged block *i* is written to log slot *i*;
//! 3. the header — `MAGIC`, `n`, the `n` home block numbers — is written to
//!    the header block: **this write is the commit**;
//! 4. each logged block is written to its home;
//! 5. the header is written again with `n = 0`.
//!
//! A refused write before step 3 leaves the operation uncommitted: the
//! logged buffers are read back from the disk (memory again says what the
//! disk says) and `end_op` answers the errno. A refused write from step 4 on
//! leaves it committed: the next mount's `Log::recover` finishes it. Either
//! way, after a refused write the log commits nothing more — every later
//! outermost `end_op` is `-EIO`, its blocks read back — until the next
//! attach: the disk stays as the last commit left it, for recovery.
//!
//! `abort_op` ends an operation whose work failed: the transaction it is
//! part of commits nothing, and the logged buffers are read back from the
//! disk. So an operation that fails half way changes nothing on the disk or
//! in memory. With only a cache attached (`attach`, the pass-through), the end of
//! the outermost operation marks the logged buffers dirty and writes them
//! with `Cache::sync` (`18-files-write`'s pass-through), and an abort lets them go
//! unwritten (there is no device to read them back from).
//!
//! The commit runs with this file's lock let go — the disk is slow, and
//! whatever the device does meanwhile (the course's tests read `logged()`
//! from inside it) must not find the lock held with interrupts off. The
//! operation's blocks are forgotten only after the commit.
//!
//! Touched with interrupts off, like every table a syscall reaches.
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests use them. Add private fields and helpers as you need them (the
//! lock-guarded state the operations share is yours to shape).
//!
//! **Your `fs/ext2.rs` changes too** (CONTRACT §B.2-21), in three places:
//!
//! - `Ext2::mount` maps `/.sboot-log` (the header is its file block 0, slot
//!   *i* its block 1 + *i*), builds the `Log`, runs `Log::recover`, reads the
//!   superblock and the descriptor again, and calls `attach_log` where it
//!   called `attach`.
//! - The wrapper every ext2 call runs in (`Ext2::op` in the course's
//!   reference) changes one line: `let c = log::end_op();` becomes
//!   `let c = if r.is_ok() { log::end_op() } else { log::abort_op(); Ok(()) };`
//!   — an operation whose body fails is aborted, not committed.
//! - A body that catches an error after it has logged something must return
//!   it, so the abort undoes it: the reference's `write_at` turns a failed
//!   block map into a short write, which stays right for `-ENOSPC` and
//!   `-EFBIG` (nothing was taken) and becomes `return Err(e)` for anything
//!   else (an allocation whose new block could not be read).

extern crate alloc;

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::blk::ata::{BlockDevice, BLOCK_SIZE};
use crate::blk::cache::{Buf, Cache};
use crate::syscall::Errno;

/// The most blocks one operation may log.
pub const MAXOPBLOCKS: usize = 10;
/// Log blocks after the header on the disk (`/.sboot-log`: 1 + `LOGSIZE`).
pub const LOGSIZE: usize = 30;
/// The header block's first eight bytes (little-endian).
pub const MAGIC: u64 = 0x5B00_7B00_4C06;

/// The header block, decoded: `MAGIC` (u64), `n` (u32), then `n` home block
/// numbers (u32 each, ext2's block width), all little-endian from byte 0;
/// the rest of the block is zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// Logged blocks the header commits; 0 when there is nothing to replay.
    pub n: u32,
    /// Where slot *i*'s block belongs, for *i* < `n`.
    pub blocks: [u32; LOGSIZE],
}

impl Header {
    /// The header of a log with nothing to replay.
    pub const EMPTY: Header = Header { n: 0, blocks: [0; LOGSIZE] };

    /// The header block `b`, decoded. A block without `MAGIC`, or with `n`
    /// past `LOGSIZE`, is not a log this kernel wrote: `-EINVAL`.
    pub fn parse(b: &[u8; BLOCK_SIZE]) -> Result<Header, Errno> {
        let _ = b;
        todo!("lab 21 step 8 — Header::parse: MAGIC checked, n (at most LOGSIZE), then n u32 home numbers, little-endian")
    }

    /// This header as the disk holds it, into `b` (the rest zeroed).
    pub fn encode(&self, b: &mut [u8; BLOCK_SIZE]) {
        let _ = b;
        todo!("lab 21 step 4 — Header::encode: MAGIC (u64), n (u32), the n homes (u32 each) from byte 0, every other byte zero")
    }
}

/// The log on the disk: the device the file system is on, the header's
/// block, and the block of each slot (`/.sboot-log`'s file blocks 0 and
/// 1…30, mapped once by `Ext2::mount`; they need not be contiguous).
pub struct Log {
    dev: Arc<dyn BlockDevice>,
    header: u64,
    slots: [u64; LOGSIZE],
}

impl Log {
    /// The log whose header is block `header` and whose slot *i* is block
    /// `slots[i]`, on `dev`.
    pub fn new(dev: Arc<dyn BlockDevice>, header: u64, slots: [u64; LOGSIZE]) -> Log {
        let _ = (dev, header, slots);
        todo!("lab 21 step 1 — Log::new: the device, the header's block and the 30 slots' blocks, kept")
    }

    /// Mount's first act on the log: a header with `n > 0` is a commit that
    /// may not have reached home — copy slot *i* to block `blocks[i]` for
    /// every *i* < `n` (through `cache`, so a buffer the cache already holds
    /// for that block says it too), then write the header with `n = 0`.
    /// Replaying twice is harmless. The answer is `n` (0: nothing to do, and
    /// nothing written); a header that does not parse is `-EINVAL`, a
    /// failed read or write the device's errno.
    pub fn recover(&self, cache: &Cache) -> Result<u32, Errno> {
        let _ = cache;
        let _ = (&self.dev, self.header, self.slots);
        todo!("lab 21 step 7 — Log::recover: read the header; for n > 0 copy each slot home through the cache, then write the header with n = 0")
    }

    /// The commit of `bufs`, the transaction's logged blocks (the module
    /// doc's steps 1–5): `end_op` calls it when a `Log` is attached. Before
    /// the header lands, a refusal reads `bufs` back from the disk.
    #[allow(dead_code)]
    fn commit(&self, cache: &Cache, bufs: &[Buf]) -> Result<(), Errno> {
        let _ = (cache, bufs);
        todo!("lab 21 step 3 — the commit: data (Cache::sync), the slots, the header, the homes, the header with n = 0, in that order")
    }

    /// `bufs` say again what the disk says (an uncommitted change undone).
    #[allow(dead_code)]
    fn restore(&self, bufs: &[Buf]) {
        let _ = bufs;
        todo!("lab 21 step 6 — restore: each logged buffer read back from the disk")
    }
}

/// The file system whose metadata the operations change from now on: its
/// cache, with no log — the pass-through (a file system without
/// `/.sboot-log`, and the course's controls, use it).
pub fn attach(cache: Arc<Cache>) {
    let _ = cache;
    todo!("lab 18 step 1 — log::attach: the cache log_write marks and the commit writes (it replaces the one before)")
}

/// The file system whose metadata the operations change from now on, and
/// its log: every commit goes through `log`. `Ext2::mount` calls it after
/// `Log::recover`.
pub fn attach_log(cache: Arc<Cache>, log: Log) {
    let _ = (cache, log);
    todo!("lab 21 step 2 — log::attach_log: the cache and the log every commit goes through (they replace the ones before; nothing is left open)")
}

/// An operation begins.
pub fn begin_op() {
    todo!("lab 18 step 2 — log::begin_op: one more operation open")
}

/// `b`, a metadata block changed inside the current operation: remembered
/// for the operation (once). `18-files-write`: marked dirty in the cache.
/// `21-the-log`: pinned in the cache until the commit instead (a `Buf` of
/// the log's own) — not marked dirty, so nothing but the commit writes it.
/// Outside an operation, or past `MAXOPBLOCKS` blocks in one, it panics.
pub fn log_write(b: &Buf) {
    let _ = b;
    todo!("lab 18 step 3 — log::log_write: b's block remembered once (at most MAXOPBLOCKS), b marked dirty in the attached cache")
}

/// The blocks the open operation has logged, in the order first logged;
/// empty between operations.
pub fn logged() -> Vec<u64> {
    todo!("lab 18 step 4 — log::logged: the open operation's blocks, in the order first logged")
}

/// An operation ends. The end of the outermost one commits, then the
/// transaction's blocks are forgotten. `18-files-write`: every dirty buffer
/// reaches the disk (`Cache::sync`). `21-the-log`: with a `Log` attached,
/// the module doc's five steps (`Log::commit`). `Err` is the first write
/// the disk refused, or `-EIO` for a transaction an `abort_op` or an earlier
/// refusal ended.
pub fn end_op() -> Result<(), Errno> {
    todo!("lab 18 step 5 — log::end_op: the last open operation commits (Cache::sync, this file's lock let go), then its blocks are forgotten")
}

/// An operation ends because its work failed: the transaction it is part of
/// commits nothing (when its outermost operation ends, every block it logged
/// is read back from the disk). Call it instead of `end_op`.
pub fn abort_op() {
    todo!("lab 21 step 5 — log::abort_op: the transaction is marked to commit nothing; the operation ends (end_op reads its blocks back)")
}
