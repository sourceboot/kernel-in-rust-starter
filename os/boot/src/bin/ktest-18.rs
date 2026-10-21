//! Lab 18's vehicle: ext2's write half — allocation, the block map on the
//! write path, `create`, `write_at` — and the log's interface (`Txn`,
//! interface 16) as a pass-through. The prelude is lab 17's. Re-cut from
//! the old lab 18's vehicle on 2026-10-19 (slice 5a): its delete, remount
//! and directory rows went to labs 19 and 20.
//!
//! The disk is `ktest-18.img` as the course's image writer made it (CONTRACT
//! §A.7). From this lab the guest writes it, so the course boots a copy. As
//! at 17, every expectation comes from this file's formulas or from its own
//! reads of the disk — its own PIO driver (never the learner's `Ata`) and
//! its own decoder of the superblock, the descriptor, both bitmaps, the
//! inode table, the block maps and the directory records. Nothing the
//! learner's ext2 reports is used as an expectation.
//!
//! The drive sits under `Spy` (`../txnspy.rs`, shared with 19 and 20) between the
//! learner's `Ata` and `partition2`: it counts the reads that reach the
//! drive and records every write, with whether the block was in
//! `log::logged()` — the open operation's blocks — at that moment. `/` is a
//! directory of this file's that forwards every call to the root of the
//! learner's `Ext2::mount(partition2(spy))`; `/boot` is the boot archive,
//! `/ktest` this file's programs. Row 1 is programs this file assembles
//! (lab 17's probes), started with the learner's `exec::spawn`, so every
//! number it checks went through the learner's syscalls from ring 3. Four
//! rows, in this order:
//!
//!   1 `ext2.create.write.readback` (derived) — `/tmp/w-<n>`, its size from
//!     the nonce past 12 blocks (into the indirect block), its bytes
//!     splitmix64 of the nonce: `open(O_WRONLY|O_CREAT|O_EXCL, 0644)`, two
//!     `write`s, a `read` on it (`-EBADF`); `O_EXCL` again (`-EEXIST`);
//!     `O_APPEND` adds 7 bytes at the end; read back whole on an
//!     `O_RDONLY` fd (a `write` there `-EBADF`). A second file written,
//!     then 3 of its bytes written over in place (its size kept); a third
//!     made by `O_CREAT|O_TRUNC` (a file the call makes has nothing to
//!     cut); a fourth by `openat(AT_FDCWD, "tmp/…", O_CREAT|O_RDWR)`; a
//!     fifth written 100 bytes at a nonce-chosen offset past file block
//!     1036 (through the double indirect block), read back, its hole read
//!     as zeros. Refused: `/.sboot-log` for writing (`-EPERM`), `/tmp` for
//!     writing (`-EISDIR`), the archive for writing and creating
//!     (`-EROFS`), a missing directory (`-ENOENT`); `Inode::create` itself
//!     on `w-<n>`'s name (`-EEXIST`, the inode bitmap and the free counts
//!     on the disk unchanged; added 2026-10-19, slice 5a). Then this file's
//!     reader: each inode's mode, links and size, the big one's indirect
//!     pointer set, the sparse one's double indirect pointer and `i_blocks`
//!     (3 blocks), every file's bytes on the disk.
//!   2 `txn.every.metadata.write` (plain) — a positive control on the log
//!     (`txnspy.rs`'s `log_control`, before the mount: a cache over a memory device;
//!     `logged()` lists 7 and 9 once each, the end of the operation writes
//!     both while they are listed, and a write after it is seen unlisted),
//!     then: every write since the mount to the superblock, the
//!     descriptor, the bitmaps or the inode table reached the disk while
//!     its block was listed; and five operations on the learner's `Inode`
//!     methods — `create`, `write_at` of 13 blocks (direct and indirect),
//!     `write_at` past file block 1036 (the double indirect), then a pointer
//!     added to each pointer block the file already has: one block at file
//!     block 13 (the indirect block), two from file block 2059 (the first
//!     middle block, then the double indirect block itself; added
//!     2026-10-19, fix round 1: every pointer block of the first three
//!     operations was new, logged by its allocation in the same operation,
//!     so a `set_pointer` that skipped the log scored full) — each between
//!     two full reads of the tree by this file, where every write to a
//!     directory or pointer block was listed too. Each of the seven kinds
//!     must have been written once. It is the learner's own `logged()`
//!     that says what was listed: a self-report. Lab 21's recovery rows
//!     sweep create + a one-block write_at + unlink, so they re-grade the
//!     create and allocation writes, never the pointer blocks (this row
//!     alone; CONTRACT §B.2-18, corrected 2026-10-19).
//!   3 `ext2.alloc.maps.agree` (plain) — after rows 1 and 2's writes, this
//!     file's walk of the whole tree from the root against the disk: every
//!     bit of both bitmaps, the superblock's and the descriptor's free
//!     counts and the directory count, every link count and `i_blocks`, no
//!     block claimed twice. The same check on the disk before the mount is
//!     this file's control.
//!   4 `ext2.write.enospc` (plain) — a second mount of the partition by
//!     the learner's `Ext2::mount`, over a device of this file's that keeps
//!     every write in memory (the disk is never written through it) and
//!     reads the block bitmap as all set and both free block counts as 0:
//!     a disk with no free block. Row 1's second file (5000 bytes): a
//!     `write_at` past its end is `-ENOSPC` and its size stays 5000 (the
//!     learner's `stat` and the inode as this mount wrote it, decoded here);
//!     a `write_at` from 5 bytes before its end, 5000 bytes long, writes
//!     what fits in its last block and answers that count, the size grown
//!     by exactly that. Last, because the mount attaches the log to its own
//!     cache.
//!
//! The course also runs `e2fsck -fn` on the copy's partition 2 after the
//! boot (measured, not graded).

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../shim.rs"]
mod shim;
#[path = "../txnspy.rs"]
mod txnspy;
use txnspy::*;

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

use kernel::addr::VirtAddr;
use kernel::blk::ata::{self, Ata, BlockDevice, BLOCK_SIZE};
use kernel::blk::cache::Cache;
use kernel::console::Fd;
use kernel::exec;
use kernel::fs::archive::Archive;
use kernel::fs::ext2::Ext2;
use kernel::fs::log;
use kernel::fs::vfs::{self, dt, mode, DirEntry, Inode, InodeRef, Stat};
use kernel::ktap;
use kernel::pipe;
use kernel::proc::{self, Pid};
use kernel::sync::SpinLock;
use kernel::syscall::Errno;
use kernel::task::{self, Tid};
use kernel::{bootinfo, frame, gdt, heap, idt, irq, sched, serial, syscall, time};

const ARCHIVE_LEN: usize = include_bytes!("../../../user/bin/boot.archive").len();

#[link_section = ".sboot_archive"]
static ARCHIVE: [u8; ARCHIVE_LEN] = *include_bytes!("../../../user/bin/boot.archive");

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 16;
const PM_HZ: u64 = 3_579_545;

const PLAN: usize = 4;
const CREATE_ID: &str = "ext2.create.write.readback";
const TXN_ID: &str = "txn.every.metadata.write";
const ALLOC_ID: &str = "ext2.alloc.maps.agree";
const ENOSPC_ID: &str = "ext2.write.enospc";

/// The first file block mapped through the double indirect block.
const DIND_FIRST: u64 = 12 + 1024;

/// Partition 2's first 4 KiB block on the disk (§A.7).
const P2_BLOCK0: u64 = 8448;

/// The probes' data segment (lab 17's layout).
const DATA: u64 = 0x0110_0000;
const MAX_STEPS: usize = 64;
const RESULTS: usize = 8 * MAX_STEPS;
/// How long a probe may run.
const RUN_MS: u64 = 20_000;

const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_STAT: u64 = 4;
const SYS_LSEEK: u64 = 8;
const SYS_OPENAT: u64 = 257;
const O_RDONLY: u64 = 0;
const O_WRONLY: u64 = 1;
const O_RDWR: u64 = 2;
const O_CREAT: u64 = 0x40;
const O_EXCL: u64 = 0x80;
const O_TRUNC: u64 = 0x200;
const O_APPEND: u64 = 0x400;
const O_NONBLOCK: u32 = 0x800;
const AT_FDCWD: u64 = -100i64 as u64;
const SEEK_SET: u64 = 0;
const EPERM: i64 = 1;
const ENOENT: i64 = 2;
const EBADF: i64 = 9;
const EEXIST: i64 = 17;
const ENOTDIR: i64 = 20;
const EISDIR: i64 = 21;
const EROFS: i64 = 30;
const EIO: i64 = 5;
const ENOSPC: i64 = 28;

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
fn answer(v: i64) -> String {
    if (-4095..0).contains(&v) {
        errno_text(-v)
    } else {
        format!("{v}")
    }
}

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
    fn expect(&mut self, cond: bool, what: &str) {
        if !cond {
            let id = self.id;
            self.fail(format_args!("{id}: {what}: observed false, expected true"));
        }
    }
    fn expect_eq(&mut self, what: &str, observed: u64, expected: u64) {
        if observed != expected {
            let id = self.id;
            self.fail(format_args!("{id}: {what}: observed {observed}, expected {expected}"));
        }
    }
    /// A syscall's result against the one expected.
    fn expect_res(&mut self, what: &str, observed: i64, expected: i64) {
        if observed != expected {
            let id = self.id;
            self.fail(format_args!("{id}: {what} returned {}, expected {}", answer(observed), answer(expected)));
        }
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

// ── The writer's formulas ────────────────────────────────────────────────

/// `tagged(magic, index)` (§A.7): word 0 = magic, word 1 = index, word k =
/// splitmix64(magic ^ (index * 512 + k)), little-endian.
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

/// Partition 2's fs block `b`, raw.
fn raw_block(b: u64) -> Result<Box<[u8; BLOCK_SIZE]>, String> {
    let mut buf = Box::new([0u8; BLOCK_SIZE]);
    if raw_disk_block(P2_BLOCK0 + b, &mut buf) {
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

// ── The devices and the root of this file's ─────────────────────────────

/// `/`: every call forwarded to the root it holds now.
struct Proxy(SpinLock<Option<InodeRef>>);

impl Proxy {
    fn get(&self) -> Result<InodeRef, Errno> {
        quiet(|| self.0.lock().clone()).ok_or(EIO)
    }
}

impl Inode for Proxy {
    fn lookup(&self, name: &str) -> Result<InodeRef, Errno> {
        self.get()?.lookup(name)
    }
    fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<usize, Errno> {
        self.get()?.read_at(off, buf)
    }
    fn write_at(&self, off: u64, buf: &[u8]) -> Result<usize, Errno> {
        self.get()?.write_at(off, buf)
    }
    fn getdents(&self, index: u64) -> Result<Option<DirEntry>, Errno> {
        self.get()?.getdents(index)
    }
    fn stat(&self) -> Stat {
        self.get().map(|r| r.stat()).unwrap_or_default()
    }
    fn create(&self, name: &str, m: u32) -> Result<InodeRef, Errno> {
        self.get()?.create(name, m)
    }
    fn unlink(&self, name: &str) -> Result<(), Errno> {
        self.get()?.unlink(name)
    }
    fn mkdir(&self, name: &str, m: u32) -> Result<InodeRef, Errno> {
        self.get()?.mkdir(name, m)
    }
    fn rmdir(&self, name: &str) -> Result<(), Errno> {
        self.get()?.rmdir(name)
    }
    fn rename(&self, old: &str, new_dir: &dyn Inode, new: &str) -> Result<(), Errno> {
        self.get()?.rename(old, new_dir, new)
    }
    fn truncate(&self, len: u64) -> Result<(), Errno> {
        self.get()?.truncate(len)
    }
}

/// The learner's file system on partition 2, under the spy.
fn mount_fs(spy: &Arc<Spy>) -> Result<Arc<Ext2>, String> {
    let p = quiet(|| ata::partition2(spy.clone())).map_err(|e| format!("partition2(drive) returned {}", errno_text(e)))?;
    Ok(Arc::new(Ext2::mount(p).map_err(|e| format!("Ext2::mount returned {}", errno_text(e)))?))
}

fn mount_rest(proxy: Arc<Proxy>) {
    if let Err(e) = vfs::mount("/", proxy) {
        note(format_args!("vfs: mount(\"/\", this file's root) returned {}", errno_text(e)));
    }
    match Archive::new(&ARCHIVE) {
        Ok(a) => {
            if let Err(e) = vfs::mount("/boot", Arc::new(a)) {
                note(format_args!("vfs: mount(\"/boot\", the archive) returned {}", errno_text(e)));
            }
        }
        Err(e) => note(format_args!("vfs: Archive::new over the boot archive returned {}", errno_text(e))),
    }
    if let Err(e) = vfs::mount("/ktest", Arc::new(KtestDir)) {
        note(format_args!("vfs: mount(\"/ktest\", this vehicle's directory) returned {}", errno_text(e)));
    }
}

// ── The formulas ─────────────────────────────────────────────────────────

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

static KTEST: SpinLock<Vec<(String, InodeRef)>> = SpinLock::new(Vec::new());

struct KtestDir;

struct MemFile {
    bytes: Vec<u8>,
    ino: u64,
}

impl Inode for MemFile {
    fn lookup(&self, _name: &str) -> Result<InodeRef, Errno> {
        Err(ENOTDIR)
    }
    fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<usize, Errno> {
        let len = self.bytes.len() as u64;
        if off >= len {
            return Ok(0);
        }
        let n = buf.len().min((len - off) as usize);
        buf[..n].copy_from_slice(&self.bytes[off as usize..off as usize + n]);
        Ok(n)
    }
    fn getdents(&self, _index: u64) -> Result<Option<DirEntry>, Errno> {
        Err(ENOTDIR)
    }
    fn stat(&self) -> Stat {
        Stat { ino: self.ino, mode: mode::S_IFREG | 0o555, nlink: 1, size: self.bytes.len() as u64, ..Stat::default() }
    }
}

impl Inode for KtestDir {
    fn lookup(&self, name: &str) -> Result<InodeRef, Errno> {
        quiet(|| KTEST.lock().iter().find(|(n, _)| n == name).map(|(_, f)| f.clone())).ok_or(ENOENT)
    }
    fn read_at(&self, _off: u64, _buf: &mut [u8]) -> Result<usize, Errno> {
        Err(EISDIR)
    }
    fn getdents(&self, index: u64) -> Result<Option<DirEntry>, Errno> {
        Ok(quiet(|| {
            KTEST.lock().get(index as usize).map(|(n, f)| DirEntry { ino: f.stat().ino, kind: dt::DT_REG, name: n.clone() })
        }))
    }
    fn stat(&self) -> Stat {
        Stat { ino: 1, mode: mode::S_IFDIR | 0o555, nlink: 2, ..Stat::default() }
    }
}

/// Serve `bytes` as `/ktest/<name>`; the path.
fn serve(name: &str, bytes: Vec<u8>) -> String {
    quiet(|| {
        let mut t = KTEST.lock();
        let ino = 100 + t.len() as u64;
        t.push((String::from(name), Arc::new(MemFile { bytes, ino })));
    });
    format!("/ktest/{name}")
}

// ── Programs: assembled here, run through the learner's exec ─────────────

/// A syscall argument: an immediate, the result of an earlier step, or the
/// address of a place in the data segment.
#[derive(Clone, Copy)]
enum A {
    I(u64),
    R(usize),
    D(usize),
}

/// `mov <arg reg>, imm64` for rdi, rsi, rdx, r10, r8, r9.
const MOV_IMM: [[u8; 2]; 6] = [[0x48, 0xbf], [0x48, 0xbe], [0x48, 0xba], [0x49, 0xba], [0x49, 0xb8], [0x49, 0xb9]];
/// `mov <arg reg>, [disp32]` (SIB, no base, no index).
const MOV_MEM: [[u8; 4]; 6] = [
    [0x48, 0x8b, 0x3c, 0x25],
    [0x48, 0x8b, 0x34, 0x25],
    [0x48, 0x8b, 0x14, 0x25],
    [0x4c, 0x8b, 0x14, 0x25],
    [0x4c, 0x8b, 0x04, 0x25],
    [0x4c, 0x8b, 0x0c, 0x25],
];

/// A straight run of syscalls; step k's `rax` is stored at `DATA + 8k`.
/// The program ends by writing its whole data segment to fd 1, then
/// `exit(0)`.
struct Probe {
    code: Vec<u8>,
    data: Vec<u8>,
    steps: usize,
}

impl Probe {
    fn new() -> Probe {
        Probe { code: Vec::new(), data: alloc::vec![0; RESULTS], steps: 0 }
    }
    fn str(&mut self, s: &str) -> A {
        let at = self.data.len();
        self.data.extend_from_slice(s.as_bytes());
        self.data.push(0);
        self.data.resize(self.data.len().next_multiple_of(8), 0);
        A::D(at)
    }
    fn buf(&mut self, n: usize) -> (A, usize) {
        let at = self.data.len();
        self.data.resize(at + n.next_multiple_of(8), 0);
        (A::D(at), at)
    }
    /// `b` in the data segment; its offset there.
    fn bytes(&mut self, b: &[u8]) -> usize {
        let at = self.data.len();
        self.data.extend_from_slice(b);
        self.data.resize(self.data.len().next_multiple_of(8), 0);
        at
    }
    fn call(&mut self, nr: u64, args: &[A]) -> usize {
        for (i, a) in args.iter().enumerate() {
            match *a {
                A::I(v) => {
                    self.code.extend_from_slice(&MOV_IMM[i]);
                    self.code.extend_from_slice(&v.to_le_bytes());
                }
                A::D(off) => {
                    self.code.extend_from_slice(&MOV_IMM[i]);
                    self.code.extend_from_slice(&(DATA + off as u64).to_le_bytes());
                }
                A::R(k) => {
                    self.code.extend_from_slice(&MOV_MEM[i]);
                    self.code.extend_from_slice(&((DATA + 8 * k as u64) as u32).to_le_bytes());
                }
            }
        }
        self.code.push(0xb8); // mov eax, imm32
        self.code.extend_from_slice(&(nr as u32).to_le_bytes());
        self.code.extend_from_slice(&[0x0f, 0x05]); // syscall
        let k = self.steps;
        assert!(k < MAX_STEPS, "a probe of more than {MAX_STEPS} steps");
        self.steps += 1;
        self.code.extend_from_slice(&[0x48, 0x89, 0x04, 0x25]); // mov [disp32], rax
        self.code.extend_from_slice(&((DATA + 8 * k as u64) as u32).to_le_bytes());
        k
    }
    /// The ELF: text (R X) at `proc::BASE` from file offset 0, the data
    /// segment (RW) at `DATA` from the next page of the file.
    fn elf(mut self) -> (Vec<u8>, usize) {
        let len = self.data.len();
        self.call(SYS_WRITE, &[A::I(1), A::I(DATA), A::I(len as u64)]);
        // xor edi, edi; mov eax, 60 (exit); syscall; ud2
        self.code.extend_from_slice(&[0x31, 0xff, 0xb8, 60, 0, 0, 0, 0x0f, 0x05, 0x0f, 0x0b]);
        let text = 176 + self.code.len();
        let data_off = text.next_multiple_of(0x1000);
        let mut b = alloc::vec![0u8; data_off + len];
        let mut put = |off: usize, v: u64, n: usize| b[off..off + n].copy_from_slice(&v.to_le_bytes()[..n]);
        put(0, 0x0101_0246_4c45_7f, 7); // \x7fELF, 64-bit, little-endian, version 1
        put(16, 2, 2); // ET_EXEC
        put(18, 0x3e, 2); // x86-64
        put(20, 1, 4);
        put(24, proc::BASE + 176, 8); // e_entry
        put(32, 64, 8); // e_phoff
        put(52, 64, 2);
        put(54, 56, 2);
        put(56, 2, 2); // e_phnum
        put(64, 1, 4); // PT_LOAD: the text
        put(68, 5, 4);
        put(80, proc::BASE, 8);
        put(88, proc::BASE, 8);
        put(96, text as u64, 8);
        put(104, text as u64, 8);
        put(112, 0x1000, 8);
        put(120, 1, 4); // PT_LOAD: the data
        put(124, 6, 4);
        put(128, data_off as u64, 8);
        put(136, DATA, 8);
        put(144, DATA, 8);
        put(152, len as u64, 8);
        put(160, len as u64, 8);
        put(168, 0x1000, 8);
        b[176..text].copy_from_slice(&self.code);
        b[data_off..].copy_from_slice(&self.data);
        (b, len)
    }
}

/// lab 13's `exec-probe` as one R X `PT_LOAD` at `proc::BASE`, the code at
/// offset 120: `execve(argv[1], &argv[2], envp)`; if that returns,
/// `exit(100 + errno)`.
fn task_of(pid: Pid) -> Option<Tid> {
    let mut tid = None;
    proc::each(|p| {
        if p.pid == pid {
            tid = Some(p.task);
        }
    });
    tid
}

fn exit_of(pid: Pid) -> Option<i32> {
    let mut out = None;
    quiet(|| {
        proc::each(|p| {
            if p.pid == pid {
                out = p.exit;
            }
        })
    });
    out
}

fn yield_until(ms: u64, mut done: impl FnMut() -> bool) -> bool {
    let mut c = Clock::new();
    loop {
        if done() {
            return true;
        }
        if c.ms() >= ms {
            return false;
        }
        sched::yield_now();
    }
}

fn status_text(s: Option<i32>) -> String {
    match s {
        None => String::from("still running"),
        Some(v) if v & 0x7f == 0 => format!("exit({})", (v >> 8) & 0xff),
        Some(v) => format!("killed by signal {} (status {v:#x})", v & 0x7f),
    }
}

/// What one program did: everything it wrote to fds 1 and 2, how it ended.
struct Ran {
    out: Vec<u8>,
    exit: Option<i32>,
    ms: u64,
}

/// Start a program with `start` (interrupts off), give it a pipe as fds 1
/// and 2 before it runs, and drain the pipe until its last write end is
/// gone or `RUN_MS` passes.
fn run(what: &str, start: impl FnOnce() -> Result<Pid, Errno>) -> Result<Ran, String> {
    let (r, w) = pipe::open(0).map_err(|e| format!("{what}: pipe::open(0) returned {}", errno_text(e)))?;
    r.flags.store(r.flags.load(Ordering::SeqCst) | O_NONBLOCK, Ordering::SeqCst);
    let started = quiet(|| -> Result<(Pid, Vec<Fd>), (String, Vec<Fd>)> {
        let pid = start().map_err(|e| (format!("{what}: starting it returned {}", errno_text(e)), Vec::new()))?;
        let Some(tid) = task_of(pid) else {
            return Err((format!("{what}: pid {} has no task", pid.0), Vec::new()));
        };
        let ends = [Fd { file: w.clone(), cloexec: false }, Fd { file: w.clone(), cloexec: false }];
        // What the table gives back is dropped after its lock (S4's rule).
        let back: Vec<Fd> = task::with_table(move |tt| {
            let mut back = Vec::new();
            match tt.get_mut(tid) {
                Some(t) => {
                    for (n, d) in [1usize, 2].into_iter().zip(ends) {
                        if let Ok(Some(old)) = t.fds.install(n, d) {
                            back.push(old);
                        }
                    }
                }
                None => back.extend(ends),
            }
            back
        });
        Ok((pid, back))
    });
    drop(w);
    let pid = match started {
        Ok((pid, back)) => {
            drop(back);
            pid
        }
        Err((why, back)) => {
            drop(back);
            return Err(why);
        }
    };
    let mut out = Vec::new();
    let mut buf = alloc::vec![0u8; 4096];
    let mut c = Clock::new();
    let mut ended = false;
    loop {
        match r.read(&mut buf) {
            Ok(0) => {
                ended = true;
                break;
            }
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(11) => {
                if c.ms() >= RUN_MS {
                    break;
                }
                sched::yield_now();
            }
            Err(e) => {
                note(format_args!("{what}: a read of its pipe returned {}", errno_text(e)));
                break;
            }
        }
    }
    let ms = c.ms();
    let _ = yield_until(500, || exit_of(pid).is_some());
    let exit = exit_of(pid);
    if !ended {
        note(format_args!("{what}: pid {} had not closed its output after {ms} ms ({})", pid.0, status_text(exit)));
    }
    Ok(Ran { out, exit, ms })
}

/// A probe run: its data segment as it wrote it, or why there is none.
fn run_probe(what: &str, p: Probe) -> Result<Vec<u8>, String> {
    let (elf, len) = p.elf();
    let path = serve(what, elf);
    let ran = run(what, || exec::spawn(&path, &["probe"], &[]))?;
    if ran.exit != Some(0) || ran.out.len() != len {
        return Err(format!(
            "{what}: the probe wrote {} of its {len} bytes and ended {} after {} ms",
            ran.out.len(),
            status_text(ran.exit),
            ran.ms
        ));
    }
    Ok(ran.out)
}

fn res(out: &[u8], k: usize) -> i64 {
    le64(out, 8 * k) as i64
}

// ── Row 2: ext2.walk.open_read ───────────────────────────────────────────


// ── A probe and what each step must return ───────────────────────────────

enum W {
    Is(i64),
    Fd,
}

struct Plan {
    p: Probe,
    want: Vec<(String, usize, W)>,
}

impl Plan {
    fn new() -> Plan {
        Plan { p: Probe::new(), want: Vec::new() }
    }
    fn is(&mut self, what: &str, nr: u64, args: &[A], v: i64) -> usize {
        let k = self.p.call(nr, args);
        self.want.push((String::from(what), k, W::Is(v)));
        k
    }
    fn fd(&mut self, what: &str, nr: u64, args: &[A]) -> usize {
        let k = self.p.call(nr, args);
        self.want.push((String::from(what), k, W::Fd));
        k
    }
    /// Run it; every step checked; the data segment as it wrote it.
    fn run(self, r: &mut Row, name: &str) -> Option<Vec<u8>> {
        let id = r.id;
        let want = self.want;
        match run_probe(name, self.p) {
            Ok(out) => {
                for (what, k, w) in &want {
                    let got = res(&out, *k);
                    match w {
                        W::Is(v) => r.expect_res(what, got, *v),
                        W::Fd if got < 0 => r.fail(format_args!("{id}: {what} returned {}, expected an fd", answer(got))),
                        W::Fd => {}
                    }
                }
                Some(out)
            }
            Err(why) => {
                r.fail(format_args!("{id}: {why}"));
                None
            }
        }
    }
}

/// This file's reader's answer, or the row failed with why there is none.
fn got<T>(r: &mut Row, v: Result<T, String>) -> Option<T> {
    match v {
        Ok(v) => Some(v),
        Err(why) => {
            let id = r.id;
            r.fail(format_args!("{id}: {why}"));
            None
        }
    }
}

/// The inode `path` names, as this file reads the disk.
fn raw_at(r: &mut Row, raw: &Raw, path: &str) -> Option<(u64, [u8; 128])> {
    let ino = got(r, raw.walk(path))?;
    let Some(ino) = ino else {
        let id = r.id;
        r.fail(format_args!("{id}: {path}: no such name on the disk (this file's reader)"));
        return None;
    };
    Some((ino, got(r, raw.inode(ino))?))
}

fn check_inode(r: &mut Row, raw: &Raw, path: &str, mode: u64, nlink: u64, size: u64) -> Option<[u8; 128]> {
    let (_, i) = raw_at(r, raw, path)?;
    r.expect_eq(&format!("{path}: i_mode on the disk"), le16(&i, 0), mode);
    r.expect_eq(&format!("{path}: i_links_count on the disk"), links(&i), nlink);
    r.expect_eq(&format!("{path}: i_size on the disk"), le32(&i, 4), size);
    Some(i)
}

// ── Row 1: ext2.create.write.readback ────────────────────────────────────

/// Row 1's second file and its size, for row 4.
struct Second {
    path: String,
    size: u64,
}

fn create_row(raw: &Raw, log_ino: u32) -> Second {
    let mut r = Row::new(CREATE_ID);
    let d = r.derived(CREATE_ID);
    let size = 49_153 + (d % 30_000) as usize;
    let tag = format!("{:08x}", d as u32);
    let (big, tpath, npath, spath, rel) = (format!("/tmp/w-{tag}"), format!("/tmp/w-{tag}.t"), format!("/tmp/w-{tag}.n"), format!("/tmp/w-{tag}.s"), format!("tmp/w-{tag}.r"));
    // The sparse file's 100 bytes: from file block 1036 + (0..4) on, at a
    // nonce-chosen offset inside that block that keeps them in it.
    let soff = (DIND_FIRST + (d >> 20) % 4) * 4096 + (d >> 8) % 3900;
    let content = mix(d, size);
    let k1 = size / 3 + 1;
    let mut pl = Plan::new();
    let (pb, pt, pn, ps, pr) = (pl.p.str(&big), pl.p.str(&tpath), pl.p.str(&npath), pl.p.str(&spath), pl.p.str(&rel));
    let (log, tmp, hi, bnew, nod) = (pl.p.str("/.sboot-log"), pl.p.str("/tmp"), pl.p.str("/boot/hi"), pl.p.str("/boot/new"), pl.p.str("/nodir/x"));
    let coff = pl.p.bytes(&content);
    let tail = pl.p.bytes(b"+append");
    let (rbuf, roff) = pl.p.buf(size + 7 + 100);
    let (rbuf2, r2off) = pl.p.buf(64);
    let (rbuf3, r3off) = pl.p.buf(112);
    let (rbuf4, r4off) = pl.p.buf(16);
    let (st2, s2off) = pl.p.buf(144);
    let (small, _) = pl.p.buf(16);
    let (i, c) = (A::I, |off: usize| A::D(coff + off));
    let o = pl.fd("open(w, O_WRONLY|O_CREAT|O_EXCL, 0644)", SYS_OPEN, &[pb, i(O_WRONLY | O_CREAT | O_EXCL), i(0o644)]);
    pl.is("the first write", SYS_WRITE, &[A::R(o), c(0), i(k1 as u64)], k1 as i64);
    pl.is("the second write", SYS_WRITE, &[A::R(o), c(k1), i((size - k1) as u64)], (size - k1) as i64);
    pl.is("read on the O_WRONLY fd", SYS_READ, &[A::R(o), small, i(16)], -EBADF);
    pl.is("close", SYS_CLOSE, &[A::R(o)], 0);
    pl.is("open(w, O_WRONLY|O_CREAT|O_EXCL) again", SYS_OPEN, &[pb, i(O_WRONLY | O_CREAT | O_EXCL), i(0o644)], -EEXIST);
    let a = pl.fd("open(w, O_WRONLY|O_APPEND)", SYS_OPEN, &[pb, i(O_WRONLY | O_APPEND)]);
    pl.is("write of 7 bytes with O_APPEND", SYS_WRITE, &[A::R(a), A::D(tail), i(7)], 7);
    pl.is("close", SYS_CLOSE, &[A::R(a)], 0);
    let ro = pl.fd("open(w, O_RDONLY)", SYS_OPEN, &[pb, i(O_RDONLY)]);
    pl.is("read of it whole (asked for 100 more)", SYS_READ, &[A::R(ro), rbuf, i(size as u64 + 107)], size as i64 + 7);
    pl.is("write on the O_RDONLY fd", SYS_WRITE, &[A::R(ro), A::D(tail), i(1)], -EBADF);
    pl.is("close", SYS_CLOSE, &[A::R(ro)], 0);
    let t = pl.fd("open(w.t, O_RDWR|O_CREAT, 0600)", SYS_OPEN, &[pt, i(O_RDWR | O_CREAT), i(0o600)]);
    pl.is("write of 5000 bytes to w.t", SYS_WRITE, &[A::R(t), c(0), i(5000)], 5000);
    pl.is("close", SYS_CLOSE, &[A::R(t)], 0);
    let t2 = pl.fd("open(w.t, O_WRONLY) (no O_TRUNC: its bytes stay)", SYS_OPEN, &[pt, i(O_WRONLY)]);
    pl.is("write of 3 bytes over w.t's first 3", SYS_WRITE, &[A::R(t2), c(1000), i(3)], 3);
    pl.is("close", SYS_CLOSE, &[A::R(t2)], 0);
    pl.is("stat(w.t)", SYS_STAT, &[pt, st2], 0);
    let n = pl.fd("open(w.n, O_WRONLY|O_CREAT|O_TRUNC, 0644) (a new file)", SYS_OPEN, &[pn, i(O_WRONLY | O_CREAT | O_TRUNC), i(0o644)]);
    pl.is("write of 4 bytes to w.n", SYS_WRITE, &[A::R(n), c(200), i(4)], 4);
    pl.is("close", SYS_CLOSE, &[A::R(n)], 0);
    let s = pl.fd("open(w.s, O_RDWR|O_CREAT|O_EXCL, 0644)", SYS_OPEN, &[ps, i(O_RDWR | O_CREAT | O_EXCL), i(0o644)]);
    pl.is("lseek(w.s) into the double indirect range", SYS_LSEEK, &[A::R(s), i(soff), i(SEEK_SET)], soff as i64);
    pl.is("write of 100 bytes there", SYS_WRITE, &[A::R(s), c(300), i(100)], 100);
    pl.is("lseek(w.s) back to them", SYS_LSEEK, &[A::R(s), i(soff), i(SEEK_SET)], soff as i64);
    pl.is("read of w.s's 100 bytes (asked for 112)", SYS_READ, &[A::R(s), rbuf3, i(112)], 100);
    pl.is("lseek(w.s) into its hole (file block 20)", SYS_LSEEK, &[A::R(s), i(20 * 4096), i(SEEK_SET)], 20 * 4096);
    pl.is("read of 16 bytes of the hole", SYS_READ, &[A::R(s), rbuf4, i(16)], 16);
    pl.is("close", SYS_CLOSE, &[A::R(s)], 0);
    pl.is("open(/.sboot-log, O_WRONLY|O_TRUNC)", SYS_OPEN, &[log, i(O_WRONLY | O_TRUNC)], -EPERM);
    pl.is("open(/.sboot-log, O_RDWR)", SYS_OPEN, &[log, i(O_RDWR)], -EPERM);
    pl.is("open(/tmp, O_WRONLY)", SYS_OPEN, &[tmp, i(O_WRONLY)], -EISDIR);
    pl.is("open(/boot/hi, O_WRONLY)", SYS_OPEN, &[hi, i(O_WRONLY)], -EROFS);
    pl.is("open(/boot/new, O_WRONLY|O_CREAT)", SYS_OPEN, &[bnew, i(O_WRONLY | O_CREAT), i(0o644)], -EROFS);
    pl.is("open(/nodir/x, O_WRONLY|O_CREAT)", SYS_OPEN, &[nod, i(O_WRONLY | O_CREAT), i(0o644)], -ENOENT);
    let q = pl.fd("openat(AT_FDCWD, tmp/w.r, O_RDWR|O_CREAT|O_EXCL, 0640)", SYS_OPENAT, &[i(AT_FDCWD), pr, i(O_RDWR | O_CREAT | O_EXCL), i(0o640)]);
    pl.is("write of 10 bytes to w.r", SYS_WRITE, &[A::R(q), c(100), i(10)], 10);
    pl.is("lseek(w.r, 0, SEEK_SET)", SYS_LSEEK, &[A::R(q), i(0), i(SEEK_SET)], 0);
    pl.is("read of w.r", SYS_READ, &[A::R(q), rbuf2, i(64)], 10);
    pl.is("close", SYS_CLOSE, &[A::R(q)], 0);
    let mut whole = content.clone();
    whole.extend_from_slice(b"+append");
    let mut tbytes = content[..5000].to_vec();
    tbytes[..3].copy_from_slice(&content[1000..1003]);
    if let Some(out) = pl.run(&mut r, "w18") {
        r.expect_bytes("w as read back through the syscalls", &out[roff..roff + size + 7], &whole);
        r.expect_bytes("w.r as read back", &out[r2off..r2off + 10], &content[100..110]);
        r.expect_bytes("w.s's 100 bytes read back", &out[r3off..r3off + 100], &content[300..400]);
        r.expect_bytes("16 bytes of w.s's hole", &out[r4off..r4off + 16], &[0u8; 16]);
        r.expect_eq("st_size of w.t after 3 bytes written over its start", le64(&out, s2off + 48), 5000);
    }
    // `Inode::create` refuses a name that exists by itself (`O_EXCL` above
    // is answered by `open`'s own lookup first): `-EEXIST`, and the inode
    // bitmap and the superblock's free counts on the disk as they were.
    match (vfs::root().and_then(|root| vfs::walk(&root, "/tmp")), raw_block(raw.inode_bitmap), raw_block(0)) {
        (Ok(tmpi), Ok(ib0), Ok(sb0)) => {
            let name = &big["/tmp/".len()..];
            match tmpi.create(name, 0o644) {
                Err(e) if e == EEXIST as Errno => {}
                Err(e) => r.fail(format_args!("{CREATE_ID}: create(\"{name}\") on /tmp's Inode, a name that exists, returned {}; expected EEXIST", errno_text(e))),
                Ok(_) => r.fail(format_args!("{CREATE_ID}: create(\"{name}\") on /tmp's Inode, a name that exists, succeeded; expected EEXIST")),
            }
            if let (Some(ib1), Some(sb1)) = (got(&mut r, raw_block(raw.inode_bitmap)), got(&mut r, raw_block(0))) {
                r.expect(ib1[..] == ib0[..], "the inode bitmap on the disk unchanged by the refused create");
                r.expect_eq("s_free_inodes_count after the refused create", le32(&sb1[1024..], 16), le32(&sb0[1024..], 16));
                r.expect_eq("s_free_blocks_count after the refused create", le32(&sb1[1024..], 12), le32(&sb0[1024..], 12));
            }
        }
        (Err(e), _, _) => r.fail(format_args!("{CREATE_ID}: /tmp not found through the mount: {}", errno_text(e))),
        (_, Err(e), _) | (_, _, Err(e)) => r.fail(format_args!("{CREATE_ID}: {e}")),
    }
    if let Some(i) = check_inode(&mut r, raw, &big, 0o100644, 1, size as u64 + 7) {
        r.expect(le32(&i, 88) != 0, &format!("{big}: i_block[12] (the indirect block) set on the disk"));
        if let Some(bytes) = got(&mut r, raw.file(&i)) {
            r.expect_bytes(&format!("{big}: its bytes on the disk"), &bytes, &whole);
        }
    }
    if let Some(i) = check_inode(&mut r, raw, &tpath, 0o100600, 1, 5000) {
        if let Some(bytes) = got(&mut r, raw.file(&i)) {
            r.expect_bytes(&format!("{tpath}: its bytes on the disk"), &bytes, &tbytes);
        }
    }
    if let Some(i) = check_inode(&mut r, raw, &npath, 0o100644, 1, 4) {
        if let Some(bytes) = got(&mut r, raw.file(&i)) {
            r.expect_bytes(&format!("{npath}: its bytes on the disk"), &bytes, &content[200..204]);
        }
    }
    if let Some(i) = check_inode(&mut r, raw, &spath, 0o100644, 1, soff + 100) {
        r.expect(le32(&i, 92) != 0, &format!("{spath}: i_block[13] (the double indirect block) set on the disk"));
        r.expect_eq(&format!("{spath}: i_blocks on the disk (one data block, one indirect, the double indirect)"), le32(&i, 28), 24);
        let k = soff / 4096;
        match (got(&mut r, raw.bmap(&i, k)), got(&mut r, raw.blocks_of(&i))) {
            (Some(b), Some((data, _))) if b != 0 => {
                r.expect_eq(&format!("{spath}: data blocks mapped"), data.len() as u64, 1);
                if let Some(blk) = got(&mut r, raw_block(b)) {
                    let at = (soff % 4096) as usize;
                    r.expect_bytes(&format!("{spath}: file block {k} on the disk, its 100 bytes"), &blk[at..at + 100], &content[300..400]);
                    let zero = blk[..at].iter().chain(&blk[at + 100..]).all(|&x| x == 0);
                    r.expect(zero, &format!("{spath}: the rest of file block {k} zeros on the disk"));
                }
            }
            (Some(_), Some(_)) => r.fail(format_args!("{CREATE_ID}: {spath}: file block {k} is a hole on the disk")),
            _ => {}
        }
    }
    check_inode(&mut r, raw, &format!("/{rel}"), 0o100640, 1, 10);
    if let Some((ino, _)) = raw_at(&mut r, raw, "/.sboot-log") {
        r.expect_eq("Ext2::log_inode()", log_ino as u64, ino);
    }
    r.finish(1);
    Second { path: tpath, size: 5000 }
}

// ── Row 2: txn.every.metadata.write ──────────────────────────────────────

fn txn_row(raw: &Raw, control: Vec<String>) {
    let mut r = Row::new(TXN_ID);
    for why in control {
        r.fail(format_args!("{TXN_ID}: the log's control: {why}"));
    }
    let w = writes();
    let unlisted: Vec<u64> = w.iter().filter(|(b, listed)| *b < raw.meta_end() && !listed).map(|(b, _)| *b).collect();
    if !unlisted.is_empty() {
        r.fail(format_args!("{TXN_ID}: row 1: {} writes of the superblock, the descriptor, a bitmap or the inode table reached the disk unlisted, first fs blocks {:?}", unlisted.len(), &unlisted[..unlisted.len().min(6)]));
    }
    note(format_args!("{TXN_ID}: row 1 wrote {} blocks, {} of them the file system's own", w.len(), w.iter().filter(|(b, _)| *b < raw.meta_end()).count()));
    let mut seen = [false; 7];
    let Ok(tmp) = vfs::root().and_then(|root| vfs::walk(&root, "/tmp")) else {
        r.fail(format_args!("{TXN_ID}: /tmp not found through the mount"));
        return r.finish(2);
    };
    let mut q: Option<InodeRef> = None;
    watched(&mut r, raw, &mut seen, "create(\"q-txn\") in /tmp", || tmp.create("q-txn", 0o644).map(|i| q = Some(i)));
    let data = mix(0x5B00_0018_0005, 13 * 4096);
    if let Some(q) = &q {
        let whole = |n: Result<usize, Errno>, want: usize| match n {
            Ok(n) if n == want => Ok(()),
            Ok(_) => Err(EIO as Errno),
            Err(e) => Err(e),
        };
        watched(&mut r, raw, &mut seen, "write_at of 13 blocks (into the indirect block)", || whole(q.write_at(0, &data), data.len()));
        watched(&mut r, raw, &mut seen, "write_at of 2 blocks from file block 1036 (the double indirect)", || whole(q.write_at(DIND_FIRST * 4096, &data[..8192]), 8192));
        // Pointers added to pointer blocks the file has already: each
        // pointer block above was allocated inside the operation that
        // filled it, where the allocation logged it, so only these two
        // calls show whether a pointer's own write is logged — at each
        // level: the indirect block; the double indirect block's first
        // middle block (file block 2059, its last pointer) and the double
        // indirect block itself (file block 2060 needs its second middle
        // block).
        watched(&mut r, raw, &mut seen, "write_at of 1 block at file block 13 (a pointer added to the indirect block it has)", || whole(q.write_at(13 * 4096, &data[..4096]), 4096));
        watched(&mut r, raw, &mut seen, "write_at of 2 blocks from file block 2059 (a pointer added to the middle block it has, then one to the double indirect block)", || whole(q.write_at((DIND_FIRST + 1023) * 4096, &data[..8192]), 8192));
    } else {
        r.fail(format_args!("{TXN_ID}: the operations stopped early (above)"));
    }
    for (k, s) in seen.iter().enumerate() {
        if !s {
            r.fail(format_args!("{TXN_ID}: no listed write of {} in the five operations: nothing shows the log saw one", KINDS[k]));
        }
    }
    r.finish(2);
}

// ── Row 3: ext2.alloc.maps.agree ─────────────────────────────────────────

fn alloc_row(raw: &Raw, control: Result<Vec<String>, String>) {
    let mut r = Row::new(ALLOC_ID);
    match control {
        Ok(v) if v.is_empty() => {}
        Ok(v) => r.fail(format_args!("{ALLOC_ID}: this file's check of the disk as written, before the mount, disagrees (the vehicle is wrong): {}", v.join("; "))),
        Err(why) => r.fail(format_args!("{ALLOC_ID}: the check before the mount: {why}")),
    }
    match scan(raw) {
        Ok(s) => {
            match agree(raw, &s) {
                Ok(v) => {
                    for why in &v {
                        r.fail(format_args!("{ALLOC_ID}: {why}"));
                    }
                }
                Err(why) => r.fail(format_args!("{ALLOC_ID}: {why}")),
            }
            note(format_args!("{ALLOC_ID}: the tree holds {} inodes, {} blocks, {} directories", s.inodes.len(), s.used.len(), s.dirs));
        }
        Err(why) => r.fail(format_args!("{ALLOC_ID}: {why}")),
    }
    r.finish(3);
}

// ── Row 4: ext2.write.enospc ─────────────────────────────────────────────

/// The drive as a disk with no free block: every write kept here (none
/// reaches the drive), the block bitmap read as all set, the superblock's
/// and the descriptor's free block counts read as 0.
struct Full {
    inner: Arc<dyn BlockDevice>,
    mem: SpinLock<BTreeMap<u64, Box<[u8; BLOCK_SIZE]>>>,
    /// The block bitmap, as the disk's block number.
    bitmap: u64,
}

impl BlockDevice for Full {
    fn read_block(&self, n: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), Errno> {
        if let Some(b) = quiet(|| self.mem.lock().get(&n).cloned()) {
            buf.copy_from_slice(&b[..]);
            return Ok(());
        }
        self.inner.read_block(n, buf)?;
        if n == self.bitmap {
            buf.fill(0xff);
        } else if n == P2_BLOCK0 {
            buf[1024 + 12..1024 + 16].fill(0);
        } else if n == P2_BLOCK0 + 1 {
            buf[12..14].fill(0);
        }
        Ok(())
    }
    fn write_block(&self, n: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), Errno> {
        let b = Box::new(*buf);
        let old = quiet(|| self.mem.lock().insert(n, b));
        drop(old);
        Ok(())
    }
    fn blocks(&self) -> u64 {
        self.inner.blocks()
    }
    fn arm_fail_after(&self, _k: u32) {}
    fn disarm(&self) {}
    fn refused_writes(&self) -> u32 {
        0
    }
}

/// `i_size` of inode `ino` as `full` holds it now (this file's decode).
fn full_size(raw: &Raw, full: &Full, ino: u64) -> Result<u64, String> {
    let at = (ino - 1) * 128;
    let mut b = [0u8; BLOCK_SIZE];
    full.read_block(P2_BLOCK0 + raw.inode_table + at / 4096, &mut b).map_err(|e| format!("this file's read of the inode table returned {}", errno_text(e)))?;
    Ok(le32(&b, (at % 4096) as usize + 4))
}

fn enospc_row(spy: &Arc<Spy>, raw: &Raw, second: &Second) {
    let mut r = Row::new(ENOSPC_ID);
    let full = Arc::new(Full { inner: spy.clone(), mem: SpinLock::new(BTreeMap::new()), bitmap: P2_BLOCK0 + raw.block_bitmap });
    let (path, size) = (second.path.as_str(), second.size);
    let Some((ino_no, _)) = raw_at(&mut r, raw, path) else { return r.finish(4) };
    let name = path.trim_start_matches("/tmp/");
    let fs = quiet(|| ata::partition2(full.clone()))
        .map_err(|e| format!("partition2(the full device) returned {}", errno_text(e)))
        .and_then(|p| Ext2::mount(p).map_err(|e| format!("Ext2::mount over the full device returned {}", errno_text(e))))
        .map(Arc::new)
        .and_then(|fs| fs.root().and_then(|root| root.lookup("tmp")).and_then(|t| t.lookup(name)).map_err(|e| format!("{path} through that mount: {}", errno_text(e))));
    let ino = match fs {
        Ok(ino) => ino,
        Err(why) => {
            r.fail(format_args!("{ENOSPC_ID}: {why}"));
            return r.finish(4);
        }
    };
    let off = size + 3 * 4096 + 10;
    match ino.write_at(off, &[0x5a; 100]) {
        Err(e) if e == ENOSPC as Errno => {}
        other => r.fail(format_args!("{ENOSPC_ID}: write_at({off}, 100 bytes) past the end of {path} on a disk with no free block returned {}, expected -28 (ENOSPC)", match other {
            Ok(n) => format!("{n}"),
            Err(e) => errno_text(e),
        })),
    }
    r.expect_eq(&format!("{path}: st_size after the refused write"), ino.stat().size, size);
    if let Some(n) = got(&mut r, full_size(raw, &full, ino_no)) {
        r.expect_eq(&format!("{path}: i_size in its inode as that mount wrote it, after the refused write"), n, size);
    }
    let start = size - 5;
    let room = 4096 - start % 4096;
    match ino.write_at(start, &[0xa5; 5000]) {
        Ok(n) if n as u64 == room => {}
        other => r.fail(format_args!("{ENOSPC_ID}: write_at({start}, 5000 bytes) with room for {room} in its last block and no free block returned {}, expected {room} (a short write)", match other {
            Ok(n) => format!("{n}"),
            Err(e) => errno_text(e),
        })),
    }
    r.expect_eq(&format!("{path}: st_size after the short write"), ino.stat().size, start + room);
    if let Some(n) = got(&mut r, full_size(raw, &full, ino_no)) {
        r.expect_eq(&format!("{path}: i_size in its inode as that mount wrote it, after the short write"), n, start + room);
    }
    let mut bb = [0u8; BLOCK_SIZE];
    if full.read_block(P2_BLOCK0 + raw.block_bitmap, &mut bb).is_ok() {
        r.expect(bb.iter().all(|&x| x == 0xff), "the block bitmap as that mount wrote it: still every bit set (nothing taken, nothing given back)");
    }
    note(format_args!("{ENOSPC_ID}: {} blocks written to the full device, none to the drive", quiet(|| full.mem.lock().len())));
    r.finish(4);
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


/// Every row from `from` on, failed for `why`.
fn fail_rest(from: usize, why: &str) {
    for (k, id) in [CREATE_ID, TXN_ID, ALLOC_ID, ENOSPC_ID].into_iter().enumerate().skip(from - 1) {
        let mut r = Row::new(id);
        r.fail(format_args!("{id}: {why}"));
        r.finish(k + 1);
    }
}

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    serial::init();
    ktap::begin(PLAN);
    prelude();
    let mut c = Clock::new();
    let raw = match Raw::open() {
        Ok(raw) => raw,
        Err(why) => {
            fail_rest(1, &format!("disk: {why}"));
            quiet(ktap::end);
            halt()
        }
    };
    // The disk as written must agree with itself: row 3's control.
    let pre = scan(&raw).and_then(|s| agree(&raw, &s));
    // The log, before any mount (row 2's control): steps 1–5.
    let control = log_control();
    let drive: Arc<dyn BlockDevice> = match quiet(Ata::probe) {
        Ok(a) => Arc::new(a),
        Err(e) => {
            fail_rest(1, &format!("Ata::probe() returned {}: no drive", errno_text(e)));
            quiet(ktap::end);
            halt()
        }
    };
    let spy = Arc::new(Spy { inner: drive, reads: AtomicU64::new(0) });
    let (fs, root) = match mount_fs(&spy).and_then(|fs| fs.root().map(|root| (fs, root)).map_err(|e| format!("Ext2::root() returned {}", errno_text(e)))) {
        Ok(v) => v,
        Err(why) => {
            fail_rest(1, &why);
            quiet(ktap::end);
            halt()
        }
    };
    let log_ino = fs.log_inode();
    let proxy = Arc::new(Proxy(SpinLock::new(Some(root))));
    mount_rest(proxy.clone());
    note(format_args!("vfs: / is this vehicle's root over ext2 on partition 2, /boot the boot archive ({ARCHIVE_LEN} bytes), /ktest this vehicle's programs"));
    let second = create_row(&raw, log_ino);
    txn_row(&raw, control);
    alloc_row(&raw, pre);
    enospc_row(&spy, &raw, &second);
    drop(fs);
    note(format_args!("the rows took {} ms; the drive saw {} reads and {} writes", c.ms(), spy.reads.load(Ordering::Relaxed), writes().len()));
    sched::yield_now();
    quiet(ktap::end);
    halt()
}
