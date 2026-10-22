//! Lab 19's vehicle: ext2 cut and deleted from — `truncate`, `unlink`,
//! `sync` — with the bitmaps and counts given back, and the log's
//! pass-through around each. The prelude is lab 17's. Cut from the old lab
//! 18's vehicle on 2026-10-19 (slice 5a): its unlink, remount, freemaps and
//! log rows, with `truncate` graded on its own.
//!
//! The disk is `ktest-19.img` as the course's image writer made it (CONTRACT
//! §A.7); the course boots a copy. As at 18, every expectation comes from
//! this file's formulas or from its own reads of the disk — its own PIO
//! driver and its own decoder of the superblock, the descriptor, both
//! bitmaps, the inode table, the block maps and the directory records.
//! Nothing the learner's ext2 reports is used as an expectation.
//!
//! The drive sits under `Spy` (`../txnspy.rs`, shared with 18 and 20): it counts the reads
//! that reach the drive and records every write, with whether the block was
//! in `log::logged()` at that moment. `/` forwards every call to the root of
//! the learner's `Ext2::mount(partition2(spy))`; `/boot` is the boot
//! archive, `/ktest` this file's programs. The files the rows cut and
//! delete are made with lab 18's calls only (`open(O_CREAT|O_EXCL)`,
//! `write`), so the first call of this lab's is row 1's first `truncate`.
//! Five rows, in this order:
//!
//!   1 `ext2.truncate.frees` (derived) — `/tmp/t-<n>`, 15 blocks of
//!     splitmix64 of the nonce (12 direct, the indirect block, 3 through
//!     it). The learner's `Inode::truncate`: to 13 blocks + 7 bytes (the
//!     last pointer of the indirect block cleared, the block kept), to 5000
//!     (the indirect block itself let go, `i_block[12]` 0) — after each, on
//!     the disk: the size, `i_blocks`, the block map as the first blocks it
//!     had, every block let go clear in the bitmap and every kept one set,
//!     both free block counts up by exactly the number let go, the last
//!     block's bytes past the new size zero. Then to 20000: the bytes read
//!     back are the first 5000 then zeros, and no block taken. `truncate`
//!     past 2 GiB is `-EFBIG`. Then through the syscalls: `O_TRUNC` on the
//!     file (its size and `i_blocks` 0, the block bitmap and both counts as
//!     before the file was made), `/.sboot-log` with `O_RDONLY|O_TRUNC`
//!     (`-EPERM`, its size kept), `/tmp` with `O_TRUNC` (`-EISDIR`). Last,
//!     `/tmp/t-<n>.d`: one block written at file block 1036 + (0..4) (a data
//!     block, an indirect block, the double indirect block), `truncate(0)`:
//!     `i_block[13]` and `i_blocks` 0, the block bitmap and the free block
//!     counts as before the write (added 2026-10-19, slice 5a).
//!   2 `ext2.unlink.frees` (plain) — `/tmp/u19a` (60000 bytes, through the
//!     indirect block) and an empty `/tmp/u19b` made, then `unlink`ed
//!     (`open` after, and `unlink` again, `-ENOENT`). Refused: `/tmp`
//!     (`-EISDIR`), `/.sboot-log` (`-EPERM`), a missing name (`-ENOENT`),
//!     row 1's file with a trailing `/` (`-ENOTDIR`), `/boot/hi`
//!     (`-EROFS`). On the disk: both bitmaps, the five counts and `/tmp`'s
//!     links byte for byte as before the row, both records gone, the freed
//!     inode's links and `i_blocks` 0 and its `i_dtime` set. A third file
//!     made and kept for row 3.
//!   3 `ext2.remount.persists` (plain) — `Ext2::sync`, every reference to
//!     the mount dropped (the first `Ext2` must be gone), `Ext2::mount`
//!     again over the same drive inside this boot: row 2's kept file reads
//!     back byte for byte, every block of it from the drive (the spy's read
//!     count), row 1's file is there and empty, the deleted names are
//!     `-ENOENT`.
//!   4 `txn.delete.metadata.write` (plain) — the log's control (`txnspy.rs`:
//!     a cache over a memory device, before any mount), then
//!     every write of rows 1–3 to the superblock, the descriptor, the
//!     bitmaps or the inode table listed when it reached the disk; then, on
//!     the second mount, a file made and written 15 blocks (watched, not
//!     counted), `truncate` to 13 blocks + 1 byte and `unlink`, each between
//!     two full reads of the tree by this file: those two must write each of
//!     the seven kinds, every one listed. The learner's own `logged()` says
//!     what was listed: a self-report. Lab 21's recovery rows re-grade
//!     unlink's frees (their operation unlinks), never truncate's (this row
//!     alone; CONTRACT §B.2-19, corrected 2026-10-19).
//!   5 `ext2.freemaps.agree` (plain) — after every row's writes, this
//!     file's walk of the whole tree from the root against the disk: every
//!     bit of both bitmaps, the free counts and the directory count, every
//!     link count and `i_blocks`, no block claimed twice. The same check on
//!     the disk before the mount is this file's control.
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

const PLAN: usize = 5;
const TRUNC_ID: &str = "ext2.truncate.frees";
const UNLINK_ID: &str = "ext2.unlink.frees";
const REMOUNT_ID: &str = "ext2.remount.persists";
const TXN_ID: &str = "txn.delete.metadata.write";
const FREE_ID: &str = "ext2.freemaps.agree";

/// A file's largest size (§B.2-18: 2 GiB).
const MAX_SIZE: u64 = 1 << 31;
/// File block 1036: the first one the double indirect block maps.
const DIND_FIRST: u64 = 12 + 1024;

/// Partition 2's first 4 KiB block on the disk (§A.7).
const P2_BLOCK0: u64 = 8448;

/// The probes' data segment (lab 17's layout).
const DATA: u64 = 0x0110_0000;
const MAX_STEPS: usize = 64;
const RESULTS: usize = 8 * MAX_STEPS;
/// How long a probe may run.
const RUN_MS: u64 = 20_000;

const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_STAT: u64 = 4;
const SYS_UNLINK: u64 = 87;
const O_RDONLY: u64 = 0;
const O_WRONLY: u64 = 1;
const O_RDWR: u64 = 2;
const O_CREAT: u64 = 0x40;
const O_EXCL: u64 = 0x80;
const O_TRUNC: u64 = 0x200;
const O_NONBLOCK: u32 = 0x800;
const EPERM: i64 = 1;
const ENOENT: i64 = 2;
const ENOTDIR: i64 = 20;
const EISDIR: i64 = 21;
const EROFS: i64 = 30;
const EIO: i64 = 5;
const EFBIG: i64 = 27;

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

/// The time the learner's inode stamps use: `EPOCH` plus whole seconds of
/// `time::now_ns`.
fn guest_now() -> u64 {
    kernel::fs::ext2::EPOCH as u64 + kernel::time::now_ns() / 1_000_000_000
}

/// Moves the guest clock forward to the middle of guest second `t`, through
/// `time::TICKS` with interrupts off (lab 06's `clock.hours` sets `TICKS`
/// the same way), and never back: 1000 ticks at a time to second `t - 1`,
/// one at a time across into `t`, then 500 more. No wait (fix round 3,
/// sk19-forger-rv2-5); half a second either side of the unlink (fix round
/// 4, sk19-forger-rv3-5), so an inode clock off `now_ns` by less than that,
/// such as `TICKS / 1000`, lands in the same second. False when the clock
/// does not follow `TICKS`.
fn clock_forward(t: u64) -> bool {
    let add = |n: u64| {
        let on = irq_off();
        time::TICKS.fetch_add(n, Ordering::Relaxed);
        irq_restore(on);
    };
    for _ in 0..1000 {
        if guest_now() + 1 >= t {
            break;
        }
        add(1000);
    }
    for _ in 0..3000 {
        if guest_now() >= t {
            break;
        }
        add(1);
    }
    if guest_now() < t {
        return false;
    }
    add(500);
    guest_now() == t
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
        Err(format!("the test's own read of fs block {b} failed"))
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
            return Err(String::from("fs block 0 holds no ext2 superblock (magic) as the test reads it"));
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
    fn set(&self, r: Option<InodeRef>) {
        let old = quiet(|| core::mem::replace(&mut *self.0.lock(), r));
        drop(old);
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
        note(format_args!("vfs: mount(\"/ktest\", the test's directory) returned {}", errno_text(e)));
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

/// What the rows leave behind, for row 4 to read back.
struct Left {
    /// (path, bytes) that must be there.
    files: Vec<(String, Vec<u8>)>,
    /// Paths that must not.
    gone: Vec<String>,
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
        note(format_args!("{what}: pid {} had not closed its output after {ms} ms; the limit is {RUN_MS} ms ({})", pid.0, status_text(exit)));
    }
    Ok(Ran { out, exit })
}

/// A probe run: its data segment as it wrote it, or why there is none.
fn run_probe(what: &str, p: Probe) -> Result<Vec<u8>, String> {
    let (elf, len) = p.elf();
    let path = serve(what, elf);
    let ran = run(what, || exec::spawn(&path, &["probe"], &[]))?;
    if ran.exit != Some(0) || ran.out.len() != len {
        return Err(format!(
            "{what}: the probe wrote {} of its {len} bytes and ended {}",
            ran.out.len(),
            status_text(ran.exit)
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

// ── Shared by the rows ───────────────────────────────────────────────────

/// Both bitmaps, the five counts, `/tmp`'s links.
fn alloc_state(raw: &Raw) -> Result<(Box<[u8; BLOCK_SIZE]>, Box<[u8; BLOCK_SIZE]>, [u64; 5], u64), String> {
    let (sb, gd) = (raw_block(0)?, raw_block(1)?);
    let counts = [le32(&sb[1024..], 12), le32(&sb[1024..], 16), le16(&gd[..], 12), le16(&gd[..], 14), le16(&gd[..], 16)];
    let tmp = raw.walk("/tmp")?.ok_or("no /tmp on the disk")?;
    Ok((raw_block(raw.block_bitmap)?, raw_block(raw.inode_bitmap)?, counts, links(&raw.inode(tmp)?)))
}

type State = (Box<[u8; BLOCK_SIZE]>, Box<[u8; BLOCK_SIZE]>, [u64; 5], u64);

/// Bit `n` of `map` (block `n`, or inode `n + 1`).
fn bit(map: &[u8; BLOCK_SIZE], n: u64) -> bool {
    map[(n / 8) as usize] >> (n % 8) & 1 == 1
}

/// `after` against `before`, the block side only (or both): every
/// difference a failure of the row.
fn same_state(r: &mut Row, when: &str, after: &State, before: &State, inodes_too: bool) {
    let first = |a: &[u8; BLOCK_SIZE], b: &[u8; BLOCK_SIZE]| (0..BLOCK_SIZE).find(|&k| a[k] != b[k]);
    let id = r.id;
    if let Some(k) = first(&after.0, &before.0) {
        r.fail(format_args!("{id}: the block bitmap {when} differs from before first at byte {k} (blocks {}–{}): {:#04x}, was {:#04x}", 8 * k, 8 * k + 7, after.0[k], before.0[k]));
    }
    let what = ["s_free_blocks_count", "s_free_inodes_count", "bg_free_blocks_count", "bg_free_inodes_count", "bg_used_dirs_count"];
    for k in [0, 2] {
        r.expect_eq(&format!("{} {when} (as before)", what[k]), after.2[k], before.2[k]);
    }
    if inodes_too {
        if let Some(k) = first(&after.1, &before.1) {
            r.fail(format_args!("{id}: the inode bitmap {when} differs from before first at byte {k} (inodes {}–{}): {:#04x}, was {:#04x}", 8 * k + 1, 8 * k + 8, after.1[k], before.1[k]));
        }
        for k in [1, 3, 4] {
            r.expect_eq(&format!("{} {when} (as before)", what[k]), after.2[k], before.2[k]);
        }
        r.expect_eq(&format!("/tmp's i_links_count {when} (as before)"), after.3, before.3);
    }
}

/// The path through the mount, as an `InodeRef`.
fn at(path: &str) -> Result<InodeRef, String> {
    vfs::root().and_then(|root| vfs::walk(&root, path)).map_err(|e| format!("{path} through the mount: {}", errno_text(e)))
}

// ── Row 1: ext2.truncate.frees ───────────────────────────────────────────

/// One shrink by the learner's `Inode::truncate`, checked on the disk:
/// the size, the blocks still mapped (the first `keep` of `data`, which it
/// held before; of its pointer blocks `had`, those in `meta`), every block
/// let go clear in the bitmap and counted free, the bytes of the last block
/// past the new size zero.
fn shrink(r: &mut Row, raw: &Raw, ino: &InodeRef, path: &str, len: u64, data: &[u64], keep: usize, had: &[u64], meta: &[u64], content: &[u8]) {
    let Some(before) = got(r, alloc_state(raw)) else { return };
    if let Err(e) = ino.truncate(len) {
        r.fail(format_args!("{TRUNC_ID}: truncate({len}) of {path} returned {}", errno_text(e)));
        return;
    }
    let Some((_, i)) = raw_at(r, raw, path) else { return };
    let Some(after) = got(r, alloc_state(raw)) else { return };
    r.expect_eq(&format!("{path}: i_size on the disk after truncate({len})"), le32(&i, 4), len);
    r.expect_eq(&format!("{path}: i_blocks on the disk after truncate({len})"), le32(&i, 28), 8 * (keep + meta.len()) as u64);
    if let Some((d, m)) = got(r, raw.blocks_of(&i)) {
        r.expect(d == data[..keep], &format!("{path}: after truncate({len}) its block map on the disk holds its first {keep} data blocks, as they were, and no other ({} now)", d.len()));
        r.expect(m == meta, &format!("{path}: after truncate({len}) its pointer blocks on the disk are {meta:?} (now {m:?})"));
    }
    if meta.is_empty() {
        r.expect_eq(&format!("{path}: i_block[12] on the disk after truncate({len})"), le32(&i, 88), 0);
    }
    let gone: Vec<u64> = data[keep..].iter().chain(had.iter().filter(|b| !meta.contains(b))).copied().collect();
    let freed = gone.len() as u64;
    for &b in &gone {
        if bit(&after.0, b) {
            r.fail(format_args!("{TRUNC_ID}: {path}: block {b}, let go by truncate({len}), still set in the block bitmap"));
        }
    }
    for &b in data[..keep].iter().chain(meta) {
        if !bit(&after.0, b) {
            r.fail(format_args!("{TRUNC_ID}: {path}: block {b}, still mapped after truncate({len}), clear in the block bitmap"));
        }
    }
    r.expect_eq(&format!("s_free_blocks_count after truncate({len}): {freed} more"), after.2[0], before.2[0] + freed);
    r.expect_eq(&format!("bg_free_blocks_count after truncate({len}): {freed} more"), after.2[2], before.2[2] + freed);
    let tail = (len % 4096) as usize;
    if tail != 0 {
        let last = data[keep - 1];
        if let Some(blk) = got(r, raw_block(last)) {
            let k = len as usize - tail;
            r.expect_bytes(&format!("{path}: the bytes of its last block before {len} on the disk"), &blk[..tail], &content[k..k + tail]);
            r.expect(blk[tail..].iter().all(|&x| x == 0), &format!("{path}: its last block on the disk zero past {len}"));
        }
    }
}

fn truncate_row(raw: &Raw) -> String {
    let mut r = Row::new(TRUNC_ID);
    let d = r.derived(TRUNC_ID);
    let path = format!("/tmp/t-{:08x}", d as u32);
    let content = mix(d, 15 * 4096);
    let Some(start) = got(&mut r, alloc_state(raw)) else {
        r.finish(1);
        return path;
    };
    // Made with lab 18's calls only.
    let i = A::I;
    let mut pl = Plan::new();
    let p = pl.p.str(&path);
    let off = pl.p.bytes(&content);
    let f = pl.fd("open(t, O_RDWR|O_CREAT|O_EXCL, 0644)", SYS_OPEN, &[p, i(O_RDWR | O_CREAT | O_EXCL), i(0o644)]);
    pl.is("write of 15 blocks", SYS_WRITE, &[A::R(f), A::D(off), i(15 * 4096)], 15 * 4096);
    pl.is("close", SYS_CLOSE, &[A::R(f)], 0);
    let made = pl.run(&mut r, "t19a").is_some();
    let maps = raw_at(&mut r, raw, &path).and_then(|(_, i)| got(&mut r, raw.blocks_of(&i)));
    let (data, meta) = match maps {
        Some((d, m)) if made && d.len() == 15 && m.len() == 1 => (d, m),
        Some((d, m)) => {
            r.fail(format_args!("{TRUNC_ID}: {path} as made holds {} data blocks and {} pointer blocks, expected 15 and 1 (lab 18's write)", d.len(), m.len()));
            r.finish(1);
            return path;
        }
        None => {
            r.finish(1);
            return path;
        }
    };
    let ino = match at(&path) {
        Ok(ino) => ino,
        Err(why) => {
            r.fail(format_args!("{TRUNC_ID}: {why}"));
            r.finish(1);
            return path;
        }
    };
    // Inside the indirect block: one pointer cleared, the block kept.
    shrink(&mut r, raw, &ino, &path, 13 * 4096 + 7, &data, 14, &meta, &meta, &content);
    // Below it: the indirect block itself let go.
    shrink(&mut r, raw, &ino, &path, 5000, &data[..14], 2, &meta, &[], &content);
    // Growing takes nothing: what lies past 5000 reads as zeros.
    let Some(before) = got(&mut r, alloc_state(raw)) else {
        r.finish(1);
        return path;
    };
    match ino.truncate(20_000) {
        Ok(()) => {
            r.expect_eq(&format!("{path}: st_size after truncate(20000)"), ino.stat().size, 20_000);
            let mut want = content[..5000].to_vec();
            want.resize(20_000, 0);
            match read_whole(&ino) {
                Ok(bytes) => r.expect_bytes(&format!("{path} read back after truncate(5000) then truncate(20000)"), &bytes, &want),
                Err(e) => r.fail(format_args!("{TRUNC_ID}: read_at of {path} returned {}", errno_text(e))),
            }
            if let Some((_, i)) = raw_at(&mut r, raw, &path) {
                r.expect_eq(&format!("{path}: i_size on the disk after truncate(20000)"), le32(&i, 4), 20_000);
                r.expect_eq(&format!("{path}: i_blocks on the disk after truncate(20000) (growing takes no block)"), le32(&i, 28), 16);
            }
            if let Some(after) = got(&mut r, alloc_state(raw)) {
                same_state(&mut r, "after truncate(20000)", &after, &before, false);
            }
        }
        Err(e) => r.fail(format_args!("{TRUNC_ID}: truncate(20000) of {path} returned {}", errno_text(e))),
    }
    match ino.truncate(MAX_SIZE + 1) {
        Err(e) if e == EFBIG as Errno => {}
        other => r.fail(format_args!("{TRUNC_ID}: truncate(2 GiB + 1) of {path} returned {}, expected -27 (EFBIG)", match other {
            Ok(()) => String::from("0"),
            Err(e) => errno_text(e),
        })),
    }
    drop(ino);
    // A directory is refused by truncate itself, before any change (fix
    // round 1, O-1; a shrink since fix round 2, sk19-forger-rv-3): the
    // image's /lost+found (4 blocks) cut must come back -EISDIR with nothing
    // changed (below); and /tmp at its own size, which no shrink-only
    // refusal catches.
    match at("/tmp") {
        Ok(t) => match t.truncate(t.stat().size) {
            Err(e) if e == EISDIR as Errno => {}
            Ok(()) => r.fail(format_args!("{TRUNC_ID}: truncate of /tmp (a directory) to its own size returned 0, expected -21 (EISDIR)")),
            Err(e) => r.fail(format_args!("{TRUNC_ID}: truncate of /tmp (a directory) to its own size returned {}, expected -21 (EISDIR)", errno_text(e))),
        },
        Err(why) => r.fail(format_args!("{TRUNC_ID}: {why}")),
    }
    // Fix round 3 (sk19-forger-rv2-3): /lost+found is cut to one block, to
    // 12 bytes (inside its first record block) and grown by a block; fix
    // round 4 (sk19-forger-rv3-2/-3) adds a length from the nonce and, last,
    // 0. Each must be refused with its size, i_blocks, records, both bitmaps
    // and the counts as they were.
    if let Some((lfi, lf)) = raw_at(&mut r, raw, "/lost+found") {
        let recs = got(&mut r, raw.records(lfi, &lf));
        if let Some(pre) = got(&mut r, alloc_state(raw)) {
            let size = le32(&lf, 4);
            let any = 1 + (d >> 33) % MAX_SIZE;
            for (len, what) in [(4096, "to one block"), (12, "to 12 bytes"), (size + 4096, "a block longer"), (any, "to a length from this run's input"), (0, "to nothing")] {
                match at("/lost+found") {
                    Ok(t) => match t.truncate(len) {
                        Err(e) if e == EISDIR as Errno => {}
                        Ok(()) => r.fail(format_args!("{TRUNC_ID}: truncate({len}) of /lost+found (a directory of 4 blocks, {what}) returned 0, expected -21 (EISDIR)")),
                        Err(e) => r.fail(format_args!("{TRUNC_ID}: truncate({len}) of /lost+found (a directory of 4 blocks, {what}) returned {}, expected -21 (EISDIR)", errno_text(e))),
                    },
                    Err(why) => r.fail(format_args!("{TRUNC_ID}: {why}")),
                }
                if let Some((_, now)) = raw_at(&mut r, raw, "/lost+found") {
                    r.expect_eq(&format!("/lost+found: i_size on the disk after its refused truncate({len})"), le32(&now, 4), size);
                    r.expect_eq(&format!("/lost+found: i_blocks on the disk after its refused truncate({len})"), le32(&now, 28), le32(&lf, 28));
                    if let (Some(a), Some(b)) = (recs.as_ref(), got(&mut r, raw.records(lfi, &now))) {
                        r.expect(*a == b, &format!("/lost+found: its records on the disk after its refused truncate({len}) read back as before"));
                    }
                }
            }
            if let Some(after) = got(&mut r, alloc_state(raw)) {
                same_state(&mut r, "after the refused truncates of /lost+found", &after, &pre, true);
            }
        }
    }
    // Through the syscalls: O_TRUNC on a file that is there.
    let mut pl = Plan::new();
    let (p, log, tmp) = (pl.p.str(&path), pl.p.str("/.sboot-log"), pl.p.str("/tmp"));
    let (st, soff) = pl.p.buf(144);
    let f = pl.fd("open(t, O_WRONLY|O_TRUNC)", SYS_OPEN, &[p, i(O_WRONLY | O_TRUNC)]);
    pl.is("close", SYS_CLOSE, &[A::R(f)], 0);
    pl.is("stat(t) after O_TRUNC", SYS_STAT, &[p, st], 0);
    pl.is("open(/.sboot-log, O_RDONLY|O_TRUNC)", SYS_OPEN, &[log, i(O_RDONLY | O_TRUNC)], -EPERM);
    pl.is("open(/tmp, O_RDONLY|O_TRUNC)", SYS_OPEN, &[tmp, i(O_RDONLY | O_TRUNC)], -EISDIR);
    if let Some(out) = pl.run(&mut r, "t19b") {
        r.expect_eq("st_size of t after O_TRUNC", le64(&out, soff + 48), 0);
    }
    if let Some((_, i)) = raw_at(&mut r, raw, &path) {
        r.expect_eq(&format!("{path}: i_size on the disk after O_TRUNC"), le32(&i, 4), 0);
        r.expect_eq(&format!("{path}: i_blocks on the disk after O_TRUNC"), le32(&i, 28), 0);
    }
    // Every block the file ever took is free again.
    if let Some(after) = got(&mut r, alloc_state(raw)) {
        same_state(&mut r, "after O_TRUNC", &after, &start, false);
    }
    if let Some((_, i)) = raw_at(&mut r, raw, "/.sboot-log") {
        r.expect(le32(&i, 4) > 0, "/.sboot-log: i_size on the disk not 0 after the refused O_TRUNC");
    }
    tree_sweep(&mut r, raw, &path, d);
    dind_case(&mut r, raw, &format!("{path}.d"), d);
    r.finish(1);
    path
}

/// Row 1's double-indirect case (added 2026-10-19, slice 5a, the skeptic's
/// m3: no row freed through `i_block[13]`): an empty file made with lab
/// 18's calls, one block written at file block 1036 + (0..4) — a data
/// block, an indirect block and the double indirect block taken — then
/// `truncate(0)`: all three given back, `i_block[13]` and `i_blocks` 0 on
/// the disk, the block bitmap and the free counts as before the write.
/// Blocks a file needs on the disk for these data blocks: the data, the
/// indirect block under `i_block[12]`, and the double indirect block with
/// one child per 1024 blocks under `i_block[13]` (the lesson's layout).
fn blocks_needed(data: &BTreeSet<u64>) -> u64 {
    let ind = data.iter().any(|&b| (12..DIND_FIRST).contains(&b)) as u64;
    let kids: BTreeSet<u64> = data.iter().filter(|&&b| b >= DIND_FIRST).map(|&b| (b - DIND_FIRST) / 1024).collect();
    let dind = !kids.is_empty() as u64;
    data.len() as u64 + ind + dind + kids.len() as u64
}

/// Fix round 5 (sk19-forger-rv4-1/-2): the boundary class, not one boundary
/// a round. For each pointer tree a cut can land in (`i_block[12]`'s,
/// `i_block[13]`'s child 0, and its child k, 1..=64 from the nonce) a file
/// of its own gets the block below the tree, the tree's first two blocks, a
/// nonce-placed middle block m and the block before it, and the tree's last
/// block; then it is cut to m blocks, to base + 1 and to base. After each
/// cut every written block below the cut is mapped and reads back, every
/// one at or past it is unmapped, and `i_blocks` and both free counts move
/// by what the kept blocks need. Then the file is cut to nothing.
///
/// Fix round 6 (sk19-forger-rv5): each file also holds one block in the tree
/// past its own, and child 0's file one in a child 256..=509 (a pointer walk
/// bounded by a 1 KiB block's 256 entries never reaches it); the cuts add
/// base + 64 and base + 2. After each cut no pointer the file gave back is
/// left in the inode or the double indirect block, and every block it gave
/// back is clear in the block bitmap; the file stops there if one is.
///
/// Fix round 7 (sk19-forger-rv6): two cuts in each file are not block
/// multiples. One ends in a hole between m and the tree's last block (the
/// file must take no block for it); the other ends inside block m, so the
/// file is grown back over that block and its bytes past the cut read as
/// zeros. First, a sparse file is left for row 5 to unlink: a hole at block
/// 0, direct blocks d and 11, one block under `i_block[12]` and one in a
/// child 256..=509 under `i_block[13]`, all drawn from the nonce.
fn tree_sweep(r: &mut Row, raw: &Raw, path: &str, d: u64) {
    let tmp = match vfs::root().and_then(|root| vfs::walk(&root, "/tmp")) {
        Ok(t) => t,
        Err(e) => return r.fail(format_args!("{TRUNC_ID}: /tmp through the mount returned {}", errno_text(e))),
    };
    let u = splitmix(d ^ 0x5eed_0007);
    let uname = format!("{}.u", &path["/tmp/".len()..]);
    let uf = match tmp.create(&uname, 0o644) {
        Ok(f) => f,
        Err(e) => return r.fail(format_args!("{TRUNC_ID}: create(\"{uname}\") in /tmp returned {} (lab 18's call)", errno_text(e))),
    };
    for b in [1 + u % 10, 11, 12 + (u >> 4) % 1024, DIND_FIRST + (256 + (u >> 14) % 254) * 1024 + (u >> 24) % 1024] {
        match uf.write_at(b * 4096, &mix(u ^ b, 4096)) {
            Ok(4096) => {}
            Ok(n) => return r.fail(format_args!("{TRUNC_ID}: write_at of one block at file block {b} of /tmp/{uname} wrote {n} (lab 18's call)")),
            Err(e) => return r.fail(format_args!("{TRUNC_ID}: write_at at file block {b} of /tmp/{uname} returned {} (lab 18's call)", errno_text(e))),
        }
    }
    drop(uf);
    let k = 1 + (d >> 20) % 64;
    for (t, base) in [(0u64, 12u64), (1, DIND_FIRST), (2, DIND_FIRST + k * 1024)] {
        let name = format!("{}.s{t}", &path["/tmp/".len()..]);
        let file = format!("/tmp/{name}");
        let f = match tmp.create(&name, 0o644) {
            Ok(f) => f,
            Err(e) => return r.fail(format_args!("{TRUNC_ID}: create(\"{name}\") in /tmp returned {} (lab 18's call)", errno_text(e))),
        };
        let m = base + 2 + (d >> (24 + 8 * t)) % 1020;
        let e = splitmix(d ^ (0x5eed_0006 + t));
        let mut written: BTreeSet<u64> = [base - 1, base, base + 1, m - 1, m, base + 1023, base + 1024 + e % 1024].into_iter().collect();
        if t == 1 {
            written.insert(DIND_FIRST + (256 + (e >> 10) % 254) * 1024 + (e >> 20) % 1024);
        }
        let h = m + 1 + (e >> 30) % (base + 1022 - m);
        let mut cuts = alloc::vec![h * 4096 + 1 + (e >> 40) % 4095, m * 4096 + 1 + (e >> 52) % 4095, (base + 64) * 4096, (base + 2) * 4096, (base + 1) * 4096, base * 4096];
        cuts.sort_unstable_by(|a, b| b.cmp(a));
        cuts.dedup();
        let mut tails: BTreeMap<u64, usize> = BTreeMap::new();
        let bytes = |b: u64| mix(d ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15), 4096);
        let want = |b: u64, tails: &BTreeMap<u64, usize>| {
            let mut v = bytes(b);
            if let Some(&t) = tails.get(&b) {
                v[t..].fill(0);
            }
            v
        };
        for &b in &written {
            match f.write_at(b * 4096, &bytes(b)) {
                Ok(4096) => {}
                Ok(n) => return r.fail(format_args!("{TRUNC_ID}: write_at of one block at file block {b} of {file} wrote {n} (lab 18's call)")),
                Err(e) => return r.fail(format_args!("{TRUNC_ID}: write_at at file block {b} of {file} returned {} (lab 18's call)", errno_text(e))),
            }
        }
        let mut held = written.clone();
        let mut back = alloc::vec![0u8; 4096];
        for cut in cuts {
            let Some(mid) = got(r, alloc_state(raw)) else { return };
            let Some(owned) = owned_blocks(r, raw, &file, &held) else { return };
            let (x, tail) = (cut / 4096, (cut % 4096) as usize);
            if let Err(e) = f.truncate(cut) {
                return r.fail(format_args!("{TRUNC_ID}: truncate({cut}) of {file} (a cut in file block {x}) returned {}", errno_text(e)));
            }
            let kept: BTreeSet<u64> = held.iter().copied().filter(|&b| b < x || (tail > 0 && b == x)).collect();
            let gone = blocks_needed(&held) - blocks_needed(&kept);
            if let Some((_, i)) = raw_at(r, raw, &file) {
                r.expect_eq(&format!("{file}: i_blocks on the disk after truncate({cut}) ({} data blocks kept)", kept.len()), le32(&i, 28), 8 * blocks_needed(&kept));
                for &b in &written {
                    match got(r, raw.bmap(&i, b)) {
                        Some(0) if kept.contains(&b) => r.fail(format_args!("{TRUNC_ID}: {file}: file block {b} unmapped on the disk after truncate({cut}), which keeps it")),
                        Some(n) if !kept.contains(&b) && n != 0 => r.fail(format_args!("{TRUNC_ID}: {file}: file block {b} still maps block {n} on the disk after truncate({cut})")),
                        _ => {}
                    }
                }
            }
            if tail > 0 {
                // Grown back over block x: no block taken, the bytes past
                // the cut zeros (a hole reads as zeros too).
                let grown = (x + 1) * 4096;
                if let Err(e) = f.truncate(grown) {
                    return r.fail(format_args!("{TRUNC_ID}: truncate({grown}) of {file}, growing it back after truncate({cut}), returned {}", errno_text(e)));
                }
                if let Some((_, i)) = raw_at(r, raw, &file) {
                    r.expect_eq(&format!("{file}: i_blocks on the disk after truncate({cut}) and a grow back to {grown}"), le32(&i, 28), 8 * blocks_needed(&kept));
                }
                if kept.contains(&x) {
                    tails.insert(x, tail);
                }
                let expect = if kept.contains(&x) { want(x, &tails) } else { alloc::vec![0u8; 4096] };
                match f.read_at(x * 4096, &mut back) {
                    Ok(4096) => r.expect_bytes(&format!("{file}: file block {x} read back after truncate({cut}) and a grow back to {grown}"), &back, &expect),
                    Ok(n) => r.fail(format_args!("{TRUNC_ID}: read_at of file block {x} of {file} after a grow back to {grown} read {n} bytes, expected 4096")),
                    Err(e) => r.fail(format_args!("{TRUNC_ID}: read_at of file block {x} of {file} after a grow back to {grown} returned {}", errno_text(e))),
                }
            }
            for &b in kept.iter().filter(|&&b| tail == 0 || b != x) {
                match f.read_at(b * 4096, &mut back) {
                    Ok(4096) => r.expect_bytes(&format!("{file}: file block {b} read back after truncate({cut})"), &back, &want(b, &tails)),
                    Ok(n) => r.fail(format_args!("{TRUNC_ID}: read_at of file block {b} of {file} after truncate({cut}) read {n} bytes, expected 4096")),
                    Err(e) => r.fail(format_args!("{TRUNC_ID}: read_at of file block {b} of {file} after truncate({cut}) returned {}", errno_text(e))),
                }
            }
            if let Some(after) = got(r, alloc_state(raw)) {
                r.expect_eq(&format!("s_free_blocks_count after truncate({cut}) of {file}: {gone} more"), after.2[0], mid.2[0] + gone);
                r.expect_eq(&format!("bg_free_blocks_count after truncate({cut}) of {file}: {gone} more"), after.2[2], mid.2[2] + gone);
            }
            if !given_back(r, raw, &file, cut, &owned, &kept) {
                return;
            }
            held = kept;
        }
        if let Err(e) = f.truncate(0) {
            return r.fail(format_args!("{TRUNC_ID}: truncate(0) of {file} returned {}", errno_text(e)));
        }
        if let Some((_, i)) = raw_at(r, raw, &file) {
            r.expect_eq(&format!("{file}: i_blocks on the disk after truncate(0)"), le32(&i, 28), 0);
        }
    }
}

/// The blocks a file's tree holds on the disk for the file blocks `held`:
/// (what each is, its block number, the file blocks it serves).
fn owned_blocks(r: &mut Row, raw: &Raw, file: &str, held: &BTreeSet<u64>) -> Option<Vec<(String, u64, Vec<u64>)>> {
    let (_, i) = raw_at(r, raw, file)?;
    let mut out = Vec::new();
    for &b in held {
        out.push((format!("file block {b}"), got(r, raw.bmap(&i, b))?, alloc::vec![b]));
    }
    let ind: Vec<u64> = held.iter().copied().filter(|&b| (12..DIND_FIRST).contains(&b)).collect();
    if !ind.is_empty() {
        out.push((String::from("i_block[12]'s indirect block"), le32(&i, 88), ind));
    }
    let dind: Vec<u64> = held.iter().copied().filter(|&b| b >= DIND_FIRST).collect();
    if !dind.is_empty() {
        let top = le32(&i, 92);
        let blk = got(r, raw_block(top))?;
        let kids: BTreeSet<u64> = dind.iter().map(|&b| (b - DIND_FIRST) / 1024).collect();
        for c in kids {
            let under: Vec<u64> = dind.iter().copied().filter(|&b| (b - DIND_FIRST) / 1024 == c).collect();
            out.push((format!("the double indirect block's child {c}"), le32(&blk[..], 4 * c as usize), under));
        }
        out.push((String::from("i_block[13]'s double indirect block"), top, dind));
    }
    Some(out)
}

/// After a cut to `cut` bytes keeping `kept`: every block of `owned` that
/// serves no kept file block is clear in the block bitmap and named by no
/// pointer the file still holds. False (the row failed) if one is not.
fn given_back(r: &mut Row, raw: &Raw, file: &str, cut: u64, owned: &[(String, u64, Vec<u64>)], kept: &BTreeSet<u64>) -> bool {
    let was = r.failed;
    r.failed = false;
    let Some((_, i)) = raw_at(r, raw, file) else { return false };
    let Some((map, ..)) = got(r, alloc_state(raw)) else { return false };
    let top = le32(&i, 92);
    let dblk = if top != 0 { got(r, raw_block(top)) } else { None };
    for (what, n, serves) in owned {
        if *n == 0 || serves.iter().any(|b| kept.contains(b)) {
            continue;
        }
        if bit(&map, *n) {
            r.fail(format_args!("{TRUNC_ID}: {file}: {what} (block {n}) still taken in the block bitmap after truncate({cut}) gave it back"));
        }
        let named = match what.as_str() {
            "i_block[12]'s indirect block" => le32(&i, 88),
            "i_block[13]'s double indirect block" => top,
            _ if what.starts_with("the double indirect block's child ") => {
                let c = (serves[0] - DIND_FIRST) / 1024;
                dblk.as_ref().map_or(0, |b| le32(&b[..], 4 * c as usize))
            }
            _ => 0,
        };
        if named != 0 {
            r.fail(format_args!("{TRUNC_ID}: {file}: {what} is still named (block {named}) on the disk after truncate({cut}) gave it back; the file's later cuts are skipped"));
        }
    }
    let ok = !r.failed;
    r.failed |= was;
    ok
}

fn dind_case(r: &mut Row, raw: &Raw, path: &str, d: u64) {
    let name = &path["/tmp/".len()..];
    let tmp = match vfs::root().and_then(|root| vfs::walk(&root, "/tmp")) {
        Ok(t) => t,
        Err(e) => return r.fail(format_args!("{TRUNC_ID}: /tmp through the mount returned {}", errno_text(e))),
    };
    let f = match tmp.create(name, 0o644) {
        Ok(f) => f,
        Err(e) => return r.fail(format_args!("{TRUNC_ID}: create(\"{name}\") in /tmp returned {} (lab 18's call)", errno_text(e))),
    };
    let Some(before) = got(r, alloc_state(raw)) else { return };
    // Where the blocks go (fix round 2, sk19-forger-rv-1/-2), all from the
    // nonce: p0 in the double indirect block's first indirect child (kept
    // whole), p1 < p2 in child c (1..=64) with the cut between them, p3 in
    // child c + 1 (cut whole). The cut keeps p0, p1, children 0 and c and the
    // double indirect block; it gives back p2, p3 and child c + 1.
    let c = 1 + (d >> 8) % 64;
    let o1 = (d >> 16) % 1022;
    let o2 = o1 + 1 + (d >> 28) % (1023 - o1);
    let p0 = DIND_FIRST + (d >> 40) % 1024;
    let p1 = DIND_FIRST + c * 1024 + o1;
    let p2 = DIND_FIRST + c * 1024 + o2;
    let p3 = DIND_FIRST + (c + 1) * 1024 + (d >> 50) % 1024;
    let bytes: Vec<Vec<u8>> = (0..4u64).map(|k| mix(d ^ (0x1036 + k * 0x1000), 4096)).collect();
    for (k, at_block) in [p0, p1, p2, p3].into_iter().enumerate() {
        match f.write_at(at_block * 4096, &bytes[k]) {
            Ok(4096) => {}
            Ok(n) => return r.fail(format_args!("{TRUNC_ID}: write_at of one block at file block {at_block} of {path} wrote {n} (lab 18's call)")),
            Err(e) => return r.fail(format_args!("{TRUNC_ID}: write_at at file block {at_block} of {path} returned {} (lab 18's call)", errno_text(e))),
        }
        if k == 0 {
            if let Some((_, i)) = raw_at(r, raw, path) {
                if le32(&i, 92) == 0 || le32(&i, 28) != 24 {
                    return r.fail(format_args!("{TRUNC_ID}: {path} after a write at file block {at_block}: i_block[13] {}, i_blocks {} on the disk, expected a double indirect pointer and 24 (lab 18's write)", le32(&i, 92), le32(&i, 28)));
                }
            }
        }
    }
    let Some(mid) = got(r, alloc_state(raw)) else { return };
    let cut = (p1 + 1) * 4096;
    if let Err(e) = f.truncate(cut) {
        return r.fail(format_args!("{TRUNC_ID}: truncate({cut}) of {path} (a cut inside the double indirect range) returned {}", errno_text(e)));
    }
    if let Some((_, i)) = raw_at(r, raw, path) {
        r.expect(le32(&i, 92) != 0, &format!("{path}: i_block[13] on the disk kept after truncate({cut})"));
        r.expect_eq(&format!("{path}: i_size on the disk after truncate({cut})"), le32(&i, 4), cut);
        r.expect_eq(&format!("{path}: i_blocks on the disk after truncate({cut}) (two data blocks, two indirect blocks, the double indirect block)"), le32(&i, 28), 40);
        for at_block in [p0, p1] {
            if let Some(b) = got(r, raw.bmap(&i, at_block)) {
                r.expect(b != 0, &format!("{path}: file block {at_block} still mapped on the disk after truncate({cut})"));
            }
        }
        for at_block in [p2, p3] {
            if let Some(b) = got(r, raw.bmap(&i, at_block)) {
                r.expect_eq(&format!("{path}: file block {at_block} mapped on the disk after truncate({cut})"), b, 0);
            }
        }
    }
    if let Some(after) = got(r, alloc_state(raw)) {
        r.expect_eq(&format!("s_free_blocks_count after truncate({cut}) of {path}: 3 more"), after.2[0], mid.2[0] + 3);
        r.expect_eq(&format!("bg_free_blocks_count after truncate({cut}) of {path}: 3 more"), after.2[2], mid.2[2] + 3);
    }
    let mut back = alloc::vec![0u8; 4096];
    for (k, at_block) in [p0, p1].into_iter().enumerate() {
        match f.read_at(at_block * 4096, &mut back) {
            Ok(4096) => r.expect_bytes(&format!("{path}: file block {at_block} read back after truncate({cut})"), &back, &bytes[k]),
            Ok(n) => r.fail(format_args!("{TRUNC_ID}: read_at of file block {at_block} of {path} after truncate({cut}) read {n} bytes, expected 4096")),
            Err(e) => r.fail(format_args!("{TRUNC_ID}: read_at of file block {at_block} of {path} after truncate({cut}) returned {}", errno_text(e))),
        }
    }
    // Fix round 3 (sk19-forger-rv2-1/-2). A dense cut inside child c: p1+1
    // and p1+2 written, the file cut to p1+2 blocks, so the first block past
    // the cut is the one to go. Then a cut exactly at child c+1's first
    // block, with a block written at a nonce offset in it: that block and
    // child c+1 go, child c stays whole.
    let (q1, q2) = (p1 + 1, p1 + 2);
    let qb = DIND_FIRST + (c + 1) * 1024 + (d >> 34) % 1024;
    // Fix round 4 (sk19-ladder-rv3-1): nothing past the cut may still be
    // mapped before the next writes, or they would go through a pointer
    // block the file gave back, or take a block it still names; the case
    // stops there with the row red.
    if let Some((_, i)) = raw_at(r, raw, path) {
        // The double indirect block's entry for child c + 1, cut whole, is
        // 0 (sk19-forger-rv4-4: a child zeroed but still named).
        if let Some(blk) = got(r, raw_block(le32(&i, 92))) {
            let e = le32(&blk[..], (c as usize + 1) * 4);
            if e != 0 {
                return r.fail(format_args!("{TRUNC_ID}: {path}: the double indirect block's entry {} still names block {e} on the disk after the cut to {} blocks gave that child back; the file's later cuts are skipped", c + 1, p1 + 1));
            }
        }
        for at_block in [p2, p3, q1, q2, qb] {
            match got(r, raw.bmap(&i, at_block)) {
                Some(0) => {}
                Some(b) => return r.fail(format_args!("{TRUNC_ID}: {path}: file block {at_block}, past the cut to {} blocks, still maps block {b} on the disk; the file's later cuts are skipped", p1 + 1)),
                None => return,
            }
        }
    }
    for (at_block, k) in [(q1, 2), (q2, 3)] {
        match f.write_at(at_block * 4096, &bytes[k]) {
            Ok(4096) => {}
            Ok(n) => return r.fail(format_args!("{TRUNC_ID}: write_at of one block at file block {at_block} of {path} wrote {n} (lab 18's call)")),
            Err(e) => return r.fail(format_args!("{TRUNC_ID}: write_at at file block {at_block} of {path} returned {} (lab 18's call)", errno_text(e))),
        }
    }
    let Some(mid) = got(r, alloc_state(raw)) else { return };
    let cut = q2 * 4096;
    if let Err(e) = f.truncate(cut) {
        return r.fail(format_args!("{TRUNC_ID}: truncate({cut}) of {path} (a cut between two neighbouring blocks) returned {}", errno_text(e)));
    }
    if let Some((_, i)) = raw_at(r, raw, path) {
        r.expect_eq(&format!("{path}: i_blocks on the disk after truncate({cut}) (three data blocks, two indirect blocks, the double indirect block)"), le32(&i, 28), 48);
        if let Some(b) = got(r, raw.bmap(&i, q1)) {
            r.expect(b != 0, &format!("{path}: file block {q1} still mapped on the disk after truncate({cut})"));
        }
        if let Some(b) = got(r, raw.bmap(&i, q2)) {
            r.expect_eq(&format!("{path}: file block {q2} mapped on the disk after truncate({cut})"), b, 0);
        }
    }
    if let Some(after) = got(r, alloc_state(raw)) {
        r.expect_eq(&format!("s_free_blocks_count after truncate({cut}) of {path}: 1 more"), after.2[0], mid.2[0] + 1);
        r.expect_eq(&format!("bg_free_blocks_count after truncate({cut}) of {path}: 1 more"), after.2[2], mid.2[2] + 1);
    }
    match f.write_at(qb * 4096, &bytes[3]) {
        Ok(4096) => {}
        Ok(n) => return r.fail(format_args!("{TRUNC_ID}: write_at of one block at file block {qb} of {path} wrote {n} (lab 18's call)")),
        Err(e) => return r.fail(format_args!("{TRUNC_ID}: write_at at file block {qb} of {path} returned {} (lab 18's call)", errno_text(e))),
    }
    let Some(mid) = got(r, alloc_state(raw)) else { return };
    let cut = (DIND_FIRST + (c + 1) * 1024) * 4096;
    if let Err(e) = f.truncate(cut) {
        return r.fail(format_args!("{TRUNC_ID}: truncate({cut}) of {path} (a cut at an indirect block's first block) returned {}", errno_text(e)));
    }
    if let Some((_, i)) = raw_at(r, raw, path) {
        r.expect_eq(&format!("{path}: i_blocks on the disk after truncate({cut}) (three data blocks, two indirect blocks, the double indirect block)"), le32(&i, 28), 48);
        if let Some(b) = got(r, raw.bmap(&i, qb)) {
            r.expect_eq(&format!("{path}: file block {qb} mapped on the disk after truncate({cut})"), b, 0);
        }
        if let Some(b) = got(r, raw.bmap(&i, q1)) {
            r.expect(b != 0, &format!("{path}: file block {q1} still mapped on the disk after truncate({cut})"));
        }
    }
    if let Some(after) = got(r, alloc_state(raw)) {
        r.expect_eq(&format!("s_free_blocks_count after truncate({cut}) of {path}: 2 more"), after.2[0], mid.2[0] + 2);
        r.expect_eq(&format!("bg_free_blocks_count after truncate({cut}) of {path}: 2 more"), after.2[2], mid.2[2] + 2);
    }
    // Fix round 4 (sk19-forger-rv3-1): cuts at exactly the first block of a
    // top pointer tree. Blocks 0 and 12 written, the file cut to 1036 blocks
    // (all of i_block[13]'s tree goes: three data blocks, two indirect, the
    // double indirect), then to 12 blocks (block 12 and i_block[12]'s block).
    for at_block in [0u64, 12] {
        match f.write_at(at_block * 4096, &bytes[0]) {
            Ok(4096) => {}
            Ok(n) => return r.fail(format_args!("{TRUNC_ID}: write_at of one block at file block {at_block} of {path} wrote {n} (lab 18's call)")),
            Err(e) => return r.fail(format_args!("{TRUNC_ID}: write_at at file block {at_block} of {path} returned {} (lab 18's call)", errno_text(e))),
        }
    }
    for (blocks, slot, left, freed) in [(DIND_FIRST, 13usize, 24u64, 6u64), (12, 12, 8, 2)] {
        let Some(mid) = got(r, alloc_state(raw)) else { return };
        let cut = blocks * 4096;
        if let Err(e) = f.truncate(cut) {
            return r.fail(format_args!("{TRUNC_ID}: truncate({cut}) of {path} (a cut at the first block under i_block[{slot}]) returned {}", errno_text(e)));
        }
        if let Some((_, i)) = raw_at(r, raw, path) {
            r.expect_eq(&format!("{path}: i_block[{slot}] on the disk after truncate({cut})"), le32(&i, 40 + 4 * slot), 0);
            r.expect_eq(&format!("{path}: i_blocks on the disk after truncate({cut})"), le32(&i, 28), left);
        }
        if let Some(after) = got(r, alloc_state(raw)) {
            r.expect_eq(&format!("s_free_blocks_count after truncate({cut}) of {path}: {freed} more"), after.2[0], mid.2[0] + freed);
            r.expect_eq(&format!("bg_free_blocks_count after truncate({cut}) of {path}: {freed} more"), after.2[2], mid.2[2] + freed);
        }
    }
    // Both top trees again, for the cut to nothing below.
    for at_block in [12, p0] {
        match f.write_at(at_block * 4096, &bytes[0]) {
            Ok(4096) => {}
            Ok(n) => return r.fail(format_args!("{TRUNC_ID}: write_at of one block at file block {at_block} of {path} wrote {n} (lab 18's call)")),
            Err(e) => return r.fail(format_args!("{TRUNC_ID}: write_at at file block {at_block} of {path} returned {} (lab 18's call)", errno_text(e))),
        }
    }
    if let Err(e) = f.truncate(0) {
        return r.fail(format_args!("{TRUNC_ID}: truncate(0) of {path} (a block through the double indirect) returned {}", errno_text(e)));
    }
    if let Some((_, i)) = raw_at(r, raw, path) {
        r.expect_eq(&format!("{path}: i_block[12] on the disk after truncate(0)"), le32(&i, 88), 0);
        r.expect_eq(&format!("{path}: i_block[13] on the disk after truncate(0)"), le32(&i, 92), 0);
        r.expect_eq(&format!("{path}: i_blocks on the disk after truncate(0) (the data, indirect and double indirect blocks given back)"), le32(&i, 28), 0);
    }
    if let Some(after) = got(r, alloc_state(raw)) {
        same_state(r, &format!("after truncate(0) of {path} through the double indirect"), &after, &before, false);
    }
}

// ── Row 2: ext2.unlink.frees ─────────────────────────────────────────────

fn unlink_row(raw: &Raw, trunc: &str, left: &mut Left) {
    let mut r = Row::new(UNLINK_ID);
    let Some(before) = got(&mut r, alloc_state(raw)) else { return r.finish(2) };
    let data = mix(0x5B00_0019_0002, 60_000);
    let i = A::I;
    let mut pl = Plan::new();
    let (a, b) = (pl.p.str("/tmp/u19a"), pl.p.str("/tmp/u19b"));
    let off = pl.p.bytes(&data);
    let fa = pl.fd("open(/tmp/u19a, O_WRONLY|O_CREAT|O_EXCL)", SYS_OPEN, &[a, i(O_WRONLY | O_CREAT | O_EXCL), i(0o644)]);
    pl.is("write of 60000 bytes", SYS_WRITE, &[A::R(fa), A::D(off), i(60_000)], 60_000);
    pl.is("close", SYS_CLOSE, &[A::R(fa)], 0);
    let fb = pl.fd("open(/tmp/u19b, O_WRONLY|O_CREAT|O_EXCL)", SYS_OPEN, &[b, i(O_WRONLY | O_CREAT | O_EXCL), i(0o600)]);
    pl.is("close", SYS_CLOSE, &[A::R(fb)], 0);
    pl.run(&mut r, "u19a");
    let ia = raw_at(&mut r, raw, "/tmp/u19a").map(|x| (x.0, le32(&x.1, 12)));
    // The guest clock is moved forward before the unlink to i_ctime + 2 + a
    // number of seconds (0..600) from the PM timer's low bits (fix rounds 2
    // and 3: sk19-ladder-rv-1, sk19-forger-rv2-4/-5), so a deletion time of
    // now is far from i_ctime, and no stand-in a fixed distance from it fits.
    // The jump runs on this thread between learner operations (every
    // program above has ended, no disk command is open, the vehicle's own
    // bounds are on the PM timer), and its size is printed only after the
    // row has read i_dtime back (TPM interim 18, the carried-bytes rule).
    let mut clock_stuck: Option<u64> = None;
    if let Some((_, ctime)) = ia {
        let to = ctime + 2 + (pm() as u64 >> 4) % 600;
        if !clock_forward(to) {
            r.failed = true;
            clock_stuck = Some(to);
        }
    }
    raw_at(&mut r, raw, "/tmp/u19b");
    let tail = format!("{trunc}/");
    let mut pl = Plan::new();
    let (a, b, tmp, log, none, file_, hi) = (pl.p.str("/tmp/u19a"), pl.p.str("/tmp/u19b"), pl.p.str("/tmp"), pl.p.str("/.sboot-log"), pl.p.str("/tmp/nothere"), pl.p.str(&tail), pl.p.str("/boot/hi"));
    pl.is("unlink(/tmp) (a directory)", SYS_UNLINK, &[tmp], -EISDIR);
    pl.is("unlink(/.sboot-log)", SYS_UNLINK, &[log], -EPERM);
    pl.is("unlink(/tmp/nothere)", SYS_UNLINK, &[none], -ENOENT);
    pl.is("unlink(<row 1's file>/)", SYS_UNLINK, &[file_], -ENOTDIR);
    pl.is("unlink(/boot/hi)", SYS_UNLINK, &[hi], -EROFS);
    pl.is("unlink(/tmp/u19a)", SYS_UNLINK, &[a], 0);
    pl.is("open(/tmp/u19a) after its unlink", SYS_OPEN, &[a, i(O_RDONLY)], -ENOENT);
    pl.is("unlink(/tmp/u19a) again", SYS_UNLINK, &[a], -ENOENT);
    pl.is("unlink(/tmp/u19b) (empty)", SYS_UNLINK, &[b], 0);
    let t0 = guest_now();
    pl.run(&mut r, "u19b");
    let t1 = guest_now();
    if let Some(after) = got(&mut r, alloc_state(raw)) {
        same_state(&mut r, "after the unlinks", &after, &before, true);
    }
    for name in ["u19a", "u19b"] {
        if let Some(Some(_)) = got(&mut r, raw.walk(&format!("/tmp/{name}"))) {
            r.fail(format_args!("{UNLINK_ID}: /tmp/{name} is still a record of /tmp on the disk"));
        }
        left.gone.push(format!("/tmp/{name}"));
    }
    if let Some((ia, ctime)) = ia {
        if let Some(i) = got(&mut r, raw.inode(ia)) {
            r.expect_eq("/tmp/u19a's inode on the disk after its unlink: i_links_count", links(&i), 0);
            r.expect_eq("/tmp/u19a's inode on the disk after its unlink: i_blocks", le32(&i, 28), 0);
            r.expect(le32(&i, 20) != 0, "/tmp/u19a's inode on the disk after its unlink: i_dtime set");
            let dtime = le32(&i, 20);
            r.expect(dtime > ctime && dtime >= t0 && dtime <= t1, &format!("/tmp/u19a's inode on the disk after its unlink: i_dtime {dtime} after its i_ctime {ctime} and within the guest time around the unlink, {t0}..={t1}"));
        }
    }
    if let Some(to) = clock_stuck {
        note(format_args!("{UNLINK_ID}: the guest clock (EPOCH + whole seconds of time::now_ns) did not reach the middle of second {to} with time::TICKS moved forward"));
    }
    first_in_block(&mut r, raw);
    // One file that stays, for row 3.
    let keep = mix(0x5B00_0019_0003, 3 * 4096 + 77);
    let mut pl = Plan::new();
    let k = pl.p.str("/tmp/u19k");
    let off = pl.p.bytes(&keep);
    let fk = pl.fd("open(/tmp/u19k, O_WRONLY|O_CREAT|O_EXCL)", SYS_OPEN, &[k, i(O_WRONLY | O_CREAT | O_EXCL), i(0o644)]);
    pl.is("write of its bytes", SYS_WRITE, &[A::R(fk), A::D(off), i(keep.len() as u64)], keep.len() as i64);
    pl.is("close", SYS_CLOSE, &[A::R(fk)], 0);
    pl.run(&mut r, "u19c");
    left.files.push((String::from("/tmp/u19k"), keep));
    left.files.push((String::from(trunc), Vec::new()));
    r.finish(2);
}

/// Row 2's first-record case (fix round 1, O-2): names made in /tmp
/// through the learner's `create` until the directory takes a new block,
/// then the record at byte 0 of that block unlinked through the learner's
/// `unlink` — it must be gone from the disk — and the other names after it.
fn first_in_block(r: &mut Row, raw: &Raw) {
    let tmp = match at("/tmp") {
        Ok(t) => t,
        Err(why) => return r.fail(format_args!("{UNLINK_ID}: {why}")),
    };
    let size = |r: &mut Row| raw_at(r, raw, "/tmp").map(|(_, i)| le32(&i, 4));
    let Some(s0) = size(r) else { return };
    let mut made: Vec<String> = Vec::new();
    loop {
        if made.len() == 40 {
            return r.fail(format_args!("{UNLINK_ID}: 40 names of 242 bytes made in /tmp and its size on the disk is still {s0}"));
        }
        let name = format!("u19f{:02}{}", made.len(), "x".repeat(236));
        match tmp.create(&name, 0o644) {
            Ok(_) => made.push(name),
            Err(e) => return r.fail(format_args!("{UNLINK_ID}: create of a 242-byte name in /tmp returned {} (lab 18's call)", errno_text(e))),
        }
        match size(r) {
            Some(s) if s > s0 => break,
            Some(_) => {}
            None => return,
        }
    }
    let Some((dir, di)) = raw_at(r, raw, "/tmp") else { return };
    let last = le32(&di, 4) / 4096 - 1;
    let Some(blk) = got(r, raw.bmap(&di, last).and_then(raw_block)) else { return };
    let (fino, nl) = (le32(&blk[..], 0), blk[6] as usize);
    let first = String::from_utf8_lossy(&blk[8..8 + nl]).into_owned();
    if fino == 0 || !made.contains(&first) {
        return r.fail(format_args!("{UNLINK_ID}: the record at byte 0 of /tmp's new block on the disk is not one of the names just made"));
    }
    match tmp.unlink(&first) {
        Ok(()) => {
            if let Some(Some(_)) = got(r, raw.find(dir, &first)) {
                r.fail(format_args!("{UNLINK_ID}: a name whose record is the first of its directory block is still a record of /tmp on the disk after its unlink"));
            }
        }
        Err(e) => r.fail(format_args!("{UNLINK_ID}: unlink of the name first in /tmp's new block returned {}", errno_text(e))),
    }
    for name in made.iter().filter(|n| **n != first) {
        if let Err(e) = tmp.unlink(name) {
            return r.fail(format_args!("{UNLINK_ID}: unlink of a 242-byte name in /tmp returned {}", errno_text(e)));
        }
    }
}

// ── Row 3: ext2.remount.persists ─────────────────────────────────────────

/// A whole file through the learner's `read_at`, 4 KiB at a time.
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

fn remount_row(spy: &Arc<Spy>, proxy: &Proxy, fs: Arc<Ext2>, left: &Left) -> Option<Arc<Ext2>> {
    let mut r = Row::new(REMOUNT_ID);
    if let Err(e) = fs.sync() {
        r.fail(format_args!("{REMOUNT_ID}: Ext2::sync() returned {}", errno_text(e)));
    }
    let old = Arc::downgrade(&fs);
    drop(fs);
    proxy.set(None);
    if old.upgrade().is_some() {
        r.fail(format_args!("{REMOUNT_ID}: with / unmounted and every inode let go, the first Ext2 is still held ({} references)", old.strong_count()));
    }
    let fs2 = match mount_fs(spy).and_then(|fs| fs.root().map(|root| (fs, root)).map_err(|e| format!("Ext2::root() returned {}", errno_text(e)))) {
        Ok((fs, root)) => {
            proxy.set(Some(root));
            fs
        }
        Err(why) => {
            r.fail(format_args!("{REMOUNT_ID}: the second mount: {why}"));
            r.finish(3);
            return None;
        }
    };
    let reads0 = spy.reads.load(Ordering::Relaxed);
    let mut blocks = 0u64;
    for (path, want) in &left.files {
        match vfs::root().and_then(|root| vfs::walk(&root, path)).and_then(|ino| read_whole(&ino)) {
            Ok(bytes) => r.expect_bytes(&format!("{path} read through the second mount"), &bytes, want),
            Err(e) => r.fail(format_args!("{REMOUNT_ID}: {path} through the second mount: {}", errno_text(e))),
        }
        blocks += (want.len() as u64).div_ceil(4096);
    }
    let reads = spy.reads.load(Ordering::Relaxed) - reads0;
    if reads < blocks {
        r.fail(format_args!("{REMOUNT_ID}: the read-back of {blocks} data blocks reached the drive {reads} times: some came from memory"));
    }
    for path in &left.gone {
        match vfs::root().and_then(|root| vfs::walk(&root, path)) {
            Err(e) if e == ENOENT as Errno => {}
            Err(e) => r.fail(format_args!("{REMOUNT_ID}: {path} through the second mount: {}, expected -ENOENT", errno_text(e))),
            Ok(_) => r.fail(format_args!("{REMOUNT_ID}: {path} is there again after the remount")),
        }
    }
    note(format_args!("{REMOUNT_ID}: {} files ({blocks} blocks) read back through a second mount, {reads} drive reads", left.files.len()));
    r.finish(3);
    Some(fs2)
}


// ── Row 4: txn.delete.metadata.write ─────────────────────────────────────

fn txn_row(raw: &Raw, control: Vec<String>, fs: Option<&Arc<Ext2>>) {
    let mut r = Row::new(TXN_ID);
    for why in control {
        r.fail(format_args!("{TXN_ID}: the log's control: {why}"));
    }
    let w = writes();
    let unlisted: Vec<u64> = w.iter().filter(|(b, listed)| *b < raw.meta_end() && !listed).map(|(b, _)| *b).collect();
    if !unlisted.is_empty() {
        r.fail(format_args!("{TXN_ID}: rows 1–3: {} writes of the superblock, the descriptor, a bitmap or the inode table reached the disk unlisted, first fs blocks {:?}", unlisted.len(), &unlisted[..unlisted.len().min(6)]));
    }
    note(format_args!("{TXN_ID}: rows 1–3 wrote {} blocks, {} of them the file system's own", w.len(), w.iter().filter(|(b, _)| *b < raw.meta_end()).count()));
    if fs.is_none() {
        r.fail(format_args!("{TXN_ID}: no file system to run the operations on (row 3)"));
        return r.finish(4);
    }
    let Ok(tmp) = vfs::root().and_then(|root| vfs::walk(&root, "/tmp")) else {
        r.fail(format_args!("{TXN_ID}: /tmp not found through the mount"));
        return r.finish(4);
    };
    // Made (watched too, not counted), then cut inside its indirect block
    // and deleted: those two must write all seven kinds.
    let (mut made, mut seen) = ([false; 7], [false; 7]);
    let mut q: Option<InodeRef> = None;
    watched(&mut r, raw, &mut made, "create(\"q-txn\") in /tmp", || tmp.create("q-txn", 0o644).map(|i| q = Some(i)));
    let data = mix(0x5B00_0019_0005, 15 * 4096);
    if let Some(q) = &q {
        watched(&mut r, raw, &mut made, "write_at of 15 blocks (into the indirect block)", || match q.write_at(0, &data) {
            Ok(n) if n == data.len() => Ok(()),
            Ok(_) => Err(EIO as Errno),
            Err(e) => Err(e),
        });
        watched(&mut r, raw, &mut seen, "truncate(13 blocks + 1) (a pointer cleared in the indirect block)", || q.truncate(13 * 4096 + 1));
        watched(&mut r, raw, &mut seen, "unlink(\"q-txn\") in /tmp", || tmp.unlink("q-txn"));
    } else {
        r.fail(format_args!("{TXN_ID}: the operations stopped early (above)"));
    }
    for (k, s) in seen.iter().enumerate() {
        if !s {
            r.fail(format_args!("{TXN_ID}: no listed write of {} in the truncate and the unlink: nothing shows the log saw one", KINDS[k]));
        }
    }
    r.finish(4);
}

// ── Row 5: ext2.freemaps.agree ───────────────────────────────────────────

fn free_row(raw: &Raw, control: Result<Vec<String>, String>, trunc: &str, mounted: bool) {
    let mut r = Row::new(FREE_ID);
    match control {
        Ok(v) if v.is_empty() => {}
        Ok(v) => r.fail(format_args!("{FREE_ID}: this file's check of the disk as written, before the mount, disagrees (the test is wrong): {}", v.join("; "))),
        Err(why) => r.fail(format_args!("{FREE_ID}: the check before the mount: {why}")),
    }
    // Fix round 7 (sk19-forger-rv6-1/-2): row 1's sparse file (a hole
    // before its last direct block, an indirect block, a double indirect
    // child), unlinked on the second mount; the walk below then holds every
    // block it had, pointer blocks too, to the bitmap.
    let uname = format!("{}.u", &trunc["/tmp/".len()..]);
    if mounted {
        match vfs::root().and_then(|root| vfs::walk(&root, "/tmp")) {
            Ok(tmp) => {
                if tmp.lookup(&uname).is_ok() {
                    if let Err(e) = tmp.unlink(&uname) {
                        r.fail(format_args!("{FREE_ID}: unlink(\"{uname}\") in /tmp on the second mount (a file with holes and blocks under both pointer trees) returned {}", errno_text(e)));
                    }
                } else {
                    note(format_args!("{FREE_ID}: /tmp/{uname} is not there to unlink (row 1 stopped before making it)"));
                }
            }
            Err(e) => r.fail(format_args!("{FREE_ID}: /tmp on the second mount returned {}", errno_text(e))),
        }
    }
    match scan(raw) {
        Ok(s) => {
            match agree(raw, &s) {
                Ok(v) => {
                    for why in &v {
                        r.fail(format_args!("{FREE_ID}: {why}"));
                    }
                }
                Err(why) => r.fail(format_args!("{FREE_ID}: {why}")),
            }
            note(format_args!("{FREE_ID}: the tree holds {} inodes, {} blocks, {} directories", s.inodes.len(), s.used.len(), s.dirs));
        }
        Err(why) => r.fail(format_args!("{FREE_ID}: {why}")),
    }
    r.finish(5);
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
    for (k, id) in [TRUNC_ID, UNLINK_ID, REMOUNT_ID, TXN_ID, FREE_ID].into_iter().enumerate().skip(from - 1) {
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
    // The disk as written must agree with itself: row 5's control.
    let pre = scan(&raw).and_then(|s| agree(&raw, &s));
    // The log, before any mount (row 4's control).
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
    let proxy = Arc::new(Proxy(SpinLock::new(Some(root))));
    mount_rest(proxy.clone());
    note(format_args!("vfs: / is the test's root over ext2 on partition 2, /boot the boot archive ({ARCHIVE_LEN} bytes), /ktest the test's programs"));
    let mut left = Left { files: Vec::new(), gone: Vec::new() };
    // The PM counter wraps every ~4.69 s, so the clock is read after each
    // row (sk19-ladder-rv2-4); a single row past that still under-counts.
    c.us();
    let trunc = truncate_row(&raw);
    c.us();
    unlink_row(&raw, &trunc, &mut left);
    c.us();
    let fs2 = remount_row(&spy, &proxy, fs, &left);
    c.us();
    txn_row(&raw, control, fs2.as_ref());
    c.us();
    free_row(&raw, pre, &trunc, fs2.is_some());
    drop(fs2);
    note(format_args!("the drive saw {} reads and {} writes", spy.reads.load(Ordering::Relaxed), writes().len()));
    sched::yield_now();
    quiet(ktap::end);
    halt()
}
