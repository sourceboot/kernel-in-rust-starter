//! Lab 21's vehicle (`21-the-log`): write-ahead logging and recovery — the
//! body of `fs/log.rs` (interface 16) and the mount that replays it.
//!
//! The disk is `ktest-21.img` as the course's image writer made it (CONTRACT
//! §A.7); the guest writes it, so the course boots a copy. Every expectation
//! comes from this file's own reads of the disk — its own PIO driver (never
//! the learner's `Ata`) and its own decoder of the superblock, the
//! descriptor, both bitmaps, the inode table, the block maps, the directory
//! records and the log's header — or from this file's formulas. Nothing the
//! learner's code reports is an expectation.
//!
//! The drive sits under a device of this file's (`Wrap`) between the
//! learner's `Ata` and `partition2`: it sees every write the learner's file
//! system makes, records its block, whether it landed, and (for the log's
//! header block) the `n` it carried, and counts the writes the drive refused
//! while the learner's fault hook was armed. It can also refuse writes
//! itself — every write after the first commit header lands — which is how
//! row 1 stops the machine between the commit and the install without the
//! learner's hook.
//!
//! The operation (rows 1–6): one transaction this file opens with the
//! learner's `log::begin_op`, holding `create` of `/tmp/<name>` and a
//! `write_at` of its bytes (one data block), closed with `log::end_op` — so
//! it touches the inode bitmap, the inode table, `/tmp`'s directory block,
//! the block bitmap, the superblock and the descriptor, and writes one data
//! block. "Before" is this file's walk of the tree before the transaction;
//! "after" is the same tree with `/tmp/<name>` added (a new inode, mode
//! 0100644, one link, the bytes) and nothing else changed. A state is
//! judged by a walk of the whole tree from the root and by the free maps:
//! both bitmaps, the five counts, every link count and `i_blocks` agree with
//! the walk. Eight rows, in this order:
//!
//!   1 `log.header.shape` (plain) — two controls on a memory device of this
//!     file's: a commit writes, in order, the data block, the two logged
//!     blocks to slots 0 and 1, the header (`MAGIC`, `n` = 2, the two home
//!     numbers, zeros), the two homes, and the header with `n` = 0; recovery
//!     of a two-block header replays both through the cache and zeroes the
//!     header, a second recovery writes nothing, and a header without the
//!     magic is `-EINVAL`. Then the disk: the operation, with
//!     `Wrap` refusing every write after the header lands; the header as
//!     this file reads it raw has the contract's shape (magic, `n` = the
//!     distinct home blocks, `n` ≤ `MAXOPBLOCKS`, each a block of the file
//!     system and not one of the log's, zeros after), the file's data block
//!     reached the disk before the header, and the tree is still "before"
//!     (nothing went home before the commit); after the remount (recovery),
//!     every home holds its slot's bytes, the header has `n` = 0, the tree is
//!     "after", and the learner's mount reads the file back.
//!   2–6 `log.recover.k1`, `.k2`, `.k3`, `.k5`, `.k8` (derived: the name and
//!     the bytes from the nonce and the row id) — `arm_fail_after(k)` on the
//!     learner's hook, the operation (`end_op` must answer the refusal),
//!     `disarm`, every in-memory structure dropped, `Ext2::mount` again
//!     (recovery runs); the tree is "after" when a header with `n` > 0
//!     landed, else "before" — never between; the header has `n` = 0; and
//!     the learner's new mount reads the file back (or does not find it).
//!     Every k lands inside the operation: at least one write is refused.
//!     Here the operation also `unlink`s `/tmp/v<k>-…` (amended 2026-10-18),
//!     a one-block file made by its own committed operation before row 1,
//!     so the transaction frees an inode and a block as well as taking
//!     them: "after" has `/tmp/v<k>` gone, and both states are judged with
//!     the free maps this file re-derives from its walk. An unlogged bitmap
//!     write (`Cache::write` for `log_write`) goes home in the commit's
//!     `Cache::sync`, before the header, and is seen here as a free map that
//!     disagrees with the tree — `txn.every.metadata.write`'s (18's)
//!     independent check, which a lying `logged()` cannot pass.
//!   7 `log.refused.count.matches` (plain) — for each k: the writes `Wrap`
//!     passed down while armed, the ones the drive refused (exactly those
//!     after the first k), and the learner's `refused_writes()` moved by
//!     exactly that many; row 1's own refusals (the hook disarmed) moved it
//!     by none.
//!   8 `log.abort.op` (plain) — an operation that fails part-way leaves the
//!     tree as it was. Controls on the memory device first: an aborted
//!     operation writes nothing and its block reads back as the device holds
//!     it, the log commits again after it, and a transaction whose inner
//!     operation aborts commits nothing of the operations before it. Then
//!     the disk, after a remount (a cold cache): one `write_at` that grows
//!     `/tmp/hdr` by a block, with `Wrap` refusing the first read of a block
//!     this file's walk says is free — the allocator has already taken the
//!     bit and moved the counts when it reads the new block, so `Ext2::op`'s
//!     body fails part-way; `write_at` must fail, no write reaches the drive,
//!     the tree is "before", and the learner's mount reads `/tmp/hdr` as it
//!     was. Last, a transaction of a `create` and a `create` of the same
//!     name (`-EEXIST`) commits nothing — no write, "before", and the
//!     learner's mount does not find the name.
//!
//! The course also runs `e2fsck -fn` on the copy's partition 2 after the
//! boot (measured, not graded).

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../shim.rs"]
mod shim;

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::arch::asm;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use kernel::addr::VirtAddr;
use kernel::blk::ata::{self, Ata, BlockDevice, BLOCK_SIZE};
use kernel::blk::cache::Cache;
use kernel::fs::ext2::Ext2;
use kernel::fs::log::{self, Log, LOGSIZE};
use kernel::fs::vfs::InodeRef;
use kernel::ktap;
use kernel::sync::SpinLock;
use kernel::syscall::Errno;
use kernel::{bootinfo, frame, gdt, heap, idt, irq, proc, sched, serial, syscall, time};

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 16;
const PM_HZ: u64 = 3_579_545;

const PLAN: usize = 8;
const SHAPE_ID: &str = "log.header.shape";
const KS: [u32; 5] = [1, 2, 3, 5, 8];
const K_IDS: [&str; 5] = ["log.recover.k1", "log.recover.k2", "log.recover.k3", "log.recover.k5", "log.recover.k8"];
const COUNT_ID: &str = "log.refused.count.matches";
const ABORT_ID: &str = "log.abort.op";

/// Partition 2's first 4 KiB block on the disk (§A.7).
const P2_BLOCK0: u64 = 8448;
/// The log header's magic and an operation's limit (§A.7), this file's own.
const MAGIC: u64 = 0x5B00_7B00_4C06;
const MAXOPBLOCKS: usize = 10;
const ENOENT: Errno = 2;
const EIO: Errno = 5;
const EEXIST: Errno = 17;
const EINVAL: Errno = 22;


// ── Time, interrupts, notes ──────────────────────────────────────────────

fn pm() -> u32 {
    let v: u32;
    // SAFETY: a 32-bit read of the ACPI PM timer port; no memory is touched.
    unsafe { asm!("in eax, dx", out("eax") v, in("dx") 0x608u16, options(nomem, nostack, preserves_flags)) };
    v & 0xff_ffff
}

/// Microseconds since `new`, from the 24-bit PM timer (it wraps every
/// 4.7 s; `us` must be called more often than that).
struct Clock {
    last: u32,
    acc: u64,
}

impl Clock {
    fn new() -> Clock {
        Clock { last: pm(), acc: 0 }
    }
    fn us(&mut self) -> u64 {
        let now = pm();
        self.acc += (now.wrapping_sub(self.last) & 0xff_ffff) as u64;
        self.last = now;
        self.acc * 1_000_000 / PM_HZ
    }
    fn ms(&mut self) -> u64 {
        self.us() / 1000
    }
}

fn ticks() -> u64 {
    time::TICKS.load(Ordering::Relaxed)
}

// ── Interrupts off around what must not be cut ───────────────────────────

fn irq_off() -> bool {
    let rflags: u64;
    // SAFETY: reads RFLAGS through the stack and clears IF.
    unsafe { asm!("pushfq", "pop {}", "cli", out(reg) rflags) };
    rflags & (1 << 9) != 0
}

fn irq_restore(on: bool) {
    if on {
        // SAFETY: setting IF touches no memory.
        unsafe { asm!("sti", options(nomem, nostack)) };
    }
}

/// `f` with interrupts off: the learner's drive and cache hold their locks
/// across a read, and a preempted holder would leave a syscall spinning.
fn quiet<R>(f: impl FnOnce() -> R) -> R {
    let on = irq_off();
    let r = f();
    irq_restore(on);
    r
}

fn note(args: core::fmt::Arguments) {
    quiet(|| ktap::note(args));
}

fn halt() -> ! {
    loop {
        // SAFETY: interrupts off, then wait: nothing runs after this.
        unsafe { asm!("cli", "hlt", options(nomem, nostack)) };
    }
}

fn errno_text(e: Errno) -> String {
    let name = match e {
        1 => "EPERM",
        2 => "ENOENT",
        5 => "EIO",
        8 => "ENOEXEC",
        9 => "EBADF",
        12 => "ENOMEM",
        13 => "EACCES",
        14 => "EFAULT",
        17 => "EEXIST",
        18 => "EXDEV",
        19 => "ENODEV",
        20 => "ENOTDIR",
        21 => "EISDIR",
        22 => "EINVAL",
        23 => "ENFILE",
        24 => "EMFILE",
        27 => "EFBIG",
        28 => "ENOSPC",
        29 => "ESPIPE",
        30 => "EROFS",
        38 => "ENOSYS",
        39 => "ENOTEMPTY",
        _ => "?",
    };
    format!("-{e} ({name})")
}

/// A syscall's answer as a reader wants it: the number, or `-N (ENAME)`.
// ── One row's recorder (ktap's shapes, printed in this file's order) ─────

struct Row {
    id: &'static str,
    failed: bool,
    used_nonce: bool,
}

impl Row {
    fn new(id: &'static str) -> Row {
        Row { id, failed: false, used_nonce: false }
    }
    fn fail(&mut self, why: core::fmt::Arguments) {
        self.failed = true;
        note(why);
    }
    fn expect_bytes(&mut self, what: &str, got: &[u8], want: &[u8]) {
        if got.len() != want.len() {
            let id = self.id;
            self.fail(format_args!("{id}: {what}: {} bytes, expected {}", got.len(), want.len()));
            return;
        }
        if let Some(i) = (0..got.len()).find(|&i| got[i] != want[i]) {
            let id = self.id;
            self.fail(format_args!("{id}: {what}: differs first at byte {i}: {:#04x}, expected {:#04x}", got[i], want[i]));
        }
    }
    fn derived(&mut self, id: &str) -> u64 {
        self.used_nonce = true;
        ktap::derive(&ktap::nonce().unwrap_or([b'0'; 32]), id)
    }
    fn finish(mut self, n: usize) {
        if self.used_nonce && ktap::nonce().is_none() {
            self.failed = true;
            note(format_args!("{}: derived from the run's nonce, and this run has none", self.id));
        }
        quiet(|| {
            if self.failed {
                kernel::kprintln!("not ok {n} - {}", self.id);
            } else {
                kernel::kprintln!("ok {n} - {}", self.id);
            }
        });
    }
}
// ── This vehicle's own view of the disk ──────────────────────────────────

fn in8(port: u16) -> u8 {
    let v: u8;
    // SAFETY: an 8-bit port read of the primary ATA bus; no memory touched.
    unsafe { asm!("in al, dx", out("al") v, in("dx") port, options(nomem, nostack, preserves_flags)) };
    v
}

fn out8(port: u16, v: u8) {
    // SAFETY: an 8-bit port write of the primary ATA bus; no memory touched.
    unsafe { asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack, preserves_flags)) };
}

fn in16(port: u16) -> u16 {
    let v: u16;
    // SAFETY: a 16-bit port read of the primary ATA bus; no memory touched.
    unsafe { asm!("in ax, dx", out("ax") v, in("dx") port, options(nomem, nostack, preserves_flags)) };
    v
}

/// The disk's 4 KiB block `n`, read by this file's own PIO (28-bit LBA,
/// polled, interrupts off): never the learner's driver.
fn raw_disk_block(n: u64, buf: &mut [u8; BLOCK_SIZE]) -> bool {
    quiet(|| {
        let lba = n * 8;
        let wait = |data: bool| -> bool {
            for _ in 0..20_000_000u32 {
                let s = in8(0x1f7);
                if s & 0x80 == 0 {
                    if !data {
                        return true;
                    }
                    if s & 0x21 != 0 {
                        return false;
                    }
                    if s & 0x08 != 0 {
                        return true;
                    }
                }
            }
            false
        };
        if !wait(false) {
            return false;
        }
        out8(0x1f6, 0xe0 | ((lba >> 24) & 0x0f) as u8);
        for _ in 0..4 {
            in8(0x3f6);
        }
        out8(0x1f2, 8);
        out8(0x1f3, lba as u8);
        out8(0x1f4, (lba >> 8) as u8);
        out8(0x1f5, (lba >> 16) as u8);
        out8(0x1f7, 0x20);
        for _ in 0..4 {
            in8(0x3f6);
        }
        for sector in buf.chunks_exact_mut(512) {
            if !wait(true) {
                return false;
            }
            for pair in sector.chunks_exact_mut(2) {
                pair.copy_from_slice(&in16(0x1f0).to_le_bytes());
            }
        }
        true
    })
}

fn le16(b: &[u8], at: usize) -> u64 {
    u16::from_le_bytes([b[at], b[at + 1]]) as u64
}

fn le32(b: &[u8], at: usize) -> u64 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as u64
}

fn le64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

/// While one walk of the tree runs (the disk does not change under it),
/// each block this file reads is kept: `Some`.
static MEMO: SpinLock<Option<BTreeMap<u64, Box<[u8; BLOCK_SIZE]>>>> = SpinLock::new(None);

/// Keep blocks (`on`) or forget them; the answer is whether they were kept.
fn memo(on: bool) -> bool {
    quiet(|| {
        let mut m = MEMO.lock();
        let was = m.is_some();
        if on && !was {
            *m = Some(BTreeMap::new());
        } else if !on {
            *m = None;
        }
        was
    })
}

/// Partition 2's fs block `b`, raw.
fn raw_block(b: u64) -> Result<Box<[u8; BLOCK_SIZE]>, String> {
    if let Some(v) = quiet(|| MEMO.lock().as_ref().and_then(|m| m.get(&b).cloned())) {
        return Ok(v);
    }
    let mut buf = Box::new([0u8; BLOCK_SIZE]);
    if raw_disk_block(P2_BLOCK0 + b, &mut buf) {
        quiet(|| {
            if let Some(m) = MEMO.lock().as_mut() {
                m.insert(b, buf.clone());
            }
        });
        Ok(buf)
    } else {
        Err(format!("this vehicle's own read of fs block {b} failed"))
    }
}

/// One directory record as the disk holds it.

/// The file system as this file reads it: what does not change (the
/// geometry), read once.
struct Raw {
    inode_table: u64,
    block_bitmap: u64,
    inode_bitmap: u64,
    blocks: u64,
    inodes: u64,
    first_ino: u64,
}

impl Raw {
    fn open() -> Result<Raw, String> {
        let sb = raw_block(0)?;
        let s = &sb[1024..2048];
        if le16(s, 56) != 0xef53 {
            return Err(String::from("fs block 0 holds no ext2 superblock (magic) as this vehicle reads it"));
        }
        let gd = raw_block(1)?;
        Ok(Raw {
            inode_table: le32(&gd[..], 8),
            block_bitmap: le32(&gd[..], 0),
            inode_bitmap: le32(&gd[..], 4),
            blocks: le32(s, 4),
            inodes: le32(s, 0),
            first_ino: le32(s, 84),
        })
    }
    /// The inode table's blocks: from 0 to here, every block is the file
    /// system's own (superblock, descriptor, bitmaps, table).
    fn meta_end(&self) -> u64 {
        self.inode_table + (self.inodes * 128).div_ceil(4096)
    }
    fn inode(&self, ino: u64) -> Result<[u8; 128], String> {
        let at = (ino - 1) * 128;
        let b = raw_block(self.inode_table + at / 4096)?;
        let off = (at % 4096) as usize;
        let mut r = [0u8; 128];
        r.copy_from_slice(&b[off..off + 128]);
        Ok(r)
    }
    /// The blocks inode `i` maps: (data, pointer blocks), holes skipped.
    fn blocks_of(&self, i: &[u8; 128]) -> Result<(Vec<u64>, Vec<u64>), String> {
        let ptrs = |b: &[u8; BLOCK_SIZE]| -> Vec<u64> { (0..1024).map(|k| le32(&b[..], 4 * k)).filter(|&p| p != 0).collect() };
        let (mut data, mut meta) = (Vec::new(), Vec::new());
        for k in 0..12 {
            let p = le32(i, 40 + 4 * k);
            if p != 0 {
                data.push(p);
            }
        }
        let ind = le32(i, 88);
        if ind != 0 {
            meta.push(ind);
            data.extend(ptrs(&*raw_block(ind)?));
        }
        let dind = le32(i, 92);
        if dind != 0 {
            meta.push(dind);
            for ind in ptrs(&*raw_block(dind)?) {
                meta.push(ind);
                data.extend(ptrs(&*raw_block(ind)?));
            }
        }
        if le32(i, 96) != 0 {
            return Err(String::from("a triple indirect pointer"));
        }
        Ok((data, meta))
    }
    /// File block `k` of inode `i` (0: a hole).
    fn bmap(&self, i: &[u8; 128], k: u64) -> Result<u64, String> {
        let at = |b: u64, x: u64| -> Result<u64, String> { if b == 0 { Ok(0) } else { Ok(le32(&raw_block(b)?[..], 4 * x as usize)) } };
        if k < 12 {
            return Ok(le32(i, 40 + 4 * k as usize));
        }
        let k = k - 12;
        if k < 1024 {
            return at(le32(i, 88), k);
        }
        let k = k - 1024;
        at(at(le32(i, 92), k / 1024)?, k % 1024)
    }
    /// The bytes of inode `i`, through its block map.
    fn file(&self, i: &[u8; 128]) -> Result<Vec<u8>, String> {
        let size = le32(i, 4) as usize;
        let mut out = alloc::vec![0u8; size];
        for k in 0..size.div_ceil(4096) {
            let b = self.bmap(i, k as u64)?;
            if b != 0 {
                let blk = raw_block(b)?;
                let n = (size - 4096 * k).min(4096);
                out[4096 * k..4096 * k + n].copy_from_slice(&blk[..n]);
            }
        }
        Ok(out)
    }
    /// Every live record of directory inode `i`: (inode, file_type, name).
    fn records(&self, ino: u64, i: &[u8; 128]) -> Result<Vec<(u64, u8, String)>, String> {
        let mut out = Vec::new();
        for k in 0..le32(i, 4).div_ceil(4096) {
            let b = raw_block(self.bmap(i, k)?)?;
            let mut at = 0usize;
            while at < 4096 {
                let rino = le32(&b[..], at);
                let rec_len = le16(&b[..], at + 4) as usize;
                let name_len = b[at + 6] as usize;
                if rec_len < 8 + name_len || rec_len % 4 != 0 || at + rec_len > 4096 {
                    return Err(format!("directory inode {ino}: the record at byte {at} of its block {k} does not parse"));
                }
                if rino != 0 {
                    out.push((rino, b[at + 7], String::from_utf8_lossy(&b[at + 8..at + 8 + name_len]).into_owned()));
                }
                at += rec_len;
            }
        }
        Ok(out)
    }
    /// The entry `name` of directory `dir`.
    fn find(&self, dir: u64, name: &str) -> Result<Option<u64>, String> {
        let i = self.inode(dir)?;
        Ok(self.records(dir, &i)?.into_iter().find(|r| r.2 == name).map(|r| r.0))
    }
    /// The inode an absolute ext2 path names; `None` when a name is missing.
    fn walk(&self, path: &str) -> Result<Option<u64>, String> {
        let mut ino = 2u64;
        for c in path.split('/').filter(|c| !c.is_empty()) {
            match self.find(ino, c)? {
                Some(n) => ino = n,
                None => return Ok(None),
            }
        }
        Ok(Some(ino))
    }
}

fn links(i: &[u8; 128]) -> u64 {
    le16(i, 26)
}

// ── The whole tree, walked by this file ──────────────────────────────────

/// What this file finds in use, walking from the root.
struct Scan {
    /// Every block in use: the file system's own, and what each inode maps.
    used: BTreeSet<u64>,
    dir_blocks: BTreeSet<u64>,
    ptr_blocks: BTreeSet<u64>,
    /// The reserved inodes and every one the tree reaches.
    inodes: BTreeSet<u64>,
    dirs: u64,
    /// What disagrees inside the tree itself (links, i_blocks, a block
    /// claimed twice).
    problems: Vec<String>,
}

fn scan(raw: &Raw) -> Result<Scan, String> {
    let mut s = Scan { used: (0..raw.meta_end()).collect(), dir_blocks: BTreeSet::new(), ptr_blocks: BTreeSet::new(), inodes: BTreeSet::new(), dirs: 0, problems: Vec::new() };
    let mut tables: BTreeMap<u64, Box<[u8; BLOCK_SIZE]>> = BTreeMap::new();
    let mut inode = |ino: u64| -> Result<[u8; 128], String> {
        if ino == 0 || ino > raw.inodes {
            return Err(format!("a record names inode {ino}, past the table"));
        }
        let at = (ino - 1) * 128;
        let t = raw.inode_table + at / 4096;
        if !tables.contains_key(&t) {
            tables.insert(t, raw_block(t)?);
        }
        let off = (at % 4096) as usize;
        let mut r = [0u8; 128];
        r.copy_from_slice(&tables[&t][off..off + 128]);
        Ok(r)
    };
    let mut refs: BTreeMap<u64, u64> = BTreeMap::new();
    let claim = |s: &mut Scan, b: u64, ino: u64| {
        if b >= raw.blocks || !s.used.insert(b) {
            s.problems.push(format!("inode {ino} maps block {b}, which {}", if b >= raw.blocks { "is past the file system" } else { "is in use already" }));
        }
    };
    let mut todo = alloc::vec![2u64];
    let mut seen: BTreeSet<u64> = BTreeSet::from([2]);
    for ino in 1..raw.first_ino {
        s.inodes.insert(ino);
        if ino != 2 {
            let i = inode(ino)?;
            let (data, meta) = raw.blocks_of(&i)?;
            for b in data.into_iter().chain(meta) {
                claim(&mut s, b, ino);
            }
        }
    }
    while let Some(ino) = todo.pop() {
        let i = inode(ino)?;
        let kind = le16(&i, 0) & 0o170000;
        if kind != 0o040000 && kind != 0o100000 {
            continue;
        }
        let (data, meta) = raw.blocks_of(&i)?;
        if le32(&i, 28) != 8 * (data.len() + meta.len()) as u64 {
            s.problems.push(format!("inode {ino}: i_blocks {}, and it maps {} blocks ({} sectors)", le32(&i, 28), data.len() + meta.len(), 8 * (data.len() + meta.len())));
        }
        if (le32(&i, 4)).div_ceil(4096) < data.len() as u64 {
            s.problems.push(format!("inode {ino}: {} data blocks for a size of {}", data.len(), le32(&i, 4)));
        }
        for &b in &meta {
            claim(&mut s, b, ino);
            s.ptr_blocks.insert(b);
        }
        for &b in &data {
            claim(&mut s, b, ino);
        }
        if kind == 0o040000 {
            s.dirs += 1;
            s.dir_blocks.extend(data.iter().copied());
            for (rino, _, _) in raw.records(ino, &i)? {
                *refs.entry(rino).or_insert(0) += 1;
                if seen.insert(rino) {
                    s.inodes.insert(rino);
                    todo.push(rino);
                }
            }
        }
    }
    for ino in seen {
        let i = inode(ino)?;
        let want = refs.get(&ino).copied().unwrap_or(0);
        if links(&i) != want {
            s.problems.push(format!("inode {ino}: i_links_count {}, and {want} records name it", links(&i)));
        }
    }
    Ok(s)
}

/// Every way the disk's bitmaps and counts disagree with `s`.
fn agree(raw: &Raw, s: &Scan) -> Result<Vec<String>, String> {
    let mut out = s.problems.clone();
    let (bb, ib, sb, gd) = (raw_block(raw.block_bitmap)?, raw_block(raw.inode_bitmap)?, raw_block(0)?, raw_block(1)?);
    let bit = |m: &[u8; BLOCK_SIZE], n: u64| m[(n / 8) as usize] >> (n % 8) & 1 == 1;
    let bad: Vec<u64> = (0..raw.blocks).filter(|&b| bit(&bb, b) != s.used.contains(&b)).collect();
    if !bad.is_empty() {
        out.push(format!("{} blocks whose bitmap bit is not what the tree says, first {:?} (bit {})", bad.len(), &bad[..bad.len().min(4)], bit(&bb, bad[0]) as u8));
    }
    let bad: Vec<u64> = (1..=raw.inodes).filter(|&i| bit(&ib, i - 1) != s.inodes.contains(&i)).collect();
    if !bad.is_empty() {
        out.push(format!("{} inodes whose bitmap bit is not what the tree says, first {:?} (bit {})", bad.len(), &bad[..bad.len().min(4)], bit(&ib, bad[0] - 1) as u8));
    }
    let free_b = raw.blocks - s.used.len() as u64;
    let free_i = raw.inodes - s.inodes.len() as u64;
    for (what, got, want) in [
        ("s_free_blocks_count", le32(&sb[1024..], 12), free_b),
        ("s_free_inodes_count", le32(&sb[1024..], 16), free_i),
        ("bg_free_blocks_count", le16(&gd[..], 12), free_b),
        ("bg_free_inodes_count", le16(&gd[..], 14), free_i),
        ("bg_used_dirs_count", le16(&gd[..], 16), s.dirs),
    ] {
        if got != want {
            out.push(format!("{what} {got}, and the tree says {want}"));
        }
    }
    Ok(out)
}
fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// `len` bytes: splitmix64(seed ^ k) for word k, little-endian.
fn mix(seed: u64, len: usize) -> Vec<u8> {
    let mut v: Vec<u8> = (0..len.div_ceil(8) as u64).flat_map(|k| splitmix(seed ^ k).to_le_bytes()).collect();
    v.truncate(len);
    v
}

// ── The tree, as states ──────────────────────────────────────────────────

/// One name in the tree, as this file reads it.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Ent {
    ino: u64,
    mode: u64,
    links: u64,
    size: u64,
    /// Every block the inode maps, data then pointer blocks.
    blocks: Vec<u64>,
}

/// Every path under the root (not `.` or `..`) and what it names.
fn tree(raw: &Raw) -> Result<BTreeMap<String, Ent>, String> {
    let on = memo(true);
    let r = tree_now(raw);
    memo(on);
    r
}

fn tree_now(raw: &Raw) -> Result<BTreeMap<String, Ent>, String> {
    let mut out = BTreeMap::new();
    let mut todo = alloc::vec![(String::new(), 2u64)];
    while let Some((path, ino)) = todo.pop() {
        let i = raw.inode(ino)?;
        for (rino, _, name) in raw.records(ino, &i)? {
            if name == "." || name == ".." {
                continue;
            }
            let p = format!("{path}/{name}");
            if out.contains_key(&p) || out.len() > 4096 {
                return Err(format!("{p} is reached twice"));
            }
            let c = raw.inode(rino)?;
            let (mut blocks, meta) = raw.blocks_of(&c)?;
            blocks.extend(meta);
            let mode = le16(&c, 0);
            if mode & 0o170000 == 0o040000 {
                todo.push((p.clone(), rino));
            }
            out.insert(p, Ent { ino: rino, mode, links: links(&c), size: le32(&c, 4), blocks });
        }
    }
    Ok(out)
}

/// What the tree on the disk is now, against `before` and the operation
/// that adds `/tmp/<name>` holding `bytes`: `Ok(false)` before, `Ok(true)`
/// after, `Err` with the first difference otherwise.
fn judge(raw: &Raw, before: &BTreeMap<String, Ent>, name: &str, bytes: &[u8]) -> Result<bool, String> {
    judge_gone(raw, before, name, bytes, None)
}

/// `judge`, for an operation that also unlinks `/tmp/<gone>`: "after" is
/// `before` with `/tmp/<name>` added and `/tmp/<gone>` removed.
fn judge_gone(raw: &Raw, before: &BTreeMap<String, Ent>, name: &str, bytes: &[u8], gone: Option<&str>) -> Result<bool, String> {
    memo(true);
    let r = judge_now(raw, before, name, bytes, gone);
    memo(false);
    r
}

fn judge_now(raw: &Raw, before: &BTreeMap<String, Ent>, name: &str, bytes: &[u8], gone: Option<&str>) -> Result<bool, String> {
    let s = scan(raw)?;
    let bad = agree(raw, &s)?;
    if !bad.is_empty() {
        return Err(format!("the free maps disagree with the tree: {}", bad.join("; ")));
    }
    let now = tree(raw)?;
    if &now == before {
        return Ok(false);
    }
    let path = format!("/tmp/{name}");
    let gone = gone.map(|g| format!("/tmp/{g}"));
    for (p, e) in before {
        if gone.as_ref() == Some(p) {
            if now.contains_key(p) {
                return Err(format!("{p} is still there"));
            }
            continue;
        }
        match now.get(p) {
            None => return Err(format!("{p} is gone")),
            Some(n) if n != e => return Err(format!("{p} changed: {n:?}, was {e:?}")),
            _ => {}
        }
    }
    for p in now.keys() {
        if !before.contains_key(p) && *p != path {
            return Err(format!("{p} appeared"));
        }
    }
    let Some(f) = now.get(&path) else { return Err(format!("the tree changed and {path} is not in it")) };
    if before.values().any(|e| e.ino == f.ino) {
        return Err(format!("{path} is inode {}, which another name already held", f.ino));
    }
    if f.mode != 0o100644 || f.links != 1 || f.size != bytes.len() as u64 {
        return Err(format!("{path}: mode {:o}, {} links, size {}; expected 100644, 1, {}", f.mode, f.links, f.size, bytes.len()));
    }
    let got = raw.file(&raw.inode(f.ino)?)?;
    if got != bytes {
        let i = (0..got.len()).find(|&i| got[i] != bytes[i]).unwrap_or(0);
        return Err(format!("{path}: its bytes on the disk differ first at byte {i}"));
    }
    Ok(true)
}

fn state_text(s: &Result<bool, String>) -> String {
    match s {
        Ok(false) => String::from("before"),
        Ok(true) => String::from("after"),
        Err(why) => format!("between ({why})"),
    }
}

// ── The devices ──────────────────────────────────────────────────────────

/// What became of one write `Wrap` was handed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fate {
    Landed,
    /// The drive refused it (the learner's hook).
    Refused,
    /// `Wrap` refused it itself (row 1's stop after the commit).
    Stopped,
}

/// One write: its fs block, its fate, and `n` when it was the header.
#[derive(Clone, Copy)]
struct Wr {
    block: u64,
    fate: Fate,
    header_n: Option<u32>,
}

/// The drive, watched (the module doc).
struct Wrap {
    inner: Arc<dyn BlockDevice>,
    /// The log's header block (fs block), from this file's reader.
    header: u64,
    /// The learner's hook is armed (this file armed it).
    armed: AtomicBool,
    /// Writes passed down while armed, and of them the ones refused.
    armed_calls: AtomicU64,
    armed_refused: AtomicU64,
    /// Refuse every write after a header with `n` > 0 lands.
    stop_after_commit: AtomicBool,
    stopped: AtomicBool,
    record: SpinLock<Vec<Wr>>,
    /// Row 8: while `Some(used)`, the first read of an fs block outside
    /// `used` (a free block) is refused, and its number kept in `refused_read`.
    refuse_free: SpinLock<Option<BTreeSet<u64>>>,
    refused_read: SpinLock<Option<u64>>,
}

impl Wrap {
    fn take(&self) -> Vec<Wr> {
        quiet(|| core::mem::take(&mut *self.record.lock()))
    }
}

impl BlockDevice for Wrap {
    fn read_block(&self, n: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), Errno> {
        let b = n.wrapping_sub(P2_BLOCK0);
        let refuse = quiet(|| {
            let free = self.refuse_free.lock();
            let mut once = self.refused_read.lock();
            match &*free {
                Some(used) if once.is_none() && !used.contains(&b) => {
                    *once = Some(b);
                    true
                }
                _ => false,
            }
        });
        if refuse {
            return Err(EIO);
        }
        self.inner.read_block(n, buf)
    }
    fn write_block(&self, n: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), Errno> {
        let b = n.wrapping_sub(P2_BLOCK0);
        let header_n = (b == self.header).then(|| u32::from_le_bytes(buf[8..12].try_into().unwrap()));
        let (fate, r) = if self.stopped.load(Ordering::Relaxed) {
            (Fate::Stopped, Err(EIO))
        } else {
            let armed = self.armed.load(Ordering::Relaxed);
            let r = self.inner.write_block(n, buf);
            if armed {
                self.armed_calls.fetch_add(1, Ordering::Relaxed);
                if r.is_err() {
                    self.armed_refused.fetch_add(1, Ordering::Relaxed);
                }
            }
            (if r.is_ok() { Fate::Landed } else { Fate::Refused }, r)
        };
        if fate == Fate::Landed && header_n.is_some_and(|h| h > 0) && self.stop_after_commit.load(Ordering::Relaxed) {
            self.stopped.store(true, Ordering::Relaxed);
        }
        quiet(|| self.record.lock().push(Wr { block: b, fate, header_n }));
        r
    }
    fn blocks(&self) -> u64 {
        self.inner.blocks()
    }
    fn arm_fail_after(&self, k: u32) {
        self.inner.arm_fail_after(k)
    }
    fn disarm(&self) {
        self.inner.disarm()
    }
    fn refused_writes(&self) -> u32 {
        self.inner.refused_writes()
    }
}

/// The controls' device: 64 blocks in memory (zeros until written), and
/// every write in order.
struct MemDisk {
    blocks: SpinLock<BTreeMap<u64, Box<[u8; BLOCK_SIZE]>>>,
    writes: SpinLock<Vec<(u64, Box<[u8; BLOCK_SIZE]>)>>,
}

impl MemDisk {
    fn new() -> MemDisk {
        MemDisk { blocks: SpinLock::new(BTreeMap::new()), writes: SpinLock::new(Vec::new()) }
    }
    /// Block `n` holds `b`, as if written long ago.
    fn seed(&self, n: u64, b: Box<[u8; BLOCK_SIZE]>) {
        quiet(|| self.blocks.lock().insert(n, b));
    }
    fn block(&self, n: u64) -> Box<[u8; BLOCK_SIZE]> {
        quiet(|| self.blocks.lock().get(&n).cloned()).unwrap_or_else(|| Box::new([0; BLOCK_SIZE]))
    }
    fn take(&self) -> Vec<(u64, Box<[u8; BLOCK_SIZE]>)> {
        quiet(|| core::mem::take(&mut *self.writes.lock()))
    }
}

impl BlockDevice for MemDisk {
    fn read_block(&self, n: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), Errno> {
        if n >= 64 {
            return Err(EIO);
        }
        buf.copy_from_slice(&self.block(n)[..]);
        Ok(())
    }
    fn write_block(&self, n: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), Errno> {
        if n >= 64 {
            return Err(EIO);
        }
        quiet(|| {
            self.blocks.lock().insert(n, Box::new(*buf));
            self.writes.lock().push((n, Box::new(*buf)));
        });
        Ok(())
    }
    fn blocks(&self) -> u64 {
        64
    }
    fn arm_fail_after(&self, _k: u32) {}
    fn disarm(&self) {}
    fn refused_writes(&self) -> u32 {
        0
    }
}

/// A block of `v`s.
fn filled(v: u8) -> Box<[u8; BLOCK_SIZE]> {
    Box::new([v; BLOCK_SIZE])
}

/// The header block this file expects: `MAGIC`, `n`, the homes, zeros.
fn header_bytes(homes: &[u64]) -> Box<[u8; BLOCK_SIZE]> {
    let mut b = Box::new([0u8; BLOCK_SIZE]);
    b[0..8].copy_from_slice(&MAGIC.to_le_bytes());
    b[8..12].copy_from_slice(&(homes.len() as u32).to_le_bytes());
    for (i, h) in homes.iter().enumerate() {
        b[12 + 4 * i..16 + 4 * i].copy_from_slice(&(*h as u32).to_le_bytes());
    }
    b
}

// ── Rows 1 and 8's controls: the log over a memory device ────────────────

const C_HDR: u64 = 40;

fn c_slots() -> [u64; LOGSIZE] {
    core::array::from_fn(|i| 41 + i as u64)
}

/// The writes `md` saw against the sequence expected: (block, bytes).
fn same_writes(bad: &mut Vec<String>, what: &str, got: &[(u64, Box<[u8; BLOCK_SIZE]>)], want: &[(u64, Box<[u8; BLOCK_SIZE]>)]) {
    let blocks: Vec<u64> = got.iter().map(|w| w.0).collect();
    let wblocks: Vec<u64> = want.iter().map(|w| w.0).collect();
    if blocks != wblocks {
        bad.push(format!("{what}: the device saw writes to blocks {blocks:?}, expected {wblocks:?}"));
        return;
    }
    for (k, (g, w)) in got.iter().zip(want).enumerate() {
        if g.1 != w.1 {
            let i = (0..BLOCK_SIZE).find(|&i| g.1[i] != w.1[i]).unwrap_or(0);
            bad.push(format!("{what}: write {} (block {}) differs at byte {i}: {:#04x}, expected {:#04x}", k + 1, g.0, g.1[i], w.1[i]));
            return;
        }
    }
}

fn control_commit_abort(bad: &mut Vec<String>, abad: &mut Vec<String>) {
    let md = Arc::new(MemDisk::new());
    md.seed(7, filled(0x07));
    md.seed(9, filled(0x09));
    let dev: Arc<dyn BlockDevice> = md.clone();
    let cache = Arc::new(Cache::new(dev.clone(), 8));
    log::attach_log(cache.clone(), Log::new(dev.clone(), C_HDR, c_slots()));
    let get = |n: u64| cache.get(n).map_err(|e| format!("the control's own Cache::get({n}) returned {}", errno_text(e)));
    // A commit: data block 12, logged 7 and 9.
    let r = quiet(|| -> Result<Result<(), Errno>, String> {
        log::begin_op();
        let (a, b, d) = (get(7)?, get(9)?, get(12)?);
        a.data().fill(0x17);
        log::log_write(&a);
        b.data().fill(0x19);
        log::log_write(&b);
        d.data().fill(0x12);
        cache.write(&d);
        log::log_write(&a);
        drop((a, b, d));
        if !md.take().is_empty() {
            return Err(String::from("a block reached the device before end_op"));
        }
        Ok(log::end_op())
    });
    match r {
        Err(why) => {
            abad.push(format!("the commit control before it: {why}"));
            return bad.push(why);
        }
        Ok(Err(e)) => bad.push(format!("the commit's end_op() returned {}", errno_text(e))),
        Ok(Ok(())) => {}
    }
    let want = [(12, filled(0x12)), (41, filled(0x17)), (42, filled(0x19)), (C_HDR, header_bytes(&[7, 9])), (7, filled(0x17)), (9, filled(0x19)), (C_HDR, header_bytes(&[]))];
    same_writes(bad, "a commit of blocks 7 and 9 with data block 12", &md.take(), &want);
    // Row 8's controls from here: an abort writes nothing and reads the
    // block back; then a commit again.
    let bad = abad;
    let r = quiet(|| -> Result<(), String> {
        log::begin_op();
        let a = get(7)?;
        a.data().fill(0xAA);
        log::log_write(&a);
        drop(a);
        log::abort_op();
        Ok(())
    });
    if let Err(why) = r {
        return bad.push(why);
    }
    if !md.take().is_empty() {
        bad.push(String::from("an aborted operation wrote to the device"));
    }
    match get(7) {
        Ok(a) if a.data()[0] != 0x17 || a.data()[4095] != 0x17 => {
            bad.push(format!("after abort_op, block 7 in the cache holds {:#04x}, and the device {:#04x}", a.data()[0], 0x17))
        }
        Err(why) => bad.push(why),
        _ => {}
    }
    if !log::logged().is_empty() {
        bad.push(format!("logged() after abort_op is {:?}, expected []", log::logged()));
    }
    let r = quiet(|| -> Result<Result<(), Errno>, String> {
        log::begin_op();
        let b = get(9)?;
        b.data().fill(0x29);
        log::log_write(&b);
        drop(b);
        Ok(log::end_op())
    });
    match r {
        Err(why) => return bad.push(why),
        Ok(Err(e)) => bad.push(format!("the commit after an abort: end_op() returned {}", errno_text(e))),
        Ok(Ok(())) => {}
    }
    let want = [(41, filled(0x29)), (C_HDR, header_bytes(&[9])), (9, filled(0x29)), (C_HDR, header_bytes(&[]))];
    same_writes(bad, "the commit after an abort", &md.take(), &want);
    // Nested: the outer transaction's earlier operation is undone with it.
    let r = quiet(|| -> Result<Result<(), Errno>, String> {
        log::begin_op();
        log::begin_op();
        let a = get(7)?;
        a.data().fill(0xB7);
        log::log_write(&a);
        drop(a);
        let inner = log::end_op();
        log::begin_op();
        let b = get(9)?;
        b.data().fill(0xB9);
        log::log_write(&b);
        drop(b);
        log::abort_op();
        if inner.is_err() || !md.take().is_empty() {
            return Err(format!("an inner end_op returned {inner:?} or wrote to the device"));
        }
        Ok(log::end_op())
    });
    match r {
        Err(why) => return bad.push(why),
        Ok(Ok(())) => bad.push(String::from("a transaction whose inner operation aborted: the outer end_op() returned Ok")),
        Ok(Err(_)) => {}
    }
    if !md.take().is_empty() {
        bad.push(String::from("a transaction whose inner operation aborted wrote to the device"));
    }
    for (n, v) in [(7u64, 0x17u8), (9, 0x29)] {
        match get(n) {
            Ok(a) if a.data()[0] != v => bad.push(format!("after the aborted transaction, block {n} in the cache holds {:#04x}, and the device {v:#04x}", a.data()[0])),
            Err(why) => bad.push(why),
            _ => {}
        }
    }
}

fn control_recover(bad: &mut Vec<String>) {
    let md = Arc::new(MemDisk::new());
    md.seed(C_HDR, header_bytes(&[20, 21]));
    md.seed(41, filled(0x50));
    md.seed(42, filled(0x51));
    md.seed(20, filled(0x20));
    md.seed(21, filled(0x21));
    let dev: Arc<dyn BlockDevice> = md.clone();
    let cache = Cache::new(dev.clone(), 8);
    // Block 20 cached before the recovery: it must say the replay too.
    let held = match cache.get(20) {
        Ok(b) => b,
        Err(e) => return bad.push(format!("the control's own Cache::get(20) returned {}", errno_text(e))),
    };
    let lg = Log::new(dev.clone(), C_HDR, c_slots());
    match quiet(|| lg.recover(&cache)) {
        Ok(2) => {}
        Ok(n) => bad.push(format!("recover() of a two-block header answered {n}")),
        Err(e) => bad.push(format!("recover() of a two-block header returned {}", errno_text(e))),
    }
    let mut ws: Vec<u64> = md.take().iter().map(|w| w.0).collect();
    if ws.last() != Some(&C_HDR) {
        bad.push(format!("recovery's writes were to {ws:?}: the header was not written last"));
    }
    ws.sort();
    if ws != [20, 21, C_HDR] {
        bad.push(format!("recovery wrote blocks {ws:?}, expected 20, 21 and the header {C_HDR}"));
    }
    for (n, v) in [(20u64, 0x50u8), (21, 0x51)] {
        if md.block(n)[..] != filled(v)[..] {
            bad.push(format!("after recovery, block {n} on the device does not hold slot {}'s bytes", n - 20));
        }
    }
    if md.block(C_HDR)[..] != header_bytes(&[])[..] {
        bad.push(String::from("after recovery, the header is not MAGIC, n = 0, zeros"));
    }
    if held.data()[0] != 0x50 {
        bad.push(format!("block 20, held in the cache across the recovery, holds {:#04x}, and the device 0x50", held.data()[0]));
    }
    drop(held);
    match quiet(|| lg.recover(&cache)) {
        Ok(0) => {}
        other => bad.push(format!("a second recover() answered {other:?}, expected Ok(0)")),
    }
    if !md.take().is_empty() {
        bad.push(String::from("a second recover() (n = 0) wrote to the device"));
    }
    let mut wrong = header_bytes(&[20]);
    wrong[0] ^= 1;
    md.seed(C_HDR, wrong);
    match quiet(|| lg.recover(&cache)) {
        Err(e) if e == EINVAL => {}
        other => bad.push(format!("recover() of a header without the magic answered {other:?}, expected -EINVAL")),
    }
    if !md.take().is_empty() {
        bad.push(String::from("recover() of a header without the magic wrote to the device"));
    }
}

// ── The disk ─────────────────────────────────────────────────────────────

/// The log as this file reads it: the header's fs block, the slots'.
struct LogMap {
    header: u64,
    slots: Vec<u64>,
    all: BTreeSet<u64>,
}

fn log_map(raw: &Raw) -> Result<LogMap, String> {
    let ino = raw.walk("/.sboot-log")?.ok_or_else(|| String::from("the root holds no /.sboot-log"))?;
    let i = raw.inode(ino)?;
    let header = raw.bmap(&i, 0)?;
    let slots: Vec<u64> = (1..=LOGSIZE as u64).map(|k| raw.bmap(&i, k)).collect::<Result<_, _>>()?;
    let (data, meta) = raw.blocks_of(&i)?;
    Ok(LogMap { header, slots, all: data.into_iter().chain(meta).collect() })
}

/// The learner's file system on partition 2, under `Wrap`.
fn mount_fs(wrap: &Arc<Wrap>) -> Result<Arc<Ext2>, String> {
    let p = quiet(|| ata::partition2(wrap.clone())).map_err(|e| format!("partition2(drive) returned {}", errno_text(e)))?;
    Ok(Arc::new(Ext2::mount(p).map_err(|e| format!("Ext2::mount returned {}", errno_text(e)))?))
}

fn read_whole(ino: &InodeRef) -> Result<Vec<u8>, Errno> {
    let mut out = Vec::new();
    let mut buf = alloc::vec![0u8; 4096];
    loop {
        let n = ino.read_at(out.len() as u64, &mut buf)?;
        if n == 0 {
            return Ok(out);
        }
        out.extend_from_slice(&buf[..n]);
    }
}

/// `/tmp/<name>` through the learner's mount: its bytes, or the errno.
fn learner_read(fs: &Arc<Ext2>, name: &str) -> Result<Vec<u8>, Errno> {
    quiet(|| read_whole(&fs.root()?.lookup("tmp")?.lookup(name)?))
}

/// The operation: one transaction of `create` and `write_at`. `twice`
/// makes it a failing one (a second `create` of the name, `-EEXIST`).
/// The answer is `end_op`'s, or why the operation itself went wrong.
fn the_op(fs: &Arc<Ext2>, name: &str, bytes: &[u8], twice: bool) -> Result<Result<(), Errno>, String> {
    the_op_gone(fs, name, bytes, twice, None)
}

/// `the_op`, and inside the same transaction, after the `write_at`, an
/// `unlink` of `/tmp/<gone>` (a file with a data block): the transaction
/// frees an inode and a block as well as taking them (rows 2–6).
fn the_op_gone(fs: &Arc<Ext2>, name: &str, bytes: &[u8], twice: bool, gone: Option<&str>) -> Result<Result<(), Errno>, String> {
    quiet(|| {
        log::begin_op();
        let work = (|| -> Result<Option<Errno>, String> {
            let tmp = fs.root().and_then(|r| r.lookup("tmp")).map_err(|e| format!("/tmp through the learner's mount: {}", errno_text(e)))?;
            let f = tmp.create(name, 0o644).map_err(|e| format!("create(/tmp/{name}) returned {}", errno_text(e)))?;
            let n = f.write_at(0, bytes).map_err(|e| format!("write_at of {} bytes returned {}", bytes.len(), errno_text(e)))?;
            if n != bytes.len() {
                return Err(format!("write_at of {} bytes wrote {n}", bytes.len()));
            }
            if let Some(g) = gone {
                tmp.unlink(g).map_err(|e| format!("unlink(/tmp/{g}) returned {}", errno_text(e)))?;
            }
            if twice {
                return match tmp.create(name, 0o644) {
                    Err(e) => Ok(Some(e)),
                    Ok(_) => Err(format!("a second create(/tmp/{name}) succeeded")),
                };
            }
            Ok(None)
        })();
        match work {
            Ok(Some(e)) if e == EEXIST => Ok(log::end_op()),
            Ok(Some(e)) => {
                let _ = log::end_op();
                Err(format!("the second create returned {}, expected -EEXIST", errno_text(e)))
            }
            Ok(None) => Ok(log::end_op()),
            Err(why) => {
                log::abort_op();
                Err(why)
            }
        }
    })
}

/// The disk's header block, raw: (magic, n, the n homes, the rest zero).
fn raw_header(lm: &LogMap) -> Result<(u64, u32, Vec<u64>, bool), String> {
    let b = raw_block(lm.header)?;
    let n = le32(&b[..], 8) as u32;
    let k = (n as usize).min(LOGSIZE);
    let homes = (0..k).map(|i| le32(&b[..], 12 + 4 * i)).collect();
    let zeros = b[12 + 4 * k..].iter().all(|&x| x == 0);
    Ok((le64(&b[..], 0), n, homes, zeros))
}

fn header_text(h: &(u64, u32, Vec<u64>, bool)) -> String {
    format!("magic {:#x}, n {}, homes {:?}{}", h.0, h.1, h.2, if h.3 { "" } else { ", non-zero bytes after them" })
}

/// One remount: every structure of the old mount dropped, a new one.
fn remount(wrap: &Arc<Wrap>, fs: Arc<Ext2>) -> Result<Arc<Ext2>, String> {
    let old = Arc::downgrade(&fs);
    drop(fs);
    if old.upgrade().is_some() {
        return Err(String::from("the first Ext2 is still held"));
    }
    mount_fs(wrap)
}

// ── Row 1 ────────────────────────────────────────────────────────────────

/// Rows 1 and 8's controls, before any mount (they attach the log to a
/// memory device): what is wrong with how the log answered, for each row.
fn controls() -> (Vec<String>, Vec<String>) {
    let (mut bad, mut abad) = (Vec::new(), Vec::new());
    control_commit_abort(&mut bad, &mut abad);
    control_recover(&mut bad);
    (bad, abad)
}

fn shape_row(raw: &Raw, lm: &LogMap, wrap: &Arc<Wrap>, fs: Arc<Ext2>, bad: Vec<String>) -> Result<Arc<Ext2>, String> {
    let mut r = Row::new(SHAPE_ID);
    for why in &bad {
        r.fail(format_args!("{SHAPE_ID}: control: {why}"));
    }
    let res = shape_disk(&mut r, raw, lm, wrap, fs);
    if let Err(why) = &res {
        r.fail(format_args!("{SHAPE_ID}: {why}"));
    }
    r.finish(1);
    res
}

fn shape_disk(r: &mut Row, raw: &Raw, lm: &LogMap, wrap: &Arc<Wrap>, fs: Arc<Ext2>) -> Result<Arc<Ext2>, String> {
    let r0 = wrap.refused_writes();
    let before = tree(raw)?;
    // The commit, then the machine stops: every write after the header.
    let name = "hdr";
    let bytes = mix(MAGIC, 3000);
    wrap.take();
    wrap.stop_after_commit.store(true, Ordering::Relaxed);
    let res = the_op(&fs, name, &bytes, false);
    wrap.stop_after_commit.store(false, Ordering::Relaxed);
    let stopped = wrap.stopped.swap(false, Ordering::Relaxed);
    let ws = wrap.take();
    match &res {
        Err(why) => r.fail(format_args!("{SHAPE_ID}: the operation: {why}")),
        Ok(Ok(())) if stopped => r.fail(format_args!("{SHAPE_ID}: the install was refused, and end_op() returned Ok")),
        _ => {}
    }
    if !stopped {
        r.fail(format_args!("{SHAPE_ID}: no write to the header block {} carried n > 0: the transaction never committed through the log ({} writes)", lm.header, ws.len()));
    }
    let at = ws.iter().position(|w| w.header_n.is_some_and(|n| n > 0)).unwrap_or(ws.len());
    let h = raw_header(lm)?;
    let homes: BTreeSet<u64> = h.2.iter().copied().collect();
    if h.0 != MAGIC || h.1 == 0 || h.1 as usize > MAXOPBLOCKS || homes.len() != h.1 as usize || !h.3 || homes.iter().any(|b| *b >= raw.blocks || lm.all.contains(b)) {
        r.fail(format_args!("{SHAPE_ID}: the header after the commit: {}; expected magic {MAGIC:#x}, 1 ≤ n ≤ {MAXOPBLOCKS} distinct blocks of the file system outside the log, zeros after", header_text(&h)));
    }
    let slots: Vec<Box<[u8; BLOCK_SIZE]>> = (0..h.2.len().min(LOGSIZE)).map(|i| raw_block(lm.slots[i])).collect::<Result<_, _>>()?;
    let still = judge(raw, &before, name, &bytes);
    if still != Ok(false) {
        r.fail(format_args!("{SHAPE_ID}: stopped right after the commit, the tree is {}, expected before (nothing goes home before the header)", state_text(&still)));
    }
    // Remount: recovery installs.
    let fs = remount(wrap, fs).map_err(|why| format!("the remount: {why}"))?;
    let after = judge(raw, &before, name, &bytes);
    if after != Ok(true) {
        r.fail(format_args!("{SHAPE_ID}: after the remount, the tree is {}, expected after", state_text(&after)));
    }
    for (i, home) in h.2.iter().enumerate().take(slots.len()) {
        if raw_block(*home)?[..] != slots[i][..] {
            r.fail(format_args!("{SHAPE_ID}: after the remount, block {home} does not hold slot {i}'s bytes"));
        }
    }
    let h2 = raw_header(lm)?;
    if h2.0 != MAGIC || h2.1 != 0 || !h2.3 {
        r.fail(format_args!("{SHAPE_ID}: the header after recovery: {}; expected magic {MAGIC:#x}, n 0, zeros", header_text(&h2)));
    }
    // Ordered: the file's data block reached the disk before the header.
    if let Ok(t) = tree(raw) {
        if let Some(e) = t.get(&format!("/tmp/{name}")) {
            let data = e.blocks.first().copied().unwrap_or(0);
            if !ws[..at].iter().any(|w| w.block == data && w.fate == Fate::Landed) {
                r.fail(format_args!("{SHAPE_ID}: the file's data block {data} did not reach the disk before the commit header"));
            }
        }
    }
    match learner_read(&fs, name) {
        Ok(got) => r.expect_bytes(&format!("/tmp/{name} through the remount"), &got, &bytes),
        Err(e) => r.fail(format_args!("{SHAPE_ID}: /tmp/{name} through the remount: {}", errno_text(e))),
    }
    let writes = |ws: &[Wr]| ws.iter().map(|w| if w.header_n.is_some() { format!("hdr(n={})", w.header_n.unwrap()) } else { format!("{}", w.block) }).collect::<Vec<_>>().join(" ");
    note(format_args!("{SHAPE_ID}: the commit wrote {} (the header was write {}); {}", writes(&ws[..(at + 1).min(ws.len())]), at + 1, header_text(&h)));
    if wrap.refused_writes() != r0 {
        r.fail(format_args!("{SHAPE_ID}: refused_writes() moved by {} across this row, whose refusals were this file's own (the hook disarmed)", wrap.refused_writes().wrapping_sub(r0)));
    }
    Ok(fs)
}

// ── Rows 2–6 ─────────────────────────────────────────────────────────────

/// Row 7's inputs, per k: writes passed down while armed, refused by the
/// drive, and `refused_writes()`'s move.
struct Count {
    k: u32,
    calls: u64,
    refused: u64,
    moved: u64,
}

/// Row `KS[n]`'s file to unlink: its name and bytes, derived from the nonce
/// and the row id.
fn victim_of(n: usize) -> (String, Vec<u8>) {
    let d = ktap::derive(&ktap::nonce().unwrap_or([b'0'; 32]), K_IDS[n]);
    (format!("v{}-{:012x}", KS[n], (d >> 16) & 0xffff_ffff_ffff), mix(!d, 1 + ((d >> 40) % 4096) as usize))
}

/// The files rows 2–6 unlink, one per row, each made by one operation (no
/// hook, nothing refused) after the learner's mount and before row 1 — so
/// no commit of the vehicle's own lands between row 1 and the rows 2–6 run.
fn victims(fs: &Arc<Ext2>) -> Vec<Result<(String, Vec<u8>), String>> {
    (0..KS.len())
        .map(|n| {
            let (name, bytes) = victim_of(n);
            match the_op(fs, &name, &bytes, false) {
                Ok(Ok(())) => Ok((name, bytes)),
                Ok(Err(e)) => Err(format!("making /tmp/{name} before row 1: end_op() returned {}", errno_text(e))),
                Err(why) => Err(format!("making /tmp/{name} before row 1: {why}")),
            }
        })
        .collect()
}

fn k_row(n: usize, raw: &Raw, lm: &LogMap, wrap: &Arc<Wrap>, fs: Arc<Ext2>, counts: &mut Vec<Count>, victim: &Result<(String, Vec<u8>), String>) -> Result<Arc<Ext2>, String> {
    let id = K_IDS[n];
    let mut r = Row::new(id);
    let res = k_run(&mut r, n, raw, lm, wrap, fs, counts, victim);
    if let Err(why) = &res {
        r.fail(format_args!("{id}: {why}"));
    }
    r.finish(n + 2);
    res
}

fn k_run(r: &mut Row, n: usize, raw: &Raw, lm: &LogMap, wrap: &Arc<Wrap>, fs: Arc<Ext2>, counts: &mut Vec<Count>, victim: &Result<(String, Vec<u8>), String>) -> Result<Arc<Ext2>, String> {
    let (k, id) = (KS[n], K_IDS[n]);
    let d = r.derived(id);
    let name = format!("k{k}-{:012x}", d & 0xffff_ffff_ffff);
    let bytes = mix(d, 1 + ((d >> 20) % 4096) as usize);
    // The file the operation unlinks, made before row 1 (`victims`).
    let (gone, gbytes) = match victim {
        Ok(v) => v.clone(),
        Err(why) => {
            r.fail(format_args!("{id}: {why}"));
            return Ok(fs);
        }
    };
    let before = tree(raw)?;
    wrap.take();
    let r0 = wrap.refused_writes();
    wrap.armed_calls.store(0, Ordering::Relaxed);
    wrap.armed_refused.store(0, Ordering::Relaxed);
    wrap.armed.store(true, Ordering::Relaxed);
    wrap.arm_fail_after(k);
    let res = the_op_gone(&fs, &name, &bytes, false, Some(&gone));
    wrap.disarm();
    wrap.armed.store(false, Ordering::Relaxed);
    let ws = wrap.take();
    let c = Count { k, calls: wrap.armed_calls.load(Ordering::Relaxed), refused: wrap.armed_refused.load(Ordering::Relaxed), moved: wrap.refused_writes().wrapping_sub(r0) as u64 };
    let committed = ws.iter().any(|w| w.fate == Fate::Landed && w.header_n.is_some_and(|n| n > 0));
    let first = ws.iter().position(|w| w.fate != Fate::Landed);
    match &res {
        Err(why) => r.fail(format_args!("{id}: the operation: {why}")),
        Ok(Ok(())) if first.is_some() => r.fail(format_args!("{id}: write {} was refused, and end_op() returned Ok", first.unwrap() + 1)),
        _ => {}
    }
    if first.is_none() {
        r.fail(format_args!("{id}: arm_fail_after({k}), and none of the operation's {} writes was refused: it does not land inside the operation", ws.len()));
    }
    let crash = judge_gone(raw, &before, &name, &bytes, Some(&gone));
    counts.push(c);
    let fs = remount(wrap, fs).map_err(|why| format!("the remount: {why}"))?;
    let st = judge_gone(raw, &before, &name, &bytes, Some(&gone));
    let want = if committed { Ok(true) } else { Ok(false) };
    if st != want {
        r.fail(format_args!(
            "{id}: after the remount the tree is {}, expected {} (a header with n > 0 {} the disk)",
            state_text(&st),
            if committed { "after" } else { "before" },
            if committed { "reached" } else { "never reached" }
        ));
    }
    let h = raw_header(lm)?;
    if h.0 != MAGIC || h.1 != 0 || !h.3 {
        r.fail(format_args!("{id}: the header after the remount: {}; expected magic {MAGIC:#x}, n 0, zeros", header_text(&h)));
    }
    match (learner_read(&fs, &name), committed) {
        (Ok(got), true) => r.expect_bytes(&format!("/tmp/{name} through the remount"), &got, &bytes),
        (Err(e), false) if e == ENOENT => {}
        (got, _) => r.fail(format_args!(
            "{id}: /tmp/{name} through the learner's remount: {}, expected {}",
            got.map(|v| format!("{} bytes", v.len())).unwrap_or_else(errno_text),
            if committed { "its bytes" } else { "-ENOENT" }
        )),
    }
    match (learner_read(&fs, &gone), committed) {
        (Err(e), true) if e == ENOENT => {}
        (Ok(got), false) => r.expect_bytes(&format!("/tmp/{gone} through the remount"), &got, &gbytes),
        (got, _) => r.fail(format_args!(
            "{id}: /tmp/{gone} (unlinked by the operation) through the learner's remount: {}, expected {}",
            got.map(|v| format!("{} bytes", v.len())).unwrap_or_else(errno_text),
            if committed { "-ENOENT" } else { "its bytes" }
        )),
    }
    let hdr = match ws.iter().position(|w| w.header_n.is_some_and(|n| n > 0)) {
        Some(i) => format!("write {}", i + 1),
        None => String::from("not reached"),
    };
    let kind = |w: &Wr| -> String {
        if w.header_n.is_some() {
            format!("the header (n={})", w.header_n.unwrap())
        } else if let Some(i) = lm.slots.iter().position(|&s| s == w.block) {
            format!("slot {i}")
        } else {
            format!("block {}", w.block)
        }
    };
    note(format_args!(
        "{id}: /tmp/{name}, {} bytes, /tmp/{gone} unlinked; {} writes, the commit header is {hdr}, the first refused is write {} ({}); stopped there the tree was {}; after the remount {}",
        bytes.len(),
        ws.len(),
        first.map(|i| i + 1).unwrap_or(0),
        first.map(|i| kind(&ws[i])).unwrap_or_default(),
        state_text(&crash).split(' ').next().unwrap_or(""),
        state_text(&st)
    ));
    Ok(fs)
}

// ── Row 7 ────────────────────────────────────────────────────────────────

fn count_row(counts: &[Count]) {
    let mut r = Row::new(COUNT_ID);
    if counts.len() != KS.len() {
        r.fail(format_args!("{COUNT_ID}: {} of the {} k runs finished", counts.len(), KS.len()));
    }
    for c in counts {
        let want = c.calls.saturating_sub(c.k as u64);
        if c.refused != want || want == 0 {
            r.fail(format_args!("{COUNT_ID}: k = {}: the drive refused {} of the {} writes after arm_fail_after({}), expected {want} (every one after the first {})", c.k, c.refused, c.calls, c.k, c.k));
        }
        if c.moved != c.refused {
            r.fail(format_args!("{COUNT_ID}: k = {}: refused_writes() moved by {}, and this file counted {} refused writes", c.k, c.moved, c.refused));
        }
    }
    note(format_args!(
        "{COUNT_ID}: (k, writes, refused, refused_writes() moved): {}",
        counts.iter().map(|c| format!("({}, {}, {}, {})", c.k, c.calls, c.refused, c.moved)).collect::<Vec<_>>().join(" ")
    ));
    r.finish(7);
}

// ── Row 8 ────────────────────────────────────────────────────────────────

fn abort_row(raw: &Raw, wrap: &Arc<Wrap>, fs: Arc<Ext2>, bad: Vec<String>) -> Result<Arc<Ext2>, String> {
    let mut r = Row::new(ABORT_ID);
    for why in &bad {
        r.fail(format_args!("{ABORT_ID}: control: {why}"));
    }
    let res = abort_disk(&mut r, raw, wrap, fs);
    *wrap.refuse_free.lock() = None;
    if let Err(why) = &res {
        r.fail(format_args!("{ABORT_ID}: {why}"));
    }
    r.finish(8);
    res
}

fn abort_disk(r: &mut Row, raw: &Raw, wrap: &Arc<Wrap>, fs: Arc<Ext2>) -> Result<Arc<Ext2>, String> {
    let r0 = wrap.refused_writes();
    // One operation that fails part-way: a cold cache, then a write that
    // needs a new block, whose first read (a free block) the drive refuses.
    let fs = remount(wrap, fs).map_err(|why| format!("the remount before the case: {why}"))?;
    let was = learner_read(&fs, "hdr").map_err(|e| format!("/tmp/hdr through the learner's mount: {}", errno_text(e)))?;
    let before = tree(raw)?;
    let used = scan(raw)?.used;
    let grow = mix(MAGIC + 2, 100);
    let off = (was.len() as u64).div_ceil(BLOCK_SIZE as u64).max(1) * BLOCK_SIZE as u64;
    wrap.take();
    *wrap.refused_read.lock() = None;
    *wrap.refuse_free.lock() = Some(used);
    let res = quiet(|| fs.root().and_then(|d| d.lookup("tmp")).and_then(|d| d.lookup("hdr")).and_then(|f| f.write_at(off, &grow)));
    *wrap.refuse_free.lock() = None;
    let refused = wrap.refused_read.lock().take();
    let ws = wrap.take();
    match (refused, res) {
        (None, res) => r.fail(format_args!("{ABORT_ID}: write_at({off}, {} bytes) on /tmp/hdr read no free block (it answered {}): the operation could not be made to fail part-way", grow.len(), res.map(|n| format!("{n}")).unwrap_or_else(errno_text))),
        (Some(b), Ok(n)) => r.fail(format_args!("{ABORT_ID}: the read of free block {b} was refused, and write_at({off}, {} bytes) on /tmp/hdr returned {n}, expected an error", grow.len())),
        (Some(_), Err(_)) => {}
    }
    if !ws.is_empty() {
        r.fail(format_args!("{ABORT_ID}: a write_at that failed part-way wrote {} blocks to the drive, first {}", ws.len(), ws[0].block));
    }
    let st = judge(raw, &before, "hdr", &was);
    if st != Ok(false) {
        r.fail(format_args!("{ABORT_ID}: after the write_at that failed part-way, the tree is {}, expected before", state_text(&st)));
    }
    match learner_read(&fs, "hdr") {
        Ok(got) => r.expect_bytes("/tmp/hdr through the learner's mount after the failed write_at", &got, &was),
        Err(e) => r.fail(format_args!("{ABORT_ID}: /tmp/hdr through the learner's mount after the failed write_at: {}", errno_text(e))),
    }
    note(format_args!("{ABORT_ID}: write_at({off}, {} bytes) on /tmp/hdr: the read of free block {} refused", grow.len(), refused.map(|b| format!("{b}")).unwrap_or_else(|| String::from("-"))));
    // A transaction of the vehicle's whose second create fails.
    let before = tree(raw)?;
    let bytes = mix(MAGIC + 1, 100);
    wrap.take();
    let res = the_op(&fs, "abort", &bytes, true);
    let ws = wrap.take();
    match res {
        Err(why) => r.fail(format_args!("{ABORT_ID}: the aborted transaction: {why}")),
        Ok(Ok(())) => r.fail(format_args!("{ABORT_ID}: a transaction whose create failed with -EEXIST: end_op() returned Ok, expected an error")),
        Ok(Err(_)) => {}
    }
    if !ws.is_empty() {
        r.fail(format_args!("{ABORT_ID}: a transaction whose create failed with -EEXIST wrote {} blocks to the drive, first {}", ws.len(), ws[0].block));
    }
    let st = judge(raw, &before, "abort", &bytes);
    if st != Ok(false) {
        r.fail(format_args!("{ABORT_ID}: after the aborted transaction the tree is {}, expected before", state_text(&st)));
    }
    match learner_read(&fs, "abort") {
        Err(e) if e == ENOENT => {}
        other => r.fail(format_args!("{ABORT_ID}: after the aborted transaction, /tmp/abort through the learner's mount: {}, expected -ENOENT", other.map(|v| format!("{} bytes", v.len())).unwrap_or_else(errno_text))),
    }
    if wrap.refused_writes() != r0 {
        r.fail(format_args!("{ABORT_ID}: refused_writes() moved by {} across this row (the hook disarmed)", wrap.refused_writes().wrapping_sub(r0)));
    }
    Ok(fs)
}

/// Every row from `from` (1-based) on, failed for `why`.
fn fail_rest(from: usize, why: &str) {
    let ids: Vec<&str> = core::iter::once(SHAPE_ID).chain(K_IDS).chain([COUNT_ID, ABORT_ID]).collect();
    for (k, id) in ids.into_iter().enumerate().skip(from - 1) {
        let mut r = Row::new(id);
        r.fail(format_args!("{id}: {why}"));
        r.finish(k + 1);
    }
}

fn stop(from: usize, why: &str) -> ! {
    fail_rest(from, why);
    quiet(ktap::end);
    halt()
}

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    serial::init();
    ktap::begin(PLAN);
    prelude();
    let mut c = Clock::new();
    let raw = Raw::open().unwrap_or_else(|why| stop(1, &format!("disk: {why}")));
    let lm = log_map(&raw).unwrap_or_else(|why| stop(1, &format!("the log: {why}")));
    let drive: Arc<dyn BlockDevice> = match quiet(Ata::probe) {
        Ok(a) => Arc::new(a),
        Err(e) => stop(1, &format!("Ata::probe() returned {}: no drive", errno_text(e))),
    };
    let wrap = Arc::new(Wrap {
        inner: drive,
        header: lm.header,
        armed: AtomicBool::new(false),
        armed_calls: AtomicU64::new(0),
        armed_refused: AtomicU64::new(0),
        stop_after_commit: AtomicBool::new(false),
        stopped: AtomicBool::new(false),
        record: SpinLock::new(Vec::new()),
        refuse_free: SpinLock::new(None),
        refused_read: SpinLock::new(None),
    });
    // The disk as written must agree with itself before anything is judged.
    match scan(&raw).and_then(|s| agree(&raw, &s)) {
        Ok(v) if v.is_empty() => {}
        Ok(v) => stop(1, &format!("this file's check of the disk as written disagrees (the vehicle is wrong): {}", v.join("; "))),
        Err(why) => stop(1, &why),
    }
    // Rows 1 and 8's controls, before the learner's mount.
    let (bad, abad) = controls();
    let fs = mount_fs(&wrap).unwrap_or_else(|why| stop(1, &why));
    let vs = victims(&fs);
    let mut fs = shape_row(&raw, &lm, &wrap, fs, bad).unwrap_or_else(|why| stop(2, &why));
    let mut counts = Vec::new();
    for n in 0..KS.len() {
        fs = k_row(n, &raw, &lm, &wrap, fs, &mut counts, &vs[n]).unwrap_or_else(|why| stop(n + 3, &why));
    }
    count_row(&counts);
    let fs = abort_row(&raw, &wrap, fs, abad).unwrap_or_else(|_| stop(9, ""));
    drop(fs);
    note(format_args!("the rows took {} ms", c.ms()));
    sched::yield_now();
    quiet(ktap::end);
    halt()
}
// ── The machine ──────────────────────────────────────────────────────────

/// CR0.EM = 0, CR0.TS = 0, CR0.MP = 1; CR4.OSFXSR = 1, CR4.OSXMMEXCPT = 1;
/// `fninit`; MXCSR = 0x1F80 — lab 15's prelude, unchanged.
fn sse_on() {
    let mxcsr: u32 = 0x1F80;
    // SAFETY: these bits change only how x87/SSE instructions behave; the
    // kernel is soft-float and uses neither.
    unsafe {
        asm!(
            "mov {t}, cr0",
            "and {t}, {clear0}",
            "or {t}, 2",
            "mov cr0, {t}",
            "mov {t}, cr4",
            "or {t}, {set4}",
            "mov cr4, {t}",
            "fninit",
            "ldmxcsr [{m}]",
            t = out(reg) _,
            clear0 = in(reg) !((1u64 << 2) | (1 << 3)),
            set4 = const (1u64 << 9) | (1 << 10),
            m = in(reg) &mxcsr,
            options(nostack),
        )
    };
}

fn prelude() {
    let (k, i, _) = shim::stacks();
    gdt::init(k, i);
    idt::init();
    let bi = bootinfo::get().expect("the shim published the BootInfo");
    frame::init(bi.memmap(), bi.kernel_range());
    syscall::init();
    syscall::set_kernel_stack(k);
    heap::init(VirtAddr(HEAP_START), HEAP_PAGES);
    sse_on();
    proc::init();
    sched::init(k);
    sched::yield_now();
    irq::init();
    let t0 = ticks();
    let mut c = Clock::new();
    while ticks() < t0 + 2 && c.ms() < 100 {
        core::hint::spin_loop();
    }
}

