//! Lab 16's vehicle: the disk and the buffer cache. The prelude is lab 15's
//! (the GDT, the IDT, the frame pool, the syscall MSRs, the heap, `sse_on`,
//! `proc::init`, task 0, the PIC and the PIT), so the disk is driven with
//! interrupts on and the tick preempting, as every later lab drives it.
//!
//! The disk is `ktest-16.img` as the course's image writer made it before
//! boot (CONTRACT §A.7 "The disk from lab 16", "The writer's tree"): 331,776
//! sectors; partition 2 = GPT entry 1 at LBA 67,584, 32,768 blocks of 4 KiB;
//! fs blocks 1033–1044 (`/pattern`) hold `tagged(0x5B007B00B10C, b)`; fs
//! blocks 32,000–32,699 and 32,765–32,767 are free; `/big`'s file blocks
//! 12–1035 sit at fs blocks 1090–2113. This vehicle knows those numbers; the
//! learner's kernel must find them.
//!
//! Five rows, printed by this file in this order (no `.ktest_array`):
//!
//!   1 `ata.identify` (plain) — `Ata::probe`'s IDENTIFY: the model string
//!     is QEMU's, and the sector count is the writer's disk size, which
//!     `blocks()` turns into 4 KiB blocks.
//!   2 `blk.read.matches.writer` (derived) — `partition2` finds partition 2
//!     through the GPT at the writer's LBA and size; a nonce-chosen
//!     `/pattern` block read through the partition, and the same block read
//!     through the drive at its disk block, each equal `tagged(…)`
//!     recomputed here. Then (fix round 1, 2026-10-19; round 2 the same
//!     day): a `/big` block chosen by `unpredictable` — the TSC, the PM
//!     timer and the tick, never the nonce, never printed — is read once
//!     through the partition and through the drive (both must equal what
//!     this file's OWN port I/O, `raw_*` below, reads there), then rewritten
//!     with that port I/O with bytes from `unpredictable`, and must read
//!     back new through both: a driver that answers a block it has read
//!     before from memory reads it stale. The block is put back with the
//!     same port I/O. And `partition2` over `MemGpt`, a device in memory
//!     whose valid GPT (CRCs and all) puts the entry array at LBA 3–31
//!     (never the writer's LBA 2), uses 256- or 512-byte entries (never the
//!     writer's 128), and places partition 2, all as `unpredictable` picks:
//!     its first LBA, its blocks() and one block read through it must be
//!     that table's.
//!   3 `cache.hit.count` (derived) — the learner's `Cache` over a device
//!     wrapper this vehicle owns, which counts the reads that reach the
//!     partition. A nonce-chosen run of `get`s over a nonce-chosen set of
//!     `/pattern` blocks (fewer than the cache's buffers): every buffer
//!     served holds its block's bytes, the device saw exactly one read per
//!     distinct block, and `hits()` equals the `get`s that reached no
//!     device — this file's own count, not the cache's.
//!   4 `cache.write.sync` (derived; added 2026-10-19, slice 5b, unit s16) —
//!     a fresh `Cache` over a second counting wrapper. Two free fs blocks X
//!     and Y picked by the nonce get nonce-seeded bytes through `get`, the
//!     bytes and `write`: no write reaches the device. Then, with a third
//!     buffer held pinned, 32 distinct `/big` blocks pass through the cache
//!     (more than its 16 buffers), each `Buf` dropped rather than released
//!     (a cache whose dropped `Buf` stays pinned panics here, out of
//!     buffers): each holds its own bytes, still no write
//!     reaches the device; the 32 are asked for again and the device must
//!     see at least 32 - 16 reads of them (no cache of 16 buffers holds
//!     more than 16); two buffers held pinned the whole time — one got on a
//!     miss, one got on a HIT — still hold their blocks, and X and
//!     Y still hold their new bytes. Then `sync` with the wrapper refusing
//!     the first write it is handed (the wrapper's own refusal, never the
//!     learner's hook, which row 5 tests): `Err(EIO)` after two attempts;
//!     `sync` again: `Ok` after one; again: `Ok` after none; X and Y read
//!     back with this file's own port I/O (no learner code) hold the new
//!     bytes. Both blocks
//!     are put back. It runs before row 5 arms anything, so a broken hook
//!     does not reach it.
//!   5 `blk.fault_hook.present` (plain) — on the partition, at free fs
//!     blocks X = 32,767 and 32,766, 32,765 (amended 2026-10-17, the TL's
//!     order): A onto X disarmed; `arm_fail_after(2)`; two writes land and
//!     read back; the third (B onto X) is `-EIO`; still armed, X reads back
//!     A (reads are served, the refused write did not land); a fourth write
//!     is `-EIO`; `refused_writes()` moved by +2; `disarm()`; C onto X
//!     lands and reads back; the count unchanged; then `arm_fail_after(0)`
//!     refuses the first write and the count is +3 (arming never resets
//!     it). Every count is a difference. The blocks are put back.
//!
//! This file's own port I/O runs only between rows, with no learner disk
//! call on the stack, interrupts off for the whole command and nIEN set
//! during it. It never waits for the drive: BSY or DRQ set at its turn
//! fails the row ("drive busy at the test's turn"). Afterwards it puts the
//! drive select back as it found it, and the device-control register
//! (write-only, so it cannot be read back) as the learner's driver needs
//! it: nIEN clear when IRQ 14 can reach the CPU (both PICs unmask it, an
//! interrupt-driven driver), set otherwise (a polled one, the lesson's). It
//! writes only before row 2, to a block no cache holds, and reads only
//! after the learner's `sync` has returned.
//!
//! Between rows, `#` lines only: the time per 4 KiB read (64 reads of
//! `/big`, timed on the ACPI PM timer) and, after row 3, one `write` and
//! `sync` through the cache (the counted device must see one write of that
//! block; the bytes written are the block's own, so the disk is unchanged).
//! After row 5, `#` lines only: `sync` across a refused write (two dirty
//! buffers, `arm_fail_after(1)`: `Err(EIO)` after two device writes, then
//! one more write from the next `sync`), and a block past the partition's
//! end read and written (`-EIO` each, no refusal counted).

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../shim.rs"]
mod shim;

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

use kernel::addr::VirtAddr;
use kernel::blk::ata::{self, Ata, BlockDevice, Partition, BLOCK_SIZE};
use kernel::blk::cache::Cache;
use kernel::ktap;
use kernel::syscall::errno::EIO;
use kernel::syscall::Errno;
use kernel::{bootinfo, frame, gdt, heap, idt, irq, proc, sched, serial, syscall, time};

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 16;
const PM_HZ: u64 = 3_579_545;

const PLAN: usize = 5;
const IDENTIFY_ID: &str = "ata.identify";
const READ_ID: &str = "blk.read.matches.writer";
const CACHE_ID: &str = "cache.hit.count";
const WRITE_ID: &str = "cache.write.sync";
const HOOK_ID: &str = "blk.fault_hook.present";

/// What QEMU's IDE disk answers IDENTIFY with (words 27–46, trimmed).
const MODEL: &str = "QEMU HARDDISK";
/// The writer's disk: 162 MiB (§A.7).
const DISK_SECTORS: u64 = 331_776;
/// Partition 2 (GPT entry 1): its first sector and its size in blocks.
const P2_FIRST_LBA: u64 = 67_584;
const P2_BLOCKS: u64 = 32_768;
/// `/pattern`: fs blocks 1033–1044, block b = `tagged(PATTERN_MAGIC, b)`.
const PATTERN_MAGIC: u64 = 0x5B00_7B00_B10C;
const PATTERN_FIRST: u64 = 1033;
const PATTERN_BLOCKS: u64 = 12;
/// `/big`: file block i = `tagged(BIG_MAGIC, i)`; i = 12… sit at fs blocks
/// 1090… (after the 12 direct blocks and the indirect block, 1089).
const BIG_MAGIC: u64 = 0x5B00_7B00_B16F;
const BIG_RUN_FIRST: u64 = 1090;
const BIG_RUN_INDEX: u64 = 12;
const TIMED_READS: u64 = 64;
/// Blocks the writer leaves free (free blocks run 4534–32767 with 20
/// programs in `/bin`; `/bin` is created last, so the top stays free): row 5's X
/// and the two it writes while armed.
const SCRATCH: [u64; 3] = [32_767, 32_766, 32_765];
/// What row 5 writes there (A, B, C at X = this ^ 0xA, 0xB, 0xC).
const HOOK_MAGIC: u64 = 0x5B00_7B00_F00C;
/// The cache row's buffers, and its accesses: 3–12 distinct blocks, 40–80 gets.
const CACHE_BUFS: usize = 16;
/// Row 4's two blocks: X in 32,000–32,349 and Y in 32,350–32,699, free fs
/// blocks below row 5's three, picked by the nonce.
const WRITE_FREE_FIRST: u64 = 32_000;
const WRITE_FREE_SPAN: u64 = 350;
/// Row 4's pressure: distinct `/big` blocks (fs blocks 1190…, file blocks
/// 112…) run through the cache, twice its buffers.
const PRESSURE_FIRST: u64 = 1190;
const PRESSURE_BLOCKS: u64 = 32;
/// Row 2's raw block: a `/big` block in fs blocks 1300–2099 (file blocks
/// 222–1021), none of which the timed reads or row 4's pressure touch.
const RAW_FIRST: u64 = 1300;
const RAW_SPAN: u64 = 800;

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
}

fn ticks() -> u64 {
    time::TICKS.load(Ordering::Relaxed)
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
    fn expect(&mut self, cond: bool, what: &str) {
        if !cond {
            self.failed = true;
            ktap::note(format_args!("{what}: observed false, expected true"));
        }
    }
    fn expect_eq(&mut self, what: &str, observed: u64, expected: u64) {
        if observed != expected {
            self.failed = true;
            ktap::note(format_args!("{what}: observed {observed:#x}, expected {expected:#x}"));
        }
    }
    fn fail(&mut self, why: core::fmt::Arguments) {
        self.failed = true;
        ktap::note(why);
    }
    fn derived(&mut self, id: &str) -> u64 {
        self.used_nonce = true;
        ktap::derive(&ktap::nonce().unwrap_or([b'0'; 32]), id)
    }
    fn finish(mut self, n: usize) {
        if self.used_nonce && ktap::nonce().is_none() {
            self.failed = true;
            ktap::note(format_args!("{}: derived from the run's nonce, and this run has none", self.id));
        }
        if self.failed {
            kernel::kprintln!("not ok {n} - {}", self.id);
        } else {
            kernel::kprintln!("ok {n} - {}", self.id);
        }
    }
}

fn errno_text(e: Errno) -> &'static str {
    match e {
        5 => "EIO",
        19 => "ENODEV",
        22 => "EINVAL",
        _ => "?",
    }
}

// ── The writer's blocks, recomputed ──────────────────────────────────────

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

/// The first 8-byte word where `a` and `b` differ.
fn first_diff(a: &[u8; BLOCK_SIZE], b: &[u8; BLOCK_SIZE]) -> Option<usize> {
    (0..BLOCK_SIZE / 8).find(|&k| a[8 * k..8 * k + 8] != b[8 * k..8 * k + 8])
}

fn word(b: &[u8; BLOCK_SIZE], k: usize) -> u64 {
    u64::from_le_bytes(b[8 * k..8 * k + 8].try_into().unwrap())
}

/// Says where a block read back differs from what it should hold.
fn compare(r: &mut Row, what: &str, got: &[u8; BLOCK_SIZE], want: &[u8; BLOCK_SIZE]) {
    if let Some(k) = first_diff(got, want) {
        r.fail(format_args!(
            "{what}: differs first at byte {} (sector {} of the block): word {:#018x}, expected {:#018x}; its first two words are {:#x}, {:#x}",
            8 * k,
            8 * k / 512,
            word(got, k),
            word(want, k),
            word(got, 0),
            word(got, 1)
        ));
    }
}

// ── A device this vehicle owns: counts what reaches the one under it ─────

struct Counted {
    inner: Arc<dyn BlockDevice>,
    reads: AtomicU64,
    writes: AtomicU64,
    /// The last block written, for the sync probe.
    last_write: AtomicU64,
    /// Row 4's own refusal, never the learner's hook: the next writes this
    /// wrapper answers `-EIO` itself, counted in `writes`, with nothing
    /// reaching the partition (0 = none).
    refuse: AtomicU64,
}

impl Counted {
    fn new(inner: Arc<dyn BlockDevice>) -> Arc<Counted> {
        Arc::new(Counted {
            inner,
            reads: AtomicU64::new(0),
            writes: AtomicU64::new(0),
            last_write: AtomicU64::new(u64::MAX),
            refuse: AtomicU64::new(0),
        })
    }
}

impl BlockDevice for Counted {
    fn read_block(&self, n: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), Errno> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.inner.read_block(n, buf)
    }
    fn write_block(&self, n: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), Errno> {
        self.writes.fetch_add(1, Ordering::Relaxed);
        self.last_write.store(n, Ordering::Relaxed);
        let refuse = self.refuse.load(Ordering::Relaxed);
        if refuse > 0 {
            self.refuse.store(refuse - 1, Ordering::Relaxed);
            return Err(EIO);
        }
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

// ── Numbers no published formula gives ──────────────────────────────────

fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    // SAFETY: reads the time-stamp counter; no memory is touched.
    unsafe { asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags)) };
    (hi as u64) << 32 | lo as u64
}

/// The TSC, the PM timer and the tick, mixed: not the nonce and not any
/// formula a kernel could run ahead of time. Kept in locals, never printed.
fn unpredictable(salt: u64) -> u64 {
    ktap::splitmix64(rdtsc() ^ (pm() as u64).rotate_left(40) ^ ticks().rotate_left(20) ^ salt)
}

// ── The drive through this file's own port I/O, never the learner's ──────

fn in8(port: u16) -> u8 {
    let v: u8;
    // SAFETY: an 8-bit port read (the ATA bus, the PICs); no memory touched.
    unsafe { asm!("in al, dx", out("al") v, in("dx") port, options(nomem, nostack, preserves_flags)) };
    v
}

fn out8(port: u16, v: u8) {
    // SAFETY: an 8-bit port write of the primary ATA bus; no memory touched.
    unsafe { asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack, preserves_flags)) };
}

fn in16(port: u16) -> u16 {
    let v: u16;
    // SAFETY: a 16-bit read of the ATA data port; no memory touched.
    unsafe { asm!("in ax, dx", out("ax") v, in("dx") port, options(nomem, nostack, preserves_flags)) };
    v
}

fn out16(port: u16, v: u16) {
    // SAFETY: a 16-bit write of the ATA data port; no memory touched.
    unsafe { asm!("out dx, ax", in("dx") port, in("ax") v, options(nomem, nostack, preserves_flags)) };
}

const ST_BSY: u8 = 0x80;
const ST_DF: u8 = 0x20;
const ST_DRQ: u8 = 0x08;
const ST_ERR: u8 = 0x01;
const CTL_NIEN: u8 = 0x02;

/// Why this file's own transfer did not happen.
#[derive(Clone, Copy)]
enum Raw {
    /// BSY or DRQ set at the test's turn (the status then).
    Busy(u8),
    /// The drive erred or never finished (the status then).
    Failed(u8),
}

fn raw_text(r: &mut Row, e: Raw, disk_block: u64) {
    match e {
        Raw::Busy(s) => r.fail(format_args!(
            "drive busy at the test's turn: status {s:#04x} (BSY or DRQ set) before disk block {disk_block}, so a command your driver gave was left unfinished"
        )),
        Raw::Failed(s) => r.fail(format_args!("the drive did not complete a direct transfer of disk block {disk_block} (status {s:#04x})")),
    }
}

/// About 400 ns: four reads of the alternate status.
fn delay400() {
    for _ in 0..4 {
        in8(0x3f6);
    }
}

/// BSY clear (and DRQ set when `data`), or the status at the give-up.
fn raw_wait(data: bool) -> Result<(), Raw> {
    let mut s = 0;
    for _ in 0..20_000_000u32 {
        s = in8(0x1f7);
        if s & ST_BSY == 0 {
            if s & (ST_ERR | ST_DF) != 0 {
                return Err(Raw::Failed(s));
            }
            if !data || s & ST_DRQ != 0 {
                return Ok(());
            }
        }
        core::hint::spin_loop();
    }
    Err(Raw::Failed(s))
}

/// Can IRQ 14 reach the CPU? Both PICs leave it unmasked (the master's
/// line 2, the slave's line 6) only for an interrupt-driven driver.
fn irq14_open() -> bool {
    in8(0x21) & (1 << 2) == 0 && in8(0xa1) & (1 << 6) == 0
}

/// Disk block `n` (eight sectors, 28-bit LBA) read into `rd` or written
/// from `wr`, by this file alone: interrupts off for the whole command,
/// nIEN set during it, the drive select and the device control put back.
fn raw_xfer(n: u64, rd: Option<&mut [u8; BLOCK_SIZE]>, wr: Option<&[u8; BLOCK_SIZE]>) -> Result<(), Raw> {
    let rflags: u64;
    // SAFETY: saves RFLAGS and turns interrupts off; no memory is touched.
    unsafe { asm!("pushfq", "pop {}", "cli", out(reg) rflags, options(nomem)) };
    let sel = in8(0x1f6);
    if sel & 0x10 != 0 {
        // The slave is selected: look at the master.
        out8(0x1f6, 0xe0);
        delay400();
    }
    let st = in8(0x3f6);
    let r = if st & (ST_BSY | ST_DRQ) != 0 {
        Err(Raw::Busy(st))
    } else {
        out8(0x3f6, CTL_NIEN);
        let lba = n * 8;
        out8(0x1f6, 0xe0 | ((lba >> 24) & 0x0f) as u8);
        delay400();
        out8(0x1f2, 8);
        out8(0x1f3, lba as u8);
        out8(0x1f4, (lba >> 8) as u8);
        out8(0x1f5, (lba >> 16) as u8);
        out8(0x1f7, if wr.is_some() { 0x30 } else { 0x20 });
        delay400();
        let moved = (|| {
            if let Some(buf) = rd {
                for sector in buf.chunks_exact_mut(512) {
                    raw_wait(true)?;
                    for pair in sector.chunks_exact_mut(2) {
                        pair.copy_from_slice(&in16(0x1f0).to_le_bytes());
                    }
                }
            } else if let Some(buf) = wr {
                for sector in buf.chunks_exact(512) {
                    raw_wait(true)?;
                    for pair in sector.chunks_exact(2) {
                        out16(0x1f0, u16::from_le_bytes([pair[0], pair[1]]));
                    }
                }
                raw_wait(false)?;
                out8(0x1f7, 0xe7);
                delay400();
                raw_wait(false)?;
            }
            Ok(())
        })();
        // Nothing left pending; then the device control as the driver needs it.
        in8(0x1f7);
        out8(0x3f6, if irq14_open() { 0 } else { CTL_NIEN });
        moved
    };
    out8(0x1f6, sel);
    delay400();
    if rflags & (1 << 9) != 0 {
        // SAFETY: interrupts were on when this began.
        unsafe { asm!("sti", options(nomem, nostack)) };
    }
    r
}

fn raw_read(n: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), Raw> {
    raw_xfer(n, Some(buf), None)
}

fn raw_write(n: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), Raw> {
    raw_xfer(n, None, Some(buf))
}

// ── A second device, in memory, with its own GPT ─────────────────────────

/// CRC-32 (IEEE 802.3, reflected), as the GPT header and array carry it.
fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

/// Blocks 0.. hold `head` (a protective MBR, a GPT header, its array);
/// the last five blocks hold `tail` (the backup array and the backup
/// header in the last sector); every other block `n` holds
/// `tagged(fill, n)`. Read-only.
struct MemGpt {
    head: Vec<u8>,
    tail: Vec<u8>,
    fill: u64,
    blocks: u64,
}

/// The blocks at the end of a `MemGpt` that hold its backup GPT.
const MEM_TAIL_BLOCKS: u64 = 5;

impl BlockDevice for MemGpt {
    fn read_block(&self, n: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), Errno> {
        if n >= self.blocks {
            return Err(EIO);
        }
        let at = n as usize * BLOCK_SIZE;
        if n >= self.blocks - MEM_TAIL_BLOCKS {
            let t = (n - (self.blocks - MEM_TAIL_BLOCKS)) as usize * BLOCK_SIZE;
            buf.copy_from_slice(&self.tail[t..t + BLOCK_SIZE]);
        } else if at < self.head.len() {
            let end = self.head.len().min(at + BLOCK_SIZE);
            buf.fill(0);
            buf[..end - at].copy_from_slice(&self.head[at..end]);
        } else {
            buf.copy_from_slice(&tagged(self.fill, n)[..]);
        }
        Ok(())
    }
    fn write_block(&self, _n: u64, _buf: &[u8; BLOCK_SIZE]) -> Result<(), Errno> {
        Err(EIO)
    }
    fn blocks(&self) -> u64 {
        self.blocks
    }
    fn arm_fail_after(&self, _k: u32) {}
    fn disarm(&self) {}
    fn refused_writes(&self) -> u32 {
        0
    }
}

/// EFI System, then Linux filesystem data: type GUIDs in their on-disk order.
const GUID_ESP: [u8; 16] = [0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b];
const GUID_LINUX: [u8; 16] = [0xaf, 0x3d, 0xc6, 0x0f, 0x83, 0x84, 0x72, 0x47, 0x8e, 0x79, 0x3d, 0x69, 0xd8, 0x47, 0x7d, 0xe4];

/// A valid GPT over 2^20 blocks: a 16 KiB entry array at LBA 3–31, entries
/// of 256 or 512 bytes, and partition 2 at a block-aligned first LBA and a
/// size, all from `s`, and the backup array and header at the disk's end,
/// each CRC right (UEFI 2.10 §5.3). The writer's disk has its array at LBA 2 and
/// 128-byte entries; this table never repeats either. Returns the device
/// and (the array's LBA, the entry size, partition 2's first LBA, its
/// blocks).
fn mem_gpt(s: u64) -> (MemGpt, u64, u64, u64, u64) {
    let blocks: u64 = 1 << 20;
    let sectors = blocks * 8;
    let entries_lba = 3 + s % 29;
    let esize: u64 = 256 << (s >> 63);
    let count = 16_384 / esize;
    let first_block = 16 + (s >> 8) % 100_000;
    let size = 1 + (s >> 32) % 65_536;
    let (first, last) = (first_block * 8, first_block * 8 + size * 8 - 1);
    let mut h = alloc::vec![0u8; (entries_lba as usize + 32) * 512];
    let put = |h: &mut Vec<u8>, at: usize, v: &[u8]| h[at..at + v.len()].copy_from_slice(v);
    // The protective MBR.
    put(&mut h, 446 + 4, &[0xee]);
    put(&mut h, 446 + 8, &1u32.to_le_bytes());
    put(&mut h, 446 + 12, &((sectors - 1).min(u32::MAX as u64) as u32).to_le_bytes());
    put(&mut h, 510, &[0x55, 0xaa]);
    // The array: entry 0 a small partition below, entry 1 partition 2.
    let e = entries_lba as usize * 512;
    put(&mut h, e, &GUID_ESP);
    put(&mut h, e + 16, &ktap::splitmix64(s ^ 0x10).to_le_bytes());
    put(&mut h, e + 32, &64u64.to_le_bytes());
    put(&mut h, e + 40, &127u64.to_le_bytes());
    let e1 = e + esize as usize;
    put(&mut h, e1, &GUID_LINUX);
    put(&mut h, e1 + 16, &ktap::splitmix64(s ^ 0x11).to_le_bytes());
    put(&mut h, e1 + 24, &ktap::splitmix64(s ^ 0x12).to_le_bytes());
    put(&mut h, e1 + 32, &first.to_le_bytes());
    put(&mut h, e1 + 40, &last.to_le_bytes());
    let array_crc = crc32(&h[e..e + 16_384]);
    // The header, sector 1.
    put(&mut h, 512, b"EFI PART");
    put(&mut h, 512 + 8, &0x0001_0000u32.to_le_bytes());
    put(&mut h, 512 + 12, &92u32.to_le_bytes());
    put(&mut h, 512 + 24, &1u64.to_le_bytes());
    put(&mut h, 512 + 32, &(sectors - 1).to_le_bytes());
    put(&mut h, 512 + 40, &(entries_lba + 32).to_le_bytes());
    put(&mut h, 512 + 48, &(sectors - 34).to_le_bytes());
    put(&mut h, 512 + 56, &ktap::splitmix64(s ^ 0x13).to_le_bytes());
    put(&mut h, 512 + 64, &ktap::splitmix64(s ^ 0x14).to_le_bytes());
    put(&mut h, 512 + 72, &entries_lba.to_le_bytes());
    put(&mut h, 512 + 80, &(count as u32).to_le_bytes());
    put(&mut h, 512 + 84, &(esize as u32).to_le_bytes());
    put(&mut h, 512 + 88, &array_crc.to_le_bytes());
    let hdr_crc = crc32(&h[512..512 + 92]);
    put(&mut h, 512 + 16, &hdr_crc.to_le_bytes());
    // The backup: the same array at LBA sectors-33 (ending before the backup
    // header), the header in the last sector with my/alternate LBAs swapped,
    // its array LBA its own, and its CRC computed with the CRC field zero.
    let tail_lba = sectors - MEM_TAIL_BLOCKS * 8;
    let mut t = alloc::vec![0u8; MEM_TAIL_BLOCKS as usize * BLOCK_SIZE];
    let ba = ((sectors - 33 - tail_lba) * 512) as usize;
    t[ba..ba + 16_384].copy_from_slice(&h[e..e + 16_384]);
    let bh = ((sectors - 1 - tail_lba) * 512) as usize;
    t[bh..bh + 92].copy_from_slice(&h[512..512 + 92]);
    put(&mut t, bh + 16, &0u32.to_le_bytes());
    put(&mut t, bh + 24, &(sectors - 1).to_le_bytes());
    put(&mut t, bh + 32, &1u64.to_le_bytes());
    put(&mut t, bh + 72, &(sectors - 33).to_le_bytes());
    let bcrc = crc32(&t[bh..bh + 92]);
    put(&mut t, bh + 16, &bcrc.to_le_bytes());
    (MemGpt { head: h, tail: t, fill: ktap::splitmix64(s ^ 0x6f), blocks }, entries_lba, esize, first, size)
}

/// Row 2's second half: `partition2` over a device whose table puts
/// partition 2 where nothing published says.
fn gpt_elsewhere(r: &mut Row) {
    let s = unpredictable(0x1602);
    let (dev, entries_lba, esize, first, size) = mem_gpt(s);
    let fill = dev.fill;
    match ata::partition2(Arc::new(dev)) {
        Ok(p) => {
            let (fl, bl) = (p.first_lba(), p.blocks());
            let j = (s >> 48) % size;
            let mut got = Box::new([0u8; BLOCK_SIZE]);
            let rd = p.read_block(j, &mut got);
            ktap::note(format_args!(
                "{READ_ID}: a second device, in memory, whose GPT puts its entry array at LBA {entries_lba}, {esize}-byte entries, and partition 2 at first LBA {first}, {size} blocks: partition2 found first LBA {fl}, {bl} blocks"
            ));
            r.expect_eq("partition 2's first LBA on a second device whose table puts it elsewhere", fl, first);
            r.expect_eq("partition 2's blocks() on that device", bl, size);
            match rd {
                Ok(()) => compare(r, "a block read through that partition", &got, &tagged(fill, first / 8 + j)),
                Err(e) => r.fail(format_args!("that partition's read_block({j}) returned -{} ({e})", errno_text(e))),
            }
        }
        Err(e) => r.fail(format_args!(
            "partition2 over a second device with a valid GPT (entry array at LBA {entries_lba}, {esize}-byte entries) returned -{} ({e})",
            errno_text(e)
        )),
    }
}

// ── Row 1: ata.identify ──────────────────────────────────────────────────

fn identify() -> Option<Arc<Ata>> {
    let mut r = Row::new(IDENTIFY_ID);
    let ata = match Ata::probe() {
        Ok(a) => Some(Arc::new(a)),
        Err(e) => {
            r.fail(format_args!("Ata::probe() returned -{} ({e}): no IDENTIFY answer from the primary master", errno_text(e)));
            None
        }
    };
    if let Some(a) = &ata {
        let model = a.model();
        let sectors = a.sectors();
        let blocks = a.blocks();
        ktap::note(format_args!(
            "{IDENTIFY_ID}: model \"{model}\", {sectors} sectors, blocks() = {blocks}; the image writer's disk is {DISK_SECTORS} sectors"
        ));
        r.expect(model == MODEL, "the model IDENTIFY reports (words 27-46) is \"QEMU HARDDISK\"");
        r.expect_eq("sectors(), from IDENTIFY words 60-61", sectors, DISK_SECTORS);
        r.expect_eq("blocks(): 4 KiB blocks on the disk", blocks, DISK_SECTORS / 8);
    }
    r.finish(1);
    ata
}

// ── Row 2: blk.read.matches.writer ───────────────────────────────────────

fn read_matches(dev: Option<&Arc<Ata>>) -> Option<Arc<Partition>> {
    let mut r = Row::new(READ_ID);
    let d = r.derived(READ_ID);
    let b = PATTERN_FIRST + d % PATTERN_BLOCKS;
    let disk_block = P2_FIRST_LBA / 8 + b;
    ktap::note(format_args!(
        "{READ_ID}: derive(nonce, \"{READ_ID}\") = {d:016x}: fs block {b} of /pattern, the disk's block {disk_block}"
    ));
    let Some(dev) = dev else {
        r.fail(format_args!("no drive: Ata::probe failed in row 1"));
        r.finish(2);
        return None;
    };
    let want = tagged(PATTERN_MAGIC, b);
    let part = match ata::partition2(dev.clone()) {
        Ok(p) => Some(Arc::new(p)),
        Err(e) => {
            r.fail(format_args!("partition2(drive) returned -{} ({e})", errno_text(e)));
            None
        }
    };
    if let Some(p) = &part {
        ktap::note(format_args!("{READ_ID}: partition2: first LBA {}, {} blocks", p.first_lba(), p.blocks()));
        r.expect_eq("partition 2's first LBA, from GPT entry 1", p.first_lba(), P2_FIRST_LBA);
        r.expect_eq("partition 2's blocks()", p.blocks(), P2_BLOCKS);
        let mut got = Box::new([0u8; BLOCK_SIZE]);
        match p.read_block(b, &mut got) {
            Ok(()) => compare(&mut r, "partition block read_block(b)", &got, &want),
            Err(e) => r.fail(format_args!("partition read_block({b}) returned -{} ({e})", errno_text(e))),
        }
    }
    let mut raw = Box::new([0u8; BLOCK_SIZE]);
    match dev.read_block(disk_block, &mut raw) {
        Ok(()) => compare(&mut r, "the drive's read_block(8448 + b)", &raw, &want),
        Err(e) => r.fail(format_args!("drive read_block({disk_block}) returned -{} ({e})", errno_text(e))),
    }
    // A block of /big your driver reads once, then this file's own port
    // I/O rewrites with bytes no formula gives, then your driver reads again.
    let secret = unpredictable(0x1601);
    let w = RAW_FIRST + secret % RAW_SPAN;
    let wdisk = P2_FIRST_LBA / 8 + w;
    let wbytes = tagged(ktap::splitmix64(secret ^ 0x5e), w);
    let mut worig = Box::new([0u8; BLOCK_SIZE]);
    let staged = match raw_read(wdisk, &mut worig) {
        Ok(()) => true,
        Err(e) => {
            raw_text(&mut r, e, wdisk);
            false
        }
    };
    if staged {
        if let Some(p) = &part {
            match p.read_block(w, &mut raw) {
                Ok(()) => compare(&mut r, "a /big block read through the partition, before it changes", &raw, &worig),
                Err(e) => r.fail(format_args!("partition read_block({w}) returned -{} ({e})", errno_text(e))),
            }
        }
        match dev.read_block(wdisk, &mut raw) {
            Ok(()) => compare(&mut r, "that block read through the drive at its disk block, before it changes", &raw, &worig),
            Err(e) => r.fail(format_args!("drive read_block({wdisk}) returned -{} ({e})", errno_text(e))),
        }
    }
    let staged = staged
        && match raw_write(wdisk, &wbytes) {
            Ok(()) => true,
            Err(e) => {
                raw_text(&mut r, e, wdisk);
                false
            }
        };
    if staged {
        if let Some(p) = &part {
            match p.read_block(w, &mut raw) {
                Ok(()) => compare(&mut r, "the same /big block after it changed on the disk, read through the partition again", &raw, &wbytes),
                Err(e) => r.fail(format_args!("partition read_block({w}) returned -{} ({e})", errno_text(e))),
            }
        }
        match dev.read_block(wdisk, &mut raw) {
            Ok(()) => compare(&mut r, "that block read through the drive again, after it changed", &raw, &wbytes),
            Err(e) => r.fail(format_args!("drive read_block({wdisk}) returned -{} ({e})", errno_text(e))),
        }
        let put_back = raw_write(wdisk, &worig).and_then(|()| raw_read(wdisk, &mut raw)).is_ok() && first_diff(&raw, &worig).is_none();
        ktap::note(format_args!("{READ_ID}: fs block {w} read through your driver, rewritten on the disk, read again, then put back as it was: {put_back}"));
    }
    gpt_elsewhere(&mut r);
    r.finish(2);
    part
}

/// `#` only: how long one 4 KiB read takes, over a run of `/big`.
fn time_reads(p: &Partition) {
    let mut buf = Box::new([0u8; BLOCK_SIZE]);
    let t0 = ticks();
    let mut c = Clock::new();
    let mut bad = 0;
    let mut slowest = 0;
    let mut last = 0;
    for i in 0..TIMED_READS {
        let ok = p.read_block(BIG_RUN_FIRST + i, &mut buf).is_ok();
        let now = c.us();
        slowest = slowest.max(now - last);
        last = now;
        if !ok || first_diff(&buf, &tagged(BIG_MAGIC, BIG_RUN_INDEX + i)).is_some() {
            bad += 1;
        }
    }
    let us = c.us();
    ktap::note(format_args!(
        "disk: {TIMED_READS} reads of 4 KiB (/big, fs blocks {BIG_RUN_FIRST}-{}) took {us} us, {} us each, the slowest {slowest} us; {bad} not as written; the tick advanced {} meanwhile",
        BIG_RUN_FIRST + TIMED_READS - 1,
        us / TIMED_READS,
        ticks() - t0
    ));
}

// ── Row 3: cache.hit.count ───────────────────────────────────────────────

fn cache_hits(part: Option<&Arc<Partition>>) -> Option<(Cache, Arc<Counted>)> {
    let mut r = Row::new(CACHE_ID);
    let seed = r.derived(CACHE_ID);
    // The distinct blocks: a nonce-chosen 3–12 of /pattern's 12, in a
    // nonce-chosen order; then 40–80 gets: each block once in that order,
    // then nonce-chosen ones among them.
    let distinct = 3 + ktap::splitmix64(seed ^ 1) % 10;
    let gets = 40 + ktap::splitmix64(seed ^ 2) % 41;
    let mut order: Vec<u64> = (0..PATTERN_BLOCKS).map(|i| PATTERN_FIRST + i).collect();
    for i in (1..order.len()).rev() {
        let j = (ktap::splitmix64(seed ^ (0x100 + i as u64)) % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    order.truncate(distinct as usize);
    let seq: Vec<u64> = (0..gets)
        .map(|k| if k < distinct { order[k as usize] } else { order[(ktap::splitmix64(seed ^ (0x1000 + k)) % distinct) as usize] })
        .collect();
    ktap::note(format_args!(
        "{CACHE_ID}: derive(nonce, \"{CACHE_ID}\") = {seed:016x}: {gets} gets over {distinct} blocks {:?} through a cache of {CACHE_BUFS} buffers; a device read for each block's first get, a hit for each of the other {}",
        &order[..],
        gets - distinct
    ));
    let Some(p) = part else {
        r.fail(format_args!("no partition: row 2 found none"));
        r.finish(3);
        return None;
    };
    let counted = Counted::new(p.clone());
    let cache = Cache::new(counted.clone(), CACHE_BUFS);
    let hits0 = cache.hits();
    let mut wrong = 0u64;
    let mut wrong_block = 0u64;
    let mut failed = 0u64;
    for &b in &seq {
        let buf = match cache.get(b) {
            Ok(buf) => buf,
            Err(e) => {
                if failed == 0 {
                    ktap::note(format_args!("{CACHE_ID}: get({b}) returned -{} ({e})", errno_text(e)));
                }
                failed += 1;
                continue;
            }
        };
        let held = buf.block();
        let same = first_diff(&buf.data(), &tagged(PATTERN_MAGIC, b)).is_none();
        if held != b || !same {
            if wrong == 0 {
                wrong_block = b;
            }
            wrong += 1;
        }
        cache.release(buf);
    }
    let hits = cache.hits() - hits0;
    let reads = counted.reads.load(Ordering::Relaxed);
    let writes = counted.writes.load(Ordering::Relaxed);
    ktap::note(format_args!(
        "{CACHE_ID}: the device under the cache saw {reads} reads and {writes} writes; hits() moved by {hits}; {wrong} gets served a buffer not holding their block"
    ));
    if wrong > 0 {
        r.fail(format_args!("{wrong} of {gets} gets handed back a buffer without its block's bytes (first: block {wrong_block})"));
    }
    r.expect_eq("gets that returned an errno", failed, 0);
    r.expect_eq("device reads under the cache (one per distinct block)", reads, distinct);
    r.expect_eq("hits(): the gets that reached no device (gets - device reads)", hits, gets - reads);
    r.expect_eq("hits() against the gets that needed no read", hits, gets - distinct);
    r.expect_eq("device writes during the gets", writes, 0);
    r.finish(3);
    Some((cache, counted))
}

/// `#` only: one buffer written back through `sync` (its own bytes).
fn sync_probe(c: Option<&(Cache, Arc<Counted>)>) {
    let Some((cache, counted)) = c else { return };
    let w0 = counted.writes.load(Ordering::Relaxed);
    let b = match cache.get(PATTERN_FIRST) {
        Ok(b) => b,
        Err(e) => {
            ktap::note(format_args!("cache: get({PATTERN_FIRST}) returned -{} ({e}); no write probe", errno_text(e)));
            return;
        }
    };
    cache.write(&b);
    cache.release(b);
    let s1 = cache.sync();
    let w1 = counted.writes.load(Ordering::Relaxed);
    let s2 = cache.sync();
    let w2 = counted.writes.load(Ordering::Relaxed);
    ktap::note(format_args!(
        "cache: write + sync of block {PATTERN_FIRST} -> {s1:?}, reached the device as {} writes (last block {}), a second sync -> {s2:?} as {} (expected Ok, 1, {PATTERN_FIRST}, Ok, 0)",
        w1 - w0,
        counted.last_write.load(Ordering::Relaxed),
        w2 - w1
    ));
}

/// `#` only, after the last row: `sync` across a refused hook write. Two buffers
/// made dirty with their own bytes (the disk does not change), the hook
/// armed to let one write land: `sync` must try both, answer `-EIO`, and
/// leave the refused one dirty, so a second `sync`, disarmed, writes it
/// alone. Then a block past the partition's end, read and written.
fn sync_refused_probe(c: Option<&(Cache, Arc<Counted>)>, part: Option<&Arc<Partition>>) {
    if let Some((cache, counted)) = c {
        let bs = [cache.get(PATTERN_FIRST), cache.get(PATTERN_FIRST + 1)];
        if let [Ok(b0), Ok(b1)] = bs {
            cache.write(&b0);
            cache.write(&b1);
            cache.release(b0);
            cache.release(b1);
            let base = counted.refused_writes();
            let w0 = counted.writes.load(Ordering::Relaxed);
            counted.arm_fail_after(1);
            let s1 = cache.sync();
            let w1 = counted.writes.load(Ordering::Relaxed);
            counted.disarm();
            let s2 = cache.sync();
            let w2 = counted.writes.load(Ordering::Relaxed);
            ktap::note(format_args!(
                "cache: two dirty buffers, arm_fail_after(1), sync -> {s1:?} after {} device writes; disarmed, sync -> {s2:?} after {} more (last block {}); refused_writes() +{} (expected Err(5), 2, Ok, 1, +1)",
                w1 - w0,
                w2 - w1,
                counted.last_write.load(Ordering::Relaxed),
                counted.refused_writes().wrapping_sub(base)
            ));
        } else {
            ktap::note(format_args!("cache: a get for the refused-sync probe returned an errno; no probe"));
        }
    }
    if let Some(p) = part {
        let mut buf = Box::new([0u8; BLOCK_SIZE]);
        let end = p.blocks();
        let base = p.refused_writes();
        let r = p.read_block(end, &mut buf);
        let w = p.write_block(end, &buf);
        ktap::note(format_args!(
            "partition: block {end} (its blocks()): read_block -> {r:?}, write_block -> {w:?}, refused_writes() +{} (expected Err(5), Err(5), +0)",
            p.refused_writes().wrapping_sub(base)
        ));
    }
}

// ── Row 4: cache.write.sync ──────────────────────────────────────────────

fn write_sync(part: Option<&Arc<Partition>>) {
    let mut r = Row::new(WRITE_ID);
    let d = r.derived(WRITE_ID);
    let x = WRITE_FREE_FIRST + d % WRITE_FREE_SPAN;
    let y = WRITE_FREE_FIRST + WRITE_FREE_SPAN + (d >> 32) % WRITE_FREE_SPAN;
    let z = PATTERN_FIRST + (d >> 16) % PATTERN_BLOCKS;
    let z2 = PATTERN_FIRST + ((d >> 16) % PATTERN_BLOCKS + 1 + (d >> 24) % (PATTERN_BLOCKS - 1)) % PATTERN_BLOCKS;
    ktap::note(format_args!(
        "{WRITE_ID}: derive(nonce, \"{WRITE_ID}\") = {d:016x}: new bytes for fs blocks {x} and {y} through a cache of {CACHE_BUFS} buffers, blocks {z} (got on a miss) and {z2} (got again, a hit) held pinned while {PRESSURE_BLOCKS} other blocks pass through it twice"
    ));
    let Some(p) = part else {
        r.fail(format_args!("no partition: row 2 found none"));
        r.finish(4);
        return;
    };
    let px = tagged(ktap::splitmix64(d ^ 0x5C), x);
    let py = tagged(ktap::splitmix64(d ^ 0x5D), y);
    // What the writer left there, put back at the end.
    let mut ox = Box::new([0u8; BLOCK_SIZE]);
    let mut oy = Box::new([0u8; BLOCK_SIZE]);
    if p.read_block(x, &mut ox).is_err() || p.read_block(y, &mut oy).is_err() {
        r.fail(format_args!("the partition did not read blocks {x} and {y} before the test"));
        r.finish(4);
        return;
    }
    let counted = Counted::new(p.clone());
    let cache = Cache::new(counted.clone(), CACHE_BUFS);

    // 1. New bytes and `write`: marked, not written.
    for (n, want) in [(x, &px), (y, &py)] {
        match cache.get(n) {
            Ok(b) => {
                b.data().copy_from_slice(&want[..]);
                cache.write(&b);
                cache.release(b);
            }
            Err(e) => r.fail(format_args!("get({n}) returned -{} ({e})", errno_text(e))),
        }
    }
    let w1 = counted.writes.load(Ordering::Relaxed);
    r.expect_eq("device writes after write() marked two buffers (a change reaches the disk at sync)", w1, 0);

    // 2. More blocks than buffers, two buffers pinned (one from a miss, one
    //    from a hit), two dirty.
    let pinned = cache.get(z);
    if let Ok(b) = cache.get(z2) {
        cache.release(b);
    }
    let pinned2 = cache.get(z2);
    let mut wrong = 0u64;
    let mut failed = 0u64;
    let mut again = 0u64;
    for k in 0..2 * PRESSURE_BLOCKS {
        if k == PRESSURE_BLOCKS {
            again = counted.reads.load(Ordering::Relaxed);
        }
        let k = k % PRESSURE_BLOCKS;
        let n = PRESSURE_FIRST + k;
        match cache.get(n) {
            Ok(b) => {
                let ok = b.block() == n && first_diff(&b.data(), &tagged(BIG_MAGIC, n - BIG_RUN_FIRST + BIG_RUN_INDEX)).is_none();
                if !ok {
                    wrong += 1;
                }
                // Dropped, not released: a dropped `Buf` is unpinned too
                // (lab 17's file system lets its buffers go that way).
                drop(b);
            }
            Err(e) => {
                if failed == 0 {
                    ktap::note(format_args!("{WRITE_ID}: get({n}) returned -{} ({e})", errno_text(e)));
                }
                failed += 1;
            }
        }
    }
    let w2 = counted.writes.load(Ordering::Relaxed);
    let reread = counted.reads.load(Ordering::Relaxed) - again;
    ktap::note(format_args!(
        "{WRITE_ID}: {PRESSURE_BLOCKS} other blocks through the cache twice: {wrong} held the wrong bytes, {failed} returned an errno; device writes {}; device reads the second time {reread} (at least {})",
        w2 - w1,
        PRESSURE_BLOCKS - CACHE_BUFS as u64
    ));
    r.expect(
        reread >= PRESSURE_BLOCKS - CACHE_BUFS as u64,
        "the device read most of the blocks the second time round (a cache of 16 buffers holds at most 16 of them)",
    );
    r.expect_eq("blocks that passed through the cache holding the wrong bytes", wrong, 0);
    r.expect_eq("gets that returned an errno while other blocks passed through", failed, 0);
    r.expect_eq("device writes while other blocks passed through (a dirty buffer is never written early or reused)", w2 - w1, 0);
    for (held, n, how) in [(pinned, z, "a miss"), (pinned2, z2, "a hit")] {
        match held {
            Ok(b) => {
                let same = b.block() == n && first_diff(&b.data(), &tagged(PATTERN_MAGIC, n)).is_none();
                if !same {
                    r.fail(format_args!("the buffer got on {how} (block {n}) and held pinned the whole time no longer holds its block and its bytes"));
                }
                cache.release(b);
            }
            Err(e) => r.fail(format_args!("get({n}) returned -{} ({e})", errno_text(e))),
        }
    }
    for (n, want) in [(x, &px), (y, &py)] {
        match cache.get(n) {
            Ok(b) => {
                let held = Box::new(*b.data());
                if let Some(k) = first_diff(&held, want) {
                    r.fail(format_args!(
                        "block {n} lost the bytes write() marked while other blocks passed through: word {k} is {:#018x}, expected {:#018x}",
                        word(&held, k),
                        word(want, k)
                    ));
                }
                cache.release(b);
            }
            Err(e) => r.fail(format_args!("get({n}) returned -{} ({e})", errno_text(e))),
        }
    }

    // 3. sync, the device refusing the first write it is handed (whichever
    //    block that is: the counts below hold in any order).
    counted.refuse.store(1, Ordering::Relaxed);
    let s1 = cache.sync();
    let w3 = counted.writes.load(Ordering::Relaxed);
    counted.refuse.store(0, Ordering::Relaxed);
    // 4. sync again: the refused buffer, still dirty, alone.
    let s2 = cache.sync();
    let w4 = counted.writes.load(Ordering::Relaxed);
    // 5. sync again: nothing left.
    let s3 = cache.sync();
    let w5 = counted.writes.load(Ordering::Relaxed);
    ktap::note(format_args!(
        "{WRITE_ID}: sync with the first device write refused -> {s1:?} after {} writes; sync -> {s2:?} after {}; sync -> {s3:?} after {} (expected Err(5) after 2, Ok after 1, Ok after 0)",
        w3 - w2,
        w4 - w3,
        w5 - w4
    ));
    if s1 != Err(EIO) {
        r.fail(format_args!("sync with a refused write returned {s1:?}, expected Err(EIO)"));
    }
    r.expect_eq("writes sync tried with two dirty buffers, the first refused (every dirty buffer is tried)", w3 - w2, 2);
    r.expect(s2 == Ok(()), "the next sync returns Ok");
    r.expect_eq("writes the next sync made (the refused buffer stayed dirty, the written one did not)", w4 - w3, 1);
    r.expect(s3 == Ok(()), "a sync with nothing dirty returns Ok");
    r.expect_eq("writes a sync with nothing dirty made", w5 - w4, 0);

    // 6. On the disk, read by this file's own port I/O after sync returned.
    for (n, want, what) in [(x, &px, "block X on the disk after sync, read without your driver"), (y, &py, "block Y on the disk after sync, read without your driver")] {
        let mut got = Box::new([0u8; BLOCK_SIZE]);
        match raw_read(P2_FIRST_LBA / 8 + n, &mut got) {
            Ok(()) => compare(&mut r, what, &got, want),
            Err(e) => raw_text(&mut r, e, P2_FIRST_LBA / 8 + n),
        }
    }

    // Put both blocks back as the writer left them.
    let mut back = Box::new([0u8; BLOCK_SIZE]);
    let mut restored = true;
    for (n, o) in [(x, &ox), (y, &oy)] {
        restored &= p.write_block(n, o).is_ok() && p.read_block(n, &mut back).is_ok() && first_diff(&back, o).is_none();
    }
    ktap::note(format_args!("{WRITE_ID}: fs blocks {x}, {y} put back as they were: {restored}"));
    r.finish(4);
}

// ── Row 5: blk.fault_hook.present ────────────────────────────────────────

fn fault_hook(part: Option<&Arc<Partition>>) {
    let mut r = Row::new(HOOK_ID);
    let Some(p) = part else {
        r.fail(format_args!("no partition: row 2 found none"));
        r.finish(5);
        return;
    };
    let [x, y1, y2] = SCRATCH;
    // What the writer left there, put back at the end.
    let mut orig = [Box::new([0u8; BLOCK_SIZE]), Box::new([0u8; BLOCK_SIZE]), Box::new([0u8; BLOCK_SIZE])];
    for (n, o) in SCRATCH.iter().zip(orig.iter_mut()) {
        if let Err(e) = p.read_block(*n, o) {
            r.fail(format_args!("read_block({n}) returned -{} ({e}) before the hook was armed", errno_text(e)));
            r.finish(5);
            return;
        }
    }
    let pa = tagged(HOOK_MAGIC ^ 0xA, x);
    let pb = tagged(HOOK_MAGIC ^ 0xB, x);
    let pc = tagged(HOOK_MAGIC ^ 0xC, x);
    let py = [tagged(HOOK_MAGIC ^ 0x1, y1), tagged(HOOK_MAGIC ^ 0x2, y2)];
    let mut back = Box::new([0u8; BLOCK_SIZE]);

    // 1. Disarmed: A onto X lands.
    let w = p.write_block(x, &pa);
    r.expect(w.is_ok(), "the disarmed write of A onto X lands");
    match p.read_block(x, &mut back) {
        Ok(()) => compare(&mut r, "X after the disarmed write of A", &back, &pa),
        Err(e) => r.fail(format_args!("read_block({x}) returned -{} ({e})", errno_text(e))),
    }
    // 2. Armed with 2.
    let base = p.refused_writes();
    p.arm_fail_after(2);
    // 3. Two writes land and read back (the reads while armed).
    for (n, want) in [y1, y2].iter().zip(py.iter()) {
        let w = p.write_block(*n, want);
        if w.is_err() {
            r.fail(format_args!("write {} after arm_fail_after(2) (block {n}) returned {w:?}, expected Ok", if *n == y1 { 1 } else { 2 }));
        }
        match p.read_block(*n, &mut back) {
            Ok(()) => compare(&mut r, "a block written while armed, before the k-th", &back, want),
            Err(e) => r.fail(format_args!("a read while armed (block {n}) returned -{} ({e}): a read is never refused", errno_text(e))),
        }
    }
    // 4. The third, B onto X, is refused.
    let w3 = p.write_block(x, &pb);
    // 5. Still armed: X reads back A — the read is served, the refused write did not land.
    match p.read_block(x, &mut back) {
        Ok(()) => compare(&mut r, "X after the refused write of B (re-read, armed)", &back, &pa),
        Err(e) => r.fail(format_args!("a read of X while armed returned -{} ({e}): a read is never refused", errno_text(e))),
    }
    // 6. One more write is refused too.
    let w4 = p.write_block(x, &pb);
    ktap::note(format_args!("{HOOK_ID}: arm_fail_after(2); writes 3 and 4 -> {w3:?}, {w4:?}"));
    if w3 != Err(EIO) {
        r.fail(format_args!("the third write after arm_fail_after(2) returned {w3:?}, expected Err(EIO)"));
    }
    if w4 != Err(EIO) {
        r.fail(format_args!("the fourth write after arm_fail_after(2) returned {w4:?}, expected Err(EIO): every write after the first k is refused"));
    }
    // 7. Counted: +2.
    let armed_delta = p.refused_writes().wrapping_sub(base);
    r.expect_eq("refused_writes() moved by the two refusals (a difference, not the count)", armed_delta as u64, 2);
    // 8.–9. Disarmed: C onto X lands and reads back.
    p.disarm();
    let mut c = Clock::new();
    let w = p.write_block(x, &pc);
    let took = c.us();
    ktap::note(format_args!("{HOOK_ID}: disarm(), write_block({x}) -> {w:?} in {took} us"));
    r.expect(w.is_ok(), "a write after disarm() lands");
    match p.read_block(x, &mut back) {
        Ok(()) => compare(&mut r, "X after the write that follows disarm()", &back, &pc),
        Err(e) => r.fail(format_args!("read_block({x}) after disarm returned -{} ({e})", errno_text(e))),
    }
    // 10. disarm() kept the count.
    let kept = p.refused_writes().wrapping_sub(base);
    r.expect_eq("refused_writes() after disarm() and a landed write (unchanged)", kept as u64, 2);
    // 11. Arming again does not reset it: arm_fail_after(0) refuses the first.
    p.arm_fail_after(0);
    let w5 = p.write_block(x, &pa);
    p.disarm();
    let again = p.refused_writes().wrapping_sub(base);
    ktap::note(format_args!("{HOOK_ID}: arm_fail_after(0), write -> {w5:?}; refused_writes() {base} -> {}", base.wrapping_add(again)));
    if w5 != Err(EIO) {
        r.fail(format_args!("the first write after arm_fail_after(0) returned {w5:?}, expected Err(EIO)"));
    }
    r.expect_eq("refused_writes() after a second arm and one refusal (arming never resets it)", again as u64, 3);
    match p.read_block(x, &mut back) {
        Ok(()) => compare(&mut r, "X after the refused write under arm_fail_after(0)", &back, &pc),
        Err(e) => r.fail(format_args!("read_block({x}) returned -{} ({e})", errno_text(e))),
    }
    // Put the blocks back as the writer left them.
    let mut restored = true;
    for (n, o) in SCRATCH.iter().zip(orig.iter()) {
        restored &= p.write_block(*n, o).is_ok() && p.read_block(*n, &mut back).is_ok() && first_diff(&back, o).is_none();
    }
    ktap::note(format_args!("{HOOK_ID}: fs blocks {x}, {y1}, {y2} put back as they were: {restored}"));
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
    while ticks() < t0 + 2 && c.us() < 100_000 {
        core::hint::spin_loop();
    }
}

fn halt() -> ! {
    loop {
        // SAFETY: interrupts off, then wait: nothing runs after this.
        unsafe { asm!("cli", "hlt", options(nomem, nostack)) };
    }
}

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    serial::init();
    ktap::begin(PLAN);
    prelude();
    let ata = identify();
    let part = read_matches(ata.as_ref());
    if let Some(p) = &part {
        time_reads(p);
    }
    let cache = cache_hits(part.as_ref());
    sync_probe(cache.as_ref());
    write_sync(part.as_ref());
    fault_hook(part.as_ref());
    sync_refused_probe(cache.as_ref(), part.as_ref());
    ktap::end();
    halt()
}
