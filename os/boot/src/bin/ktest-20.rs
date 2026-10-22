//! Lab 20's vehicle: ext2's directories — listed (`getdents`, syscall 217),
//! made (`mkdir`), removed (`rmdir`) and renamed (`rename`) — with the log's
//! pass-through around each. The prelude is lab 17's. Cut on 2026-10-19
//! (slice 5a) from the old lab 18's vehicle (its directory rows) and lab
//! 17's (its `getdents64` row, which moved here with the syscall).
//!
//! The disk is `ktest-20.img` as the course's image writer made it (CONTRACT
//! §A.7); the course boots a copy. As at 18, every expectation comes from
//! this file's formulas or from its own reads of the disk — its own PIO
//! driver and its own decoder of the superblock, the descriptor, both
//! bitmaps, the inode table, the block maps and the directory records.
//! Nothing the learner's ext2 reports is used as an expectation.
//!
//! The drive sits under `Spy` (`../txnspy.rs`, shared with 18 and 19). `/` forwards every
//! call to the root of the learner's `Ext2::mount(partition2(spy))`;
//! `/boot` is the boot archive, `/ktest` this file's programs. The first
//! call of this lab's is row 1's first `getdents`. Four rows, in this order:
//!
//!   1 `ext2.getdents.records` (derived; s20 fix rounds 1 and 2,
//!     2026-10-19) — before the mount, this file writes `/tmp`'s first
//!     block itself: three files named from the run's nonce and a word no
//!     one knows before this boot (the TSC and the PM timer; CONTRACT
//!     §A.1-5) — the first and the third empty files of inodes of their
//!     own that it allocates (round 2b), the second a hard link to
//!     `/pattern` (its links 1 → 2) — with a record of inode 0 between the
//!     first and the second, as a removal leaves one. Then the
//!     learner's `Inode::getdents` called directly on `/`, on `/t` (302
//!     records in two blocks) and on `/tmp` from index 0 until `None`: every
//!     live record, in the order the disk holds them, with its inode and
//!     `d_type`, and nothing else (the record of inode 0 neither listed nor
//!     ending its block's walk); `-ENOTDIR` from the method on a file. Then
//!     from ring 3 (lab 17's row, moved): `getdents64` of `/t` in 4096-byte
//!     calls until 0, every record parsed (`d_reclen` a multiple of 8, the
//!     NUL inside it, each `d_off` past the one before) and the set equal to
//!     the disk's; `/` in one call, then 0; `-ENOTDIR` on a regular file's
//!     fd and on a pipe, `-EINVAL` when no record fits; a d_off resumes
//!     (s20 fix round 0, 2026-10-19): one record from the middle of `/t`
//!     (40 bytes after 4096), then a second fd of `/t` moved by `lseek` to
//!     that record's `d_off` hands out the record after it on the disk
//!     first, and `lseek` returns the `d_off`; `/tmp` through `getdents64`
//!     holds the three names. Then `/bin/ls -a / /a/b/c`, exec'd from the
//!     disk (its `.bss` is 8 pages): its output the two directories as the
//!     disk holds them, sorted; exit 0. The names are kept in this file and
//!     printed only in a failure, after the row has read them back.
//!   2 `ext2.mkdir.rmdir` (plain) — `mkdir` of `/tmp/d1` and `/tmp/d1/d2/`
//!     (a trailing `/`), a file in each; refused: `mkdir` of a name that is
//!     there, of `.` and of a file's name (`-EEXIST`; the file case s20 fix
//!     round 1), under a file (`-ENOTDIR`), on the
//!     archive (`-EROFS`). On the disk: each inode's mode, links and size,
//!     `/tmp`'s links one more, each new directory's records `.` (itself),
//!     `..` (its parent) and its entries. Then `rmdir` refused: not empty
//!     (`-ENOTEMPTY`), a file (`-ENOTDIR`), `.` (`-EINVAL`), `..`
//!     (`-ENOTEMPTY`), the log (`-ENOTDIR`), a missing name (`-ENOENT`); the
//!     files unlinked (lab 19's), both directories removed (`rmdir` again
//!     `-ENOENT`). Both bitmaps, the five counts (the directory count among
//!     them) and `/tmp`'s links byte for byte as before the row.
//!   3 `ext2.rename` (derived) — two directories and three files of
//!     splitmix64 of the nonce, each opening with an unpublished word (fix
//!     round 2, §A.1-5); twenty-four renames, twenty-seven by fix round 6 (fix round 1 added a
//!     directory over an empty directory, freed whole; fix round 2 made the
//!     rename into its own subtree 3–8 levels deep, chosen from the nonce,
//!     and added the same rename spelled through `..`, both `-EINVAL`; fix
//!     round 3 added it with no path, the learner's `Inode::rename` called
//!     directly, `-EINVAL` and nothing changed; fix round 4 named the
//!     subtree's directories with unpublished words, called it directly
//!     with `new_dir` at the chain's bottom and at a nonce-chosen level, and
//!     added two moves that must succeed, a directory into the chain, two
//!     or more levels from its parent and outside its own subtree, once by
//!     `rename` and once by `Inode::rename`; fix round 5 gave every one of
//!     those names one shape, `d` and the word, gave each moved directory a
//!     child directory whose `..` must still name it, and made one of the
//!     two, drawn from the nonce, before the chain and the other after;
//!     fix round 6 refused it into itself, by `rename` and directly, moved
//!     a directory into the early mover's child before the mover moved, and
//!     then refused it into that child, by `rename` and directly): a
//!     file across
//!     directories, over another file (the replaced inode's links and
//!     `i_blocks` 0, `i_dtime` set, its inode bit and every block of it
//!     clear), a directory across directories (its `..` rewritten), a
//!     directory named with a trailing `/` on both sides (and back);
//!     refused (s20 fix round 0, 2026-10-19): a file named with a trailing
//!     `/` on either side (`-ENOTDIR`), `.` or `..` as either name
//!     (`-EINVAL`, §A.4's pin); refused
//!     into its own subtree (`-EINVAL`), a file over a directory
//!     (`-EISDIR`), a directory over a file (`-ENOTDIR`), over a non-empty
//!     directory (`-ENOTEMPTY`), to `/boot` (`-EXDEV`), the log either way
//!     (`-EPERM`), a missing name (`-ENOENT`); onto itself (0). The moved
//!     file's bytes read back; on the disk, both directories' records, the
//!     link counts, no moved name left behind; then this file's walk of the
//!     whole tree against both bitmaps and every count (lab 18's check, its
//!     control the disk before the mount).
//!   4 `txn.dirs.metadata.write` (plain) — the log's control (`txnspy.rs`),
//!     then every write of rows 1–3 to the superblock, the descriptor, the
//!     bitmaps or the inode table listed when it reached the disk; then two
//!     `mkdir`s, two `rename`s (a file, a directory) and two `rmdir`s on the
//!     learner's `Inode` methods, each between two full reads of the tree by
//!     this file: together they must write six kinds (all but a pointer
//!     block), every one listed. A self-report, graded by this row alone:
//!     no later row runs mkdir, rmdir or rename inside a swept transaction
//!     (CONTRACT §B.2-20, corrected 2026-10-19).
//!
//! The course also runs `e2fsck -fn` on the copy's partition 2 after the
//! boot (measured, not graded). Not graded: `rename` from the archive into
//! ext2 answers `-EROFS` (the archive refuses first), not `-EXDEV`.

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
const DENTS_ID: &str = "ext2.getdents.records";
const MKDIR_ID: &str = "ext2.mkdir.rmdir";
const RENAME_ID: &str = "ext2.rename";
const TXN_ID: &str = "txn.dirs.metadata.write";

const SYS_GETDENTS64: u64 = 217;
const O_DIRECTORY: u64 = 0x1_0000;

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
const SYS_LSEEK: u64 = 8;
const SYS_RENAME: u64 = 82;
const SYS_MKDIR: u64 = 83;
const SYS_RMDIR: u64 = 84;
const SYS_UNLINK: u64 = 87;
const O_RDONLY: u64 = 0;
const O_WRONLY: u64 = 1;
const O_CREAT: u64 = 0x40;
const O_EXCL: u64 = 0x80;
const O_NONBLOCK: u32 = 0x800;
const EPERM: i64 = 1;
const ENOENT: i64 = 2;
const EEXIST: i64 = 17;
const EXDEV: i64 = 18;
const ENOTDIR: i64 = 20;
const EISDIR: i64 = 21;
const EINVAL: i64 = 22;
const EROFS: i64 = 30;
const ENOTEMPTY: i64 = 39;
const EIO: i64 = 5;

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
        16 => "EBUSY",
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

fn out16(port: u16, v: u16) {
    // SAFETY: a 16-bit port write of the primary ATA bus; no memory touched.
    unsafe { asm!("out dx, ax", in("dx") port, in("ax") v, options(nomem, nostack, preserves_flags)) };
}

/// The disk's 4 KiB block `n` written by this file's own PIO (28-bit LBA,
/// polled, interrupts off), then the drive's write cache flushed. Used
/// once, before the learner's driver or file system has read anything.
fn raw_disk_write(n: u64, buf: &[u8; BLOCK_SIZE]) -> bool {
    quiet(|| {
        let lba = n * 8;
        let wait = |data: bool| -> bool {
            for _ in 0..20_000_000u32 {
                let s = in8(0x1f7);
                if s & 0x80 == 0 {
                    if s & 0x21 != 0 {
                        return false;
                    }
                    if !data || s & 0x08 != 0 {
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
        out8(0x1f7, 0x30);
        for _ in 0..4 {
            in8(0x3f6);
        }
        for sector in buf.chunks_exact(512) {
            if !wait(true) {
                return false;
            }
            for pair in sector.chunks_exact(2) {
                out16(0x1f0, u16::from_le_bytes([pair[0], pair[1]]));
            }
        }
        if !wait(false) {
            return false;
        }
        out8(0x1f7, 0xe7);
        for _ in 0..4 {
            in8(0x3f6);
        }
        wait(false)
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

/// Partition 2's fs block `b`, written raw.
fn raw_put(b: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), String> {
    if raw_disk_write(P2_BLOCK0 + b, buf) {
        Ok(())
    } else {
        Err(format!("the test's own write of fs block {b} failed"))
    }
}

/// A number no one knows before this moment: the time-stamp counter and
/// the PM timer, mixed (CONTRACT §A.1-5). Kept in this file; never printed
/// before the row that carries it has read it back.
fn unpublished(salt: u64) -> u64 {
    // SAFETY: rdtsc reads a counter; no memory is touched.
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
    splitmix(tsc ^ (pm() as u64) << 40 ^ salt) & 0xffff_ffff_ffff
}

/// Row 1's input, written by this file before the mount: in `/tmp`'s first
/// block, after its last record, three files named from `seed` (the run's
/// nonce) and an unpublished word — the first and the third empty regular
/// files of inodes of their own, made here (a free inode an unpublished
/// number of free inodes in, its bitmap bit, its record, both free-inode
/// counts; s20 round 2b, so the records do not all name one published
/// inode), the second a hard link to `/pattern` — and between the first and
/// the second a record of inode 0, as a removal leaves one. Returns the
/// three names.
fn seed_tmp(raw: &Raw, seed: u64) -> Result<Vec<String>, String> {
    let tmp = raw.walk("/tmp")?.ok_or(String::from("no /tmp on the disk"))?;
    let pat = raw.walk("/pattern")?.ok_or(String::from("no /pattern on the disk"))?;
    let blk = raw.bmap(&raw.inode(tmp)?, 0)?;
    let mut b = raw_block(blk)?;
    let (mut at, mut last) = (0usize, 0usize);
    while at < BLOCK_SIZE {
        let rl = le16(&b[..], at + 4) as usize;
        if rl < 8 {
            return Err(format!("/tmp's first block: a record at byte {at} of length {rl}"));
        }
        last = at;
        at += rl;
    }
    let need = |n: usize| (8 + n + 3) & !3;
    let keep = need(b[last + 6] as usize);
    let names: Vec<String> = (0..3u64).map(|k| format!("n{:012x}{:012x}", splitmix(seed ^ (0x0201 + k)) & 0xffff_ffff_ffff, unpublished(k))).collect();
    let ghost = format!("g{:012x}", unpublished(7));
    // Two free inodes, each found after skipping an unpublished number (0..7)
    // of free ones.
    let mut ib = raw_block(raw.inode_bitmap)?;
    let mut new = Vec::new();
    let mut skip = unpublished(8) % 8;
    let mut ino = raw.first_ino;
    while new.len() < 2 && ino <= raw.inodes {
        let (byte, bit) = (((ino - 1) / 8) as usize, (ino - 1) % 8);
        if ib[byte] >> bit & 1 == 0 {
            if skip == 0 {
                ib[byte] |= 1 << bit;
                new.push(ino);
                skip = unpublished(9) % 8;
            } else {
                skip -= 1;
            }
        }
        ino += 1;
    }
    if new.len() < 2 {
        return Err(String::from("no two free inodes for the test's files"));
    }
    let recs = [(new[0], &names[0]), (0, &ghost), (pat, &names[1]), (new[1], &names[2])];
    let room: usize = recs.iter().map(|r| need(r.1.len())).sum();
    if last + keep + room > BLOCK_SIZE {
        return Err(String::from("/tmp's first block has no room for the test's records"));
    }
    let put16 = |b: &mut [u8], at: usize, v: usize| b[at..at + 2].copy_from_slice(&(v as u16).to_le_bytes());
    put16(&mut b[..], last + 4, keep);
    let mut at = last + keep;
    for (k, (ino, name)) in recs.iter().enumerate() {
        let len = if k + 1 == recs.len() { BLOCK_SIZE - at } else { need(name.len()) };
        b[at..at + 4].copy_from_slice(&(*ino as u32).to_le_bytes());
        put16(&mut b[..], at + 4, len);
        b[at + 6] = name.len() as u8;
        b[at + 7] = 1;
        b[at + 8..at + 8 + name.len()].copy_from_slice(name.as_bytes());
        at += len;
    }
    raw_put(blk, &b)?;
    // The inodes: /pattern one link more; each new one an empty regular file
    // (mode 0644, one link, no blocks, every other field 0).
    for (ino, new_file) in [(pat, false), (new[0], true), (new[1], true)] {
        let at = (ino - 1) * 128;
        let (tb, off) = (raw.inode_table + at / 4096, (at % 4096) as usize);
        let mut t = raw_block(tb)?;
        if new_file {
            t[off..off + 128].fill(0);
            put16(&mut t[..], off, 0o100644);
            put16(&mut t[..], off + 26, 1);
        } else {
            let n = le16(&t[..], off + 26) as usize + 1;
            put16(&mut t[..], off + 26, n);
        }
        raw_put(tb, &t)?;
    }
    raw_put(raw.inode_bitmap, &ib)?;
    let mut sb = raw_block(0)?;
    let n = le32(&sb[1024..], 16) - 2;
    sb[1024 + 16..1024 + 20].copy_from_slice(&(n as u32).to_le_bytes());
    raw_put(0, &sb)?;
    let mut gd = raw_block(1)?;
    let n = le16(&gd[..], 14) as usize - 2;
    put16(&mut gd[..], 14, n);
    raw_put(1, &gd)?;
    Ok(names)
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
    /// The u64 the program finds at this place in its data segment when
    /// the call runs (a value an earlier call wrote there).
    M(usize),
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
                A::M(off) => {
                    self.code.extend_from_slice(&MOV_MEM[i]);
                    self.code.extend_from_slice(&((DATA + off as u64) as u32).to_le_bytes());
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

/// The names of directory `path`'s live records, sorted, with their inodes.
fn names(r: &mut Row, raw: &Raw, path: &str) -> Vec<(String, u64)> {
    let Some((ino, i)) = raw_at(r, raw, path) else { return Vec::new() };
    let mut v: Vec<(String, u64)> = got(r, raw.records(ino, &i)).unwrap_or_default().into_iter().map(|(n, _, s)| (s, n)).collect();
    v.sort();
    v
}

fn check_inode(r: &mut Row, raw: &Raw, path: &str, mode: u64, nlink: u64, size: u64) -> Option<[u8; 128]> {
    let (_, i) = raw_at(r, raw, path)?;
    r.expect_eq(&format!("{path}: i_mode on the disk"), le16(&i, 0), mode);
    r.expect_eq(&format!("{path}: i_links_count on the disk"), links(&i), nlink);
    r.expect_eq(&format!("{path}: i_size on the disk"), le32(&i, 4), size);
    Some(i)
}

// ── Row 1: ext2.getdents.records ─────────────────────────────────────────

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Rec {
    ino: u64,
    dtype: u8,
    name: String,
}

/// ext2's `file_type` as `d_type`.
fn dtype(ft: u8) -> u8 {
    match ft {
        1 => 8,
        2 => 4,
        3 => 2,
        4 => 6,
        5 => 1,
        6 => 12,
        7 => 10,
        _ => 0,
    }
}

/// Directory `ino`'s live records as this file reads the disk, in the
/// order they lie there.
fn dir(raw: &Raw, ino: u64) -> Result<Vec<Rec>, String> {
    let i = raw.inode(ino)?;
    Ok(raw.records(ino, &i)?.into_iter().map(|(n, ft, name)| Rec { ino: n, dtype: dtype(ft), name }).collect())
}

/// The learner's `Inode::getdents` from index 0 until `None`, against the
/// disk's records in their order.
fn walk_dents(r: &mut Row, what: &str, d: &InodeRef, want: &[Rec]) {
    let mut seen = Vec::new();
    for k in 0..=want.len() as u64 + 4 {
        match d.getdents(k) {
            Ok(Some(e)) => seen.push(Rec { ino: e.ino, dtype: e.kind, name: e.name }),
            Ok(None) => break,
            Err(e) => {
                r.fail(format_args!("{DENTS_ID}: {what}: getdents({k}) returned {}", errno_text(e)));
                return;
            }
        }
    }
    if seen != want {
        let k = seen.iter().zip(want).position(|(a, b)| a != b).unwrap_or(seen.len().min(want.len()));
        r.fail(format_args!("{DENTS_ID}: {what}: getdents(0..) handed out {} entries, the disk holds {} live records; first difference at index {k}: {:?}, expected {:?}", seen.len(), want.len(), seen.get(k), want.get(k)));
    }
}

/// The records `getdents64` wrote: (d_ino, d_type, name) and each `d_off`.
fn parse_dents(buf: &[u8], recs: &mut Vec<Rec>, offs: &mut Vec<i64>) -> Result<(), String> {
    let n = buf.len();
    let mut at = 0usize;
    while at < n {
        if at + 20 > n {
            return Err(format!("a record at byte {at} runs past the {n} bytes returned"));
        }
        let reclen = le16(buf, at + 16) as usize;
        if reclen < 20 || reclen % 8 != 0 || at + reclen > n {
            return Err(format!("the record at byte {at} has d_reclen {reclen} (a multiple of 8, at least 20, inside the {n} bytes)"));
        }
        let name = &buf[at + 19..at + reclen];
        let Some(z) = name.iter().position(|&c| c == 0) else {
            return Err(format!("the record at byte {at} has no NUL inside its d_reclen {reclen}"));
        };
        recs.push(Rec { ino: le64(buf, at), dtype: buf[at + 18], name: String::from_utf8_lossy(&name[..z]).into_owned() });
        offs.push(le64(buf, at + 8) as i64);
        at += reclen;
    }
    Ok(())
}

fn compare_dir(r: &mut Row, what: &str, mut got: Vec<Rec>, mut want: Vec<Rec>) {
    if got.len() != want.len() {
        r.fail(format_args!("{DENTS_ID}: {what}: {} entries, expected {} (the disk's live records)", got.len(), want.len()));
    }
    got.sort();
    want.sort();
    let missing: Vec<&Rec> = want.iter().filter(|w| !got.contains(w)).collect();
    let extra: Vec<&Rec> = got.iter().filter(|g| !want.contains(g)).collect();
    if let Some(m) = missing.first() {
        r.fail(format_args!(
            "{DENTS_ID}: {what}: {} of the disk's records missing or different, first {{ino {}, d_type {}, {:?}}}",
            missing.len(),
            m.ino,
            m.dtype,
            m.name
        ));
    }
    if let Some(x) = extra.first() {
        r.fail(format_args!(
            "{DENTS_ID}: {what}: {} entries the disk does not hold, first {{ino {}, d_type {}, {:?}}}",
            extra.len(),
            x.ino,
            x.dtype,
            x.name
        ));
    }
}

fn dents_row(raw: &Raw, root: &InodeRef, seeded: Result<Vec<String>, String>) {
    let mut r = Row::new(DENTS_ID);
    let want = (dir(raw, 2), raw.walk("/t").and_then(|t| t.ok_or(String::from("no /t on the disk"))).and_then(|t| dir(raw, t)));
    let (root_want, t_want) = match want {
        (Ok(a), Ok(b)) => (a, b),
        (a, b) => {
            r.fail(format_args!("{DENTS_ID}: the test's own read of / or /t failed: {:?} {:?}", a.err(), b.err()));
            return r.finish(1);
        }
    };
    note(format_args!("{DENTS_ID}: on the disk: / holds {} live records, /t {} (in its two blocks)", root_want.len(), t_want.len()));
    // (0) /tmp as this file wrote it before the mount (seed_tmp): three
    // names no table holds and a record of inode 0 between them (s20 fix
    // round 2, 2026-10-19).
    let _ = r.derived(DENTS_ID);
    let fresh = match seeded {
        Ok(v) => v,
        Err(why) => {
            r.fail(format_args!("{DENTS_ID}: the test's own write of /tmp before the mount: {why}"));
            Vec::new()
        }
    };
    let tmp_ino = raw.walk("/tmp").ok().flatten().unwrap_or(0);
    let tmp_want = match dir(raw, tmp_ino) {
        Ok(v) => v,
        Err(why) => {
            r.fail(format_args!("{DENTS_ID}: the test's own read of /tmp failed: {why}"));
            Vec::new()
        }
    };
    let held = fresh.iter().filter(|n| tmp_want.iter().any(|w| &w.name == *n)).count();
    r.expect_eq("the test's own read of /tmp: its three records written before the mount", held as u64, 3);
    // (a) The learner's `Inode::getdents`, called directly, in order.
    walk_dents(&mut r, "/", root, &root_want);
    match root.lookup("t") {
        Ok(t) => walk_dents(&mut r, "/t", &t, &t_want),
        Err(e) => r.fail(format_args!("{DENTS_ID}: lookup(\"t\") in / returned {}", errno_text(e))),
    }
    match root.lookup("tmp") {
        Ok(t) => walk_dents(&mut r, "/tmp (three records and an unused one written before the mount)", &t, &tmp_want),
        Err(e) => r.fail(format_args!("{DENTS_ID}: lookup(\"tmp\") in / returned {}", errno_text(e))),
    }
    match root.lookup("pattern") {
        Ok(f) => match f.getdents(0) {
            Err(e) if e == ENOTDIR => {}
            Err(e) => r.fail(format_args!("{DENTS_ID}: getdents(0) of /pattern (a file) returned {}, expected {}", errno_text(e), errno_text(ENOTDIR))),
            Ok(x) => r.fail(format_args!("{DENTS_ID}: getdents(0) of /pattern (a file) returned Ok({:?}), expected {}", x.map(|e| e.name), errno_text(ENOTDIR))),
        },
        Err(e) => r.fail(format_args!("{DENTS_ID}: lookup(\"pattern\") in / returned {}", errno_text(e))),
    }
    // (b) getdents64 from ring 3.
    let mut p = Probe::new();
    let t = p.str("/t");
    let rootp = p.str("/");
    let pat = p.str("/pattern");
    let s0 = p.call(SYS_OPEN, &[t, A::I(O_DIRECTORY)]);
    let mut tcalls = Vec::new();
    for _ in 0..5 {
        let (b, o) = p.buf(4096);
        tcalls.push((p.call(SYS_GETDENTS64, &[A::R(s0), b, A::I(4096)]), o));
    }
    let s6 = p.call(SYS_OPEN, &[rootp, A::I(O_DIRECTORY)]);
    let (br, or) = p.buf(4096);
    let s7 = p.call(SYS_GETDENTS64, &[A::R(s6), br, A::I(4096)]);
    let (bx, _) = p.buf(4096);
    let s8 = p.call(SYS_GETDENTS64, &[A::R(s6), bx, A::I(4096)]);
    let s9 = p.call(SYS_OPEN, &[pat, A::I(0)]);
    let s10 = p.call(SYS_GETDENTS64, &[A::R(s9), bx, A::I(4096)]);
    let s11 = p.call(SYS_OPEN, &[t, A::I(O_DIRECTORY)]);
    let s12 = p.call(SYS_GETDENTS64, &[A::R(s11), bx, A::I(20)]);
    let s13 = p.call(SYS_GETDENTS64, &[A::I(1), bx, A::I(4096)]);
    // A d_off resumes after its own record: one record from the middle of
    // /t (a 40-byte call after a 4096-byte one), then a second open of /t
    // moved by lseek to that record's d_off, read again.
    let s14 = p.call(SYS_OPEN, &[t, A::I(O_DIRECTORY)]);
    let (bm, _) = p.buf(4096);
    let _s15 = p.call(SYS_GETDENTS64, &[A::R(s14), bm, A::I(4096)]);
    let (b1, o1) = p.buf(40);
    let s16 = p.call(SYS_GETDENTS64, &[A::R(s14), b1, A::I(40)]);
    let s17 = p.call(SYS_OPEN, &[t, A::I(O_DIRECTORY)]);
    let s18 = p.call(SYS_LSEEK, &[A::R(s17), A::M(o1 + 8), A::I(0)]);
    let (b2, o2) = p.buf(40);
    let s19 = p.call(SYS_GETDENTS64, &[A::R(s17), b2, A::I(40)]);
    let tmpp = p.str("/tmp");
    let s20 = p.call(SYS_OPEN, &[tmpp, A::I(O_DIRECTORY)]);
    let (bt, ot) = p.buf(4096);
    let s21 = p.call(SYS_GETDENTS64, &[A::R(s20), bt, A::I(4096)]);
    match run_probe(DENTS_ID, p) {
        Ok(out) => {
            let counts: Vec<String> = tcalls.iter().map(|&(k, _)| answer(res(&out, k))).collect();
            note(format_args!("{DENTS_ID}: getdents64 of /t in 4096-byte calls returned {}; of / {} then {}", counts.join(", "), answer(res(&out, s7)), answer(res(&out, s8))));
            r.expect(res(&out, s0) >= 3, "open(\"/t\", O_DIRECTORY) is an fd");
            let mut got = Vec::new();
            let mut offs = Vec::new();
            let mut ended = false;
            for &(k, o) in &tcalls {
                let n = res(&out, k);
                if n == 0 {
                    ended = true;
                    break;
                }
                if !(1..=4096).contains(&n) {
                    r.fail(format_args!("{DENTS_ID}: a getdents64 of /t returned {}", answer(n)));
                    break;
                }
                if let Err(why) = parse_dents(&out[o..o + n as usize], &mut got, &mut offs) {
                    r.fail(format_args!("{DENTS_ID}: /t: {why}"));
                    break;
                }
            }
            r.expect(ended, "getdents64 of /t returns 0 once every entry has been handed out (within 5 calls of 4096 bytes)");
            r.expect(offs.windows(2).all(|w| w[0] < w[1]), "each d_off of /t's listing is past the one before");
            for want in [".", ".."] {
                r.expect(got.iter().any(|g| g.name == want), &format!("/t's listing holds {want:?}"));
            }
            // The resume: the record after the one whose d_off was used.
            let one = |n: i64, o: usize| -> Result<(Rec, i64), String> {
                if !(1..=40).contains(&n) {
                    return Err(format!("returned {}", answer(n)));
                }
                let (mut v, mut d) = (Vec::new(), Vec::new());
                parse_dents(&out[o..o + n as usize], &mut v, &mut d)?;
                match (v.pop(), d.pop()) {
                    (Some(x), Some(off)) if v.is_empty() => Ok((x, off)),
                    _ => Err(String::from("did not hand out exactly one record in 40 bytes")),
                }
            };
            match (one(res(&out, s16), o1), one(res(&out, s19), o2)) {
                (Ok((x, off)), Ok((y, _))) => {
                    r.expect_res(&format!("lseek(fd, {off}, SEEK_SET) on a second fd of /t (the d_off of {:?})", x.name), res(&out, s18), off);
                    let k = t_want.iter().position(|w| *w == x);
                    let next = k.and_then(|k| t_want.get(k + 1));
                    if next != Some(&y) {
                        r.fail(format_args!(
                            "{DENTS_ID}: /t: after lseek to the d_off of {:?} (live record {}), getdents64 handed out {:?} first, expected {} (the record after it)",
                            x.name,
                            k.map_or(String::from("none on the disk"), |k| format!("{k}")),
                            y.name,
                            next.map_or(String::from("none"), |n| format!("{:?}", n.name))
                        ));
                    }
                }
                (a, b) => r.fail(format_args!(
                    "{DENTS_ID}: /t: a 40-byte getdents64 after a 4096-byte one: {}; the same after lseek to its d_off on a second fd: {}",
                    a.err().unwrap_or(String::from("one record")),
                    b.err().unwrap_or(String::from("one record"))
                )),
            }
            compare_dir(&mut r, "/t", got, t_want.clone());
            let n21 = res(&out, s21);
            let mut tgot = Vec::new();
            let mut toffs = Vec::new();
            if (1..=4096).contains(&n21) {
                if let Err(why) = parse_dents(&out[ot..ot + n21 as usize], &mut tgot, &mut toffs) {
                    r.fail(format_args!("{DENTS_ID}: /tmp: {why}"));
                }
            } else {
                r.fail(format_args!("{DENTS_ID}: getdents64 of /tmp returned {}", answer(n21)));
            }
            compare_dir(&mut r, "/tmp (three records and an unused one written before the mount)", tgot, tmp_want.clone());
            let n7 = res(&out, s7);
            let mut rgot = Vec::new();
            let mut roffs = Vec::new();
            if (1..=4096).contains(&n7) {
                if let Err(why) = parse_dents(&out[or..or + n7 as usize], &mut rgot, &mut roffs) {
                    r.fail(format_args!("{DENTS_ID}: /: {why}"));
                }
            } else {
                r.fail(format_args!("{DENTS_ID}: getdents64 of / returned {}", answer(n7)));
            }
            for want in [".", "..", ".sboot-log", "pattern"] {
                r.expect(rgot.iter().any(|g| g.name == want), &format!("/'s listing holds {want:?}"));
            }
            compare_dir(&mut r, "/", rgot, root_want.clone());
            r.expect_res("the next getdents64 of / (all handed out)", res(&out, s8), 0);
            r.expect_res("getdents64 of a regular file's fd", res(&out, s10), -ENOTDIR);
            r.expect_res("getdents64 with a 20-byte buffer (no record fits)", res(&out, s12), -EINVAL);
            r.expect_res("getdents64 of fd 1 (a pipe)", res(&out, s13), -ENOTDIR);
        }
        Err(why) => r.fail(format_args!("{why}")),
    }
    // (c) ls, exec'd from /bin by exec::spawn (its .bss is 8 pages).
    let sorted = |v: Vec<Rec>| -> Vec<String> {
        let mut v: Vec<String> = v.into_iter().map(|x| x.name).collect();
        v.sort();
        v
    };
    match raw.walk("/a/b/c").and_then(|c| c.ok_or(String::from("no /a/b/c on the disk"))).and_then(|c| dir(raw, c)) {
        Ok(abc) => {
            let want = format!("/:\n{}\n\n/a/b/c:\n{}\n", sorted(root_want).join("\n"), sorted(abc).join("\n"));
            let argv = ["ls", "-a", "/", "/a/b/c"];
            match run("ls", || exec::spawn("/bin/ls", &argv, &[])) {
                Ok(ran) => {
                    note(format_args!("{DENTS_ID}: /bin/ls -a / /a/b/c: {} bytes out, {}", ran.out.len(), status_text(ran.exit)));
                    if ran.out != want.as_bytes() {
                        r.fail(format_args!(
                            "{DENTS_ID}: ls -a / /a/b/c printed {:?}, expected {:?} (the directories as the disk holds them, sorted)",
                            String::from_utf8_lossy(&ran.out).replace('\n', "\\n"),
                            want.replace('\n', "\\n")
                        ));
                    }
                    r.expect(ran.exit == Some(0), "/bin/ls exited 0");
                }
                Err(why) => r.fail(format_args!("{why}")),
            }
        }
        Err(why) => r.fail(format_args!("{DENTS_ID}: the test's own read of /a/b/c failed: {why}")),
    }
    r.finish(1);
}

// ── Row 2: ext2.mkdir.rmdir ──────────────────────────────────────────────

/// Both bitmaps, the five counts, `/tmp`'s links.
fn alloc_state(raw: &Raw) -> Result<(Box<[u8; BLOCK_SIZE]>, Box<[u8; BLOCK_SIZE]>, [u64; 5], u64), String> {
    let (sb, gd) = (raw_block(0)?, raw_block(1)?);
    let counts = [le32(&sb[1024..], 12), le32(&sb[1024..], 16), le16(&gd[..], 12), le16(&gd[..], 14), le16(&gd[..], 16)];
    let tmp = raw.walk("/tmp")?.ok_or("no /tmp on the disk")?;
    Ok((raw_block(raw.block_bitmap)?, raw_block(raw.inode_bitmap)?, counts, links(&raw.inode(tmp)?)))
}

fn mkdir_row(raw: &Raw) {
    let mut r = Row::new(MKDIR_ID);
    let Some(before) = got(&mut r, alloc_state(raw)) else { return r.finish(2) };
    let data = mix(0x5B00_0020_0002, 60_000);
    let i = A::I;
    let mut pl = Plan::new();
    let (d1, d2s, f, g, f_x, bx, dot) = (pl.p.str("/tmp/d1"), pl.p.str("/tmp/d1/d2/"), pl.p.str("/tmp/d1/f"), pl.p.str("/tmp/d1/d2/g"), pl.p.str("/tmp/d1/f/x"), pl.p.str("/boot/x"), pl.p.str("."));
    let off = pl.p.bytes(&data);
    pl.is("mkdir(/tmp/d1, 0755)", SYS_MKDIR, &[d1, i(0o755)], 0);
    pl.is("mkdir(/tmp/d1/d2/, 0755) (a trailing /)", SYS_MKDIR, &[d2s, i(0o755)], 0);
    let fd = pl.fd("open(/tmp/d1/f, O_WRONLY|O_CREAT|O_EXCL)", SYS_OPEN, &[f, i(O_WRONLY | O_CREAT | O_EXCL), i(0o644)]);
    pl.is("write of 60000 bytes", SYS_WRITE, &[A::R(fd), A::D(off), i(60_000)], 60_000);
    pl.is("close", SYS_CLOSE, &[A::R(fd)], 0);
    let gd = pl.fd("open(/tmp/d1/d2/g, O_WRONLY|O_CREAT)", SYS_OPEN, &[g, i(O_WRONLY | O_CREAT), i(0o600)]);
    pl.is("close", SYS_CLOSE, &[A::R(gd)], 0);
    pl.is("mkdir(/tmp/d1) again", SYS_MKDIR, &[d1, i(0o755)], -EEXIST);
    pl.is("mkdir(\".\")", SYS_MKDIR, &[dot, i(0o755)], -EEXIST);
    pl.is("mkdir(/tmp/d1/f) (a file there)", SYS_MKDIR, &[f, i(0o755)], -EEXIST);
    pl.is("mkdir(/tmp/d1/f/x)", SYS_MKDIR, &[f_x, i(0o755)], -ENOTDIR);
    pl.is("mkdir(/boot/x)", SYS_MKDIR, &[bx, i(0o755)], -EROFS);
    pl.run(&mut r, "m20a");
    let tmp_links = before.3;
    if let Some((d1i, _)) = raw_at(&mut r, raw, "/tmp/d1") {
        check_inode(&mut r, raw, "/tmp/d1", 0o040755, 3, 4096);
        check_inode(&mut r, raw, "/tmp/d1/d2", 0o040755, 2, 4096);
        check_inode(&mut r, raw, "/tmp/d1/f", 0o100644, 1, 60_000);
        check_inode(&mut r, raw, "/tmp/d1/d2/g", 0o100600, 1, 0);
        if let Some((_, t)) = raw_at(&mut r, raw, "/tmp") {
            r.expect_eq("/tmp's i_links_count on the disk (one more for d1's ..)", links(&t), tmp_links + 1);
        }
        let tmp = raw.walk("/tmp").ok().flatten().unwrap_or(0);
        let n1 = names(&mut r, raw, "/tmp/d1");
        let d2i = n1.iter().find(|e| e.0 == "d2").map_or(0, |e| e.1);
        let want1: Vec<(String, u64)> = alloc::vec![(String::from("."), d1i), (String::from(".."), tmp), (String::from("d2"), d2i), (String::from("f"), n1.iter().find(|e| e.0 == "f").map_or(0, |e| e.1))];
        r.expect(n1 == want1, &format!("/tmp/d1's records {n1:?} are . (itself), .. (/tmp), d2 and f"));
        let n2 = names(&mut r, raw, "/tmp/d1/d2");
        r.expect(n2.len() == 3 && n2[0] == (String::from("."), d2i) && n2[1] == (String::from(".."), d1i), &format!("/tmp/d1/d2's records {n2:?} are . (itself), .. (/tmp/d1) and g"));
    }
    let mut pl = Plan::new();
    let (d1, d2, d2s, f, fs_, g, log, none, d1dot) = (
        pl.p.str("/tmp/d1"),
        pl.p.str("/tmp/d1/d2"),
        pl.p.str("/tmp/d1/d2/"),
        pl.p.str("/tmp/d1/f"),
        pl.p.str("/tmp/d1/f/"),
        pl.p.str("/tmp/d1/d2/g"),
        pl.p.str("/.sboot-log"),
        pl.p.str("/tmp/nothere"),
        pl.p.str("/tmp/d1/."),
    );
    let dotdot = pl.p.str("/tmp/d1/..");
    pl.is("rmdir(/tmp/d1) (not empty)", SYS_RMDIR, &[d1], -ENOTEMPTY);
    pl.is("rmdir(/tmp/d1/f) (a file)", SYS_RMDIR, &[f], -ENOTDIR);
    pl.is("rmdir(/tmp/d1/.)", SYS_RMDIR, &[d1dot], -EINVAL);
    pl.is("rmdir(/tmp/d1/..)", SYS_RMDIR, &[dotdot], -ENOTEMPTY);
    pl.is("rmdir(/.sboot-log) (a file)", SYS_RMDIR, &[log], -ENOTDIR);
    pl.is("rmdir(/tmp/nothere)", SYS_RMDIR, &[none], -ENOENT);
    pl.is("rmdir(/tmp/d1/f/)", SYS_RMDIR, &[fs_], -ENOTDIR);
    pl.is("unlink(/tmp/d1/d2/g)", SYS_UNLINK, &[g], 0);
    pl.is("rmdir(/tmp/d1/d2/) (empty now; a trailing /)", SYS_RMDIR, &[d2s], 0);
    pl.is("rmdir(/tmp/d1/d2) again", SYS_RMDIR, &[d2], -ENOENT);
    pl.is("unlink(/tmp/d1/f)", SYS_UNLINK, &[f], 0);
    pl.is("rmdir(/tmp/d1)", SYS_RMDIR, &[d1], 0);
    pl.is("rmdir(/tmp/d1) again", SYS_RMDIR, &[d1], -ENOENT);
    pl.run(&mut r, "m20b");
    if let Some(after) = got(&mut r, alloc_state(raw)) {
        let first = |a: &[u8; BLOCK_SIZE], b: &[u8; BLOCK_SIZE]| (0..BLOCK_SIZE).find(|&k| a[k] != b[k]);
        if let Some(k) = first(&after.0, &before.0) {
            r.fail(format_args!("{MKDIR_ID}: the block bitmap after the removals differs from before the row first at byte {k} (blocks {}–{}): {:#04x}, was {:#04x}", 8 * k, 8 * k + 7, after.0[k], before.0[k]));
        }
        if let Some(k) = first(&after.1, &before.1) {
            r.fail(format_args!("{MKDIR_ID}: the inode bitmap after the removals differs from before the row first at byte {k} (inodes {}–{}): {:#04x}, was {:#04x}", 8 * k + 1, 8 * k + 8, after.1[k], before.1[k]));
        }
        let what = ["s_free_blocks_count", "s_free_inodes_count", "bg_free_blocks_count", "bg_free_inodes_count", "bg_used_dirs_count"];
        for k in 0..5 {
            r.expect_eq(&format!("{} after the removals (as before the row)", what[k]), after.2[k], before.2[k]);
        }
        r.expect_eq("/tmp's i_links_count after the removals (as before the row)", after.3, before.3);
    }
    if let Some(Some(_)) = got(&mut r, raw.walk("/tmp/d1")) {
        r.fail(format_args!("{MKDIR_ID}: /tmp/d1 is still a record of /tmp on the disk"));
    }
    r.finish(2);
}

// ── Row 3: ext2.rename ───────────────────────────────────────────────────

/// A move of a directory into its own subtree, through the learner's own
/// `Inode::rename` on `pdir`'s inode (`new_dir` walked from `ndir`, or one
/// held from before): -22 (EINVAL), and `pdir`'s records, `ndir`'s records
/// and `src`'s links as they were.
#[allow(clippy::too_many_arguments)]
fn refused_direct(r: &mut Row, raw: &Raw, pdir: &str, old: &str, ndir: &str, held: Option<&InodeRef>, new: &str, src: &str, what: &str) {
    let found = vfs::root().and_then(|root| {
        let p = vfs::walk(&root, pdir)?;
        let n = match held {
            Some(h) => h.clone(),
            None => vfs::walk(&root, ndir)?,
        };
        Ok((p, n))
    });
    let (p, n) = match found {
        Ok(v) => v,
        Err(e) => {
            r.fail(format_args!("{RENAME_ID}: {pdir} or {ndir} not found through the mount: {}", errno_text(e)));
            return;
        }
    };
    let state = |r: &mut Row| (names(r, raw, pdir), names(r, raw, ndir), raw_at(r, raw, src).map(|(_, i)| links(&i)));
    let before = state(r);
    let got = p.rename(old, &*n, new);
    if got != Err(EINVAL) {
        let text = match got {
            Ok(()) => String::from("0"),
            Err(e) => errno_text(e),
        };
        r.fail(format_args!("{RENAME_ID}: Inode::rename(\"{old}\", {ndir}, \"{new}\") called on {pdir}'s inode ({what}) returned {text}, expected -22 (EINVAL)"));
    }
    let after = state(r);
    if before != after {
        r.fail(format_args!("{RENAME_ID}: after Inode::rename(\"{old}\", {ndir}, \"{new}\"): {pdir}'s records, {ndir}'s records and {src}'s links were {before:?}, are {after:?}"));
    }
}

fn rename_row(raw: &Raw, control: Result<Vec<String>, String>) {
    let mut r = Row::new(RENAME_ID);
    // The three files' bytes: splitmix64 of the nonce, the first word of
    // each replaced by a word no one knows before this boot (CONTRACT
    // §A.1-5: the read-back is bytes the learner's code only carries; s20
    // fix round 2).
    let d = r.derived(RENAME_ID);
    let stamp = |mut v: Vec<u8>, k: u64| {
        v[..8].copy_from_slice(&unpublished(0x0300 + k).to_le_bytes());
        v
    };
    let (x, y, z) = (stamp(mix(d ^ 0x0301, 100), 1), stamp(mix(d ^ 0x0302, 200), 2), stamp(mix(d ^ 0x0303, 300), 3));
    let i = A::I;
    let mut pl = Plan::new();
    let (ra, rb, sub) = (pl.p.str("/tmp/ra"), pl.p.str("/tmp/rb"), pl.p.str("/tmp/ra/sub"));
    let (emp, mv) = (pl.p.str("/tmp/rb/emp"), pl.p.str("/tmp/ra/mv"));
    for (what, d) in [("mkdir(/tmp/ra)", ra), ("mkdir(/tmp/rb)", rb), ("mkdir(/tmp/ra/sub)", sub), ("mkdir(/tmp/rb/emp)", emp), ("mkdir(/tmp/ra/mv)", mv)] {
        pl.is(what, SYS_MKDIR, &[d, i(0o755)], 0);
    }
    // The subtree cases (s20 fix rounds 2-5): a directory /tmp/<u> with a
    // chain of `depth` directories below it, 3 to 8 (chosen from the nonce;
    // a climb that stops early misses the deepest). Every name here — <u>,
    // the chain's, the new name <v>, the two directories <m1> and <m2>,
    // their children and their new names — is `d` and a word no one knows
    // before this boot (CONTRACT §A.1-5; fix round 4: a fixed name was a
    // needle; fix round 5: so was a role's first letter). <m1> and <m2>
    // are moved INTO the chain, 1 to `depth` levels below <u> (so 2 or more
    // below /tmp, never a sibling, never their own subtree), which must
    // succeed: a rule that refuses every move but a sibling's fails it.
    // Each holds a directory of its own (fix round 5: a rule that refuses
    // any source with a subdirectory fails it), and one of the two, drawn
    // from the nonce, is made before the chain, the other after it (fix
    // round 5: a rule keyed on which inode is older fails it).
    let depth = 3 + splitmix(d ^ 0x0310) % 6;
    let word = |k: u64| format!("d{:012x}", unpublished(0x0320 + k));
    let (un, vn, m1, m2, w1, w2) = (word(0), word(1), word(2), word(3), word(4), word(5));
    let (k1, k2) = (word(20), word(21));
    let top = format!("/tmp/{un}");
    // chain[j]: the directory j levels below <u> (chain[0] is <u>).
    let mut chain = alloc::vec![top.clone()];
    for k in 1..=depth {
        let next = format!("{}/{}", chain[(k - 1) as usize], word(5 + k));
        chain.push(next);
    }
    let (from1, from2) = (format!("/tmp/{m1}"), format!("/tmp/{m2}"));
    let (kid1, kid2) = (format!("{from1}/{k1}"), format!("{from2}/{k2}"));
    // The mover made before the chain (fix round 6): /tmp/<x>, made with
    // it, moves into its child first, so the mover holds a grandchild when
    // it moves (a rule that refuses any source with one fails it) and an
    // honest climb has passed through the mover before it moved. After it
    // moves, <u> into that child must be refused: a climb that trusts a
    // parent remembered from before the move, or that stops at the first
    // directory older than <u>, lets it through.
    let e1 = splitmix(d ^ 0x0314) & 1 == 0;
    let (xn, x2, zn) = (word(22), word(23), word(24));
    let xfrom = format!("/tmp/{xn}");
    let (early, late) = if e1 { ([&from1, &kid1, &xfrom], [&from2, &kid2]) } else { ([&from2, &kid2, &xfrom], [&from1, &kid1]) };
    for path in early.into_iter().chain(chain.iter()).chain(late) {
        let pp = pl.p.str(path);
        pl.is(&format!("mkdir({path})"), SYS_MKDIR, &[pp, i(0o755)], 0);
    }
    let ekid = if e1 { &kid1 } else { &kid2 };
    let xto = format!("{ekid}/{x2}");
    let deep_in = format!("{}/{vn}", chain[depth as usize]);
    let dotted = format!("/tmp/ra/../{un}");
    // The early mover goes at most `depth` - 2 levels below <u>, so the
    // refusal into its child (fix round 6) is never deeper than the chain
    // and the climb floor stays at `depth` (CONTRACT: eight `..` steps).
    let lv = |salt: u64, early: bool| (1 + splitmix(d ^ salt) % if early { depth - 2 } else { depth }) as usize;
    let (lv1, lv2) = (lv(0x0312, e1), lv(0x0313, !e1));
    let to1 = format!("{}/{w1}", chain[lv1]);
    note(format_args!("{RENAME_ID}: the rename into its own subtree goes {depth} levels below it"));
    for (path, bytes) in [("/tmp/ra/x", &x), ("/tmp/rb/y", &y), ("/tmp/ra/z", &z)] {
        let p = pl.p.str(path);
        let off = pl.p.bytes(bytes);
        let fd = pl.fd(&format!("open({path}, O_WRONLY|O_CREAT|O_EXCL)"), SYS_OPEN, &[p, i(O_WRONLY | O_CREAT | O_EXCL), i(0o644)]);
        pl.is(&format!("write to {path}"), SYS_WRITE, &[A::R(fd), A::D(off), i(bytes.len() as u64)], bytes.len() as i64);
        pl.is("close", SYS_CLOSE, &[A::R(fd)], 0);
    }
    pl.run(&mut r, "r20a");
    let ino = |r: &mut Row, p: &str| raw_at(r, raw, p).map_or(0, |(n, _)| n);
    let (ino_x, ino_y, ino_z, ino_sub, ino_ra, ino_rb) = (ino(&mut r, "/tmp/ra/x"), ino(&mut r, "/tmp/rb/y"), ino(&mut r, "/tmp/ra/z"), ino(&mut r, "/tmp/ra/sub"), ino(&mut r, "/tmp/ra"), ino(&mut r, "/tmp/rb"));
    // The blocks of the file a rename will replace, to see them given back.
    let y_blocks = raw_at(&mut r, raw, "/tmp/rb/y").and_then(|(_, i)| got(&mut r, raw.blocks_of(&i))).map(|(d, m)| [d, m].concat()).unwrap_or_default();
    // The empty directory a directory will replace, to see it freed whole.
    let (ino_emp, ino_mv) = (ino(&mut r, "/tmp/rb/emp"), ino(&mut r, "/tmp/ra/mv"));
    let (ino_m1, ino_m2) = (ino(&mut r, &from1), ino(&mut r, &from2));
    let (ino_k1, ino_k2) = (ino(&mut r, &kid1), ino(&mut r, &kid2));
    let ino_g = ino(&mut r, &xfrom);
    let emp_blocks = raw_at(&mut r, raw, "/tmp/rb/emp").and_then(|(_, i)| got(&mut r, raw.blocks_of(&i))).map(|(d, m)| [d, m].concat()).unwrap_or_default();
    let mut pl = Plan::new();
    let into_self = format!("{top}/{vn}");
    // <x> moves into the early mover's child after the last rename that
    // frees an inode and before either mover moves.
    let cases: [(&str, &str, i64); 23] = [
        ("/tmp/ra/x", "/tmp/rb/x2", 0),
        ("/tmp/rb/x2", "/tmp/rb/y", 0),
        ("/tmp/ra/sub", "/tmp/rb/sub2", 0),
        ("/tmp/ra/mv", "/tmp/rb/emp", 0),
        (&xfrom, &xto, 0),
        (&from1, &to1, 0),
        (&top, &deep_in, -EINVAL),
        (&dotted, &deep_in, -EINVAL),
        (&top, &into_self, -EINVAL),
        ("/tmp/rb/sub2/", "/tmp/rb/s3/", 0),
        ("/tmp/rb/s3", "/tmp/rb/sub2", 0),
        ("/tmp/ra/z/", "/tmp/ra/z3", -ENOTDIR),
        ("/tmp/ra/z", "/tmp/ra/z3/", -ENOTDIR),
        ("/tmp/rb", "/tmp/rb/sub2/in", -EINVAL),
        ("/tmp/ra/z", "/tmp/rb/sub2", -EISDIR),
        ("/tmp/rb/sub2", "/tmp/rb/y", -ENOTDIR),
        ("/tmp/rb", "/tmp/ra", -ENOTEMPTY),
        ("/tmp/ra/z", "/boot/z", -EXDEV),
        ("/.sboot-log", "/tmp/l", -EPERM),
        ("/tmp/ra/z", "/.sboot-log", -EPERM),
        ("/tmp/ra/none", "/tmp/rb/n", -ENOENT),
        ("/tmp/ra/z", "/tmp/ra/z", 0),
        ("/tmp/ra/z", "/tmp/ra/z2", 0),
    ];
    for (a, b, want) in cases {
        let (pa, pb) = (pl.p.str(a), pl.p.str(b));
        pl.is(&format!("rename({a}, {b})"), SYS_RENAME, &[pa, pb], want);
    }
    // `.` or `..` as either name: -EINVAL, CONTRACT §A.4's pin (POSIX's
    // errno; Linux answers EBUSY).
    for (a, b) in [("/tmp/rb/.", "/tmp/rdot"), ("/tmp/rb/sub2/..", "/tmp/rdd"), ("/tmp/ra/z2", "/tmp/rb/.")] {
        let (pa, pb) = (pl.p.str(a), pl.p.str(b));
        pl.is(&format!("rename({a}, {b})"), SYS_RENAME, &[pa, pb], -EINVAL);
    }
    let (px, py) = (pl.p.str("/tmp/ra/x"), pl.p.str("/tmp/rb/y"));
    let (buf, boff) = pl.p.buf(400);
    pl.is("open(/tmp/ra/x) after it moved", SYS_OPEN, &[px, i(O_RDONLY)], -ENOENT);
    let fd = pl.fd("open(/tmp/rb/y)", SYS_OPEN, &[py, i(O_RDONLY)]);
    pl.is("read of /tmp/rb/y (x's bytes now)", SYS_READ, &[A::R(fd), buf, i(400)], 100);
    pl.is("close", SYS_CLOSE, &[A::R(fd)], 0);
    if let Some(out) = pl.run(&mut r, "r20b") {
        r.expect_bytes("/tmp/rb/y read back", &out[boff..boff + 100], &x);
    }
    // The same refusal with no path at all (s20 fix rounds 3-4): the
    // learner's own `Inode::rename`, called on /tmp's inode with <u> as
    // `old` and a directory of the chain as `new_dir` — the deepest, then
    // one at a level chosen from the nonce. A check made on the path's
    // text, however it is normalised, never sees these calls; only a climb
    // from `new_dir` does. Refused, each must leave /tmp's records, the
    // target's records and <u>'s links as they were. Then <m2> moved into
    // the chain the same way, which must succeed.
    let drawn = (1 + splitmix(d ^ 0x0311) % depth) as usize;
    match vfs::root().and_then(|root| Ok((root.clone(), vfs::walk(&root, "/tmp")?))) {
        Ok((root, tdir)) => {
            for level in [depth as usize, drawn, 0] {
                let target = &chain[level];
                let Ok(ndir) = vfs::walk(&root, target) else {
                    r.fail(format_args!("{RENAME_ID}: {target} not found through the mount"));
                    continue;
                };
                let state = |r: &mut Row| (names(r, raw, "/tmp"), names(r, raw, target), raw_at(r, raw, &top).map(|(_, i)| links(&i)));
                let before = state(&mut r);
                let got = tdir.rename(&un, &*ndir, &vn);
                let text = match got {
                    Ok(()) => String::from("0"),
                    Err(e) => errno_text(e),
                };
                if got != Err(EINVAL) {
                    let into = if level == 0 { String::from("a directory into itself") } else { format!("a directory into its own subtree, level {level} below it") };
                    r.fail(format_args!("{RENAME_ID}: Inode::rename(\"{un}\", {target}, \"{vn}\") called on /tmp's inode ({into}, no path) returned {text}, expected -22 (EINVAL)"));
                }
                let after = state(&mut r);
                if before != after {
                    r.fail(format_args!("{RENAME_ID}: after Inode::rename(\"{un}\", {target}, \"{vn}\"): /tmp's records, {target}'s records and {top}'s links were {before:?}, are {after:?}"));
                }
            }
            let target = &chain[lv2];
            match vfs::walk(&root, target) {
                Ok(ndir) => {
                    if let Err(e) = tdir.rename(&m2, &*ndir, &w2) {
                        r.fail(format_args!("{RENAME_ID}: Inode::rename(\"{m2}\", {target}, \"{w2}\") called on /tmp's inode (a directory into its sibling {top}'s subtree, level {lv2} below it, not its own) returned {}, expected 0", errno_text(e)));
                    }
                }
                Err(e) => r.fail(format_args!("{RENAME_ID}: {target} not found through the mount: {}", errno_text(e))),
            }
        }
        Err(e) => r.fail(format_args!("{RENAME_ID}: /tmp not found through the mount: {}", errno_text(e))),
    }
    // <u> into the child of the mover made before the chain, now that the
    // mover lies below <u> (fix round 6): refused by `rename`, then by the
    // learner's `Inode::rename` with that child as `new_dir`, nothing
    // changed.
    let (eto, ekn) = if e1 { (to1.clone(), &k1) } else { (format!("{}/{w2}", chain[lv2]), &k2) };
    let ein = format!("{eto}/{ekn}");
    let ezin = format!("{ein}/{zn}");
    let mut pl = Plan::new();
    let (pa, pb) = (pl.p.str(&top), pl.p.str(&ezin));
    pl.is(&format!("rename({top}, {ezin}) (into a directory moved below it)"), SYS_RENAME, &[pa, pb], -EINVAL);
    pl.run(&mut r, "r20c");
    match vfs::root().and_then(|root| Ok((vfs::walk(&root, "/tmp")?, vfs::walk(&root, &ein)?))) {
        Ok((tdir, ndir)) => {
            let state = |r: &mut Row| (names(r, raw, "/tmp"), names(r, raw, &ein), raw_at(r, raw, &top).map(|(_, i)| links(&i)));
            let before = state(&mut r);
            let got = tdir.rename(&un, &*ndir, &zn);
            if got != Err(EINVAL) {
                let text = match got {
                    Ok(()) => String::from("0"),
                    Err(e) => errno_text(e),
                };
                r.fail(format_args!("{RENAME_ID}: Inode::rename(\"{un}\", {ein}, \"{zn}\") called on /tmp's inode (a directory into a directory moved below it, no path) returned {text}, expected -22 (EINVAL)"));
            }
            let after = state(&mut r);
            if before != after {
                r.fail(format_args!("{RENAME_ID}: after Inode::rename(\"{un}\", {ein}, \"{zn}\"): /tmp's records, {ein}'s records and {top}'s links were {before:?}, are {after:?}"));
            }
        }
        Err(e) => r.fail(format_args!("{RENAME_ID}: /tmp or {ein} not found through the mount: {}", errno_text(e))),
    }
    // The grandchild moved in before its grandparent moved: its `..` still
    // names the child, which now has three links.
    let ekino = if e1 { ino_k1 } else { ino_k2 };
    let gn = names(&mut r, raw, &format!("{ein}/{x2}"));
    r.expect(gn.iter().any(|e| e.0 == ".." && e.1 == ekino) && gn.iter().any(|e| e.0 == "." && e.1 == ino_g), &format!("{ein}/{x2} (inode {ino_g}, once {xfrom})'s . and .. name inodes {ino_g} and {ekino}: {gn:?}"));
    if let Some((_, i)) = raw_at(&mut r, raw, &ein) {
        r.expect_eq(&format!("{ein}: i_links_count on the disk"), links(&i), 3);
    }
    // Both moves into the chain, on the disk: the new record names the
    // moved inode, its `..` names its new parent, the old name is gone, and
    // the child it carried still names it as `..` (three links: its record,
    // its `.`, the child's `..`).
    for (to, ino_m, from, kid, ino_k) in [(to1.clone(), ino_m1, &from1, &k1, ino_k1), (format!("{}/{w2}", chain[lv2]), ino_m2, &from2, &k2, ino_k2)] {
        let parent = &to[..to.rfind('/').unwrap_or(0)];
        let pino = ino(&mut r, parent);
        let (pn, mn) = (names(&mut r, raw, parent), names(&mut r, raw, &to));
        let leaf = String::from(&to[parent.len() + 1..]);
        r.expect(pn.iter().any(|e| e.0 == leaf && e.1 == ino_m), &format!("{parent}'s records name {leaf} as inode {ino_m} (moved from {from}): {pn:?}"));
        r.expect(mn.iter().any(|e| e.0 == ".." && e.1 == pino), &format!("{to}'s .. names {parent} (inode {pino}): {mn:?}"));
        if let Some(Some(_)) = got(&mut r, raw.walk(from)) {
            r.fail(format_args!("{RENAME_ID}: {from} is still a record on the disk after it moved"));
        }
        let kn = names(&mut r, raw, &format!("{to}/{kid}"));
        r.expect(kn.iter().any(|e| e.0 == ".." && e.1 == ino_m) && kn.iter().any(|e| e.0 == "." && e.1 == ino_k), &format!("{to}/{kid} (inode {ino_k}, moved with {from})'s . and .. name inodes {ino_k} and {ino_m}: {kn:?}"));
        if let Some((_, i)) = raw_at(&mut r, raw, &to) {
            r.expect_eq(&format!("{to}: i_links_count on the disk"), links(&i), 3);
        }
    }
    let tmp = raw.walk("/tmp").ok().flatten().unwrap_or(0);
    let s = String::from;
    let mut want_a = alloc::vec![(s("."), ino_ra), (s(".."), tmp), (s("z2"), ino_z)];
    want_a.sort();
    let got_a = names(&mut r, raw, "/tmp/ra");
    r.expect(got_a == want_a, &format!("/tmp/ra's records {got_a:?}, expected {want_a:?}"));
    let mut want_b = alloc::vec![(s("."), ino_rb), (s(".."), tmp), (s("y"), ino_x), (s("sub2"), ino_sub), (s("emp"), ino_mv)];
    want_b.sort();
    let got_b = names(&mut r, raw, "/tmp/rb");
    r.expect(got_b == want_b, &format!("/tmp/rb's records {got_b:?}, expected {want_b:?}"));
    let got_s = names(&mut r, raw, "/tmp/rb/sub2");
    r.expect(got_s.iter().any(|e| e.0 == ".." && e.1 == ino_rb), &format!("/tmp/rb/sub2's .. names /tmp/rb (inode {ino_rb}): {got_s:?}"));
    let got_e = names(&mut r, raw, "/tmp/rb/emp");
    r.expect(got_e.iter().any(|e| e.0 == ".." && e.1 == ino_rb), &format!("/tmp/rb/emp (once /tmp/ra/mv)'s .. names /tmp/rb (inode {ino_rb}): {got_e:?}"));
    for (path, n) in [("/tmp/ra", 2), ("/tmp/rb", 4), ("/tmp/rb/sub2", 2), ("/tmp/rb/emp", 2), ("/tmp/rb/y", 1)] {
        if let Some((_, i)) = raw_at(&mut r, raw, path) {
            r.expect_eq(&format!("{path}: i_links_count on the disk"), links(&i), n);
        }
    }
    if ino_y != 0 {
        if let (Some(i), Some(ib)) = (got(&mut r, raw.inode(ino_y)), got(&mut r, raw_block(raw.inode_bitmap))) {
            r.expect_eq(&format!("the replaced /tmp/rb/y's inode {ino_y}: i_links_count"), links(&i), 0);
            r.expect(le32(&i, 20) != 0, &format!("the replaced inode {ino_y}: i_dtime set"));
            r.expect_eq(&format!("the replaced inode {ino_y}: its inode bitmap bit"), (ib[((ino_y - 1) / 8) as usize] >> ((ino_y - 1) % 8) & 1) as u64, 0);
            r.expect_eq(&format!("the replaced inode {ino_y}: i_blocks"), le32(&i, 28), 0);
        }
    }
    if ino_emp != 0 {
        if let (Some(i), Some(ib)) = (got(&mut r, raw.inode(ino_emp)), got(&mut r, raw_block(raw.inode_bitmap))) {
            r.expect_eq(&format!("the replaced empty directory /tmp/rb/emp's inode {ino_emp}: i_links_count"), links(&i), 0);
            r.expect(le32(&i, 20) != 0, &format!("the replaced directory's inode {ino_emp}: i_dtime set"));
            r.expect_eq(&format!("the replaced directory's inode {ino_emp}: its inode bitmap bit"), (ib[((ino_emp - 1) / 8) as usize] >> ((ino_emp - 1) % 8) & 1) as u64, 0);
            r.expect_eq(&format!("the replaced directory's inode {ino_emp}: i_blocks"), le32(&i, 28), 0);
        }
    }
    r.expect(!emp_blocks.is_empty(), "the empty directory the rename replaces (/tmp/rb/emp) held a block before it");
    if let Some(bb) = got(&mut r, raw_block(raw.block_bitmap)) {
        for b in &emp_blocks {
            if bb[(b / 8) as usize] >> (b % 8) & 1 == 1 {
                r.fail(format_args!("{RENAME_ID}: block {b} of the replaced directory /tmp/rb/emp still set in the block bitmap"));
            }
        }
    }
    r.expect(!y_blocks.is_empty(), "the file the rename replaces (/tmp/rb/y) held a block before it");
    if let Some(bb) = got(&mut r, raw_block(raw.block_bitmap)) {
        for b in &y_blocks {
            if bb[(b / 8) as usize] >> (b % 8) & 1 == 1 {
                r.fail(format_args!("{RENAME_ID}: block {b} of the replaced /tmp/rb/y still set in the block bitmap"));
            }
        }
    }
    for path in ["/tmp/ra/x", "/tmp/rb/x2", "/tmp/ra/sub", "/tmp/ra/z", "/tmp/rb/s3", "/tmp/ra/mv"] {
        if let Some(Some(_)) = got(&mut r, raw.walk(path)) {
            r.fail(format_args!("{RENAME_ID}: {path} is still a record on the disk after it moved"));
        }
    }
    for path in ["/tmp/ra/z3", "/tmp/rdot", "/tmp/rdd"] {
        if let Some(Some(_)) = got(&mut r, raw.walk(path)) {
            r.fail(format_args!("{RENAME_ID}: {path} is a record on the disk, though the rename to it was refused"));
        }
    }
    // A directory never becomes its own descendant, judged by inode, under
    // every path shape (s20 fix round 7): each case through `rename`, then
    // through the learner's own `Inode::rename`, each refused target at
    // most `depth` levels below its source (the climb floor stays at
    // `depth`). Every name is a word no one knows before this boot.
    //  - over an existing empty directory inside <u>'s subtree;
    //  - a directory directly under / into its own subtree;
    //  - a directory moved under another tree's directory of the same
    //    name, which must succeed (a check on names refuses it);
    //  - into a directory made in the inode a removed directory gave back,
    //    after a climb passed through the removed one;
    //  - into a directory held from before its parent moved below <u2>.
    let (ae, ca, cb, cs, ct, cn, ck, cn2) = (word(25), word(26), word(27), word(28), word(29), word(30), word(31), word(32));
    let (ds, dx, da, dy, dz) = (word(33), word(34), word(35), word(36), word(37));
    let (eq, ea, eb, ez) = (word(38), word(39), word(40), word(41));
    let (gn, gk, gz) = (word(42), word(43), word(44));
    let over = format!("{}/{ae}", chain[drawn]);
    let (ta, tb) = (format!("/tmp/{ca}"), format!("/tmp/{cb}"));
    let (dsx, dad) = (format!("/tmp/{ds}/{dx}"), format!("/tmp/{da}"));
    let dady = format!("{dad}/{dy}");
    let (teq, tea) = (format!("/tmp/{eq}"), format!("/tmp/{ea}"));
    let teb = format!("{tea}/{eb}");
    let (g, ggk) = (format!("/{gn}"), format!("/{gn}/{gk}"));
    let mut pl = Plan::new();
    for path in [over.clone(), ta.clone(), format!("{ta}/{cs}"), format!("{ta}/{ct}"), tb.clone(), format!("{tb}/{cs}"), format!("{tb}/{ct}"), format!("{tb}/{ct}/{ck}"), format!("/tmp/{ds}"), dsx.clone(), dad.clone(), teq.clone(), tea.clone(), teb.clone(), g.clone(), ggk.clone()] {
        let pp = pl.p.str(&path);
        pl.is(&format!("mkdir({path})"), SYS_MKDIR, &[pp, i(0o755)], 0);
    }
    pl.run(&mut r, "r20d");
    let ino_dx = ino(&mut r, &dsx);
    let (ino_cs, ino_ct) = (ino(&mut r, &format!("{ta}/{cs}")), ino(&mut r, &format!("{ta}/{ct}")));
    let twin_to = format!("{tb}/{cs}/{cn}");
    let mut pl = Plan::new();
    for (a, b, want, how) in [
        (top.clone(), over.clone(), -EINVAL, "over an empty directory inside it"),
        (g.clone(), format!("{ggk}/{gz}"), -EINVAL, "a directory under / into its own subtree"),
        (g.clone(), format!("{g}/{gz}"), -EINVAL, "a directory under / into itself"),
        (format!("{ta}/{cs}"), twin_to.clone(), 0, "under another tree's directory of the same name"),
        (format!("/tmp/{ds}"), format!("{dsx}/{dz}"), -EINVAL, "into its own child"),
    ] {
        let (pa, pb) = (pl.p.str(&a), pl.p.str(&b));
        pl.is(&format!("rename({a}, {b}) ({how})"), SYS_RENAME, &[pa, pb], want);
    }
    pl.run(&mut r, "r20e");
    refused_direct(&mut r, raw, "/tmp", &un, &chain[drawn], None, &ae, &top, &format!("over an empty directory inside it, level {drawn} below it"));
    refused_direct(&mut r, raw, "/", &gn, &ggk, None, &gz, &g, "a directory under / into its own subtree");
    refused_direct(&mut r, raw, "/", &gn, &g, None, &gz, &g, "a directory under / into itself");
    // The climb above passed through <dx>; it goes, and a directory made
    // right after it, in another tree, takes the inode it gave back.
    let mut pl = Plan::new();
    let (pdx, pdy, pda, pdz) = (pl.p.str(&dsx), pl.p.str(&dady), pl.p.str(&dad), pl.p.str(&format!("{dady}/{dz}")));
    pl.is(&format!("rmdir({dsx})"), SYS_RMDIR, &[pdx], 0);
    pl.is(&format!("mkdir({dady})"), SYS_MKDIR, &[pdy, i(0o755)], 0);
    pl.is(&format!("rename({dad}, {dady}/{dz}) (into its child, made after {dsx} went)"), SYS_RENAME, &[pda, pdz], -EINVAL);
    pl.run(&mut r, "r20f");
    let ino_dy = ino(&mut r, &dady);
    if ino_dy != 0 && ino_dy != ino_dx {
        note(format_args!("{RENAME_ID}: {dady} is inode {ino_dy}, not the inode {ino_dx} {dsx} gave back, so this kernel's allocator did not reuse it"));
    }
    refused_direct(&mut r, raw, "/tmp", &da, &dady, None, &dz, &dad, &format!("into its child, made after {dsx} went"));
    // A directory held from before its parent moved below <eq>.
    match vfs::root().and_then(|root| vfs::walk(&root, &teb)) {
        Ok(held) => {
            let mut pl = Plan::new();
            let (pa, pb) = (pl.p.str(&tea), pl.p.str(&format!("{teq}/{ea}")));
            pl.is(&format!("rename({tea}, {teq}/{ea})"), SYS_RENAME, &[pa, pb], 0);
            pl.run(&mut r, "r20g");
            let now = format!("{teq}/{ea}/{eb}");
            refused_direct(&mut r, raw, "/tmp", &eq, &now, Some(&held), &ez, &teq, &format!("into {now}, held from before {tea} moved below it"));
        }
        Err(e) => r.fail(format_args!("{RENAME_ID}: {teb} not found through the mount: {}", errno_text(e))),
    }
    // The second twin through the learner's own `Inode::rename`.
    match vfs::root().and_then(|root| Ok((vfs::walk(&root, &ta)?, vfs::walk(&root, &format!("{tb}/{ct}/{ck}"))?))) {
        Ok((pdir, ndir)) => {
            if let Err(e) = pdir.rename(&ct, &*ndir, &cn2) {
                r.fail(format_args!("{RENAME_ID}: Inode::rename(\"{ct}\", {tb}/{ct}/{ck}, \"{cn2}\") called on {ta}'s inode (under another tree's directory of the same name, no path) returned {}, expected 0", errno_text(e)));
            }
        }
        Err(e) => r.fail(format_args!("{RENAME_ID}: {ta} or {tb}/{ct}/{ck} not found through the mount: {}", errno_text(e))),
    }
    // Both twins on the disk: the new record names the moved inode, its
    // `..` names its new parent, and the old record is gone.
    for (to, ino_m, from) in [(twin_to.clone(), ino_cs, format!("{ta}/{cs}")), (format!("{tb}/{ct}/{ck}/{cn2}"), ino_ct, format!("{ta}/{ct}"))] {
        let parent = &to[..to.rfind('/').unwrap_or(0)];
        let pino = ino(&mut r, parent);
        let (pn, mn) = (names(&mut r, raw, parent), names(&mut r, raw, &to));
        let leaf = String::from(&to[parent.len() + 1..]);
        r.expect(ino_m != 0 && pn.iter().any(|e| e.0 == leaf && e.1 == ino_m), &format!("{parent}'s records name {leaf} as inode {ino_m} (moved from {from}): {pn:?}"));
        r.expect(mn.iter().any(|e| e.0 == ".." && e.1 == pino), &format!("{to}'s .. names {parent} (inode {pino}): {mn:?}"));
        if let Some(Some(_)) = got(&mut r, raw.walk(&from)) {
            r.fail(format_args!("{RENAME_ID}: {from} is still a record on the disk after it moved"));
        }
    }
    // After every move: the whole tree, from the root, against both
    // bitmaps and every count (lab 18's check; the disk before the mount
    // is its control).
    match control {
        Ok(v) if v.is_empty() => {}
        Ok(v) => r.fail(format_args!("{RENAME_ID}: this file's check of the disk as written, before the mount, disagrees (the test is wrong): {}", v.join("; "))),
        Err(why) => r.fail(format_args!("{RENAME_ID}: the check before the mount: {why}")),
    }
    match scan(raw).and_then(|s| agree(raw, &s)) {
        Ok(v) => {
            for why in &v {
                r.fail(format_args!("{RENAME_ID}: after the renames: {why}"));
            }
        }
        Err(why) => r.fail(format_args!("{RENAME_ID}: after the renames: {why}")),
    }
    r.finish(3);
}


// ── Row 4: txn.dirs.metadata.write ───────────────────────────────────────

fn txn_row(raw: &Raw, control: Vec<String>) {
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
    let Ok(tmp) = vfs::root().and_then(|root| vfs::walk(&root, "/tmp")) else {
        r.fail(format_args!("{TXN_ID}: /tmp not found through the mount"));
        return r.finish(4);
    };
    // A file made and later deleted around them (watched, not counted);
    // the directory operations must write six kinds (no pointer block).
    let (mut other, mut seen) = ([false; 7], [false; 7]);
    let mut q: Option<InodeRef> = None;
    watched(&mut r, raw, &mut other, "create(\"q-txn\") in /tmp", || tmp.create("q-txn", 0o644).map(|i| q = Some(i)));
    let mut made: Vec<InodeRef> = Vec::new();
    for name in ["qd-txn", "qe-txn"] {
        watched(&mut r, raw, &mut seen, &format!("mkdir(\"{name}\") in /tmp"), || tmp.mkdir(name, 0o755).map(|d| made.push(d)));
    }
    if let (Some(_), [qd, qe]) = (&q, &made[..]) {
        watched(&mut r, raw, &mut seen, "rename(q-txn → qd-txn/q2)", || tmp.rename("q-txn", &**qd, "q2"));
        watched(&mut r, raw, &mut seen, "rename(qd-txn → qe-txn/qd2) (a directory)", || tmp.rename("qd-txn", &**qe, "qd2"));
        watched(&mut r, raw, &mut other, "unlink(\"q2\") in qd2", || qd.unlink("q2"));
        watched(&mut r, raw, &mut seen, "rmdir(\"qd2\") in qe-txn", || qe.rmdir("qd2"));
        watched(&mut r, raw, &mut seen, "rmdir(\"qe-txn\") in /tmp", || tmp.rmdir("qe-txn"));
    } else {
        r.fail(format_args!("{TXN_ID}: the operations stopped early (above)"));
    }
    for (k, s) in seen.iter().enumerate().take(6) {
        if !s {
            r.fail(format_args!("{TXN_ID}: no listed write of {} in the two mkdirs, two renames and two rmdirs: nothing shows the log saw one", KINDS[k]));
        }
    }
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
    for (k, id) in [DENTS_ID, MKDIR_ID, RENAME_ID, TXN_ID].into_iter().enumerate().skip(from - 1) {
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
    let raw = match Raw::open() {
        Ok(raw) => raw,
        Err(why) => {
            fail_rest(1, &format!("disk: {why}"));
            quiet(ktap::end);
            halt()
        }
    };
    // Row 1's input, written before the mount, before row 3's control.
    let seeded = seed_tmp(&raw, ktap::derive(&ktap::nonce().unwrap_or([b'0'; 32]), DENTS_ID));
    // The disk as written must agree with itself: row 3's control.
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
    let proxy = Arc::new(Proxy(SpinLock::new(Some(root.clone()))));
    mount_rest(proxy.clone());
    note(format_args!("vfs: / is the test's root over ext2 on partition 2, /boot the boot archive ({ARCHIVE_LEN} bytes), /ktest the test's programs"));
    dents_row(&raw, &root, seeded);
    drop(root);
    mkdir_row(&raw);
    rename_row(&raw, pre);
    txn_row(&raw, control);
    drop(fs);
    note(format_args!("the drive saw {} reads and {} writes", spy.reads.load(Ordering::Relaxed), writes().len()));
    sched::yield_now();
    quiet(ktap::end);
    halt()
}
