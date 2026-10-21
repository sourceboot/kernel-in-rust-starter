//! Labs 17 to 20 — ext2: the disk's file system behind lab 13's `Inode`
//! trait: read (lab 17), written (lab 18), cut and deleted from (lab 19),
//! and its directories listed, made, removed and renamed (lab 20). This
//! file is yours.
//!
//! The course's disk carries ext2 revision 1 in partition 2 (CONTRACT §A.7):
//! one block group, 4 KiB blocks, 128-byte inodes, directory records with a
//! type byte (`INCOMPAT_FILETYPE`). Every number on the disk is
//! little-endian; read each field out of the bytes with `from_le_bytes`.
//!
//! - **The superblock** is the 1024 bytes at byte 1024 of block 0. `mount`
//!   reads block 0 before anything else and refuses a file system it cannot
//!   serve with `-EINVAL`, before any other read: a wrong magic, a revision
//!   other than 1, a block size other than 4096, an inode size other than
//!   128, more than one group, or a feature it does not know. An
//!   *incompatible* feature changes how the disk must be read, so one this
//!   file does not implement is a refusal, never a guess.
//! - **The group descriptor** is the first 32 bytes of block 1: where the
//!   bitmaps and the inode table are.
//! - **An inode** is 128 bytes of the inode table: inode *n* (counted from 1)
//!   is entry *n − 1*. Its `i_block[15]` maps the file's blocks: 12 direct
//!   pointers, then one indirect block (1024 more pointers), then one double
//!   indirect block (1024 indirect blocks). The triple indirect pointer is
//!   never used on this disk.
//! - **A directory** is a file of records `{inode u32, rec_len u16, name_len
//!   u8, file_type u8, name}`; `rec_len` steps to the next record, a record
//!   never crosses a block, and a record whose inode is 0 is unused.
//!
//! An `Ext2` owns its partition and a buffer cache over it (lab 16): every
//! block this file reads comes through `Cache::get`, and a read the device
//! fails is passed up with `?`. Every operation runs with interrupts off
//! (`task::irq_save`): the drive and the cache are locked while a block is
//! read, and a syscall — interrupts off — that found either held by a
//! preempted task would spin for ever.
//!
//! **Labs 18 to 20 — the write half.** Lab 18 allocates: a block is taken
//! from the block bitmap (the first free one), an inode from the inode
//! bitmap (the first free one from `s_first_ino`), and each one taken moves
//! the free counts in the superblock and the group descriptor in the same
//! step, so the bitmaps and the counts always agree. A file's blocks are
//! mapped by `i_block` as the read half reads them, an indirect or double
//! indirect block allocated (zeroed) when the first block under it is;
//! `i_blocks` counts every block an inode holds, pointer blocks included,
//! in 512-byte units. A directory gains a record in the first block with
//! room for it (an unused record, or the slack after a record's name) and
//! else a new block. Lab 19 gives back: `truncate` and `unlink` free blocks
//! and inodes and move the same counts back, and a removed record is
//! folded into the one before it, or marked unused when it is a block's
//! first. Lab 20 lists a directory (`getdents`), makes and removes one — a
//! new directory holds `.` and `..`, has 2 links, and adds one to its
//! parent's and one to the descriptor's directory count — and renames.
//!
//! Every metadata change — a bitmap, the inode table, a directory block, a
//! pointer block, the superblock, the descriptor — is made inside one log
//! operation (`log::begin_op` … `log::end_op`, lab 18's interface 16) and
//! handed to `log::log_write` instead of `Cache::write`; a file's own data
//! blocks go to `Cache::write`. No operation logs more than
//! `log::MAXOPBLOCKS` blocks, so a long `write_at` is several operations of
//! at most `WRITE_CHUNK` blocks each. The end of an operation is when it
//! reaches the disk (`end_op`'s answer, the disk's errno, is the call's).
//!
//! `/.sboot-log` (the log's own file, lab 21) is never unlinked, renamed,
//! renamed over, truncated or written through this trait: `-EPERM`.
//!
//! The kernel keeps no calendar: a time written to an inode is `EPOCH`
//! plus the seconds since boot.
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests use them. Add private fields and helpers as you need them.

extern crate alloc;

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::blk::ata::Partition;
use crate::elf::ReadAt;
use crate::fs::vfs::{DirEntry, Inode, InodeRef, Stat};
use crate::syscall::Errno;

/// The superblock's magic number.
pub const MAGIC: u16 = 0xEF53;
/// The root directory's inode.
pub const ROOT_INO: u32 = 2;
/// Bytes per inode on this disk.
pub const INODE_SIZE: usize = 128;
/// Where the superblock starts, in block 0.
pub const SUPERBLOCK_OFFSET: usize = 1024;
/// Buffers in the cache an `Ext2` keeps.
pub const NBUF: usize = 32;
/// The log's own file, in the root directory (CONTRACT §A.7).
pub const LOG_NAME: &str = ".sboot-log";
/// The most file blocks one operation of `write_at` writes.
pub const WRITE_CHUNK: u64 = 8;
/// The largest file `write_at` and `truncate` make (2 GiB): `-EFBIG` past it.
pub const MAX_SIZE: u64 = 1 << 31;
/// What a time written to an inode counts from (2026-01-01 00:00 UTC).
pub const EPOCH: u32 = 1_767_225_600;

/// The feature bits this file knows (`s_feature_*`).
pub mod feature {
    /// Incompatible: directory records carry a type byte.
    pub const INCOMPAT_FILETYPE: u32 = 0x0002;
    /// Read-only compatible: superblock copies only in some groups.
    pub const RO_COMPAT_SPARSE_SUPER: u32 = 0x0001;
    /// Read-only compatible: a file may be 2 GiB or larger.
    pub const RO_COMPAT_LARGE_FILE: u32 = 0x0002;
}

/// The superblock's fields this kernel reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Superblock {
    pub inodes_count: u32,
    pub blocks_count: u32,
    pub free_blocks_count: u32,
    pub free_inodes_count: u32,
    pub first_data_block: u32,
    pub log_block_size: u32,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub magic: u16,
    pub rev_level: u32,
    pub first_ino: u32,
    pub inode_size: u16,
    pub feature_compat: u32,
    pub feature_incompat: u32,
    pub feature_ro_compat: u32,
}

impl Superblock {
    /// The superblock in `b`, its 1024 bytes (block 0 from byte 1024), and
    /// whether this file can serve it: the magic, revision 1, 4 KiB blocks,
    /// 128-byte inodes, the first data block 0, one group, the `filetype`
    /// feature present and no incompatible or read-only-compatible feature
    /// beyond the ones in `feature`. `-EINVAL` for anything else.
    pub fn parse(b: &[u8]) -> Result<Superblock, Errno> {
        let _ = b;
        todo!("lab 17 step 2 — Superblock::parse: the fields at their offsets, then -EINVAL for anything this file cannot serve")
    }
}

/// The one group's descriptor: where its bitmaps and inode table are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupDesc {
    pub block_bitmap: u32,
    pub inode_bitmap: u32,
    pub inode_table: u32,
}

impl GroupDesc {
    /// The descriptor in the first 32 bytes of `b` (block 1).
    pub fn parse(b: &[u8]) -> GroupDesc {
        let _ = b;
        todo!("lab 17 step 3 — GroupDesc::parse: the bitmaps' and the inode table's block numbers")
    }
}

/// A mounted ext2 file system (add the superblock, the group descriptor,
/// the device and a cache over it — an `Arc<Cache>` from lab 18, which the
/// log shares).
pub struct Ext2 {}

impl Ext2 {
    /// Serve the ext2 file system on `p`. The superblock is read first and
    /// checked (`Superblock::parse`) before any other block is read: a file
    /// system this file cannot serve is `-EINVAL` after that one read. Then
    /// the group descriptor. A partition smaller than the superblock says is
    /// `-EINVAL`; a failed read is the device's errno.
    pub fn mount(p: Partition) -> Result<Ext2, Errno> {
        let _ = p;
        todo!("lab 17 step 1 — Ext2::mount: a cache over the partition, block 0's superblock checked before any other read, then block 1")
    }

    /// The superblock `mount` read.
    pub fn superblock(&self) -> &Superblock {
        todo!("lab 17 step 4 — Ext2::superblock: the one mount read")
    }

    /// The group descriptor `mount` read.
    pub fn group(&self) -> &GroupDesc {
        todo!("lab 17 step 5 — Ext2::group: the one mount read")
    }

    /// The root directory (inode 2), for `vfs::mount`.
    pub fn root(self: &Arc<Self>) -> Result<InodeRef, Errno> {
        todo!("lab 17 step 6 — Ext2::root: inode 2, as an InodeRef")
    }

    /// Inode `ino`, read from the inode table. `-EINVAL` for 0 or a number
    /// past `s_inodes_count`.
    pub fn inode(self: &Arc<Self>, ino: u32) -> Result<Ext2Inode, Errno> {
        let _ = ino;
        todo!("lab 17 step 7 — Ext2::inode: entry ino - 1 of the inode table, its 128 bytes kept")
    }

    /// Lab 18: `/.sboot-log`'s inode number, as `mount` found it in the
    /// root; 0 when there is none. Lab 18 grows `mount`: it finds the log's
    /// file, and hands its cache to the log (`log::attach`).
    pub fn log_inode(&self) -> u32 {
        todo!("lab 18 step 6 — Ext2::log_inode: /.sboot-log's number, found in the root by mount (which also hands its cache to log::attach)")
    }

    /// Lab 18, with `mount`'s cache (one line, given on the page): every
    /// change made so far, on the disk (`Cache::sync`).
    pub fn sync(&self) -> Result<(), Errno> {
        todo!("lab 18 step 6 — Ext2::sync: every dirty buffer of the cache to the disk (one line, given on the page)")
    }
}

/// An ext2 file or directory (add its file system, its number and its 128
/// bytes). From lab 18 a copy of the 128 bytes kept here goes stale the
/// moment another `Ext2Inode` of the same number writes: read them from the
/// cache each time instead.
pub struct Ext2Inode {}

impl Ext2Inode {
    /// This inode's number.
    pub fn ino(&self) -> u32 {
        todo!("lab 17 step 13 — Ext2Inode::ino: the number it was read as")
    }

    /// The file system block that holds file block `i`: a direct pointer,
    /// one step through the indirect block, or two through the double
    /// indirect one. 0 is a hole (no block: it reads as zeros). `-EIO` past
    /// what a double indirect block maps.
    pub fn bmap(&self, i: u64) -> Result<u32, Errno> {
        let _ = i;
        todo!("lab 17 step 10 — Ext2Inode::bmap: 12 direct, then the indirect block, then the double indirect")
    }
}

/// The live records of one directory block, in order (a record whose inode
/// is 0 is skipped). `-EIO` for a block whose records do not tile it: a
/// `rec_len` under 8 + `name_len`, not a multiple of 4, or running past the
/// block's end.
pub fn dir_records(block: &[u8]) -> Result<Vec<DirEntry>, Errno> {
    let _ = block;
    todo!("lab 17 step 11 — dir_records: step by rec_len, skip inode 0, file_type as DT_*")
}

impl Inode for Ext2Inode {
    fn lookup(&self, name: &str) -> Result<InodeRef, Errno> {
        let _ = name;
        todo!("lab 17 step 9 — lookup: the record named `name` in this directory's blocks (-ENOTDIR, -ENOENT)")
    }

    fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<usize, Errno> {
        let _ = (off, buf);
        todo!("lab 17 step 12 — read_at: block by block through bmap, up to the size; -EISDIR for a directory")
    }

    fn getdents(&self, index: u64) -> Result<Option<DirEntry>, Errno> {
        let _ = index;
        todo!("lab 20 step 1 — getdents: live record `index` of the directory, Ok(None) past the last; -ENOTDIR")
    }

    fn stat(&self) -> Stat {
        todo!("lab 17 step 8 — stat: the inode's fields; st_blocks is i_blocks, in 512-byte units")
    }

    fn write_at(&self, off: u64, buf: &[u8]) -> Result<usize, Errno> {
        let _ = (off, buf);
        todo!("lab 18 step 8 — write_at: blocks allocated through direct, indirect and double indirect pointers, at most WRITE_CHUNK an operation; -EPERM for the log, -EFBIG past MAX_SIZE")
    }

    fn create(&self, name: &str, mode: u32) -> Result<InodeRef, Errno> {
        let _ = (name, mode);
        todo!("lab 18 step 7 — create: a free inode (the first from s_first_ino) and a record in this directory, in one operation; -EEXIST")
    }

    fn unlink(&self, name: &str) -> Result<(), Errno> {
        let _ = name;
        todo!("lab 19 step 2 — unlink: the record removed; at 0 links the blocks and the inode freed; -EISDIR, -EPERM for the log")
    }

    fn mkdir(&self, name: &str, mode: u32) -> Result<InodeRef, Errno> {
        let _ = (name, mode);
        todo!("lab 20 step 2 — mkdir: . and .., 2 links and one more for this directory, the descriptor's directory count; -EEXIST")
    }

    fn rmdir(&self, name: &str) -> Result<(), Errno> {
        let _ = name;
        todo!("lab 20 step 3 — rmdir: only . and .. left (-ENOTEMPTY), then this directory's link and the directory count given back")
    }

    fn rename(&self, old: &str, new_dir: &dyn Inode, new: &str) -> Result<(), Errno> {
        let _ = (old, new_dir, new);
        todo!("lab 20 step 4 — rename: within this file system (-EXDEV); a replaced target freed; a moved directory's .. and both parents' links")
    }

    fn truncate(&self, len: u64) -> Result<(), Errno> {
        let _ = len;
        todo!("lab 19 step 1 — truncate: the blocks past len freed, the rest of the last one zeroed; -EPERM for the log")
    }
}

/// The ELF loader reads a program off the disk through this (interface 9).
impl ReadAt for Ext2Inode {
    fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<usize, Errno> {
        let _ = (off, buf);
        todo!("lab 17 step 14 — ReadAt::read_at: the same bytes as Inode::read_at")
    }
}
