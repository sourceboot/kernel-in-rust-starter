//! Lab 17's vehicle: ext2, the read half. The prelude is lab 16's (the GDT,
//! the IDT, the frame pool, the syscall MSRs, the heap, `sse_on`,
//! `proc::init`, task 0, the PIC and the PIT).
//!
//! The disk is `ktest-17.img` as the course's image writer made it before
//! boot (CONTRACT §A.7 "The disk from lab 16", "The writer's tree"). Every
//! value this file expects comes from the writer's formulas (`tagged`,
//! `t_file`) or from this file's own reads of the disk: a PIO driver of its
//! own (never the learner's `Ata`) reads partition 2's blocks — fs block
//! *b* is the disk's 4 KiB block 8448 + *b* — and a decoder of its own
//! reads the superblock, the inode table, the block maps and the directory
//! records. Nothing the learner's ext2 reports is used as an expectation.
//!
//! Some blocks change on their way up from the drive (`arm`, fix rounds 1
//! and 2, 2026-10-19), in the learner's reads and in this file's own alike,
//! so the image is never written. `/big`'s four pointer blocks — its
//! indirect block, its double indirect block and two indirect blocks under
//! that — are served at four nonce-chosen block numbers the writer left
//! free, and `/big`'s inode names them; the writer's own pointer blocks
//! still read as the writer made them. In the served ones two nonce-chosen
//! entries trade places in the indirect block and in the first block under
//! the double indirect; the rest of that block's 1024 entries name
//! nonce-chosen data blocks of `/big`, and the double indirect block's
//! second entry names the fourth block, so `/big` reads as a file of
//! 2061–2076 blocks. `t/<m>.txt`'s last direct pointer and its indirect
//! block's first entry trade places; `/big`'s inode carries three distinct
//! times and a uid and gid with high halves. A block map that computes
//! where a pointer block or a data block is, instead of reading the pointer
//! that names it, maps blocks wrong.
//!
//! The learner's file system is mounted through the learner's code: `/` is
//! `Ext2::mount(partition2(drive))`'s root, `/boot` the boot archive. Rows
//! 2–4 are each one program this file assembles (a straight run of
//! syscalls, each result stored, then everything written to fd 1 and
//! `exit(0)`), served from `/ktest` — a directory of this file's, mounted
//! through the learner's `vfs::mount` — and started with the learner's
//! `exec::spawn`, its fds 1 and 2 a pipe whose read end this file drains — so every number
//! a row checks went through the learner's syscalls, from ring 3. Six
//! rows, printed by this file in this order (2026-10-19, slice 5a: the
//! split moved `ext2.getdents.records`, and the `ls` run that needs
//! `getdents64`, to lab 20's vehicle; `exec.from_disk` runs `cat` instead):
//!
//!   1 `ext2.superblock.valid` (plain) — a device this vehicle owns sits
//!     between the learner's drive and `partition2`; it counts the reads
//!     that reach the drive and can change fs block 0 on its way up. With
//!     the magic, the revision, the block size or the incompatible features
//!     made wrong, `Ext2::mount` is `-EINVAL` after exactly one read, of fs
//!     block 0. Clean, it mounts, reads fs block 0 first, and `superblock()`
//!     and `group()` hold what this file decodes.
//!   2 `ext2.walk.open_read` (derived) — `/t/<n>.txt`, n from the nonce:
//!     `open`, a 1000-byte `read`, a `read` of the rest (asked for more),
//!     then 0, `close`, and a `read` on the closed fd (`-EBADF`); `openat
//!     (AT_FDCWD, "t/<m>.txt")` for a nonce-chosen m among the 22 files that
//!     reach the indirect block, read whole; `/a/b/c/deep.txt`; `/boot/hi`
//!     through the archive mounted at `/boot`; `-ENOENT`, `-ENOTDIR` (a
//!     path through a file, and `O_DIRECTORY` on a file), `-EISDIR` (a read
//!     of a directory), `O_LARGEFILE` accepted, `-EINVAL` for an unknown
//!     flag bit; then `open`, a `read` of 1 to 18 bytes (the whole file every
//!     18th time) and `close` of a `/ktest` file 300 times in a row, each
//!     `open` an fd, each `read` all it asked for and each `close` 0 — more
//!     than the 256 files a kernel may hold open (fix round 0; the read since
//!     fix round 4, its length since fix round 5). Two files stay open
//!     through that program and the next two groups: one the test opens
//!     itself, from ring 0 with `FileRef::open_inode`, on an inode of its own
//!     holding three nonce-drawn lines, which must read back its next line
//!     after the program and after each group (fix round 6); and
//!     a `/ktest` file named per run from the nonce (fix round 8), three
//!     nonce-drawn lines a program opens with `open` before that program starts and holds until both groups have ended,
//!     then reads back whole (fix round 7). Those reads show that the opens
//!     and closes meanwhile left these two files their slots and their
//!     inodes; any other file is checked for that only while the groups hold
//!     their files, below. Then (fix round 2, 2026-10-19) ten programs at
//!     once each open `/ktest/once` 28 times and wait: exactly 254 opens succeed
//!     in all (`NFILE`, the page's number, less the two files held across)
//!     and the rest are `-ENFILE`; once they have closed their files and
//!     ended, ten more do the same and again get exactly 254. While each
//!     group holds its files, every one of them and the two held across must
//!     answer a slot of its own below 256 (`FileObj::as_file`, the page's
//!     API; fix round 3). Then
//!     (fix rounds 4 to 6) the test opens 256 inodes of its own, 18 bytes of
//!     its own each, with `FileRef::open_inode` until `-ENFILE` (exactly
//!     256); it closes the file in a nonce-chosen place and opens a new
//!     inode: with the table full, the new file must take the slot the close
//!     freed, the only free one, as the same `FileObj`, whatever order the
//!     kernel takes free slots in. Before it closes, the test's i-th file
//!     reads `i % 20` bytes and must get its own: none, part of the file, or
//!     all 18 and then a read at its end. Once all have ended, at most 16
//!     references to `/ktest/once`'s inode and the test's 258 together may
//!     remain besides the test's own, so a closed file drops its inode
//!     whatever offset, 0 to 18, it was read to, and whether or not a read
//!     met its end; a leak keyed on anything else about the file, one that
//!     stays under 16 in all, or one that keeps a closed file's inode until no
//!     file is open, is not checked. (Nothing here
//!     pins what 18 changes: `O_WRONLY`'s `-EROFS`
//!     is not graded, because this vehicle runs on every later cut.)
//!   3 `ext2.lseek` (derived) — on `/pattern`: two `open`s and a `dup` of
//!     the first. A seek through one fd moves the dup's offset and not the
//!     other open's; `SEEK_SET`, `SEEK_CUR` and `SEEK_END` to nonce-chosen
//!     places, each read back against `tagged(…)`; a read past the end is
//!     0; `-EINVAL` for a negative result and for a `whence` of 5 to 255
//!     drawn from the nonce (Linux serves 0-4; 3 and 4 are not graded),
//!     `-ESPIPE` on the pipe, `-EBADF` on a closed fd. Then (fix round 9 and
//!     the close-out, `shared_check`) a second program: an open file stays
//!     whole, its offset and its slot, through a dup chain, a forked child's
//!     closes and the parent's own. It opens `/pattern` twice and reads k
//!     bytes from each, and on each dups the fd and dups the dup, then forks;
//!     the child reads j bytes through one of the first chain's copies,
//!     closes all six and exits; the parent waits, closes the first chain's
//!     fd and then its dup and the second chain's dup and then its fd, and
//!     reads on through each last fd with no seek (`/pattern` at k + j and at
//!     k), opens and closes `/bin/hi` 300 times and reads on again, and
//!     closes the second chain's last fd. With the first chain's last fd
//!     open, the test fills the table from ring 0 (exactly 255, then
//!     `-ENFILE`); the program closes it and ends, and the next open must
//!     take its slot. k and j are drawn from the nonce. Not checked: a file
//!     shared only by fork, a backward `SEEK_CUR`, a forward `SEEK_END`.
//!   4 `ext2.stat.fields` (plain) — `stat`, `fstat` and `newfstatat` of nine
//!     inodes against the inode bytes this file decodes: `st_ino`,
//!     `st_mode`, `st_nlink`, `st_uid`, `st_gid`, `st_rdev`, `st_size`,
//!     `st_blksize` 4096, `st_blocks` (512-byte units, `i_blocks`), the
//!     three times and their zero nanoseconds (`st_dev` is lab 22's);
//!     `fstat` of the pipe a FIFO; `-ENOENT`, `-EFAULT`, `-EBADF`,
//!     `-EINVAL`.
//!   5 `exec.from_disk` (derived) — programs off ext2's `/bin`: `args`,
//!     exec'd by a program of this file's with a nonce-derived argv, prints
//!     its five lab-13 rows `ok` and echoes the argv; `cat` (a `.bss` of
//!     its own) of a nonce-chosen `/t/<k>.txt` and `/a/b/c/deep.txt`
//!     prints both files' bytes, in order; `hi` prints its nonce-derived argument and `ok`. Then the
//!     learner's `Ext2Inode` of `/bin/hi` (`ino()` the number this file
//!     found) read as a `ReadAt` — interface 9, the loader's reader — at
//!     four places, against the bytes this file reads.
//!   6 `ext2.double_indirect` (derived since fix round 1) — `/big` as the
//!     kernel reads it (2061–2076 blocks, `arm`), found by `vfs::walk` and
//!     read whole with `read_at`, 4 KiB at a time: every block equals
//!     `tagged(…)` of the writer's block its pointers name, through both
//!     entries of the double indirect block (file block 1036 on, and 2060
//!     on); unaligned reads across both boundaries; 10 bytes at the end,
//!     then 0. Timed on the ACPI PM timer.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../shim.rs"]
mod shim;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::arch::asm;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

use kernel::addr::VirtAddr;
use kernel::blk::ata::{self, Ata, BlockDevice, BLOCK_SIZE};
use kernel::console::{Fd, FileObj, FileRef};
use kernel::exec;
use kernel::fs::archive::Archive;
use kernel::fs::ext2::Ext2;
use kernel::elf::ReadAt;
use kernel::fs::vfs::{self, dt, mode, DirEntry, Inode, InodeRef, Stat};
use kernel::ktap;
use kernel::pipe;
use kernel::proc::{self, Pid};
use kernel::sync::SpinLock;
use kernel::syscall::Errno;
use kernel::task::{self, Tid};
use kernel::{bootinfo, frame, gdt, heap, idt, irq, sched, serial, syscall, time};

const ARCHIVE_LEN: usize = include_bytes!("../../../user/bin/boot.archive").len();
/// The boot archive, in the kernel image as a section of its own (§A.7).
#[link_section = ".sboot_archive"]
static ARCHIVE: [u8; ARCHIVE_LEN] = *include_bytes!("../../../user/bin/boot.archive");

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 16;
const PM_HZ: u64 = 3_579_545;

const PLAN: usize = 6;
const SB_ID: &str = "ext2.superblock.valid";
const WALK_ID: &str = "ext2.walk.open_read";
const LSEEK_ID: &str = "ext2.lseek";
const STAT_ID: &str = "ext2.stat.fields";
const EXEC_ID: &str = "exec.from_disk";
const DIND_ID: &str = "ext2.double_indirect";

/// fs block 0 of partition 2 as the disk's 4 KiB block (LBA 67,584 / 8).
const P2_BLOCK0: u64 = 8448;
/// `/pattern`: fs blocks 1033–1044, block b = `tagged(PATTERN_MAGIC, b)`.
const PATTERN_MAGIC: u64 = 0x5B00_7B00_B10C;
const PATTERN_FIRST: u64 = 1033;
const PATTERN_SIZE: u64 = 12 * 4096;
/// `/big`: file block i = `tagged(BIG_MAGIC, i)`, 1280 blocks.
const BIG_MAGIC: u64 = 0x5B00_7B00_B16F;
const BIG_BLOCKS: u64 = 1280;
/// The first file block mapped through the double indirect block.
const DIND_FIRST: u64 = 12 + 1024;
/// Pointers in one block.
const PER: u64 = 1024;
/// The first file block behind the double indirect block's second entry:
/// `/big` reaches past it on the disk the kernel reads (`arm`).
const DIND_SECOND: u64 = DIND_FIRST + PER;
/// The step between the writer's blocks that `arm`'s new pointers name.
const FILL_MUL: u64 = 389;
/// `/t/<n>.txt`, n = 0…299.
const T_FILES: u64 = 300;
/// `/big`'s inode as the learner's kernel reads it (`arm`): three distinct
/// times, and an owner whose uid and gid both have a high half.
const BIG_ATIME: u64 = 0x5ec0_1111;
const BIG_CTIME: u64 = 0x5ec0_2222;
const BIG_MTIME: u64 = 0x5ec0_3333;
const BIG_UID: u64 = 0x0007_03e8;
const BIG_GID: u64 = 0x0009_03e9;

/// The probes' data segment (results, strings, buffers).
const DATA: u64 = 0x0110_0000;
const MAX_STEPS: usize = 64;
const RESULTS: usize = 8 * MAX_STEPS;
/// Row 2's open/close pairs: more than the 256 files a kernel of this
/// course may hold open at once (CONTRACT: `-ENFILE` past 256).
const CYCLES: u32 = 300;
/// Row 2's programs that hold files open at once, and the opens each makes:
/// 280 in all, more than `NFILE`.
const HOLDERS: usize = 10;
const HOLD_OPENS: usize = 28;
/// How many files a kernel of this course holds open at once (the page's
/// `NFILE`; CONTRACT §B.2-17).
const NFILE: u64 = 256;
const ENFILE: i64 = 23;
/// References to `/ktest/once` a kernel may still hold, besides the test's
/// own, after every file opened on it has been closed: a cache of the
/// kernel's may keep the inode (one, in a one-entry `vfs::walk` cache), and
/// 812 opens with no `InodeRef` given back leave far more.
const REF_SLACK: usize = 16;

/// `FileObj::as_file` (lab 17 on) where the cut has it: method lookup takes
/// an inherent method of `FileObj` before this trait's, which is on
/// `&FileObj`, so before lab 17, where nothing is a file, every object
/// answers `None` and this file still builds.
trait SlotOf {
    #[allow(dead_code)]
    fn as_file(self) -> Option<u32>;
}

impl SlotOf for &FileObj {
    fn as_file(self) -> Option<u32> {
        None
    }
}

/// `FileRef::open_inode` (lab 17 on) where the cut has it: a path to an
/// associated function takes the inherent one first, so before lab 17 this
/// answers `-ENOSYS` and this file still builds.
trait OpenInodeOf {
    #[allow(dead_code)]
    fn open_inode(ino: InodeRef, flags: u32) -> Result<FileRef, Errno>;
}

impl OpenInodeOf for FileRef {
    fn open_inode(_: InodeRef, _: u32) -> Result<FileRef, Errno> {
        Err(38)
    }
}

/// How long one program may run before this file stops waiting.
const RUN_MS: u64 = 10_000;

/// Linux's numbers.
const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_STAT: u64 = 4;
const SYS_FSTAT: u64 = 5;
const SYS_LSEEK: u64 = 8;
const SYS_DUP: u64 = 32;
const SYS_FORK: u64 = 57;
const SYS_WAIT4: u64 = 61;
const SYS_OPENAT: u64 = 257;
const SYS_NEWFSTATAT: u64 = 262;
const O_LARGEFILE: u64 = 0x8000;
const O_DIRECTORY: u64 = 0x1_0000;
const O_CLOEXEC: u64 = 0x8_0000;
/// A bit Linux's `open` gives no meaning (§A.4: `-EINVAL`).
const O_UNKNOWN: u64 = 0x100_0000;
const O_NONBLOCK: u32 = 0x800;
const AT_FDCWD: u64 = -100i64 as u64;
const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
const SEEK_SET: u64 = 0;
const SEEK_CUR: u64 = 1;
const SEEK_END: u64 = 2;
const EBADF: i64 = 9;
const ENOENT: i64 = 2;
const EFAULT: i64 = 14;
const ENOTDIR: i64 = 20;
const EISDIR: i64 = 21;
const EINVAL: i64 = 22;
const ESPIPE: i64 = 29;
const S_IFIFO: u64 = 0o010000;
const S_IFMT: u64 = 0o170000;

// ── Time that does not depend on the learner's tick ──────────────────────

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
        2 => "ENOENT",
        5 => "EIO",
        8 => "ENOEXEC",
        9 => "EBADF",
        12 => "ENOMEM",
        13 => "EACCES",
        14 => "EFAULT",
        19 => "ENODEV",
        20 => "ENOTDIR",
        21 => "EISDIR",
        22 => "EINVAL",
        23 => "ENFILE",
        24 => "EMFILE",
        29 => "ESPIPE",
        30 => "EROFS",
        38 => "ENOSYS",
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
fn tagged(magic: u64, index: u64) -> Box<[u8; BLOCK_SIZE]> {
    let mut b = Box::new([0u8; BLOCK_SIZE]);
    for k in 0..BLOCK_SIZE / 8 {
        let w = match k {
            0 => magic,
            1 => index,
            _ => ktap::splitmix64(magic ^ (index * 512 + k as u64)),
        };
        b[8 * k..8 * k + 8].copy_from_slice(&w.to_le_bytes());
    }
    b
}

/// `/pattern`'s bytes `off .. off + len` (cut at its end).
fn pattern(off: u64, len: u64) -> Vec<u8> {
    let end = (off + len).min(PATTERN_SIZE);
    let mut v = Vec::new();
    let mut at = off;
    while at < end {
        let blk = tagged(PATTERN_MAGIC, PATTERN_FIRST + at / 4096);
        let from = (at % 4096) as usize;
        let n = (4096 - from).min((end - at) as usize);
        v.extend_from_slice(&blk[from..from + n]);
        at += n as u64;
    }
    v
}

/// `t_file(n)`'s size: `1 + (n * 4999) % 53000`.
fn t_size(n: u64) -> u64 {
    1 + (n * 4999) % 53000
}

/// `t_file(n)`: the lines `"<n>.txt line <k>\n"`, cut at its size.
fn t_file(n: u64) -> Vec<u8> {
    let size = t_size(n) as usize;
    let mut v = Vec::with_capacity(size + 32);
    let mut k = 0u64;
    while v.len() < size {
        v.extend_from_slice(format!("{n}.txt line {k}\n").as_bytes());
        k += 1;
    }
    v.truncate(size);
    v
}

fn hex16(v: u64) -> String {
    format!("{v:016x}")
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

/// Partition 2's fs block `b`, raw.
fn raw_block(b: u64) -> Result<Box<[u8; BLOCK_SIZE]>, String> {
    let mut buf = Box::new([0u8; BLOCK_SIZE]);
    if raw_disk_block(P2_BLOCK0 + b, &mut buf) {
        apply_puts(b, &mut buf);
        Ok(buf)
    } else {
        Err(format!("the test's own read of fs block {b} failed"))
    }
}

/// One directory record as the disk holds it.
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

/// The file system as this file reads it.
struct Raw {
    sb: Box<[u8; BLOCK_SIZE]>,
    inode_table: u64,
}

impl Raw {
    fn open() -> Result<Raw, String> {
        let sb = raw_block(0)?;
        if le16(&sb[..], 1024 + 56) != 0xef53 {
            return Err(String::from("fs block 0 holds no ext2 superblock (magic) as the test reads it"));
        }
        let gd = raw_block(1)?;
        Ok(Raw { sb, inode_table: le32(&gd[..], 8) })
    }
    fn sbf(&self, at: usize, n: usize) -> u64 {
        let s = &self.sb[1024..2048];
        match n {
            2 => le16(s, at),
            _ => le32(s, at),
        }
    }
    fn inode(&self, ino: u64) -> Result<[u8; 128], String> {
        let at = (ino - 1) * 128;
        let b = raw_block(self.inode_table + at / 4096)?;
        let off = (at % 4096) as usize;
        let mut r = [0u8; 128];
        r.copy_from_slice(&b[off..off + 128]);
        Ok(r)
    }
    fn bmap(&self, inode: &[u8; 128], i: u64) -> Result<u64, String> {
        let ib = |k: usize| le32(inode, 40 + 4 * k);
        if i < 12 {
            return Ok(ib(i as usize));
        }
        let i = i - 12;
        if i < 1024 {
            return Ok(le32(&raw_block(ib(12))?[..], 4 * i as usize));
        }
        let i = i - 1024;
        let ind = le32(&raw_block(ib(13))?[..], 4 * (i / 1024) as usize);
        Ok(le32(&raw_block(ind)?[..], 4 * (i % 1024) as usize))
    }
    fn size(inode: &[u8; 128]) -> u64 {
        le32(inode, 4)
    }
    /// Where inode `ino` sits: its fs block in the inode table, and the
    /// byte in that block.
    fn inode_place(&self, ino: u64) -> (u64, usize) {
        let at = (ino - 1) * 128;
        (self.inode_table + at / 4096, (at % 4096) as usize)
    }
    /// The fs blocks the writer left free (clear in the block bitmap).
    fn free_blocks(&self) -> Result<Vec<u64>, String> {
        let gd = raw_block(1)?;
        let bm = raw_block(le32(&gd[..], 0))?;
        let n = self.sbf(4, 4).min(8 * BLOCK_SIZE as u64);
        Ok((0..n).filter(|&b| bm[(b / 8) as usize] & (1 << (b % 8)) == 0).collect())
    }
    /// A file's bytes, block by block through this file's own map.
    fn file(&self, path: &str) -> Result<Vec<u8>, String> {
        let inode = self.inode(self.walk(path)?)?;
        let size = Raw::size(&inode);
        let mut out = Vec::new();
        for i in 0..size.div_ceil(4096) {
            out.extend_from_slice(&raw_block(self.bmap(&inode, i)?)?[..]);
        }
        out.truncate(size as usize);
        Ok(out)
    }
    /// Every live record of directory `ino`, block by block.
    fn dir(&self, ino: u64) -> Result<Vec<Rec>, String> {
        let inode = self.inode(ino)?;
        let mut out = Vec::new();
        for i in 0..Raw::size(&inode).div_ceil(4096) {
            let b = raw_block(self.bmap(&inode, i)?)?;
            let mut at = 0usize;
            while at < 4096 {
                let rino = le32(&b[..], at);
                let rec_len = le16(&b[..], at + 4) as usize;
                let name_len = b[at + 6] as usize;
                if rec_len < 8 + name_len || at + rec_len > 4096 {
                    return Err(format!("directory inode {ino}: a record at byte {at} of its block {i} does not parse (the test's reader)"));
                }
                if rino != 0 {
                    let name = String::from_utf8_lossy(&b[at + 8..at + 8 + name_len]).into_owned();
                    out.push(Rec { ino: rino, dtype: dtype(b[at + 7]), name });
                }
                at += rec_len;
            }
        }
        Ok(out)
    }
    /// The inode an absolute path names (ext2 only: no mounts).
    fn walk(&self, path: &str) -> Result<u64, String> {
        let mut ino = 2u64;
        for c in path.split('/').filter(|c| !c.is_empty()) {
            ino = self.dir(ino)?.into_iter().find(|r| r.name == c).map(|r| r.ino).ok_or_else(|| format!("{path}: no {c} on the disk"))?;
        }
        Ok(ino)
    }
}

// ── What the test changes on the disk the kernel reads ───────────────────

/// Byte ranges this file puts into fs blocks on their way up from the
/// drive, in the learner's reads (through `Patch`, under the mounted file
/// system) and in this file's own (`raw_block`) alike, so every expectation
/// below is of the disk as the learner's kernel sees it. Armed once, before
/// row 1, from this file's reads of the unchanged disk. Nothing is written:
/// the image stays the writer's.
const NPUTS: usize = 16;
static PUTS: AtomicUsize = AtomicUsize::new(0);
static PUT_BLOCK: [AtomicU64; NPUTS] = [const { AtomicU64::new(0) }; NPUTS];
static PUT_PLACE: [AtomicU64; NPUTS] = [const { AtomicU64::new(0) }; NPUTS];
static PUT_VALUE: [AtomicU64; NPUTS] = [const { AtomicU64::new(0) }; NPUTS];

fn put(b: u64, at: usize, len: usize, v: u64) {
    let k = PUTS.load(Ordering::Relaxed);
    assert!(k < NPUTS, "more than {NPUTS} changes");
    PUT_BLOCK[k].store(b, Ordering::Relaxed);
    PUT_PLACE[k].store(at as u64 | (len as u64) << 32, Ordering::Relaxed);
    PUT_VALUE[k].store(v, Ordering::Relaxed);
    PUTS.store(k + 1, Ordering::Release);
}

/// Whole blocks this file serves in place of free ones (`arm`): `/big`'s
/// pointer blocks as the kernel reads them. Each is a leaked block, never
/// written or freed after `serve_block`.
const NSERVED: usize = 4;
static SERVED: AtomicUsize = AtomicUsize::new(0);
static SERVED_BLOCK: [AtomicU64; NSERVED] = [const { AtomicU64::new(0) }; NSERVED];
static SERVED_AT: [AtomicUsize; NSERVED] = [const { AtomicUsize::new(0) }; NSERVED];

fn serve_block(b: u64, data: Box<[u8; BLOCK_SIZE]>) {
    let k = SERVED.load(Ordering::Relaxed);
    assert!(k < NSERVED, "more than {NSERVED} served blocks");
    SERVED_BLOCK[k].store(b, Ordering::Relaxed);
    SERVED_AT[k].store(Box::into_raw(data) as usize, Ordering::Relaxed);
    SERVED.store(k + 1, Ordering::Release);
}

fn apply_puts(b: u64, buf: &mut [u8; BLOCK_SIZE]) {
    for k in 0..SERVED.load(Ordering::Acquire) {
        if SERVED_BLOCK[k].load(Ordering::Relaxed) == b {
            // SAFETY: a block `serve_block` leaked; nothing writes or frees it.
            buf.copy_from_slice(unsafe { &*(SERVED_AT[k].load(Ordering::Relaxed) as *const [u8; BLOCK_SIZE]) });
        }
    }
    for k in 0..PUTS.load(Ordering::Acquire) {
        if PUT_BLOCK[k].load(Ordering::Relaxed) == b {
            let place = PUT_PLACE[k].load(Ordering::Relaxed);
            let (at, len) = ((place & 0xffff_ffff) as usize, (place >> 32) as usize);
            buf[at..at + len].copy_from_slice(&PUT_VALUE[k].load(Ordering::Relaxed).to_le_bytes()[..len]);
        }
    }
}

/// What `arm` changed, for the rows that expect it.
struct Changes {
    /// Entries that trade places: `a` and `b` in the indirect block (file
    /// blocks 12 + a, 12 + b), `c` and `e` in the first block under the
    /// double indirect (`DIND_FIRST` + c, + e).
    a: u64,
    b: u64,
    c: u64,
    e: u64,
    /// Where the new pointers start among the writer's blocks: entry k of
    /// the first inner block past the writer's end names the writer's file
    /// block `(k * FILL_MUL + x) % BIG_BLOCKS`; entry k of the second,
    /// `(k * FILL_MUL + y) % DIND_FIRST`.
    x: u64,
    y: u64,
    /// `/big`'s size as the kernel reads it.
    size: u64,
    /// The served pointer blocks: indirect, double indirect, the two inner.
    at: [u64; 4],
    /// The writer's: indirect, double indirect, the inner one.
    was: [u64; 3],
}

impl Changes {
    fn blocks(&self) -> u64 {
        self.size.div_ceil(4096)
    }
    /// The writer's file block whose data `/big`'s file block `i` holds.
    fn big_src(&self, i: u64) -> u64 {
        let swap = |k: u64, p: u64, q: u64| if k == p { q } else if k == q { p } else { k };
        if i < 12 {
            i
        } else if i < DIND_FIRST {
            12 + swap(i - 12, self.a, self.b)
        } else if i < BIG_BLOCKS {
            DIND_FIRST + swap(i - DIND_FIRST, self.c, self.e)
        } else if i < DIND_SECOND {
            ((i - DIND_FIRST) * FILL_MUL + self.x) % BIG_BLOCKS
        } else {
            ((i - DIND_SECOND) * FILL_MUL + self.y) % DIND_FIRST
        }
    }
}

fn set32(b: &mut [u8; BLOCK_SIZE], k: u64, v: u64) {
    b[4 * k as usize..4 * k as usize + 4].copy_from_slice(&(v as u32).to_le_bytes());
}

/// Row 2's two files: `/t/<n>.txt`, and `t/<m>.txt` among the files past
/// the 12 direct blocks.
fn walk_pick(d: u64) -> (u64, u64, usize) {
    let n = d % T_FILES;
    let big: Vec<u64> = (0..T_FILES).filter(|&k| t_size(k) > 12 * 4096).collect();
    // m is never n: the first file's reads expect the disk as the writer
    // made it, and m's blocks 11 and 12 trade places (fix round 3; nonce
    // 8e8c0132… drew 286 for both).
    let mut i = (ktap::splitmix64(d) % big.len() as u64) as usize;
    if big[i] == n {
        i = (i + 1) % big.len();
    }
    (n, big[i], big.len())
}

/// Arm the changes, from the run's nonce (all read from the unchanged disk
/// first, then put in place): `/big`'s pointer blocks served at four free
/// block numbers, with two entries trading places in the indirect block
/// and in the first block under the double indirect, that block's entries
/// past the writer's end filled, and a second block under the double
/// indirect; `/big`'s inode naming them, its size reaching into the second,
/// its times and owner `BIG_*`'s; `t/<m>.txt`'s last direct pointer and its
/// indirect block's first entry trading places.
fn arm(raw: &Raw) -> Result<Changes, String> {
    let nonce = ktap::nonce().unwrap_or([b'0'; 32]);
    let d = ktap::derive(&nonce, DIND_ID);
    let d2 = ktap::splitmix64(d);
    let (_, m, _) = walk_pick(ktap::derive(&nonce, WALK_ID));
    let big_ino = raw.walk("/big")?;
    let big = raw.inode(big_ino)?;
    let (ind, dind) = (le32(&big, 40 + 48), le32(&big, 40 + 52));
    let ind_b = raw_block(ind)?;
    let inner = le32(&raw_block(dind)?[..], 0);
    let inner_b = raw_block(inner)?;
    // The writer's fs block of its file block j.
    let phys = |j: u64| -> u64 {
        if j < 12 {
            le32(&big, 40 + 4 * j as usize)
        } else if j < DIND_FIRST {
            le32(&ind_b[..], 4 * (j - 12) as usize)
        } else {
            le32(&inner_b[..], 4 * (j - DIND_FIRST) as usize)
        }
    };
    let behind = BIG_BLOCKS - DIND_FIRST;
    let a = d % PER;
    let b = (a + 1 + (d >> 12) % (PER - 1)) % PER;
    let c = (d >> 24) % behind;
    let e = (c + 1 + (d >> 36) % (behind - 1)) % behind;
    let past = 1 + d2 % 16;
    // The last block holds 200 to 4096 bytes, so the last 10 are in it and,
    // when it is file block 2060, the read across 2060's start gets its 200
    // (fix round 7: with 10 to 4096, a last block 2060 of under 200 bytes
    // failed the reference, 14 bytes measured).
    let size = (DIND_SECOND + past - 1) * 4096 + 200 + (d2 >> 8) % 3897;
    let free = raw.free_blocks()?;
    if free.len() < 4 {
        return Err(format!("the writer left {} blocks free; the test needs 4", free.len()));
    }
    let mut at = [0u64; 4];
    let mut k = 0usize;
    let mut h = d2;
    while k < 4 {
        h = ktap::splitmix64(h);
        let f = free[(h % free.len() as u64) as usize];
        if !at[..k].contains(&f) {
            at[k] = f;
            k += 1;
        }
    }
    let ch = Changes { a, b, c, e, x: (d2 >> 20) % BIG_BLOCKS, y: (d2 >> 40) % DIND_FIRST, size, at, was: [ind, dind, inner] };
    let mut ib = Box::new([0u8; BLOCK_SIZE]);
    let mut first = Box::new([0u8; BLOCK_SIZE]);
    let mut second = Box::new([0u8; BLOCK_SIZE]);
    let mut db = Box::new([0u8; BLOCK_SIZE]);
    for k in 0..PER {
        set32(&mut ib, k, phys(ch.big_src(12 + k)));
        set32(&mut first, k, phys(ch.big_src(DIND_FIRST + k)));
    }
    for k in 0..past {
        set32(&mut second, k, phys(ch.big_src(DIND_SECOND + k)));
    }
    set32(&mut db, 0, at[2]);
    set32(&mut db, 1, at[3]);
    let mut puts: Vec<(u64, usize, usize, u64)> = Vec::new();
    let m_ino = raw.walk(&format!("/t/{m}.txt"))?;
    let mi = raw.inode(m_ino)?;
    let m_ind = le32(&mi, 40 + 48);
    let (mb, mo) = raw.inode_place(m_ino);
    puts.push((mb, mo + 40 + 44, 4, le32(&raw_block(m_ind)?[..], 0)));
    puts.push((m_ind, 0, 4, le32(&mi, 40 + 44)));
    let (bb, bo) = raw.inode_place(big_ino);
    for (at, len, v) in [
        (2, 2, BIG_UID & 0xffff),
        (4, 4, size),
        (8, 4, BIG_ATIME),
        (12, 4, BIG_CTIME),
        (16, 4, BIG_MTIME),
        (24, 2, BIG_GID & 0xffff),
        (40 + 48, 4, at[0]),
        (40 + 52, 4, at[1]),
        (120, 2, BIG_UID >> 16),
        (122, 2, BIG_GID >> 16),
    ] {
        puts.push((bb, bo + at, len, v));
    }
    for (blk, at, len, v) in puts {
        put(blk, at, len, v);
    }
    for (blk, data) in [(at[0], ib), (at[1], db), (at[2], first), (at[3], second)] {
        serve_block(blk, data);
    }
    Ok(ch)
}

// ── Row 1: a device between the drive and the partition ──────────────────

/// Counts the reads that reach the learner's drive, notes the first, and
/// changes fs block 0's superblock on its way up as `case` says.
struct Patch {
    inner: Arc<dyn BlockDevice>,
    reads: AtomicU64,
    first: AtomicU64,
    case: AtomicU32,
}

/// What each case changes, and how it reads.
const CASES: [(u32, &str); 4] = [
    (1, "s_magic 0xEF54"),
    (2, "s_rev_level 0"),
    (3, "s_log_block_size 0 (1 KiB blocks)"),
    (4, "s_feature_incompat with 0x40 (extents) beside filetype"),
];

impl BlockDevice for Patch {
    fn read_block(&self, n: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), Errno> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let _ = self.first.compare_exchange(u64::MAX, n, Ordering::Relaxed, Ordering::Relaxed);
        self.inner.read_block(n, buf)?;
        if n == P2_BLOCK0 {
            let sb = &mut buf[1024..2048];
            match self.case.load(Ordering::Relaxed) {
                1 => sb[56] = 0x54,
                2 => sb[76..80].fill(0),
                3 => sb[24..28].fill(0),
                4 => sb[96] |= 0x40,
                _ => {}
            }
        }
        if n >= P2_BLOCK0 {
            apply_puts(n - P2_BLOCK0, buf);
        }
        Ok(())
    }
    fn write_block(&self, n: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), Errno> {
        self.inner.write_block(n, buf)
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

fn superblock_row(raw: Option<&Raw>) -> Option<Arc<Ext2>> {
    let mut r = Row::new(SB_ID);
    let drive: Arc<dyn BlockDevice> = match quiet(Ata::probe) {
        Ok(a) => Arc::new(a),
        Err(e) => {
            r.fail(format_args!("{SB_ID}: Ata::probe() returned {}: no drive", errno_text(e)));
            r.finish(1);
            return None;
        }
    };
    let dev = Arc::new(Patch { inner: drive, reads: AtomicU64::new(0), first: AtomicU64::new(u64::MAX), case: AtomicU32::new(0) });
    let start = |case: u32| {
        dev.case.store(case, Ordering::Relaxed);
        let p = quiet(|| ata::partition2(dev.clone()));
        dev.reads.store(0, Ordering::Relaxed);
        dev.first.store(u64::MAX, Ordering::Relaxed);
        p
    };
    for (case, what) in CASES {
        let p = match start(case) {
            Ok(p) => p,
            Err(e) => {
                r.fail(format_args!("{SB_ID}: partition2(drive) returned {}", errno_text(e)));
                continue;
            }
        };
        let got = quiet(|| Ext2::mount(p));
        let reads = dev.reads.load(Ordering::Relaxed);
        let first = dev.first.load(Ordering::Relaxed);
        note(format_args!(
            "{SB_ID}: superblock with {what}: Ext2::mount {} after {reads} device reads, the first of disk block {}",
            match &got {
                Ok(_) => String::from("mounted it"),
                Err(e) => format!("returned {}", errno_text(*e)),
            },
            if first == u64::MAX { String::from("(none)") } else { format!("{first}") }
        ));
        match got {
            Err(e) if e == EINVAL => {}
            Err(e) => r.fail(format_args!("{SB_ID}: mount with {what} returned {}, expected -22 (EINVAL)", errno_text(e))),
            Ok(_) => r.fail(format_args!("{SB_ID}: mount accepted a superblock with {what}; expected -22 (EINVAL)")),
        }
        r.expect_eq(&format!("device reads before mount refused {what} (fs block 0 only)"), reads, 1);
        r.expect_eq(&format!("the first block mount read ({what}): the disk's block of fs block 0"), first, P2_BLOCK0);
    }
    let fs = match start(0) {
        Ok(p) => quiet(|| Ext2::mount(p)),
        Err(e) => Err(e),
    };
    let reads = dev.reads.load(Ordering::Relaxed);
    let first = dev.first.load(Ordering::Relaxed);
    let fs = match fs {
        Ok(fs) => Arc::new(fs),
        Err(e) => {
            r.fail(format_args!("{SB_ID}: Ext2::mount on the writer's partition returned {}", errno_text(e)));
            r.finish(1);
            return None;
        }
    };
    note(format_args!("{SB_ID}: the writer's superblock: mounted after {reads} device reads, the first of disk block {first}"));
    r.expect_eq("the first block a clean mount read: the disk's block of fs block 0", first, P2_BLOCK0);
    match raw {
        Some(raw) => {
            let sb = fs.superblock();
            let pairs: [(&str, u64, u64); 10] = [
                ("s_inodes_count", sb.inodes_count as u64, raw.sbf(0, 4)),
                ("s_blocks_count", sb.blocks_count as u64, raw.sbf(4, 4)),
                ("s_log_block_size", sb.log_block_size as u64, raw.sbf(24, 4)),
                ("s_blocks_per_group", sb.blocks_per_group as u64, raw.sbf(32, 4)),
                ("s_inodes_per_group", sb.inodes_per_group as u64, raw.sbf(40, 4)),
                ("s_magic", sb.magic as u64, raw.sbf(56, 2)),
                ("s_rev_level", sb.rev_level as u64, raw.sbf(76, 4)),
                ("s_first_ino", sb.first_ino as u64, raw.sbf(84, 4)),
                ("s_inode_size", sb.inode_size as u64, raw.sbf(88, 2)),
                ("s_feature_incompat", sb.feature_incompat as u64, raw.sbf(96, 4)),
            ];
            for (what, got, want) in pairs {
                r.expect_eq(&format!("superblock().{what}"), got, want);
            }
            let gd = fs.group();
            match raw_block(1) {
                Ok(b) => {
                    r.expect_eq("group().block_bitmap", gd.block_bitmap as u64, le32(&b[..], 0));
                    r.expect_eq("group().inode_bitmap", gd.inode_bitmap as u64, le32(&b[..], 4));
                    r.expect_eq("group().inode_table", gd.inode_table as u64, le32(&b[..], 8));
                }
                Err(why) => r.fail(format_args!("{SB_ID}: {why}")),
            }
        }
        None => r.fail(format_args!("{SB_ID}: the test could not read the disk itself, so nothing can be compared")),
    }
    r.finish(1);
    Some(fs)
}

// ── The vehicle's own directory, `/ktest` ────────────────────────────────

/// The programs this file assembles, served at `/ktest/<name>` (mounted
/// through the learner's `vfs::mount`, so the learner's `exec::spawn` loads
/// them as it loads any file). Only this file's thread touches the table,
/// with interrupts off.
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
    /// `open(path, 0)`, a `read` of `pairs % 18 + 1` bytes, then `close` of
    /// that fd, `times` times, as one counted loop whose state is in the
    /// data segment (so nothing rests on a register across a syscall): at
    /// `at`, the pairs finished; at `at + 8`, the last `open`'s result; at
    /// `at + 16`, the last `close`'s; at `at + 24`, the last `read`'s; at
    /// `at + 32`, the last `read`'s length. It stops at the first `open`
    /// below 0, `read` short of its length or `close` not 0. Returns `at`.
    fn cycle(&mut self, path: A, times: u32) -> usize {
        let (_, at) = self.buf(64);
        let A::D(off) = path else { panic!("cycle: the path must be a string") };
        let slot = |k: u64| ((DATA + at as u64 + 8 * k) as u32).to_le_bytes();
        let c = &mut self.code;
        let top = c.len();
        c.extend_from_slice(&MOV_IMM[0]);
        c.extend_from_slice(&(DATA + off as u64).to_le_bytes());
        c.extend_from_slice(&MOV_IMM[1]);
        c.extend_from_slice(&0u64.to_le_bytes());
        c.extend_from_slice(&[0xb8, SYS_OPEN as u8, 0, 0, 0, 0x0f, 0x05]); // mov eax, 2; syscall
        c.extend_from_slice(&[0x48, 0x89, 0x04, 0x25]); // mov [last open], rax
        c.extend_from_slice(&slot(1));
        c.extend_from_slice(&[0x48, 0x85, 0xc0, 0x0f, 0x88]); // test rax, rax; js out
        let js = c.len();
        c.extend_from_slice(&[0; 4]);
        c.extend_from_slice(&[0x48, 0x8b, 0x04, 0x25]); // mov rax, [pairs]
        c.extend_from_slice(&slot(0));
        c.extend_from_slice(&[0x31, 0xd2, 0xb9, 18, 0, 0, 0, 0x48, 0xf7, 0xf1]); // xor edx, edx; mov ecx, 18; div rcx
        c.extend_from_slice(&[0x48, 0xff, 0xc2, 0x48, 0x89, 0x14, 0x25]); // inc rdx; mov [length], rdx
        c.extend_from_slice(&slot(4));
        c.extend_from_slice(&MOV_MEM[0]); // mov rdi, [last open]
        c.extend_from_slice(&slot(1));
        c.extend_from_slice(&MOV_IMM[1]); // mov rsi, &scratch
        c.extend_from_slice(&(DATA + at as u64 + 40).to_le_bytes());
        c.extend_from_slice(&MOV_MEM[2]); // mov rdx, [length]
        c.extend_from_slice(&slot(4));
        c.extend_from_slice(&[0xb8, SYS_READ as u8, 0, 0, 0, 0x0f, 0x05]); // mov eax, 0; syscall
        c.extend_from_slice(&[0x48, 0x89, 0x04, 0x25]); // mov [last read], rax
        c.extend_from_slice(&slot(3));
        c.extend_from_slice(&[0x48, 0x3b, 0x04, 0x25]); // cmp rax, [length]
        c.extend_from_slice(&slot(4));
        c.extend_from_slice(&[0x0f, 0x85]); // jne out
        let jne = c.len();
        c.extend_from_slice(&[0; 4]);
        c.extend_from_slice(&MOV_MEM[0]); // mov rdi, [last open]
        c.extend_from_slice(&slot(1));
        c.extend_from_slice(&[0xb8, SYS_CLOSE as u8, 0, 0, 0, 0x0f, 0x05]); // mov eax, 3; syscall
        c.extend_from_slice(&[0x48, 0x89, 0x04, 0x25]); // mov [last close], rax
        c.extend_from_slice(&slot(2));
        c.extend_from_slice(&[0x48, 0x85, 0xc0, 0x0f, 0x85]); // test rax, rax; jnz out
        let jnz = c.len();
        c.extend_from_slice(&[0; 4]);
        c.extend_from_slice(&[0x48, 0xff, 0x04, 0x25]); // inc qword [pairs]
        c.extend_from_slice(&slot(0));
        c.extend_from_slice(&[0x48, 0x81, 0x3c, 0x25]); // cmp qword [pairs], times
        c.extend_from_slice(&slot(0));
        c.extend_from_slice(&times.to_le_bytes());
        c.extend_from_slice(&[0x0f, 0x82]); // jb top
        let jb = c.len();
        c.extend_from_slice(&[0; 4]);
        let out = c.len();
        for (at, to) in [(js, out), (jne, out), (jnz, out), (jb, top)] {
            c[at..at + 4].copy_from_slice(&((to as i64 - (at as i64 + 4)) as i32).to_le_bytes());
        }
        at
    }
    /// Steps `body` adds run only where step `k` returned 0 (a forked
    /// child, fix round 9), and that copy then writes its data segment as
    /// it is so far to fd 1 and calls `exit(0)`; elsewhere they are skipped.
    /// The length that copy writes.
    fn child(&mut self, k: usize, body: impl FnOnce(&mut Probe)) -> usize {
        self.code.extend_from_slice(&[0x48, 0x83, 0x3c, 0x25]); // cmp qword [disp32], 0
        self.code.extend_from_slice(&((DATA + 8 * k as u64) as u32).to_le_bytes());
        self.code.extend_from_slice(&[0x00, 0x0f, 0x85]); // jne past
        let at = self.code.len();
        self.code.extend_from_slice(&[0; 4]);
        body(self);
        let len = self.data.len();
        self.call(SYS_WRITE, &[A::I(1), A::I(DATA), A::I(len as u64)]);
        // xor edi, edi; mov eax, 60 (exit); syscall; ud2
        self.code.extend_from_slice(&[0x31, 0xff, 0xb8, 60, 0, 0, 0, 0x0f, 0x05, 0x0f, 0x0b]);
        let past = self.code.len();
        self.code[at..at + 4].copy_from_slice(&((past - (at + 4)) as i32).to_le_bytes());
        len
    }
    /// The ELF: text (R X) at `proc::BASE` from file offset 0, the data
    /// segment (RW) at `DATA` from the next page of the file.
    fn elf(mut self) -> (Vec<u8>, usize) {
        let len = self.data.len();
        self.call(SYS_WRITE, &[A::I(1), A::I(DATA), A::I(len as u64)]);
        self.image(len)
    }
    /// The ELF of a program that writes its data segment to fd 1, then
    /// reads fd 0 until its end, then closes the fds steps `0..opens`
    /// returned, then `exit(0)`: its files stay open while fd 0 does.
    fn elf_held(mut self, opens: usize) -> (Vec<u8>, usize) {
        let (scratch, _) = self.buf(8);
        let len = self.data.len();
        self.call(SYS_WRITE, &[A::I(1), A::I(DATA), A::I(len as u64)]);
        self.call(SYS_READ, &[A::I(0), scratch, A::I(1)]);
        for k in 0..opens {
            self.call(SYS_CLOSE, &[A::R(k)]);
        }
        self.image(len)
    }
    fn image(mut self, len: usize) -> (Vec<u8>, usize) {
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
fn exec_probe() -> Vec<u8> {
    const CODE: [u8; 42] = [
        0x48, 0x8b, 0x7c, 0x24, 0x10, // mov rdi, [rsp+16]          argv[1]: the path
        0x48, 0x8d, 0x74, 0x24, 0x18, // lea rsi, [rsp+24]          &argv[2]: the new argv
        0x48, 0x8b, 0x0c, 0x24, //       mov rcx, [rsp]             argc
        0x48, 0x8d, 0x54, 0xcc, 0x10, // lea rdx, [rsp+rcx*8+16]    envp
        0xb8, 59, 0, 0, 0, //            mov eax, 59 (execve)
        0x0f, 0x05, //                   syscall
        0xf7, 0xd8, //                   neg eax
        0x83, 0xc0, 100, //              add eax, 100
        0x89, 0xc7, //                   mov edi, eax
        0xb8, 60, 0, 0, 0, //            mov eax, 60 (exit)
        0x0f, 0x05, //                   syscall
        0x0f, 0x0b, //                   ud2
    ];
    let len = 120 + CODE.len();
    let mut b = alloc::vec![0u8; len];
    let mut put = |off: usize, v: u64, n: usize| b[off..off + n].copy_from_slice(&v.to_le_bytes()[..n]);
    put(0, 0x0101_0246_4c45_7f, 7);
    put(16, 2, 2);
    put(18, 0x3e, 2);
    put(20, 1, 4);
    put(24, proc::BASE + 120, 8);
    put(32, 64, 8);
    put(52, 64, 2);
    put(54, 56, 2);
    put(56, 1, 2);
    put(64, 1, 4);
    put(68, 5, 4);
    put(80, proc::BASE, 8);
    put(88, proc::BASE, 8);
    put(96, len as u64, 8);
    put(104, len as u64, 8);
    put(112, 0x1000, 8);
    b[120..].copy_from_slice(&CODE);
    b
}

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

/// Start `n` copies of the program at `path` at once, each with fd 0 the
/// read end of one pipe whose write end this file holds and fds 1 and 2 a
/// pipe of its own; wait until each has written `len` bytes (it holds its
/// files open then); close the write end, so each reads fd 0's end, closes
/// its files and exits; wait for all of them to end. Each one's `len`
/// bytes.
fn run_held(what: &str, path: &str, n: usize, len: usize, look: &mut dyn FnMut(&[Pid])) -> Result<Vec<Vec<u8>>, String> {
    let (hr, hw) = pipe::open(0).map_err(|e| format!("{what}: pipe::open(0) returned {}", errno_text(e)))?;
    let mut progs: Vec<(Fd, Pid, Vec<u8>, bool)> = Vec::new();
    let mut why: Option<String> = None;
    for k in 0..n {
        let (r, w) = match pipe::open(0) {
            Ok(p) => p,
            Err(e) => {
                why = Some(format!("{what}: pipe::open(0) for program {k} returned {}", errno_text(e)));
                break;
            }
        };
        r.flags.store(r.flags.load(Ordering::SeqCst) | O_NONBLOCK, Ordering::SeqCst);
        let fd0 = hr.clone();
        let started = quiet(|| -> Result<(Pid, Vec<Fd>), String> {
            let pid = exec::spawn(path, &["held"], &[]).map_err(|e| format!("{what}: starting program {k} returned {}", errno_text(e)))?;
            let Some(tid) = task_of(pid) else {
                return Err(format!("{what}: pid {} has no task", pid.0));
            };
            let ends = [
                Fd { file: fd0, cloexec: false },
                Fd { file: w.clone(), cloexec: false },
                Fd { file: w.clone(), cloexec: false },
            ];
            // What the table gives back is dropped after its lock (S4's rule).
            let back: Vec<Fd> = task::with_table(move |tt| {
                let mut back = Vec::new();
                match tt.get_mut(tid) {
                    Some(t) => {
                        for (n, d) in [0usize, 1, 2].into_iter().zip(ends) {
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
        match started {
            Ok((pid, back)) => {
                drop(back);
                progs.push((Fd { file: r, cloexec: false }, pid, Vec::new(), false));
            }
            Err(e) => {
                why = Some(e);
                break;
            }
        }
    }
    drop(hr);
    let mut buf = alloc::vec![0u8; 4096];
    let mut drain = |progs: &mut Vec<(Fd, Pid, Vec<u8>, bool)>| {
        for (r, _, out, ended) in progs.iter_mut() {
            while !*ended {
                match r.file.read(&mut buf) {
                    Ok(0) => *ended = true,
                    Ok(k) => out.extend_from_slice(&buf[..k]),
                    Err(11) => break,
                    Err(_) => *ended = true,
                }
            }
        }
    };
    let mut c = Clock::new();
    if why.is_none() {
        loop {
            drain(&mut progs);
            if progs.iter().all(|(_, _, out, ended)| out.len() >= len || *ended) || c.ms() >= RUN_MS {
                break;
            }
            sched::yield_now();
        }
    }
    let held: Vec<Vec<u8>> = progs.iter().map(|(_, _, out, _)| out.clone()).collect();
    if why.is_none() {
        // Every program is still holding what it opened (blocked on fd 0).
        let pids: Vec<Pid> = progs.iter().map(|(_, pid, _, _)| *pid).collect();
        look(&pids);
    }
    drop(hw);
    c = Clock::new();
    loop {
        drain(&mut progs);
        if progs.iter().all(|(_, _, _, ended)| *ended) || c.ms() >= RUN_MS {
            break;
        }
        sched::yield_now();
    }
    let pids: Vec<Pid> = progs.iter().map(|(_, pid, _, _)| *pid).collect();
    let _ = yield_until(500, || pids.iter().all(|&p| exit_of(p).is_some()));
    if let Some(why) = why {
        return Err(why);
    }
    for (k, (&pid, out)) in pids.iter().zip(&held).enumerate() {
        let exit = exit_of(pid);
        if out.len() < len || exit != Some(0) {
            return Err(format!(
                "{what}: program {k} of {n} wrote {} of its {len} bytes and ended {}",
                out.len().min(len),
                status_text(exit)
            ));
        }
    }
    Ok(held)
}

/// A program started with fd 0 the read end of a pipe whose write end the
/// test holds, and fds 1 and 2 a pipe of its own (fix round 7): it runs
/// until it reads fd 0, and that waits until `release`.
struct Keeper {
    pid: Pid,
    out: FileRef,
    hold: Option<FileRef>,
    got: Vec<u8>,
    ended: bool,
}

impl Keeper {
    fn start(what: &str, path: &str) -> Result<Keeper, String> {
        let (hr, hw) = pipe::open(0).map_err(|e| format!("{what}: pipe::open(0) returned {}", errno_text(e)))?;
        let (r, w) = pipe::open(0).map_err(|e| format!("{what}: pipe::open(0) returned {}", errno_text(e)))?;
        r.flags.store(r.flags.load(Ordering::SeqCst) | O_NONBLOCK, Ordering::SeqCst);
        let started = quiet(|| -> Result<(Pid, Vec<Fd>), String> {
            let pid = exec::spawn(path, &["keep"], &[]).map_err(|e| format!("{what}: starting {path} returned {}", errno_text(e)))?;
            let Some(tid) = task_of(pid) else {
                return Err(format!("{what}: pid {} has no task", pid.0));
            };
            let ends = [
                Fd { file: hr.clone(), cloexec: false },
                Fd { file: w.clone(), cloexec: false },
                Fd { file: w.clone(), cloexec: false },
            ];
            // What the table gives back is dropped after its lock (S4's rule).
            let back: Vec<Fd> = task::with_table(move |tt| {
                let mut back = Vec::new();
                match tt.get_mut(tid) {
                    Some(t) => {
                        for (n, d) in [0usize, 1, 2].into_iter().zip(ends) {
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
        drop(hr);
        let (pid, back) = started?;
        drop(back);
        Ok(Keeper { pid, out: r, hold: Some(hw), got: Vec::new(), ended: false })
    }
    fn drain(&mut self) {
        let mut buf = alloc::vec![0u8; 1024];
        while !self.ended {
            match self.out.read(&mut buf) {
                Ok(0) => self.ended = true,
                Ok(k) => self.got.extend_from_slice(&buf[..k]),
                Err(11) => break,
                Err(_) => self.ended = true,
            }
        }
    }
    /// Wait until it has written `len` bytes in all, or ended.
    fn wait(&mut self, len: usize) -> bool {
        let mut c = Clock::new();
        loop {
            self.drain();
            if self.got.len() >= len || self.ended || c.ms() >= RUN_MS {
                return self.got.len() >= len;
            }
            sched::yield_now();
        }
    }
    /// The slot of the file at its `fd` (`FileObj::as_file`).
    fn slot(&self, fd: u64) -> Option<u32> {
        let tid = task_of(self.pid)?;
        // The reference the table gives is dropped after its lock (S4's rule).
        let f: Option<FileRef> = task::with_table(|tt| tt.get(tid).and_then(|t| t.fds.get(fd).ok()));
        let s = f.as_ref().and_then(|f| f.object.as_file());
        drop(f);
        s
    }
    /// Close fd 0's write end, so it carries on, and wait for it to end.
    fn release(&mut self) -> Option<i32> {
        drop(self.hold.take());
        let mut c = Clock::new();
        loop {
            self.drain();
            if self.ended || c.ms() >= RUN_MS {
                break;
            }
            sched::yield_now();
        }
        let pid = self.pid;
        let _ = yield_until(500, || exit_of(pid).is_some());
        exit_of(pid)
    }
}

/// The bytes of the file a program holds open through row 2's programs (fix
/// round 7): three lines of 35 bytes drawn from the nonce.
fn keep_bytes(d: u64) -> Vec<u8> {
    (0..3).map(|k| format!("held by a program {:016x}\n", d.rotate_right(16 * k + 4))).collect::<String>().into_bytes()
}

/// References to `/ktest/once`'s inode besides the test's own.
fn once_refs() -> usize {
    quiet(|| KTEST.lock().iter().find(|(n, _)| n == "once").map(|(_, f)| Arc::strong_count(f))).unwrap_or(1).saturating_sub(1)
}

/// The bytes of the file the test keeps open from ring 0 through row 2's
/// programs (fix round 6): three lines of 27 bytes drawn from the nonce,
/// one read back at each of three points.
fn kept_lines(d: u64) -> [String; 3] {
    core::array::from_fn(|k| format!("kept file {:016x}\n", d.rotate_left(16 * k as u32 + 8)))
}

/// Point `k` of the kept file (fix round 6): it reads back its line `k`, its
/// own bytes, so no file opened since took its slot or its inode.
fn kept_check(r: &mut Row, kept: &Result<FileRef, Errno>, lines: &[String; 3], k: usize, when: &str) {
    if let Ok(f) = kept {
        let want = lines[k].as_bytes();
        let mut buf = [0u8; 27];
        let got = f.read(&mut buf[..want.len()]);
        note(format_args!(
            "{WALK_ID}: the file the test keeps open from ring 0 (slot {}), {when}: its read of line {} returned {}",
            slot_text(f.object.as_file()),
            k + 1,
            match &got {
                Ok(n) if *n == want.len() && buf[..*n] == *want => String::from("its bytes"),
                Ok(n) => format!("{n} bytes that are not its line"),
                Err(e) => answer(-e),
            }
        ));
        match got {
            Ok(n) if n == want.len() && buf[..n] == *want => {}
            Ok(n) => r.fail(format_args!(
                "{WALK_ID}: a file the test keeps open from ring 0, on an inode of its own, read {when}: line {} came back as {n} bytes, {:?}; expected its own {} bytes (the opens and closes of other files leave a file that is still open its slot and its inode)",
                k + 1,
                String::from_utf8_lossy(&buf[..n]),
                want.len()
            )),
            Err(e) => r.fail(format_args!(
                "{WALK_ID}: a file the test keeps open from ring 0, on an inode of its own, read {when}: line {} returned {}; expected its own {} bytes (the opens and closes of other files leave a file that is still open its slot and its inode)",
                k + 1,
                answer(-e),
                want.len()
            )),
        }
    }
}

/// The reads the ring-0 fill gives the file opened `i`-th (counting from 0)
/// before it closes: `i % 20` bytes asked of its 18, none when that is 0,
/// and from 18 on a second read at the end, which is 0. `None` when each
/// read returned what the file holds, else what went wrong.
fn read_some(i: usize, f: &FileRef, body: &[u8]) -> Option<String> {
    let k = i % 20;
    if k == 0 {
        return None;
    }
    let mut buf = [0u8; 19];
    let want = k.min(18);
    match f.read(&mut buf[..k]) {
        Ok(n) if n == want && buf[..n] == body[..n] => {}
        Ok(n) => return Some(format!("the file opened {}, read({k}) at offset 0 returned {n} bytes{}, expected the file's own first {want}", i + 1, if n == want { " of another file" } else { "" })),
        Err(e) => return Some(format!("the file opened {}, read({k}) at offset 0 returned {}, expected {want}", i + 1, answer(-e))),
    }
    if k >= 18 {
        match f.read(&mut buf[..1]) {
            Ok(0) => {}
            Ok(n) => return Some(format!("the file opened {}, a read at its end returned {n}, expected 0", i + 1)),
            Err(e) => return Some(format!("the file opened {}, a read at its end returned {}, expected 0", i + 1, answer(-e))),
        }
    }
    None
}

fn slot_text(s: Option<u32>) -> String {
    match s {
        Some(s) => format!("{s}"),
        None => String::from("none (FileObj::as_file answered None)"),
    }
}

fn res(out: &[u8], k: usize) -> i64 {
    le64(out, 8 * k) as i64
}

// ── Row 2: ext2.walk.open_read ───────────────────────────────────────────

fn walk_row(mounted: bool, raw: Option<&Raw>) {
    let mut r = Row::new(WALK_ID);
    let d = r.derived(WALK_ID);
    let (n, m, nbig) = walk_pick(d);
    let (sn, sm) = (t_size(n), t_size(m));
    note(format_args!(
        "{WALK_ID}: derive(nonce, \"{WALK_ID}\") = {d:016x}: /t/{n}.txt ({sn} bytes), and t/{m}.txt ({sm} bytes, one of the {nbig} files past the 12 direct blocks; its file blocks 11 and 12 hold each other's data on the disk the kernel reads)"
    ));
    let Some(raw) = raw else {
        r.fail(format_args!("{WALK_ID}: the test could not read the disk itself"));
        r.finish(2);
        return;
    };
    let want_m = match raw.file(&format!("/t/{m}.txt")) {
        Ok(b) => b,
        Err(why) => {
            r.fail(format_args!("{WALK_ID}: {why}"));
            r.finish(2);
            return;
        }
    };
    if !mounted {
        r.fail(format_args!("{WALK_ID}: not run, / is not mounted"));
        r.finish(2);
        return;
    }
    let once = serve("once", Vec::from(&b"opened and closed\n"[..]));
    let first = sn.min(1000);
    let mut p = Probe::new();
    let path_n = p.str(&format!("/t/{n}.txt"));
    let rel_m = p.str(&format!("t/{m}.txt"));
    let deep = p.str("/a/b/c/deep.txt");
    let boot_hi = p.str("/boot/hi");
    let nope = p.str("/t/nope.txt");
    let through = p.str("/pattern/x");
    let t = p.str("/t");
    let pat = p.str("/pattern");
    let (ba, oa) = p.buf(1000);
    let (bb, ob) = p.buf((sn - first + 100) as usize);
    let (bc, _) = p.buf(16);
    let (bd, od) = p.buf((sm + 100) as usize);
    let (be, oe) = p.buf(64);
    let (bf, of) = p.buf(8);
    let s0 = p.call(SYS_OPEN, &[path_n, A::I(0)]);
    let s1 = p.call(SYS_READ, &[A::R(s0), ba, A::I(1000)]);
    let s2 = p.call(SYS_READ, &[A::R(s0), bb, A::I(sn - first + 100)]);
    let s3 = p.call(SYS_READ, &[A::R(s0), bc, A::I(16)]);
    let s4 = p.call(SYS_CLOSE, &[A::R(s0)]);
    let s5 = p.call(SYS_READ, &[A::R(s0), bc, A::I(16)]);
    let s6 = p.call(SYS_OPENAT, &[A::I(AT_FDCWD), rel_m, A::I(O_CLOEXEC)]);
    let s7 = p.call(SYS_READ, &[A::R(s6), bd, A::I(sm + 100)]);
    let s8 = p.call(SYS_OPEN, &[deep, A::I(0)]);
    let s9 = p.call(SYS_READ, &[A::R(s8), be, A::I(64)]);
    let s10 = p.call(SYS_OPEN, &[boot_hi, A::I(0)]);
    let s11 = p.call(SYS_READ, &[A::R(s10), bf, A::I(4)]);
    let s12 = p.call(SYS_OPEN, &[nope, A::I(0)]);
    let s13 = p.call(SYS_OPEN, &[through, A::I(0)]);
    let s14 = p.call(SYS_OPEN, &[t, A::I(0)]);
    let s15 = p.call(SYS_READ, &[A::R(s14), bc, A::I(16)]);
    let s16 = p.call(SYS_OPEN, &[pat, A::I(O_DIRECTORY)]);
    let s17 = p.call(SYS_OPEN, &[pat, A::I(O_LARGEFILE)]);
    let s18 = p.call(SYS_OPEN, &[pat, A::I(O_UNKNOWN)]);
    let once_path = p.str(&once);
    let cyc = p.cycle(once_path, CYCLES);
    // A file the test keeps open from ring 0 through the program below and
    // the held programs after it (fix round 6), on an inode of its own.
    let lines = kept_lines(d);
    let kept_ino: InodeRef = Arc::new(MemFile { bytes: lines.concat().into_bytes(), ino: 0x3fff });
    let kept = FileRef::open_inode(kept_ino.clone(), 0);
    let kept_slot = kept.as_ref().ok().and_then(|f| f.object.as_file());
    if let Err(e) = &kept {
        r.fail(format_args!("{WALK_ID}: FileRef::open_inode of an inode of the test's, with nothing else open, returned {}; expected an open file", answer(-*e)));
    }
    // A program that opens a file of its own before the program below runs,
    // holds it open through that program and the held programs after it, and
    // then reads it back (fix round 7): a file opened from ring 3 keeps its
    // slot and its inode while other files open and close, too.
    let kbytes = keep_bytes(d);
    let kfile = serve(&format!("{:012x}", d.rotate_left(20) & 0xffff_ffff_ffff), kbytes.clone());
    let mut kp = Probe::new();
    let kpath = kp.str(&kfile);
    let (kbuf, okb) = kp.buf(kbytes.len() + 32);
    let (kscratch, _) = kp.buf(8);
    let k0 = kp.call(SYS_OPEN, &[kpath, A::I(0)]);
    let klen = kp.data.len();
    kp.call(SYS_WRITE, &[A::I(1), A::I(DATA), A::I(klen as u64)]);
    kp.call(SYS_READ, &[A::I(0), kscratch, A::I(1)]);
    let k3 = kp.call(SYS_READ, &[A::R(k0), kbuf, A::I(kbytes.len() as u64 + 32)]);
    let k4 = kp.call(SYS_CLOSE, &[A::R(k0)]);
    let (kelf, _) = kp.elf();
    let kprog = serve("keeper", kelf);
    let mut keeper = match Keeper::start(WALK_ID, &kprog) {
        Ok(k) => Some(k),
        Err(why) => {
            r.fail(format_args!("{why}"));
            None
        }
    };
    let mut keeper_open = false;
    let mut keeper_slot: Option<u32> = None;
    if let Some(k) = keeper.as_mut() {
        let wrote = k.wait(klen);
        let kfd = if wrote { res(&k.got, k0) } else { 0 };
        if !wrote || kfd < 3 {
            r.fail(format_args!(
                "{WALK_ID}: a program that opens {kfile} and then waits wrote {} of its {klen} bytes; its open returned {}; expected an fd",
                k.got.len().min(klen),
                if wrote { answer(kfd) } else { String::from("nothing it wrote") }
            ));
        } else {
            keeper_open = true;
            keeper_slot = k.slot(kfd as u64);
            note(format_args!("{WALK_ID}: a program holds {kfile} open as fd {kfd}, in slot {}", slot_text(keeper_slot)));
            if keeper_slot.is_none() {
                r.fail(format_args!("{WALK_ID}: a program's fd {kfd}, open on {kfile}, names no file in the kernel's table (FileObj::as_file)"));
            }
        }
    }
    let out = match run_probe(WALK_ID, p) {
        Ok(o) => o,
        Err(why) => {
            r.fail(format_args!("{why}"));
            if let Some(k) = keeper.as_mut() {
                let _ = k.release();
            }
            r.finish(2);
            return;
        }
    };
    let fd = res(&out, s0);
    note(format_args!(
        "{WALK_ID}: open(/t/{n}.txt) = {}; reads {}, {}, {}; close {}; openat(AT_FDCWD, t/{m}.txt) = {}, read {}",
        answer(fd),
        answer(res(&out, s1)),
        answer(res(&out, s2)),
        answer(res(&out, s3)),
        answer(res(&out, s4)),
        answer(res(&out, s6)),
        answer(res(&out, s7))
    ));
    r.expect(fd >= 3, "open(\"/t/<n>.txt\", O_RDONLY) is an fd (3 or more: 0-2 are open)");
    let want_n = t_file(n);
    let got1 = res(&out, s1);
    r.expect_res("read(fd, buf, 1000)", got1, first as i64);
    if got1 == first as i64 {
        r.expect_bytes("the first read's bytes", &out[oa..oa + first as usize], &want_n[..first as usize]);
    }
    let got2 = res(&out, s2);
    r.expect_res("the second read (asked for 100 more than is left)", got2, (sn - first) as i64);
    if got2 == (sn - first) as i64 {
        r.expect_bytes("the second read's bytes (from where the first stopped)", &out[ob..ob + got2 as usize], &want_n[first as usize..]);
    }
    r.expect_res("a read at the end of the file", res(&out, s3), 0);
    r.expect_res("close(fd)", res(&out, s4), 0);
    r.expect_res("a read on the closed fd", res(&out, s5), -EBADF);
    r.expect(res(&out, s6) >= 3, "openat(AT_FDCWD, \"t/<m>.txt\", O_CLOEXEC) is an fd");
    let got7 = res(&out, s7);
    r.expect_res("read of t/<m>.txt whole (asked for 100 more)", got7, sm as i64);
    if got7 == sm as i64 {
        r.expect_bytes(&format!("t/{m}.txt's bytes (its last {} through the indirect block)", sm - 12 * 4096), &out[od..od + sm as usize], &want_m);
    }
    r.expect_res("read of /a/b/c/deep.txt", res(&out, s9), 16);
    r.expect_bytes("/a/b/c/deep.txt's bytes", &out[oe..oe + 16], b"/a/b/c/deep.txt\n");
    r.expect_res("read(fd of /boot/hi, 4) (the archive at /boot)", res(&out, s11), 4);
    r.expect_bytes("/boot/hi's first 4 bytes", &out[of..of + 4], b"\x7fELF");
    r.expect_res("open(\"/t/nope.txt\")", res(&out, s12), -ENOENT);
    r.expect_res("open(\"/pattern/x\") (a path through a file)", res(&out, s13), -ENOTDIR);
    r.expect(res(&out, s14) >= 3, "open(\"/t\", O_RDONLY) is an fd");
    r.expect_res("read of a directory's fd", res(&out, s15), -EISDIR);
    r.expect_res("open(\"/pattern\", O_DIRECTORY)", res(&out, s16), -ENOTDIR);
    r.expect(res(&out, s17) >= 3, "open(\"/pattern\", O_LARGEFILE) is an fd");
    r.expect_res("open(\"/pattern\", 0x1000000) (a flag bit no kernel of this course knows)", res(&out, s18), -EINVAL);
    let (pairs, last_open, last_close) = (le64(&out, cyc), le64(&out, cyc + 8) as i64, le64(&out, cyc + 16) as i64);
    let (last_read, last_len) = (le64(&out, cyc + 24) as i64, le64(&out, cyc + 32) as i64);
    note(format_args!(
        "{WALK_ID}: open(\"{once}\"), read(fd, buf, k) for k = 1, 2, ... 18, 1, 2, ..., close(fd), {CYCLES} times in a row: {pairs} finished; the last open returned {}, the last read (of {last_len}) {}, the last close {}",
        answer(last_open),
        answer(last_read),
        answer(last_close)
    ));
    if pairs != CYCLES as u64 {
        let k = pairs + 1;
        if last_open < 0 {
            r.fail(format_args!("{WALK_ID}: open(\"{once}\") number {k} of {CYCLES}, each after the one before was closed, returned {}, expected an fd", answer(last_open)));
        } else if last_read != last_len {
            r.fail(format_args!("{WALK_ID}: read(fd, buf, {last_len}) of {once} after open number {k} of {CYCLES} returned {}, expected {last_len}", answer(last_read)));
        } else {
            r.fail(format_args!("{WALK_ID}: close(fd) number {k} of {CYCLES} returned {}, expected 0", answer(last_close)));
        }
    }
    kept_check(&mut r, &kept, &lines, 0, &format!("after the program's {CYCLES} opens and closes of {once}"));
    // Files held open at once: exactly NFILE less the test's kept file and
    // the program's held one, twice.
    let held_across = kept.is_ok() as u64 + keeper_open as u64;
    let want_open = NFILE - held_across;
    let mut hp = Probe::new();
    let hpath = hp.str(&once);
    for _ in 0..HOLD_OPENS {
        hp.call(SYS_OPEN, &[hpath, A::I(0)]);
    }
    let (helf, hlen) = hp.elf_held(HOLD_OPENS);
    let held_path = serve("held", helf);
    for round in 1..=2 {
        // The slot of every file the programs hold, read from their fds
        // while they hold them (`FileObj::as_file`, the page's API).
        let mut slots: Vec<u32> = Vec::new();
        let mut look = |pids: &[Pid]| {
            for &pid in pids {
                let Some(tid) = task_of(pid) else { continue };
                // The references the table gives are dropped after its lock (S4's rule).
                let files: Vec<FileRef> = task::with_table(|tt| match tt.get(tid) {
                    Some(t) => (3..32u64).filter_map(|fd| t.fds.get(fd).ok()).collect(),
                    None => Vec::new(),
                });
                for f in &files {
                    if let Some(s) = f.object.as_file() {
                        slots.push(s);
                    }
                }
                drop(files);
            }
        };
        let outs = match run_held(WALK_ID, &held_path, HOLDERS, hlen, &mut look) {
            Ok(o) => o,
            Err(why) => {
                r.fail(format_args!("{why}"));
                break;
            }
        };
        let mut opened = 0u64;
        let mut refused = 0u64;
        let mut other: Option<(usize, usize, i64)> = None;
        for (j, out) in outs.iter().enumerate() {
            for k in 0..HOLD_OPENS {
                match res(out, k) {
                    v if v >= 3 => opened += 1,
                    v if v == -ENFILE => refused += 1,
                    v => {
                        other.get_or_insert((j, k, v));
                    }
                }
            }
        }
        let when = if round == 1 { "at once" } else { "at once, after the first ten had closed their files and ended" };
        note(format_args!(
            "{WALK_ID}: {HOLDERS} programs {when}, {HOLD_OPENS} opens of {once} each: {opened} files open together, {refused} opens -23 (ENFILE)"
        ));
        if let Some((j, k, v)) = other {
            r.fail(format_args!("{WALK_ID}: open(\"{once}\") number {} in program {} of {HOLDERS} {when} returned {}, expected an fd or -23 (ENFILE)", k + 1, j + 1, answer(v)));
        }
        let held_n = slots.len();
        // The test's kept file and the program's held one are in the table
        // too: their slots count.
        slots.extend(kept_slot);
        slots.extend(keeper_slot);
        slots.sort_unstable();
        let all = slots.len();
        let past = slots.iter().filter(|&&s| s as u64 >= NFILE).count();
        slots.dedup();
        let shared = all - slots.len();
        let top = slots.last().copied().unwrap_or(0);
        note(format_args!(
            "{WALK_ID}: the files they held and the {held_across} held across them sat in {} distinct slots, {} to {top}",
            slots.len(),
            slots.first().copied().unwrap_or(0)
        ));
        if held_n as u64 != opened {
            r.fail(format_args!("{WALK_ID}: {HOLDERS} programs {when} had {opened} opens of {once} succeed, but their fds name {held_n} files in the kernel's table (FileObj::as_file)"));
        } else if past != 0 || shared != 0 {
            r.fail(format_args!(
                "{WALK_ID}: {HOLDERS} programs {when}: the {held_n} files they held and the {held_across} held open across them sat in slots up to {top}, {past} of them at {NFILE} or past it and {shared} sharing a slot; expected each of these open files in a slot of its own below {NFILE}"
            ));
        }
        if opened != want_open {
            if round == 1 {
                r.fail(format_args!("{WALK_ID}: {HOLDERS} programs holding files open at once, with {held_across} other files open, opened {opened} in all; expected {want_open} (NFILE, {NFILE}, less the {held_across} already open), every open after that -23 (ENFILE)"));
            } else {
                r.fail(format_args!("{WALK_ID}: after the first {HOLDERS} programs closed their files and ended, {HOLDERS} more opened {opened} in all; expected {want_open} again (a closed file gives its slot back)"));
            }
        }
        let after = if round == 1 { "after the first ten programs ended" } else { "after ten more ended" };
        kept_check(&mut r, &kept, &lines, round, after);
    }
    if let Some(mut k) = keeper.take() {
        let exit = k.release();
        if keeper_open {
            let got = &k.got;
            if got.len() < 2 * klen || exit != Some(0) {
                r.fail(format_args!(
                    "{WALK_ID}: the program holding {kfile}, let go after the held programs, wrote {} of its {} bytes and ended {}",
                    got.len().min(2 * klen),
                    2 * klen,
                    status_text(exit)
                ));
            } else {
                let again = &got[klen..2 * klen];
                let n = res(again, k3);
                let want = kbytes.len() as i64;
                let same = n == want && again[okb..okb + kbytes.len()] == kbytes[..];
                note(format_args!(
                    "{WALK_ID}: the program holding {kfile}, after the {CYCLES} opens and closes and the held programs: read(fd, buf, {}) returned {}{}, close {}",
                    want + 32,
                    answer(n),
                    if same { ", its bytes" } else { "" },
                    answer(res(again, k4))
                ));
                if !same {
                    let shown = if n > 0 { format!(", {:?}", String::from_utf8_lossy(&again[okb..okb + (n as usize).min(kbytes.len() + 32)])) } else { String::new() };
                    r.fail(format_args!(
                        "{WALK_ID}: a program opened {kfile}, held it open while the program above opened and closed {once} {CYCLES} times and the held programs came and went, then read it: read(fd, buf, {}) returned {}{shown}; expected its own {want} bytes (the opens and closes of other files leave a file that is still open its slot and its inode)",
                        want + 32,
                        answer(n)
                    ));
                }
            }
        }
    }
    drop(kept);
    // The same table from ring 0 (fix rounds 4 to 6): files of the test's
    // own, each a new inode of 18 bytes of its own, opened with
    // `FileRef::open_inode` until -ENFILE. Then the file in a nonce-chosen
    // place is closed and one more opened: with the table full, the slot that
    // close freed is the only one free, so the new file must sit in it,
    // whatever order a kernel takes free slots in. Before it is closed, file
    // i reads `i % 20` bytes and must get its own: none, part of the file, or
    // (from 18 on) all of it and then a read at its end.
    let pick = 1 + (d >> 24) as usize % (NFILE as usize - 1);
    let bodies: Vec<Vec<u8>> = (0..=NFILE).map(|i| format!("open file #{i:03} ok\n").into_bytes()).collect();
    let own: Vec<InodeRef> = (0..=NFILE)
        .map(|i| -> InodeRef { Arc::new(MemFile { bytes: bodies[i as usize].clone(), ino: 0x4000 + i }) })
        .collect();
    let mut files: Vec<(usize, FileRef)> = Vec::new();
    let mut bad_reads = 0usize;
    let mut first_bad: Option<String> = None;
    let mut last: Result<(), Errno> = Ok(());
    while files.len() < own.len() {
        let i = files.len();
        match FileRef::open_inode(own[i].clone(), 0) {
            Ok(f) => files.push((i, f)),
            Err(e) => {
                last = Err(e);
                break;
            }
        }
    }
    let refusal = match last {
        Ok(()) => String::from("no refusal"),
        Err(e) => answer(-e),
    };
    note(format_args!("{WALK_ID}: FileRef::open_inode from the test, each time on a new inode of 18 bytes: {} files, then {refusal}", files.len()));
    if files.len() as u64 != NFILE || last != Err(ENFILE) {
        r.fail(format_args!(
            "{WALK_ID}: FileRef::open_inode, again and again with nothing else open, gave {} files and then {refusal}; expected {NFILE} and then -23 (ENFILE)",
            files.len()
        ));
    } else {
        let freed = files[pick].1.object.as_file();
        // The freed slot is identified by the object `open_inode` gave
        // (TPM interim 20): the new file must be that same object, not
        // only answer the same `as_file` label.
        let freed_obj = files[pick].1.object;
        let (gi, gone) = files.remove(pick);
        if let Some(why) = read_some(gi, &gone, &bodies[gi]) {
            bad_reads += 1;
            first_bad.get_or_insert(why);
        }
        drop(gone);
        match FileRef::open_inode(own[NFILE as usize].clone(), 0) {
            Ok(f) => {
                let again = f.object.as_file();
                note(format_args!("{WALK_ID}: closed the file in slot {}, the next open took slot {}", slot_text(freed), slot_text(again)));
                if again.is_none() || again != freed || f.object != freed_obj {
                    r.fail(format_args!(
                        "{WALK_ID}: with {NFILE} files open, closing the one in slot {} and opening again gave slot {}{}; expected slot {}, the only one free (open_inode takes a free slot)",
                        slot_text(freed),
                        slot_text(again),
                        if f.object != freed_obj { format!(" as another FileObj ({:?}, not {:?})", f.object, freed_obj) } else { String::new() },
                        slot_text(freed)
                    ));
                }
                files.push((NFILE as usize, f));
            }
            Err(e) => r.fail(format_args!(
                "{WALK_ID}: with {NFILE} files open, closing one and opening again returned {}; expected the slot that close freed",
                answer(-e)
            )),
        }
    }
    for (i, f) in files {
        if let Some(why) = read_some(i, &f, &bodies[i]) {
            bad_reads += 1;
            first_bad.get_or_insert(why);
        }
        drop(f);
    }
    if let Some(why) = first_bad {
        r.fail(format_args!("{WALK_ID}: {bad_reads} of the test's files read wrong before they were closed, the first: {why}"));
    }
    // Every file is closed and every program ended.
    let once_refs = once_refs();
    let own_refs: usize = own.iter().chain([&kept_ino]).map(|i| Arc::strong_count(i) - 1).sum();
    let refs = once_refs + own_refs;
    note(format_args!(
        "{WALK_ID}: after every file was closed and every program ended, references besides the test's own: {once_refs} to {once}, {own_refs} to the test's {} inodes",
        own.len() + 1
    ));
    if refs > REF_SLACK {
        r.fail(format_args!(
            "{WALK_ID}: after every file opened on {once} and on the test's {} inodes was closed, however far it was read, and every program ended, {refs} references to those inodes remain ({once_refs} to {once}); expected a closed file to drop its inode, so at most the few a cache keeps",
            own.len() + 1
        ));
    }
    let _ = (s10, s8);
    r.finish(2);
}

// ── Row 3: ext2.lseek ────────────────────────────────────────────────────

fn lseek_row(mounted: bool) {
    let mut r = Row::new(LSEEK_ID);
    let d = r.derived(LSEEK_ID);
    let a = 4096 + d % 40_000;
    let c = 1 + (d >> 20) % 1000;
    let e = 1 + (d >> 40) % 4000;
    // A whence Linux refuses as well (it serves 0-4; close-out, the TL's pick (b)).
    let wb = 5 + (d >> 56) % 251;
    note(format_args!("{LSEEK_ID}: derive(nonce, \"{LSEEK_ID}\") = {d:016x}: SEEK_SET to {a}, SEEK_CUR by {c}, SEEK_END back {e}, a bad whence {wb}, on /pattern ({PATTERN_SIZE} bytes)"));
    if !mounted {
        r.fail(format_args!("{LSEEK_ID}: not run, / is not mounted"));
        r.finish(3);
        return;
    }
    let mut p = Probe::new();
    let pat = p.str("/pattern");
    let (ba, oa) = p.buf(16);
    let (bb, ob) = p.buf(16);
    let (bc, oc) = p.buf(64);
    let (bd, od) = p.buf(8);
    let (be, oe) = p.buf(64);
    let s0 = p.call(SYS_OPEN, &[pat, A::I(0)]);
    let s1 = p.call(SYS_OPEN, &[pat, A::I(0)]);
    let s2 = p.call(SYS_DUP, &[A::R(s0)]);
    let s3 = p.call(SYS_LSEEK, &[A::R(s0), A::I(a), A::I(SEEK_SET)]);
    let s4 = p.call(SYS_READ, &[A::R(s2), ba, A::I(16)]);
    let s5 = p.call(SYS_LSEEK, &[A::R(s0), A::I(0), A::I(SEEK_CUR)]);
    let s6 = p.call(SYS_LSEEK, &[A::R(s1), A::I(0), A::I(SEEK_CUR)]);
    let s7 = p.call(SYS_READ, &[A::R(s1), bb, A::I(16)]);
    let s8 = p.call(SYS_LSEEK, &[A::R(s2), A::I(e.wrapping_neg()), A::I(SEEK_END)]);
    let s9 = p.call(SYS_READ, &[A::R(s0), bc, A::I(64)]);
    let s10 = p.call(SYS_READ, &[A::R(s0), be, A::I(64)]);
    let s11 = p.call(SYS_LSEEK, &[A::R(s1), A::I(c), A::I(SEEK_CUR)]);
    let s12 = p.call(SYS_READ, &[A::R(s1), bd, A::I(8)]);
    let s13 = p.call(SYS_LSEEK, &[A::R(s1), A::I(60_000), A::I(SEEK_SET)]);
    let s14 = p.call(SYS_READ, &[A::R(s1), be, A::I(8)]);
    let s15 = p.call(SYS_LSEEK, &[A::R(s0), A::I(u64::MAX), A::I(SEEK_SET)]);
    let s16 = p.call(SYS_LSEEK, &[A::R(s0), A::I(0), A::I(wb)]);
    let s17 = p.call(SYS_LSEEK, &[A::I(1), A::I(0), A::I(SEEK_CUR)]);
    let s18 = p.call(SYS_LSEEK, &[A::I(31), A::I(0), A::I(SEEK_SET)]);
    let s19 = p.call(SYS_LSEEK, &[A::R(s2), A::I(0), A::I(SEEK_CUR)]);
    let out = match run_probe(LSEEK_ID, p) {
        Ok(o) => o,
        Err(why) => {
            r.fail(format_args!("{why}"));
            r.finish(3);
            return;
        }
    };
    note(format_args!(
        "{LSEEK_ID}: fds {}, {} and dup {}; SEEK_SET {}; read through the dup {}; CUR on the first {}, on the second open {}; SEEK_END {}",
        answer(res(&out, s0)),
        answer(res(&out, s1)),
        answer(res(&out, s2)),
        answer(res(&out, s3)),
        answer(res(&out, s4)),
        answer(res(&out, s5)),
        answer(res(&out, s6)),
        answer(res(&out, s8))
    ));
    r.expect(res(&out, s0) >= 3 && res(&out, s1) >= 3 && res(&out, s2) >= 3, "two opens of /pattern and a dup of the first are fds");
    r.expect_res("lseek(fd1, a, SEEK_SET)", res(&out, s3), a as i64);
    r.expect_res("read(dup of fd1, 16) after fd1 moved", res(&out, s4), 16);
    r.expect_bytes("the bytes the dup read (from fd1's offset: one open file)", &out[oa..oa + 16], &pattern(a, 16));
    r.expect_res("lseek(fd1, 0, SEEK_CUR) after the dup read 16", res(&out, s5), a as i64 + 16);
    r.expect_res("lseek(fd2, 0, SEEK_CUR): a second open has its own offset", res(&out, s6), 0);
    r.expect_res("read(fd2, 16)", res(&out, s7), 16);
    r.expect_bytes("the bytes fd2 read (from 0)", &out[ob..ob + 16], &pattern(0, 16));
    let end_at = PATTERN_SIZE - e;
    r.expect_res("lseek(dup, -e, SEEK_END)", res(&out, s8), end_at as i64);
    let tail = e.min(64);
    r.expect_res("read(fd1, 64) after SEEK_END through the dup", res(&out, s9), tail as i64);
    r.expect_bytes("the bytes before the end", &out[oc..oc + tail as usize], &pattern(end_at, tail));
    // What is left after the first read: e - tail bytes, of which the next read takes up to 64.
    let next = (e - tail).min(64);
    r.expect_res("the next read(fd1, 64)", res(&out, s10), next as i64);
    r.expect_bytes("the bytes the next read took", &out[oe..oe + next as usize], &pattern(end_at + tail, next));
    r.expect_res("lseek(fd2, c, SEEK_CUR)", res(&out, s11), 16 + c as i64);
    r.expect_res("read(fd2, 8)", res(&out, s12), 8);
    r.expect_bytes("the bytes after SEEK_CUR", &out[od..od + 8], &pattern(16 + c, 8));
    r.expect_res("lseek(fd2, 60000, SEEK_SET) (past the end)", res(&out, s13), 60_000);
    r.expect_res("a read past the end", res(&out, s14), 0);
    r.expect_res("lseek(fd1, -1, SEEK_SET)", res(&out, s15), -EINVAL);
    r.expect_res(&format!("lseek(fd1, 0, {wb}) (no such whence)"), res(&out, s16), -EINVAL);
    r.expect_res("lseek(1, 0, SEEK_CUR) (fd 1 is a pipe)", res(&out, s17), -ESPIPE);
    r.expect_res("lseek(31, …) (nothing open)", res(&out, s18), -EBADF);
    let after = end_at + tail + next;
    r.expect_res("lseek(dup, 0, SEEK_CUR) at the end (the refused seeks moved nothing)", res(&out, s19), after as i64);
    shared_check(&mut r);
    r.finish(3);
}

/// Row 3's second program (fix round 9; the close-out added the second
/// chain): an open file stays whole, its offset and its slot, through a dup
/// chain, a forked child's closes and the parent's own. The program opens
/// `/pattern` twice, reads `k` bytes from each, and on each dups the fd and
/// dups the dup; it forks; the child reads `j` bytes through one of the
/// first chain's copies (which one is drawn from a word this file never
/// prints), closes all six and exits; the parent waits for it, closes the
/// first chain's fd and then its dup, and the second chain's dup and then
/// its fd, so both orders run every time; it reads 16 bytes through each
/// chain's last fd with no seek (`/pattern`'s at `k + j` and at `k`), opens
/// and closes `/bin/hi` `CYCLES` times, more than the table's 256 slots,
/// reads 16 more through each, and closes the second chain's last fd. Then it writes what it has and waits on
/// fd 0 with that one file open: from ring 0 the test opens files until
/// `-ENFILE`, which must be exactly 255, lets the program close its last fd
/// and end, and opens one more, which must take the slot that fd's file had.
fn shared_check(r: &mut Row) {
    let w = ktap::derive(&ktap::nonce().unwrap_or([b'0'; 32]), "ext2.lseek shared");
    let k = 1 + w % 1500;
    let j = 1 + (w >> 16) % 1500;
    let via = ((w >> 32) % 3) as usize;
    note(format_args!("{LSEEK_ID}: two open files of /pattern, each named by an fd, a dup, a dup of that and a forked child's copies of all three: {k} bytes read from each, then {j} by the child from the first"));
    let mut q = Probe::new();
    let pat = q.str("/pattern");
    let other = q.str("/bin/hi");
    let (bk, ok) = q.buf(1500);
    let (bj, oj) = q.buf(1500);
    let (bm, om) = q.buf(16);
    let (bn, on) = q.buf(16);
    let (bx, _) = q.buf(16);
    let (bm2, om2) = q.buf(16);
    let (bn2, on2) = q.buf(16);
    let (sc, _) = q.buf(8);
    let f0 = q.call(SYS_OPEN, &[pat, A::I(0)]);
    let f1 = q.call(SYS_READ, &[A::R(f0), bk, A::I(k)]);
    let d1 = q.call(SYS_DUP, &[A::R(f0)]);
    let d2 = q.call(SYS_DUP, &[A::R(d1)]);
    let g0 = q.call(SYS_OPEN, &[pat, A::I(0)]);
    let g1 = q.call(SYS_READ, &[A::R(g0), bk, A::I(k)]);
    let e1 = q.call(SYS_DUP, &[A::R(g0)]);
    let e2 = q.call(SYS_DUP, &[A::R(e1)]);
    let fk = q.call(SYS_FORK, &[]);
    let fds = [f0, d1, d2, g0, e1, e2];
    let mut cj = 0;
    let mut cc = [0usize; 6];
    let half = q.child(fk, |q| {
        cj = q.call(SYS_READ, &[A::R(fds[via]), bj, A::I(j)]);
        for (i, f) in fds.into_iter().enumerate() {
            cc[i] = q.call(SYS_CLOSE, &[A::R(f)]);
        }
    });
    let wt = q.call(SYS_WAIT4, &[A::I(u64::MAX), A::I(0), A::I(0), A::I(0)]);
    let c1 = q.call(SYS_CLOSE, &[A::R(f0)]);
    let c2 = q.call(SYS_CLOSE, &[A::R(d1)]);
    let c4 = q.call(SYS_CLOSE, &[A::R(e1)]);
    let c5 = q.call(SYS_CLOSE, &[A::R(g0)]);
    let gone = q.call(SYS_READ, &[A::R(f0), bx, A::I(16)]);
    let m1 = q.call(SYS_READ, &[A::R(d2), bm, A::I(16)]);
    let m3 = q.call(SYS_READ, &[A::R(e2), bm2, A::I(16)]);
    let cyc = q.cycle(other, CYCLES);
    let m2 = q.call(SYS_READ, &[A::R(d2), bn, A::I(16)]);
    let m4 = q.call(SYS_READ, &[A::R(e2), bn2, A::I(16)]);
    let c6 = q.call(SYS_CLOSE, &[A::R(e2)]);
    let lp = q.data.len();
    q.call(SYS_WRITE, &[A::I(1), A::I(DATA), A::I(lp as u64)]);
    q.call(SYS_READ, &[A::I(0), sc, A::I(1)]);
    let c3 = q.call(SYS_CLOSE, &[A::R(d2)]);
    let gone2 = q.call(SYS_READ, &[A::R(d2), bx, A::I(16)]);
    let (elf, len) = q.elf();
    assert!(len == lp);
    let path = serve("shared", elf);
    let mut kp = match Keeper::start(LSEEK_ID, &path) {
        Ok(kp) => kp,
        Err(why) => {
            r.fail(format_args!("{why}"));
            return;
        }
    };
    if !kp.wait(half + lp) {
        let n = kp.got.len();
        let exit = kp.release();
        r.fail(format_args!(
            "{LSEEK_ID}: a program that opens /pattern, dups it twice and forks wrote {n} bytes, and its child and it were to write {half} and {lp} before it waits on fd 0; it ended {}",
            status_text(exit)
        ));
        return;
    }
    let ch = kp.got[..half].to_vec();
    let mid = kp.got[half..half + lp].to_vec();
    let fd2 = res(&mid, d2);
    let slot2 = if fd2 >= 0 { kp.slot(fd2 as u64) } else { None };
    // With that one file open, fill the table from ring 0.
    let ino: InodeRef = Arc::new(MemFile { bytes: b"filler\n".to_vec(), ino: 0x5000 });
    let mut held: Vec<FileRef> = Vec::new();
    let mut last: Result<(), Errno> = Ok(());
    while (held.len() as u64) < NFILE {
        match FileRef::open_inode(ino.clone(), 0) {
            Ok(f) => held.push(f),
            Err(e) => {
                last = Err(e);
                break;
            }
        }
    }
    let filled = held.len();
    let exit = kp.release();
    let back = if last == Err(ENFILE) { Some(FileRef::open_inode(ino.clone(), 0)) } else { None };
    let back_slot = back.as_ref().and_then(|b| b.as_ref().ok()).and_then(|f| f.object.as_file());
    let back_text = match &back {
        None => String::from("not tried"),
        Some(Ok(_)) => format!("an open file in slot {}", slot_text(back_slot)),
        Some(Err(e)) => answer(-*e),
    };
    drop(back);
    drop(held);
    let refusal = match last {
        Ok(()) => String::from("no refusal"),
        Err(e) => answer(-e),
    };
    let got = core::mem::take(&mut kp.got);
    let fin: &[u8] = if got.len() == half + 2 * lp { &got[half + lp..] } else { &mid };
    note(format_args!(
        "{LSEEK_ID}: open {}, read {}, dup {}, dup {}; open {}, read {}, dup {}, dup {}; fork {}; the child read {} and closed {}; wait4 {}; the parent closed fd then dup {}, {}, and dup then fd {}, {}; its read on the first fd {}; through each chain's last fd: read {}, {}, {CYCLES} opens and closes of /bin/hi ({} done), read {}, {}, then close {}",
        answer(res(fin, f0)),
        answer(res(fin, f1)),
        answer(res(fin, d1)),
        answer(res(fin, d2)),
        answer(res(fin, g0)),
        answer(res(fin, g1)),
        answer(res(fin, e1)),
        answer(res(fin, e2)),
        answer(res(fin, fk)),
        answer(res(&ch, cj)),
        cc.iter().map(|&c| answer(res(&ch, c))).collect::<Vec<_>>().join(", "),
        answer(res(fin, wt)),
        answer(res(fin, c1)),
        answer(res(fin, c2)),
        answer(res(fin, c4)),
        answer(res(fin, c5)),
        answer(res(fin, gone)),
        answer(res(fin, m1)),
        answer(res(fin, m3)),
        le64(fin, cyc),
        answer(res(fin, m2)),
        answer(res(fin, m4)),
        answer(res(fin, c6))
    ));
    note(format_args!(
        "{LSEEK_ID}: with the program holding that file open as fd {fd2} (slot {}), FileRef::open_inode gave {filled} files, then {refusal}; the program then closed it ({}), read it ({}) and ended {}; the next open: {back_text}",
        slot_text(slot2),
        answer(res(fin, c3)),
        answer(res(fin, gone2)),
        status_text(exit)
    ));
    r.expect(
        [f0, d1, d2, g0, e1, e2].iter().all(|&s| res(fin, s) >= 3),
        "two opens of \"/pattern\", a dup of each and a dup of each dup are fds",
    );
    r.expect_res("read(fd, k) of /pattern", res(fin, f1), k as i64);
    r.expect_res("read(fd, k) of the second open of /pattern", res(fin, g1), k as i64);
    r.expect_bytes("the first k bytes of /pattern", &fin[ok..ok + (k as usize).min(1500)], &pattern(0, k));
    r.expect(res(fin, fk) > 0 && res(&ch, fk) == 0, "fork returns the child's pid to the parent and 0 to the child");
    r.expect_res("read(copy, j) in the forked child, where the parent's read stopped", res(&ch, cj), j as i64);
    r.expect_bytes("the bytes the child read (the open file's offset is shared with the parent)", &ch[oj..oj + (j as usize).min(1500)], &pattern(k, j));
    r.expect(cc.iter().all(|&c| res(&ch, c) == 0), "the child's close of each of its six copies is 0");
    r.expect_res("wait4(-1, …) in the parent", res(fin, wt), res(fin, fk));
    r.expect(
        [c1, c2, c4, c5].iter().all(|&c| res(fin, c) == 0),
        "the parent's closes of the first two fds of each chain (fd then dup, dup then fd) are 0",
    );
    r.expect_res("a read on the first fd once the parent closed it", res(fin, gone), -EBADF);
    r.expect_res("read(last fd, 16) with no seek, once every other fd in both processes was closed", res(fin, m1), 16);
    r.expect_bytes("those bytes (/pattern's, from where the child's read left the shared offset)", &fin[om..om + 16], &pattern(k + j, 16));
    r.expect_eq("open, read and close of /bin/hi, pairs done", le64(fin, cyc), CYCLES as u64);
    r.expect_res(&format!("read(last fd, 16) after {CYCLES} opens and closes of another file"), res(fin, m2), 16);
    r.expect_bytes("those bytes (still /pattern's, next on from the last read)", &fin[on..on + 16], &pattern(k + j + 16, 16));
    r.expect_res("read(second chain's last fd, 16) with no seek, once its dup and then its fd were closed in both processes", res(fin, m3), 16);
    r.expect_bytes("those bytes (/pattern's, from k, where that open file's own offset was)", &fin[om2..om2 + 16], &pattern(k, 16));
    r.expect_res(&format!("read(second chain's last fd, 16) after {CYCLES} opens and closes of another file"), res(fin, m4), 16);
    r.expect_bytes("those bytes (still /pattern's, next on)", &fin[on2..on2 + 16], &pattern(k + 16, 16));
    r.expect_res("close(second chain's last fd)", res(fin, c6), 0);
    if slot2.is_none() {
        r.fail(format_args!("{LSEEK_ID}: the program's fd {fd2}, open on /pattern, names no file in the kernel's table (FileObj::as_file)"));
    }
    if filled as u64 != NFILE - 1 || last != Err(ENFILE) {
        r.fail(format_args!(
            "{LSEEK_ID}: with one file open in the kernel (the program's last fd on /pattern), FileRef::open_inode gave {filled} files, then {refusal}; expected {} (every slot but that file's), then -23 (ENFILE)",
            NFILE - 1
        ));
    } else if back_slot.is_none() || back_slot != slot2 {
        r.fail(format_args!(
            "{LSEEK_ID}: with the table full, the program closed its last fd on /pattern and ended; the next open gave {back_text}; expected the slot that file had, {}",
            slot_text(slot2)
        ));
    }
    r.expect_res("close(last fd) of /pattern", res(fin, c3), 0);
    r.expect_res("a read on the last fd once it is closed", res(fin, gone2), -EBADF);
    if got.len() != half + 2 * lp || exit != Some(0) {
        r.fail(format_args!("{LSEEK_ID}: the program and its child wrote {} of their {} bytes and the program ended {}", got.len(), half + 2 * lp, status_text(exit)));
    }
}

// ── Row 4: ext2.stat.fields ──────────────────────────────────────────────

fn check_stat(r: &mut Row, what: &str, st: &[u8], ino: u64, raw: &[u8; 128]) {
    let mode = le16(raw, 0);
    let size = le32(raw, 4) | if mode & S_IFMT == 0o100000 { le32(raw, 108) << 32 } else { 0 };
    let fields: [(&str, usize, u64, u64); 15] = [
        ("st_ino", 8, le64(st, 8), ino),
        ("st_nlink", 16, le64(st, 16), le16(raw, 26)),
        ("st_mode", 24, le32(st, 24), mode),
        ("st_uid", 28, le32(st, 28), le16(raw, 2) | le16(raw, 120) << 16),
        ("st_gid", 32, le32(st, 32), le16(raw, 24) | le16(raw, 122) << 16),
        ("st_rdev", 40, le64(st, 40), 0),
        ("st_size", 48, le64(st, 48), size),
        ("st_blksize", 56, le64(st, 56), 4096),
        ("st_blocks", 64, le64(st, 64), le32(raw, 28)),
        ("st_atime", 72, le64(st, 72), le32(raw, 8)),
        ("st_atime_nsec", 80, le64(st, 80), 0),
        ("st_mtime", 88, le64(st, 88), le32(raw, 16)),
        ("st_mtime_nsec", 96, le64(st, 96), 0),
        ("st_ctime", 104, le64(st, 104), le32(raw, 12)),
        ("st_ctime_nsec", 112, le64(st, 112), 0),
    ];
    for (name, at, got, want) in fields {
        if got != want {
            r.fail(format_args!("{STAT_ID}: {what}: {name} (byte {at}) is {got:#x}, the inode on the disk says {want:#x}"));
        }
    }
}

fn stat_row(mounted: bool, raw: Option<&Raw>) {
    let mut r = Row::new(STAT_ID);
    let Some(raw) = raw else {
        r.fail(format_args!("{STAT_ID}: the test could not read the disk itself"));
        r.finish(4);
        return;
    };
    let m = (0..T_FILES).find(|&k| t_size(k) > 12 * 4096).unwrap_or(0);
    let k = 5u64;
    let paths: [String; 8] = [
        format!("/t/{m}.txt"),
        String::from("/big"),
        String::from("/"),
        String::from("/bin/ls"),
        String::from("/a/b/c/deep.txt"),
        String::from("/.sboot-log"),
        String::from("/lost+found"),
        String::from("/t"),
    ];
    if !mounted {
        r.fail(format_args!("{STAT_ID}: not run, / is not mounted"));
        r.finish(4);
        return;
    }
    let mut p = Probe::new();
    let mut calls = Vec::new();
    for path in &paths {
        let s = p.str(path);
        let (b, o) = p.buf(144);
        calls.push((format!("stat({path:?})"), p.call(SYS_STAT, &[s, b]), o, path.clone()));
    }
    let big = p.str("/big");
    let pat = p.str("/pattern");
    let rel = p.str(&format!("t/{k}.txt"));
    let nope = p.str("/t/nope");
    let fd = p.call(SYS_OPEN, &[big, A::I(0)]);
    let (b, o) = p.buf(144);
    calls.push((String::from("fstat(fd of /big)"), p.call(SYS_FSTAT, &[A::R(fd), b]), o, String::from("/big")));
    let (b, o) = p.buf(144);
    calls.push((String::from("newfstatat(AT_FDCWD, \"/pattern\", 0)"), p.call(SYS_NEWFSTATAT, &[A::I(AT_FDCWD), pat, b, A::I(0)]), o, String::from("/pattern")));
    let (b, o) = p.buf(144);
    calls.push((
        format!("newfstatat(AT_FDCWD, \"t/{k}.txt\", AT_SYMLINK_NOFOLLOW)"),
        p.call(SYS_NEWFSTATAT, &[A::I(AT_FDCWD), rel, b, A::I(AT_SYMLINK_NOFOLLOW)]),
        o,
        format!("/t/{k}.txt"),
    ));
    let (bx, _) = p.buf(144);
    let (bp, op) = p.buf(144);
    let e0 = p.call(SYS_STAT, &[nope, bx]);
    let e1 = p.call(SYS_STAT, &[pat, A::I(0x10)]);
    let e2 = p.call(SYS_FSTAT, &[A::I(1), bp]);
    let e3 = p.call(SYS_FSTAT, &[A::I(31), bx]);
    let e4 = p.call(SYS_NEWFSTATAT, &[A::I(AT_FDCWD), pat, bx, A::I(0x2)]);
    let out = match run_probe(STAT_ID, p) {
        Ok(o) => o,
        Err(why) => {
            r.fail(format_args!("{why}"));
            r.finish(4);
            return;
        }
    };
    let mut seen = Vec::new();
    for (what, step, o, path) in &calls {
        let v = res(&out, *step);
        if v != 0 {
            r.fail(format_args!("{STAT_ID}: {what} returned {}, expected 0", answer(v)));
            continue;
        }
        match raw.walk(path).and_then(|ino| raw.inode(ino).map(|i| (ino, i))) {
            Ok((ino, inode)) => {
                seen.push(format!("{path} ino {ino} size {} blocks {}", le64(&out[*o..], 48), le64(&out[*o..], 64)));
                check_stat(&mut r, what, &out[*o..*o + 144], ino, &inode);
            }
            Err(why) => r.fail(format_args!("{STAT_ID}: {why}")),
        }
    }
    note(format_args!("{STAT_ID}: {}", seen.join("; ")));
    r.expect_res("stat(\"/t/nope\")", res(&out, e0), -ENOENT);
    r.expect_res("stat(\"/pattern\", 0x10) (no memory there)", res(&out, e1), -EFAULT);
    r.expect_res("fstat(1) (a pipe)", res(&out, e2), 0);
    r.expect_eq("fstat(1)'s st_mode file type (S_IFIFO)", le32(&out[op..], 24) & S_IFMT, S_IFIFO);
    r.expect_res("fstat(31) (nothing open)", res(&out, e3), -EBADF);
    r.expect_res("newfstatat(…, flags 0x2) (no such flag)", res(&out, e4), -EINVAL);
    r.finish(4);
}

// ── Row 5: exec.from_disk ────────────────────────────────────────────────

fn has_line(out: &[u8], line: &str) -> bool {
    out.split(|&c| c == b'\n').any(|l| l == line.as_bytes())
}

fn tail(out: &[u8]) -> String {
    let s = String::from_utf8_lossy(&out[out.len().saturating_sub(160)..]).into_owned();
    s.replace('\n', "\\n")
}

fn exec_row(fs: Option<&Arc<Ext2>>, mounted: bool, raw: Option<&Raw>) {
    let mut r = Row::new(EXEC_ID);
    let d = r.derived(EXEC_ID);
    let n1 = format!("{}{}", hex16(d), hex16(ktap::splitmix64(d)));
    let n2 = format!("{}{}", hex16(ktap::splitmix64(d ^ 1)), hex16(ktap::splitmix64(d ^ 2)));
    let n1b: [u8; 32] = n1.as_bytes().try_into().unwrap();
    let d16 = hex16(ktap::derive(&n1b, "exec.argc_argv"));
    note(format_args!("{EXEC_ID}: derive(nonce, \"{EXEC_ID}\") = {d:016x}: args gets argv[1] {n1}, argv[3] {d16}; hi gets {n2}"));
    let (Some(fs), Some(raw), true) = (fs, raw, mounted) else {
        r.fail(format_args!("{EXEC_ID}: not run: / is not mounted, or the test could not read the disk"));
        r.finish(5);
        return;
    };
    // (a) args, exec'd from /bin by a program of this file's.
    let probe = serve("exec-probe", exec_probe());
    let env = format!("SBOOT_NONCE={n1}");
    let argv = ["exec-probe", "/bin/args", "args", n1.as_str(), "1", d16.as_str()];
    match run("args", || exec::spawn(&probe, &argv, &[env.as_str()])) {
        Ok(ran) => {
            note(format_args!("{EXEC_ID}: exec-probe -> /bin/args: {} bytes out, {}", ran.out.len(), status_text(ran.exit)));
            let ids = ["exec.argc_argv", "exec.envp", "exec.rsp.aligned", "exec.auxv.phdr_pagesz", "exec.auxv.random.readable"];
            for (i, id) in ids.iter().enumerate() {
                let line = format!("ok {} - {id}", i + 1);
                if !has_line(&ran.out, &line) {
                    r.fail(format_args!("{EXEC_ID}: /bin/args did not print {line:?}; its output ends {:?}", tail(&ran.out)));
                }
            }
            r.expect(has_line(&ran.out, &format!("# argv[3]={d16}")), "/bin/args echoed argv[3], the nonce-derived argument");
            r.expect(ran.exit == Some(0), "/bin/args exited 0 (exec-probe exits 100 + errno when execve fails)");
            if let Some(s) = ran.exit.filter(|s| s & 0x7f == 0 && (s >> 8) & 0xff > 100) {
                note(format_args!("{EXEC_ID}: execve(\"/bin/args\") returned {}", errno_text(((s >> 8) & 0xff) as i64 - 100)));
            }
        }
        Err(why) => r.fail(format_args!("{why}")),
    }
    // (b) cat, exec'd from /bin by exec::spawn (a .bss of its own): a
    // nonce-chosen /t file, then deep.txt, byte for byte.
    let k = d % T_FILES;
    let tk = format!("/t/{k}.txt");
    let mut want = match raw.file(&tk) {
        Ok(b) => b,
        Err(why) => {
            r.fail(format_args!("{EXEC_ID}: {why}"));
            Vec::new()
        }
    };
    want.extend_from_slice(b"/a/b/c/deep.txt\n");
    let argv = ["cat", tk.as_str(), "/a/b/c/deep.txt"];
    match run("cat", || exec::spawn("/bin/cat", &argv, &[])) {
        Ok(ran) => {
            note(format_args!("{EXEC_ID}: /bin/cat {tk} /a/b/c/deep.txt: {} bytes out, {}", ran.out.len(), status_text(ran.exit)));
            if ran.out != want {
                let at = first_diff(&ran.out, &want).unwrap_or(ran.out.len().min(want.len()));
                r.fail(format_args!(
                    "{EXEC_ID}: cat {tk} /a/b/c/deep.txt printed {} bytes, expected {} (the two files as the disk holds them); first difference at byte {at}",
                    ran.out.len(),
                    want.len()
                ));
            }
            r.expect(ran.exit == Some(0), "/bin/cat exited 0");
        }
        Err(why) => r.fail(format_args!("{why}")),
    }
    // (c) hi, from /bin; then the learner's Ext2Inode of it as a ReadAt
    // (interface 9's reader), byte for byte against this file's own reads.
    let argv = ["hi", n2.as_str(), "1"];
    match run("hi", || exec::spawn("/bin/hi", &argv, &[])) {
        Ok(ran) => {
            note(format_args!("{EXEC_ID}: /bin/hi: {} bytes out, {}", ran.out.len(), status_text(ran.exit)));
            r.expect(has_line(&ran.out, &format!("hi: cs=0x2b rpl=3 nonce={n2}")), "hi printed its line with the nonce-derived argument");
            r.expect(has_line(&ran.out, "ok 1 - ring3.cs.rpl"), "hi printed \"ok 1 - ring3.cs.rpl\"");
            r.expect(ran.exit == Some(0), "/bin/hi exited 0");
        }
        Err(why) => r.fail(format_args!("{why}")),
    }
    match raw.walk("/bin/hi").and_then(|ino| raw.inode(ino).map(|i| (ino, i))) {
        Ok((ino, inode)) => readat_check(&mut r, fs, ino, raw, &inode),
        Err(why) => r.fail(format_args!("{EXEC_ID}: {why}")),
    }
    r.finish(5);
}

/// The learner's `Ext2Inode` for `ino` read through `ReadAt` — the
/// interface the ELF loader takes — at the start, across its first block
/// boundary, at the end and past it, against the bytes this file reads.
fn readat_check(r: &mut Row, fs: &Arc<Ext2>, ino: u64, raw: &Raw, inode: &[u8; 128]) {
    let size = Raw::size(inode);
    let mut want = Vec::new();
    for i in 0..size.div_ceil(4096) {
        match raw.bmap(inode, i).and_then(raw_block) {
            Ok(b) => want.extend_from_slice(&b[..]),
            Err(why) => {
                r.fail(format_args!("{EXEC_ID}: {why}"));
                return;
            }
        }
    }
    want.truncate(size as usize);
    let file = match quiet(|| fs.inode(ino as u32)) {
        Ok(f) => f,
        Err(e) => {
            r.fail(format_args!("{EXEC_ID}: Ext2::inode({ino}) (/bin/hi) returned {}", errno_text(e)));
            return;
        }
    };
    r.expect_eq("Ext2::inode(n).ino() for /bin/hi", file.ino() as u64, ino);
    let mut buf = alloc::vec![0u8; 300];
    for (off, len) in [(0u64, 64usize), (4096 - 100, 300), (size - 10, 100), (size, 100)] {
        let what = format!("ReadAt::read_at({off}, {len} bytes) of /bin/hi ({size} bytes)");
        let expect = &want[(off as usize).min(want.len())..((off as usize) + len).min(want.len())];
        match quiet(|| ReadAt::read_at(&file, off, &mut buf[..len])) {
            Ok(n) if n == expect.len() => r.expect_bytes(&what, &buf[..n], expect),
            Ok(n) => r.fail(format_args!("{EXEC_ID}: {what} returned {n}, expected {}", expect.len())),
            Err(e) => r.fail(format_args!("{EXEC_ID}: {what} returned {}", errno_text(e))),
        }
    }
}

// ── Row 6: ext2.double_indirect ──────────────────────────────────────────

fn first_diff(a: &[u8], b: &[u8]) -> Option<usize> {
    (0..a.len().min(b.len())).find(|&i| a[i] != b[i])
}

fn dind_row(mounted: bool, raw: Option<&Raw>, ch: Option<&Changes>) {
    let mut r = Row::new(DIND_ID);
    let d = r.derived(DIND_ID);
    let Some(ch) = ch else {
        r.fail(format_args!("{DIND_ID}: the test could not read the disk itself"));
        r.finish(6);
        return;
    };
    let (a, b, c, e) = (12 + ch.a, 12 + ch.b, DIND_FIRST + ch.c, DIND_FIRST + ch.e);
    let blocks = ch.blocks();
    note(format_args!(
        "{DIND_ID}: derive(nonce, \"{DIND_ID}\") = {d:016x}: on the disk the kernel reads, /big is {} bytes ({blocks} blocks, {} behind the double indirect block's second entry); its indirect, double indirect and two inner blocks are fs blocks {}, {}, {}, {} (the writer's are {}, {}, {}); file blocks {a} and {b} (indirect), {c} and {e} (double indirect) hold each other's data",
        ch.size,
        blocks - DIND_SECOND,
        ch.at[0],
        ch.at[1],
        ch.at[2],
        ch.at[3],
        ch.was[0],
        ch.was[1],
        ch.was[2]
    ));
    if let Some(raw) = raw {
        let m = raw.walk("/big").and_then(|ino| raw.inode(ino).map(|i| (ino, i)));
        if let Ok((ino, i)) = m {
            note(format_args!(
                "{DIND_ID}: on the disk: /big is inode {ino}, i_size {}, i_blocks {}, indirect block {}, double indirect block {}; file block {DIND_FIRST} is fs block {}",
                le32(&i, 4),
                le32(&i, 28),
                le32(&i, 40 + 48),
                le32(&i, 40 + 52),
                raw.bmap(&i, DIND_FIRST).unwrap_or(0)
            ));
        }
    }
    if !mounted {
        r.fail(format_args!("{DIND_ID}: not run, / is not mounted"));
        r.finish(6);
        return;
    }
    let big: InodeRef = match quiet(|| vfs::root().and_then(|root| vfs::walk(&root, "/big"))) {
        Ok(b) => b,
        Err(e) => {
            r.fail(format_args!("{DIND_ID}: vfs::walk(\"/big\") returned {}", errno_text(e)));
            r.finish(6);
            return;
        }
    };
    let size = big.stat().size;
    r.expect_eq("/big's stat().size", size, ch.size);
    let mut buf = Box::new([0u8; BLOCK_SIZE]);
    let mut bad = 0u64;
    let mut first_bad: Option<(u64, String)> = None;
    let t0 = ticks();
    let mut c = Clock::new();
    let mut read_us = 0u64;
    for i in 0..blocks {
        let before = c.us();
        let got = quiet(|| big.read_at(i * 4096, &mut buf[..]));
        read_us += c.us() - before;
        let full = (ch.size - i * 4096).min(4096) as usize;
        let why = match got {
            Ok(n) if n == full => {
                let want = tagged(BIG_MAGIC, ch.big_src(i));
                first_diff(&buf[..full], &want[..full]).map(|k| {
                    format!("its byte {k} differs: its first two words are {:#x}, {:#x} (block i holds words {BIG_MAGIC:#x} and the block the disk maps there)", le64(&buf[..], 0), le64(&buf[..], 8))
                })
            }
            Ok(n) => Some(format!("read_at returned {n}, expected {full}")),
            Err(e) => Some(format!("read_at returned {}", errno_text(e))),
        };
        if let Some(why) = why {
            bad += 1;
            first_bad.get_or_insert((i, why));
        }
    }
    let us = c.us();
    note(format_args!(
        "{DIND_ID}: read /big whole through read_at, {blocks} calls of 4 KiB: {} us inside read_at, {us} us with the checks, {} us a block; the tick advanced {}",
        read_us,
        read_us / blocks,
        ticks() - t0
    ));
    if let Some((i, why)) = first_bad {
        let kind = if i < 12 {
            String::from("direct")
        } else if i < DIND_FIRST {
            String::from("indirect")
        } else {
            format!("double indirect, its entry {}", (i - DIND_FIRST) / PER)
        };
        r.fail(format_args!("{DIND_ID}: {bad} of {blocks} blocks wrong; the first, file block {i} (mapped {kind}): {why}"));
    }
    // Across each boundary into the double indirect, unaligned.
    for (first, what) in [(DIND_FIRST, "the indirect/double-indirect boundary"), (DIND_SECOND, "the double indirect block's first and second entries")] {
        let at = first * 4096 - 100;
        let mut want = Vec::from(&tagged(BIG_MAGIC, ch.big_src(first - 1))[3996..]);
        want.extend_from_slice(&tagged(BIG_MAGIC, ch.big_src(first))[..200]);
        let mut b300 = [0u8; 300];
        match quiet(|| big.read_at(at, &mut b300)) {
            Ok(300) => r.expect_bytes(&format!("read_at(file block {first}'s start - 100, 300): across {what}"), &b300, &want),
            other => r.fail(format_args!("{DIND_ID}: read_at({at}, 300) returned {other:?}, expected Ok(300)")),
        }
    }
    let mut b100 = [0u8; 100];
    let tail_at = ((ch.size - 10) % 4096) as usize;
    match quiet(|| big.read_at(ch.size - 10, &mut b100)) {
        Ok(10) => r.expect_bytes("the last 10 bytes", &b100[..10], &tagged(BIG_MAGIC, ch.big_src(blocks - 1))[tail_at..tail_at + 10]),
        other => r.fail(format_args!("{DIND_ID}: read_at(size - 10, 100) returned {other:?}, expected Ok(10)")),
    }
    match quiet(|| big.read_at(ch.size, &mut b100)) {
        Ok(0) => {}
        other => r.fail(format_args!("{DIND_ID}: read_at(size, 100) returned {other:?}, expected Ok(0)")),
    }
    r.finish(6);
}

// ── Mounting ─────────────────────────────────────────────────────────────

fn mount_all(fs: Option<&Arc<Ext2>>) -> bool {
    let Some(fs) = fs else {
        note(format_args!("vfs: nothing to mount at /: Ext2::mount failed in row 1"));
        return false;
    };
    let root = match quiet(|| fs.root()) {
        Ok(r) => r,
        Err(e) => {
            note(format_args!("vfs: Ext2::root() returned {}", errno_text(e)));
            return false;
        }
    };
    if let Err(e) = vfs::mount("/", root) {
        note(format_args!("vfs: mount(\"/\", ext2's root) returned {}", errno_text(e)));
        return false;
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
    note(format_args!("vfs: / is ext2 on partition 2, /boot the boot archive ({ARCHIVE_LEN} bytes), /ktest the test's programs"));
    true
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

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    serial::init();
    ktap::begin(PLAN);
    prelude();
    let raw = match Raw::open() {
        Ok(raw) => Some(raw),
        Err(why) => {
            note(format_args!("disk: {why}"));
            None
        }
    };
    let changes = match raw.as_ref().map(arm) {
        Some(Ok(ch)) => Some(ch),
        Some(Err(why)) => {
            note(format_args!("disk: {why}"));
            None
        }
        None => None,
    };
    let fs = superblock_row(raw.as_ref());
    let mounted = mount_all(fs.as_ref());
    walk_row(mounted, raw.as_ref());
    lseek_row(mounted);
    stat_row(mounted, raw.as_ref());
    exec_row(fs.as_ref(), mounted, raw.as_ref());
    dind_row(mounted, raw.as_ref(), changes.as_ref());
    sched::yield_now();
    quiet(ktap::end);
    halt()
}
