//! Lab 16's vehicle: the disk and the buffer cache. The prelude is lab 15's
//! (the GDT, the IDT, the frame pool, the syscall MSRs, the heap, `sse_on`,
//! `proc::init`, task 0, the PIC and the PIT), so the disk is driven with
//! interrupts on and the tick preempting, as every later lab drives it.
//!
//! The disk is `ktest-16.img` as the course's image writer made it before
//! boot (CONTRACT §A.7 "The disk from lab 16", "The writer's tree"): 331,776
//! sectors; partition 2 = GPT entry 1 at LBA 67,584, 32,768 blocks of 4 KiB;
//! fs blocks 1033–1044 (`/pattern`) hold `tagged(0x5B007B00B10C, b)`; fs
//! blocks 32,765–32,767 are free. This vehicle knows those numbers; the learner's
//! kernel must find them.
//!
//! Four rows, printed by this file in this order (no `.ktest_array`):
//!
//!   1 `ata.identify` (plain) — `Ata::probe`'s IDENTIFY: the model string
//!     is QEMU's, and the sector count is the writer's disk size, which
//!     `blocks()` turns into 4 KiB blocks.
//!   2 `blk.read.matches.writer` (derived) — `partition2` finds partition 2
//!     through the GPT at the writer's LBA and size; a nonce-chosen
//!     `/pattern` block read through the partition, and the same block read
//!     through the drive at its disk block, each equal `tagged(…)`
//!     recomputed here.
//!   3 `cache.hit.count` (derived) — the learner's `Cache` over a device
//!     wrapper this vehicle owns, which counts the reads that reach the
//!     partition. A nonce-chosen run of `get`s over a nonce-chosen set of
//!     `/pattern` blocks (fewer than the cache's buffers): every buffer
//!     served holds its block's bytes, the device saw exactly one read per
//!     distinct block, and `hits()` equals the `get`s that reached no
//!     device — this file's own count, not the cache's.
//!   4 `blk.fault_hook.present` (plain) — on the partition, at free fs
//!     blocks X = 32,767 and 32,766, 32,765 (amended 2026-10-17, the TL's
//!     order): A onto X disarmed; `arm_fail_after(2)`; two writes land and
//!     read back; the third (B onto X) is `-EIO`; still armed, X reads back
//!     A (reads are served, the refused write did not land); a fourth write
//!     is `-EIO`; `refused_writes()` moved by +2; `disarm()`; C onto X
//!     lands and reads back; the count unchanged; then `arm_fail_after(0)`
//!     refuses the first write and the count is +3 (arming never resets
//!     it). Every count is a difference. The blocks are put back.
//!
//! Between rows, `#` lines only: the time per 4 KiB read (64 reads of
//! `/big`, timed on the ACPI PM timer) and, after row 3, one `write` and
//! `sync` through the cache (the counted device must see one write of that
//! block; the bytes written are the block's own, so the disk is unchanged).
//! After row 4, `#` lines only: `sync` across a refused write (two dirty
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

const PLAN: usize = 4;
const IDENTIFY_ID: &str = "ata.identify";
const READ_ID: &str = "blk.read.matches.writer";
const CACHE_ID: &str = "cache.hit.count";
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
/// programs in `/bin`; `/bin` is created last, so the top stays free): row 4's X
/// and the two it writes while armed.
const SCRATCH: [u64; 3] = [32_767, 32_766, 32_765];
/// What row 4 writes there (A, B, C at X = this ^ 0xA, 0xB, 0xC).
const HOOK_MAGIC: u64 = 0x5B00_7B00_F00C;
/// The cache row's buffers, and its accesses: 3–12 distinct blocks, 40–80 gets.
const CACHE_BUFS: usize = 16;

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
}

impl BlockDevice for Counted {
    fn read_block(&self, n: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), Errno> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.inner.read_block(n, buf)
    }
    fn write_block(&self, n: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), Errno> {
        self.writes.fetch_add(1, Ordering::Relaxed);
        self.last_write.store(n, Ordering::Relaxed);
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
    let counted = Arc::new(Counted {
        inner: p.clone(),
        reads: AtomicU64::new(0),
        writes: AtomicU64::new(0),
        last_write: AtomicU64::new(u64::MAX),
    });
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

/// `#` only, after the last row: `sync` across a refused write. Two buffers
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

// ── Row 4: blk.fault_hook.present ────────────────────────────────────────

fn fault_hook(part: Option<&Arc<Partition>>) {
    let mut r = Row::new(HOOK_ID);
    let Some(p) = part else {
        r.fail(format_args!("no partition: row 2 found none"));
        r.finish(4);
        return;
    };
    let [x, y1, y2] = SCRATCH;
    // What the writer left there, put back at the end.
    let mut orig = [Box::new([0u8; BLOCK_SIZE]), Box::new([0u8; BLOCK_SIZE]), Box::new([0u8; BLOCK_SIZE])];
    for (n, o) in SCRATCH.iter().zip(orig.iter_mut()) {
        if let Err(e) = p.read_block(*n, o) {
            r.fail(format_args!("read_block({n}) returned -{} ({e}) before the hook was armed", errno_text(e)));
            r.finish(4);
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
    fault_hook(part.as_ref());
    sync_refused_probe(cache.as_ref(), part.as_ref());
    ktap::end();
    halt()
}
