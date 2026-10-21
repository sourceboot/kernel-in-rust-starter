//! Lab 16 — the disk: the block-device trait, the ATA driver under it, and
//! partition 2 found through the GPT. This file is yours.
//!
//! A block device here is anything that reads and writes numbered 4 KiB
//! blocks (`BlockDevice`). The disk under it speaks 512-byte sectors, so one
//! block is eight sectors: block `n` is sectors `8n` to `8n + 7`.
//!
//! The disk is the primary ATA bus's master, driven by programmed I/O with
//! 28-bit LBA: the command block at ports `0x1F0`–`0x1F7`, the control
//! register at `0x3F6`. One command reads or writes up to 256 sectors; after
//! it, the drive raises DRQ once per sector and the 256 words of that sector
//! move through the data port. Poll the status register (BSY, DRQ, ERR, DF)
//! rather than wait for an interrupt, and leave interrupts on while you
//! poll; set nIEN in the control register so the drive never raises IRQ 14
//! (nothing handles it). IDENTIFY DEVICE tells you the model string (words
//! 27–46, two characters per word, the first in the HIGH byte) and the
//! sectors 28-bit LBA reaches (words 60–61, low word first).
//!
//! Every block device carries a write-fault hook: after `arm_fail_after(k)`
//! the first `k` writes succeed and every later one returns `-EIO`, reaches
//! nothing on the disk, and is counted; `disarm()` stops refusing and keeps
//! the count; a read is never refused. The count is since boot (arming
//! never resets it). A later lab uses the hook to test what a crash in the
//! middle of an update leaves on the disk. A block past a device's end is
//! `-EIO` as well, and is not a refusal.
//!
//! The disk has a GPT; partition 2 (the second entry of the table) is the
//! course's file system. Find it by reading the GPT through the device —
//! never by a fixed LBA.
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests use them. Add private fields and helpers as you need them.

extern crate alloc;

use alloc::sync::Arc;

use crate::syscall::Errno;

/// A block: what every `BlockDevice` reads and writes.
pub const BLOCK_SIZE: usize = 4096;
/// A sector: what the ATA disk reads and writes.
pub const SECTOR_SIZE: usize = 512;
/// Sectors in a block.
pub const SECTORS_PER_BLOCK: u64 = (BLOCK_SIZE / SECTOR_SIZE) as u64;

/// Numbered 4 KiB blocks, read and written whole. `Send + Sync`, so one
/// device can sit behind an `Arc` that every task shares.
pub trait BlockDevice: Send + Sync {
    /// Block `n` into `buf`. Never refused by the fault hook; `-EIO` for
    /// `n >= blocks()`.
    fn read_block(&self, n: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), Errno>;
    /// `buf` onto block `n`; `-EIO` when the fault hook refuses it (and
    /// nothing reaches the disk), or for `n >= blocks()` (not counted).
    fn write_block(&self, n: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), Errno>;
    /// How many blocks the device holds.
    fn blocks(&self) -> u64;
    /// After `arm_fail_after(k)` the first `k` writes succeed, and every
    /// later one returns `-EIO` and is counted, until `disarm`.
    fn arm_fail_after(&self, k: u32);
    /// Stop refusing; the count stays.
    fn disarm(&self);
    /// Writes refused since boot. Arming never resets it: take a
    /// difference across the writes you care about.
    fn refused_writes(&self) -> u32;
}

/// The primary bus's master, as IDENTIFY described it (add what you keep,
/// and a lock so two tasks' commands never interleave).
pub struct Ata {}

impl Ata {
    /// IDENTIFY DEVICE on the primary master. `-ENODEV` when nothing answers
    /// or the device is not ATA; `-EIO` when it errs.
    pub fn probe() -> Result<Ata, Errno> {
        todo!("lab 16 step 1 — Ata::probe: IDENTIFY DEVICE on the primary master, and keep what it answers")
    }

    /// The model string IDENTIFY reported, trailing spaces dropped.
    pub fn model(&self) -> &str {
        todo!("lab 16 step 2 — Ata::model: the model string from IDENTIFY words 27-46")
    }

    /// Sectors on the disk (IDENTIFY words 60–61).
    pub fn sectors(&self) -> u64 {
        todo!("lab 16 step 3 — Ata::sectors: the sector count from IDENTIFY words 60-61")
    }
}

impl BlockDevice for Ata {
    fn read_block(&self, n: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), Errno> {
        let _ = (n, buf);
        todo!("lab 16 step 6 — Ata::read_block: READ SECTORS for the block's eight sectors, polling for each")
    }

    fn write_block(&self, n: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), Errno> {
        let _ = (n, buf);
        todo!("lab 16 step 17 — Ata::write_block: the fault hook first, then WRITE SECTORS and a cache flush")
    }

    fn blocks(&self) -> u64 {
        todo!("lab 16 step 4 — Ata::blocks: the disk's size in 4 KiB blocks")
    }

    fn arm_fail_after(&self, k: u32) {
        let _ = k;
        todo!("lab 16 step 19 — Ata::arm_fail_after: let k writes land, refuse every one after")
    }

    fn disarm(&self) {
        todo!("lab 16 step 20 — Ata::disarm: stop refusing writes, keep the count")
    }

    fn refused_writes(&self) -> u32 {
        todo!("lab 16 step 18 — Ata::refused_writes: the writes the hook refused since boot")
    }
}

/// One partition of a device: a `BlockDevice` whose block `n` is the
/// device's block `first_lba / 8 + n`. The fault hook is the device's.
pub struct Partition {
    dev: Arc<dyn BlockDevice>,
    /// The partition's first sector on the device.
    first_lba: u64,
    /// Its size in sectors.
    sectors: u64,
}

impl Partition {
    /// The partition's first sector on the device.
    pub fn first_lba(&self) -> u64 {
        self.first_lba
    }
}

impl BlockDevice for Partition {
    fn read_block(&self, n: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), Errno> {
        let _ = (n, buf, &self.dev);
        todo!("lab 16 step 7 — Partition::read_block: the partition's block n is which block of the device?")
    }

    fn write_block(&self, n: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), Errno> {
        let _ = (n, buf);
        todo!("lab 16 step 16 — Partition::write_block: as read_block, the other way")
    }

    fn blocks(&self) -> u64 {
        self.sectors / SECTORS_PER_BLOCK
    }

    fn arm_fail_after(&self, k: u32) {
        self.dev.arm_fail_after(k)
    }

    fn disarm(&self) {
        self.dev.disarm()
    }

    fn refused_writes(&self) -> u32 {
        self.dev.refused_writes()
    }
}

/// The GPT's second partition (entry index 1) on `dev`. The GPT header is
/// sector 1 (it starts `"EFI PART"`) and says where the entry array starts
/// and how big an entry is. `-EINVAL` for no GPT, or a partition that does
/// not start on a block boundary or runs past the device; `-ENODEV` when
/// entry 1 is unused (its type GUID all zero).
pub fn partition2(dev: Arc<dyn BlockDevice>) -> Result<Partition, Errno> {
    let _ = dev;
    todo!("lab 16 step 5 — partition2: read the GPT through the device and find entry 1")
}
