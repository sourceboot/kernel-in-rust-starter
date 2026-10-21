//! Lab 10's vehicle: the heap. The prelude brings up what lab 09's did (the
//! GDT, the IDT, the frame pool, the syscall MSRs; the 8259s masked), then
//! calls `heap::init` at `HEAP_START` with `HEAP_PAGES` pages. From there
//! `Box`, `Vec`, `String` and `Arc` in this file go through the learner's
//! `#[global_allocator]`.
//!
//! `console.files.on_heap` holds far more console files open at once than
//! lab 09's pool had room for, and asks that each open file sit inside the
//! heap `init` placed: the `FileRef` became an `Arc<OpenFile>` this lab. It
//! probes the heap with one small block first, so a heap that refuses
//! everything fails the row instead of panicking inside `Arc::new`; and it
//! runs after the registered rows, so a panic it cannot head off ends the
//! run with only the deadlock row left unprinted.
//!
//! The deadlock row raises a software interrupt (`PROBE_VEC`) whose handler
//! allocates — once with the heap's lock free, once while this vehicle holds
//! it (`heap::HEAP.lock()`), once after. The handler's allocation runs
//! single-stepped (RFLAGS.TF): every instruction raises `#DB`, which no
//! interrupt mask stops, and the `#DB` handler watches the ACPI PM timer.
//! An allocation that spins on the held lock is therefore stopped after
//! `BOUND_US` and reported as the deadlock it is — `not ok`, then
//! `# sboot end` — instead of hanging the run, whether the spin has
//! interrupts on or off. The row runs last, after the registered ones and
//! the open-files row, and prints its own verdict, so a run it ends early
//! loses no other row.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../shim.rs"]
mod shim;

use alloc::boxed::Box;
use alloc::format;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::arch::asm;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use kernel::addr::{PhysAddr, VirtAddr};
use kernel::console::{FileObj, FileRef, OpenFile};
use kernel::idt::{self, TrapFrame};
use kernel::ktap;
use kernel::ktest;
use kernel::{bootinfo, frame, gdt, heap, serial, syscall};

/// Where this vehicle places the heap: in the top 512 GiB, above the kernel
/// image (`0xffffffff80000000`) and clear of the heap's default start.
const HEAP_START: u64 = 0xffff_ffff_d000_0000;
/// Room for everything `heap.box_vec` holds at once, and far less than
/// `heap.grows` asks for.
const HEAP_PAGES: usize = 16;
/// The software interrupt whose handler allocates.
const PROBE_VEC: u8 = 0x50;
/// How long an allocation from the handler may run before it is a deadlock.
const BOUND_US: u64 = 500_000;
const PM_HZ: u64 = 3_579_545;

/// Frames in use just before `heap::init`: every frame taken after it is the
/// heap's (its pages and their page tables) — this vehicle takes none itself.
static USED_BEFORE_HEAP: AtomicU64 = AtomicU64::new(0);
/// Frames `heap::init` took: its `HEAP_PAGES` pages, the private PML4 its
/// handle builds and the page tables that map them.
static INIT_FRAMES: AtomicU64 = AtomicU64::new(0);
/// What `init` may take beyond its pages: a PML4 and three levels of tables
/// below it, twice over.
const INIT_TABLES: u64 = 8;

fn used() -> u64 {
    frame::stats().used as u64
}

/// The usable memory at or above 1 MiB that this vehicle hands
/// `frame::init`: far more than any row here takes, far less than the
/// machine has (128 MiB). Each frame-count verdict drains the pool through
/// `frame::alloc`, and an allocator that searches from the bottom every time
/// takes time that grows with the square of the pool for each drain.
const POOL_BYTES: u64 = 16 << 20;
const ONE_MIB: u64 = 1 << 20;

/// `map` with its usable memory at or above 1 MiB cut to `POOL_BYTES`, in
/// address order: a region past the budget is cut short or dropped. The rest
/// of the map is passed as it is.
fn clipped(map: &[bootinfo::MemRegion], out: &mut [bootinfo::MemRegion; bootinfo::MEMMAP_MAX]) -> usize {
    let mut left = POOL_BYTES;
    let mut n = 0;
    for r in map {
        let mut r = *r;
        if r.kind == 0 && r.base + r.len > ONE_MIB {
            let low = ONE_MIB.saturating_sub(r.base).min(r.len);
            let high = (r.len - low).min(left);
            left -= high;
            r.len = low + high;
            if r.len == 0 {
                continue;
            }
        }
        if n < out.len() {
            out[n] = r;
            n += 1;
        }
    }
    n
}

/// The pool's size as `frame::stats()` first gave it at a measure point.
static TOTAL: AtomicU64 = AtomicU64::new(u64::MAX);
/// Drains so far, and what the last one found free.
static DRAINS: AtomicU64 = AtomicU64::new(0);
static LAST_DRAIN: AtomicU64 = AtomicU64::new(0);

/// What `frame::stats()` said, and what a drain of `frame::alloc` found, at
/// the first measure point where they disagreed.
#[derive(Clone, Copy)]
struct Mismatch {
    said: u64,
    found: u64,
}

/// Takes every free frame with `frame::alloc` until it returns `None`,
/// linking each through its first word by the HHDM, then frees every one;
/// the number taken. No interrupt is taken meanwhile: IF is cleared and
/// restored.
fn drain() -> u64 {
    let hh = bootinfo::hhdm_offset();
    let flags: u64;
    // SAFETY: reads RFLAGS, then clears IF; restored below.
    unsafe { asm!("pushfq", "pop {}", "cli", out(reg) flags, options(nomem)) };
    let mut head = u64::MAX;
    let mut n = 0u64;
    while let Some(f) = frame::alloc() {
        let pa = f.0 .0;
        // SAFETY: `f` was free and is now this function's; the HHDM maps it.
        unsafe { ((pa + hh) as *mut u64).write(head) };
        head = pa;
        n += 1;
    }
    while head != u64::MAX {
        // SAFETY: as above; read before the frame goes back.
        let next = unsafe { ((head + hh) as *const u64).read() };
        frame::free(frame::Frame(PhysAddr(head)));
        head = next;
    }
    if flags & (1 << 9) != 0 {
        // SAFETY: IF was set on entry.
        unsafe { asm!("sti", options(nomem, nostack)) };
    }
    n
}

/// Frames in use, as `frame::stats()` reports them at a point a verdict
/// reads, cross-checked by a drain: its free count must be what the drain
/// finds, used and free must add up to a total that never changes, and the
/// refill must leave `stats()` as it was. The first disagreement is kept in
/// `bad`.
fn used_checked(bad: &mut Option<Mismatch>) -> u64 {
    let s = frame::stats();
    let d = drain();
    let r = frame::stats();
    DRAINS.fetch_add(1, Ordering::SeqCst);
    LAST_DRAIN.store(d, Ordering::SeqCst);
    let (used, free, total) = (s.used as u64, s.free as u64, s.total as u64);
    let _ = TOTAL.compare_exchange(u64::MAX, total, Ordering::SeqCst, Ordering::SeqCst);
    let pinned = TOTAL.load(Ordering::SeqCst);
    let agree = free == d
        && used + free == total
        && total == pinned
        && r.used == s.used
        && r.free == s.free;
    if !agree && bad.is_none() {
        *bad = Some(Mismatch { said: used, found: pinned.saturating_sub(d) });
    }
    used
}

fn fail_mismatch(t: &mut ktap::TestCtx, bad: Option<Mismatch>) {
    if let Some(m) = bad {
        t.fail(format_args!(
            "frame::stats() says {} in use; a drain of frame::alloc finds {}: the frames counted in use are not the frames frame::alloc cannot hand out",
            m.said, m.found
        ));
    }
}

/// The first disagreement at `heap::init`'s two measure points, if any.
static INIT_SAID: AtomicU64 = AtomicU64::new(u64::MAX);
static INIT_FOUND: AtomicU64 = AtomicU64::new(u64::MAX);

fn pm() -> u32 {
    let v: u32;
    // SAFETY: a 32-bit read of the ACPI PM timer port; no memory is touched.
    unsafe { asm!("in eax, dx", out("eax") v, in("dx") 0x608u16, options(nomem, nostack, preserves_flags)) };
    v & 0xff_ffff
}

fn pm_us_since(start: u32) -> u64 {
    (pm().wrapping_sub(start) & 0xff_ffff) as u64 * 1_000_000 / PM_HZ
}

// ── heap.box_vec ─────────────────────────────────────────────────────────

trait Shape {
    fn area(&self) -> u64;
}
struct Square(u64);
struct Rect(u64, u64);
impl Shape for Square {
    fn area(&self) -> u64 {
        self.0 * self.0
    }
}
impl Shape for Rect {
    fn area(&self) -> u64 {
        self.0 * self.1
    }
}

const LIVE: usize = 64;
/// The reuse cycles: each holds HELD 1 KiB blocks at once, and all of them
/// together ask for about 160 MiB, ten times the frame pool (`POOL_BYTES`).
const CYCLES: u32 = 10_000;
const HELD: usize = 16;

ktest!("heap.box_vec", fn box_vec(t: &mut TestCtx) {
    // Two blocks taken before either is written: an allocator that hands
    // out memory it still counts as free overlaps them, and the first write
    // into such a block would wreck its own records — so this looks first,
    // and stops here if they overlap.
    let l = Layout::from_size_align(64, 16).expect("64 bytes, 16-aligned");
    // SAFETY: a valid, non-zero-sized layout; neither block is written.
    let (p1, p2) = unsafe { (alloc::alloc::alloc(l) as u64, alloc::alloc::alloc(l) as u64) };
    t.note(format_args!("heap.box_vec: two 64-byte blocks, nothing written to either: {p1:#x} and {p2:#x}"));
    if p1 == 0 || p2 == 0 || (p1 < p2 + 64 && p2 < p1 + 64) {
        t.fail(format_args!("two live 64-byte blocks came back at {p1:#x} and {p2:#x}: null, or overlapping"));
        return;
    }
    // SAFETY: each came from `alloc` with this layout and is freed once.
    unsafe {
        alloc::alloc::dealloc(p1 as *mut u8, l);
        alloc::alloc::dealloc(p2 as *mut u8, l);
    }

    // Alignment above the heap's 16-byte unit (added 2026-10-17, TL review of
    // slice 1c): `GlobalAlloc` must honour `Layout::align()`, and the requests
    // above ask for 8 and 16 only. Three live 40-byte blocks per alignment:
    // blocks laid end to end at the 16-byte unit are 48 bytes apart, so no
    // placement makes all three 64-aligned, let alone 512 or 4096.
    let mut misaligned = 0u64;
    for align in [64usize, 512, 4096] {
        let la = Layout::from_size_align(40, align).expect("40 bytes, a power-of-two alignment");
        // SAFETY: a valid, non-zero-sized layout; no block is written.
        let ps = unsafe { [alloc::alloc::alloc(la), alloc::alloc::alloc(la), alloc::alloc::alloc(la)] };
        let bad = ps.iter().filter(|p| p.is_null() || (**p as usize) % align != 0).count() as u64;
        t.note(format_args!(
            "heap.box_vec: three 40-byte blocks aligned to {align}: {:#x} {:#x} {:#x}; {bad} null or misaligned",
            ps[0] as u64, ps[1] as u64, ps[2] as u64
        ));
        misaligned += bad;
        for p in ps.into_iter().filter(|p| !p.is_null()) {
            // SAFETY: each came from `alloc` with this layout and is freed once.
            unsafe { alloc::alloc::dealloc(p, la) };
        }
    }
    t.expect_eq("blocks of alignment 64, 512 and 4096 that came back null or misaligned", misaligned, 0);

    // One Box, read back, where init put the heap.
    let b = Box::new(0x5eed_cafe_u64);
    let at = &*b as *const u64 as u64;
    t.note(format_args!("heap.box_vec: Box::new(u64) placed it at {at:#x}; heap::init was given {HEAP_START:#x}"));
    t.expect_eq("the boxed u64 read back", *b, 0x5eed_cafe);
    t.expect(at >= HEAP_START && at - HEAP_START < 256 << 20, "the Box lies in the 256 MiB above heap::init's start");

    // Many blocks alive at once: each keeps its own contents, none overlaps another.
    let mut live: Vec<Box<[u64; 8]>> = Vec::new();
    for i in 0..LIVE as u64 {
        live.push(Box::new([i.wrapping_mul(0x0101_0101_0101_0101) ^ 0xa5a5; 8]));
    }
    let changed = live
        .iter()
        .enumerate()
        .filter(|(i, b)| b.iter().any(|&w| w != (*i as u64).wrapping_mul(0x0101_0101_0101_0101) ^ 0xa5a5))
        .count() as u64;
    let mut addrs = [0u64; LIVE];
    for (a, b) in addrs.iter_mut().zip(live.iter()) {
        *a = b.as_ptr() as u64;
    }
    addrs.sort_unstable();
    let overlaps = addrs.windows(2).filter(|w| w[0] + 64 > w[1]).count() as u64;
    t.note(format_args!(
        "heap.box_vec: {LIVE} live Box<[u64; 8]> span {:#x}..{:#x}; {overlaps} overlapping pairs, {changed} with changed contents",
        addrs[0], addrs[LIVE - 1] + 64
    ));
    t.expect_eq("pairs of live 64-byte blocks that overlap", overlaps, 0);
    t.expect_eq("live blocks whose contents another allocation changed", changed, 0);
    drop(live);

    // A Vec that reallocates as it grows.
    let mut v: Vec<u64> = Vec::new();
    for i in 0..3000u64 {
        v.push(i * 3);
    }
    let sum: u64 = v.iter().sum();
    t.expect_eq("the sum of a 3000-element Vec pushed one by one", sum, 3 * 2999 * 3000 / 2);

    // Trait objects on the heap, a String, an Arc's count.
    let shapes: Vec<Box<dyn Shape>> = vec![Box::new(Square(7)), Box::new(Rect(3, 5))];
    let area: u64 = shapes.iter().map(|s| s.area()).sum();
    t.expect_eq("Box<dyn Shape> areas through the vtable (7*7 + 3*5)", area, 64);
    let s = format!("{}-{}", "heap", 10);
    t.expect(s == "heap-10", "format! built \"heap-10\"");
    let a = Arc::new(41u64);
    let a2 = Arc::clone(&a);
    t.expect_eq("Arc::strong_count after one clone", Arc::strong_count(&a) as u64, 2);
    drop(a2);
    t.expect_eq("Arc::strong_count after dropping the clone", Arc::strong_count(&a) as u64, 1);

    // Freed memory comes back. Each of CYCLES cycles holds HELD 1 KiB blocks
    // and a 16-byte block at once, marks the first and last byte of each and
    // reads every mark back, then frees the 1 KiB blocks in a scattered order
    // and the 16-byte one last. A pin block taken after each cycle's blocks
    // stays live until the next cycle's blocks are freed, so a cycle's blocks
    // are never at the top of a bump pointer when they are freed; and HELD
    // blocks of one size are freed together, so a fixed handful of saved
    // blocks cannot stand in for a free list. The
    // cycles ask for far more memory than the frame pool holds: a heap that does
    // not hand freed blocks out again takes frames for every cycle, or runs
    // out. The first cycle may grow the heap; the frames are counted after it.
    let lx = Layout::from_size_align(1024, 8).expect("1 KiB, 8-aligned");
    let ly = Layout::from_size_align(16, 8).expect("16 bytes, 8-aligned");
    let lg = Layout::from_size_align(64, 8).expect("64 bytes, 8-aligned");
    let mut xs = [core::ptr::null_mut::<u8>(); HELD];
    let mut pin = core::ptr::null_mut::<u8>();
    let mut lost = 0u64;
    let mut bad = None;
    let mut before = used();
    let (mut top_x, mut top_y) = (0u64, 0u64);
    let (mut back_x, mut back_y) = (0u64, 0u64);
    let (mut lo, mut hi) = (u64::MAX, 0u64);
    let mut refused_at = 0u32;
    let mut stopped_at = 0u32;
    'cycles: for i in 0..CYCLES {
        let mut n = 0;
        while n < HELD {
            // SAFETY: a valid, non-zero-sized layout; the block is marked and
            // freed once below.
            let p = core::hint::black_box(unsafe { alloc::alloc::alloc(lx) });
            if p.is_null() {
                refused_at = i + 1;
                for &q in &xs[..n] {
                    // SAFETY: taken above with `lx`, freed once.
                    unsafe { alloc::alloc::dealloc(q, lx) };
                }
                break 'cycles;
            }
            let a = p as u64;
            if a <= top_x && top_x != 0 {
                back_x += 1;
            }
            top_x = top_x.max(a);
            lo = lo.min(a);
            hi = hi.max(a + 1024);
            xs[n] = p;
            n += 1;
        }
        // SAFETY: as above.
        let y = core::hint::black_box(unsafe { alloc::alloc::alloc(ly) });
        if y.is_null() {
            refused_at = i + 1;
            for &q in &xs {
                // SAFETY: taken above with `lx`, freed once.
                unsafe { alloc::alloc::dealloc(q, lx) };
            }
            break 'cycles;
        }
        let a = y as u64;
        if a <= top_y && top_y != 0 {
            back_y += 1;
        }
        top_y = top_y.max(a);
        // Every block of this cycle is live: mark the first and last byte of
        // each with its own value, then read every mark back.
        for (k, &q) in xs.iter().enumerate() {
            // SAFETY: `q` is a live 1024-byte block taken this cycle.
            unsafe { q.write(k as u8 + 1); q.add(1023).write(k as u8 + 1) };
        }
        // SAFETY: `y` is a live 16-byte block.
        unsafe { y.write(0xee); y.add(15).write(0xee) };
        for (k, &q) in xs.iter().enumerate() {
            // SAFETY: as above.
            if unsafe { q.read() != k as u8 + 1 || q.add(1023).read() != k as u8 + 1 } {
                lost += 1;
            }
        }
        // SAFETY: as above.
        if unsafe { y.read() != 0xee || y.add(15).read() != 0xee } {
            lost += 1;
        }
        // SAFETY: as above; freed once, after the next cycle's blocks or after
        // the loop.
        let next_pin = core::hint::black_box(unsafe { alloc::alloc::alloc(lg) });
        if next_pin.is_null() {
            refused_at = i + 1;
            for &q in &xs {
                // SAFETY: taken above with `lx`, freed once.
                unsafe { alloc::alloc::dealloc(q, lx) };
            }
            // SAFETY: taken this cycle with `ly`, freed once.
            unsafe { alloc::alloc::dealloc(y, ly) };
            break 'cycles;
        }
        if i == 0 {
            before = used_checked(&mut bad);
        }
        for j in 0..HELD {
            // SAFETY: each `xs[k]` was taken this cycle with `lx`; k runs over
            // every index once (7 and HELD share no factor).
            unsafe { alloc::alloc::dealloc(xs[(j * 7 + 3) % HELD], lx) };
        }
        // SAFETY: taken this cycle with `ly`.
        unsafe { alloc::alloc::dealloc(y, ly) };
        if !pin.is_null() {
            // SAFETY: the last cycle's pin, taken with `lg`, freed once.
            unsafe { alloc::alloc::dealloc(pin, lg) };
        }
        pin = next_pin;
        // A heap that is leaking stops here, before it takes the frame pool
        // and the rows after this one with it.
        if i > 0 && used().saturating_sub(before) > 1 {
            stopped_at = i + 1;
            break;
        }
    }
    if !pin.is_null() {
        // SAFETY: the last cycle's pin, taken with `lg`, freed once.
        unsafe { alloc::alloc::dealloc(pin, lg) };
    }
    let moved = used_checked(&mut bad).saturating_sub(before);
    fail_mismatch(t, bad);
    t.note(format_args!(
        "heap.box_vec: {CYCLES} cycles of {HELD} live 1 KiB blocks and one 16-byte block, freed in a scattered order under a pin block, took {moved} frames after the first; {lost} lost a byte written to them while live; the 1 KiB blocks lay in {lo:#x}..{hi:#x}; {back_x} 1 KiB and {back_y} 16-byte blocks came back at or below an earlier one"
    ));
    if lost != 0 {
        t.fail(format_args!(
            "{lost} blocks lost a byte written to them while every block of their cycle was live: two live blocks overlap"
        ));
    }
    if refused_at != 0 {
        t.fail(format_args!(
            "the heap refused a block in cycle {refused_at} of {CYCLES}, each of which frees everything it takes: a freed block is not handed out again"
        ));
    }
    if moved > 1 {
        t.fail(format_args!(
            "cycles that each free everything they take took {moved} frames after the first (stopped at cycle {stopped_at} of {CYCLES}): a freed block is not handed out again"
        ));
    }
    for (back, what) in [(back_x, "1 KiB"), (back_y, "16-byte")] {
        if refused_at == 0 && stopped_at == 0 && back == 0 {
            t.fail(format_args!(
                "in {CYCLES} cycles every {what} block came back above all the earlier ones: a freed block is not handed out again"
            ));
        }
    }
});

// ── heap.grows ───────────────────────────────────────────────────────────

ktest!("heap.grows", fn grows(t: &mut TestCtx) {
    // [128 KiB, 384 KiB), byte-granular, chosen per run: past anything the
    // other row leaves in the heap, so the heap must take frames for it.
    let size = 128 * 1024 + t.pick("heap.grows", 256 * 1024) as usize;
    let mut bad = match (INIT_SAID.load(Ordering::SeqCst), INIT_FOUND.load(Ordering::SeqCst)) {
        (u64::MAX, _) => None,
        (said, found) => Some(Mismatch { said, found }),
    };
    let held = used_checked(&mut bad).saturating_sub(USED_BEFORE_HEAP.load(Ordering::SeqCst));
    // `init` maps the pages it is given, no more: a heap that maps a large
    // region up front never has to grow, and its `held` would hide that.
    let init_took = INIT_FRAMES.load(Ordering::SeqCst);
    t.note(format_args!("heap.grows: heap::init was asked for {HEAP_PAGES} pages and took {init_took} frames"));
    if init_took > HEAP_PAGES as u64 + INIT_TABLES {
        t.fail(format_args!(
            "heap::init was asked for {HEAP_PAGES} pages and took {init_took} frames: init maps the pages it is given, and the heap takes more only when a request does not fit"
        ));
    }
    // The heap holds at most `held` pages, so a block this size needs at
    // least this many frames more.
    let need = (size as u64).div_ceil(4096).saturating_sub(held);
    let before = used_checked(&mut bad);
    let mut v: Vec<u8> = Vec::new();
    if v.try_reserve_exact(size).is_err() {
        t.fail(format_args!(
            "an allocation of {size} bytes was refused; the heap holds at most {held} frames and took {} more trying",
            used().saturating_sub(before)
        ));
        fail_mismatch(t, bad);
        return;
    }
    let grew = used_checked(&mut bad).saturating_sub(before);
    t.note(format_args!(
        "heap.grows: frame::stats() was checked against a drain of frame::alloc at {} points; the last found {} free",
        DRAINS.load(Ordering::SeqCst),
        LAST_DRAIN.load(Ordering::SeqCst)
    ));
    fail_mismatch(t, bad);
    let at = v.as_ptr() as u64;
    let seed = t.derived("heap.grows/fill") | 1;
    let byte = |i: usize| (seed.wrapping_mul(i as u64 + 1) >> 56) as u8;
    for i in 0..size {
        v.push(byte(i));
    }
    let wrong = v.iter().enumerate().filter(|&(i, &b)| b != byte(i)).count();
    t.note(format_args!(
        "heap.grows: {size} bytes at {at:#x}..{:#x}; the heap held at most {held} frames, and took {grew} more (at least {need} needed); {wrong} bytes read back wrong",
        at + size as u64
    ));
    if grew < need {
        t.fail(format_args!(
            "a {size}-byte block from a heap of at most {held} pages took {grew} frames; it needs at least {need}"
        ));
    }
    t.expect_eq("bytes of the grown block that read back wrong", wrong as u64, 0);
});

// ── console.files.on_heap ────────────────────────────────────────────────

/// Open files held at once: four times lab 09's pool, far below any limit a
/// later lab sets on the console (it sets none).
const OPEN_FILES: usize = 64;
/// Linux's O_RDWR (asm-generic/fcntl.h): the console opens read-write.
const O_RDWR: u32 = 2;

const FILES_ID: &str = "console.files.on_heap";

/// `console.files.on_heap` (plain): the KTAP number `n`, after every
/// registered row and before the deadlock row. It is not registered, so it
/// runs there by construction: an `Arc::new` or a console open that panics
/// can end the run, and from here that costs no other row but the last.
fn files_on_heap(n: usize) {
    let mut t = Row { failed: false };
    // Lab 09's pool held a fixed number of open files and refused the next
    // with -ENFILE; from lab 10 a `FileRef` is an `Arc<OpenFile>`, so the
    // open file lives on the heap and only the heap limits how many there are.
    //
    // `Arc::new` panics when the heap refuses, and a panic ends the run: a
    // heap that refuses a small block fails `heap.box_vec` already, so this
    // row stops instead of opening anything and the deadlock row still runs.
    let probe = Layout::from_size_align(64, 16).expect("64 bytes, 16-aligned");
    // SAFETY: a valid, non-zero-sized layout; the block is freed unwritten.
    // `black_box` keeps the optimiser from deleting an allocation it can see
    // is freed unused, which would make the probe never ask the heap at all.
    let p = core::hint::black_box(unsafe { alloc::alloc::alloc(probe) });
    if p.is_null() {
        t.fail(format_args!("console.files.on_heap: the heap refused a 64-byte block, so no file was opened"));
        kernel::kprintln!("not ok {n} - {FILES_ID}");
        return;
    }
    // SAFETY: `p` came from `alloc` with this layout and is freed once.
    unsafe { alloc::alloc::dealloc(p, probe) };
    let lo = HEAP_START;
    let hi = HEAP_START + (256 << 20);
    let mut held: Vec<FileRef> = Vec::new();
    let mut refused = 0u64;
    let mut first_refused = 0usize;
    for i in 0..OPEN_FILES {
        match FileRef::open(FileObj::CONSOLE, O_RDWR) {
            Ok(f) => held.push(f),
            Err(e) => {
                if refused == 0 {
                    first_refused = i + 1;
                    t.note(format_args!("console.files.on_heap: open number {} on the console returned error {e}", i + 1));
                }
                refused += 1;
            }
        }
    }
    let outside = held
        .iter()
        .filter(|f| {
            let a = &***f as *const OpenFile as u64;
            a < lo || a >= hi
        })
        .count() as u64;
    let not_one = held.iter().filter(|f| f.refs() != 1).count() as u64;
    let first = held.first().map_or(0, |f| &**f as *const OpenFile as u64);
    t.note(format_args!(
        "console.files.on_heap: {OPEN_FILES} console files opened and held: {} open, {refused} refused, {outside} outside the heap (the first at {first:#x}), {not_one} with refs() other than 1",
        held.len()
    ));
    if refused > 0 {
        t.fail(format_args!(
            "console.files.on_heap: open file number {first_refused} was refused while {} were held: FileRef::open still takes its open file from a fixed pool instead of the heap (Arc::new)",
            held.len()
        ));
    }
    if outside > 0 {
        t.fail(format_args!(
            "console.files.on_heap: {outside} open files live outside the heap init placed: the OpenFile behind a FileRef is not allocated with Arc::new"
        ));
    }
    t.expect_eq("open files held once each whose refs() is not 1", not_one, 0);

    // One more reference through clone, then back.
    if let Some(f) = held.first() {
        let c = f.clone();
        let (two, same) = (f.refs(), core::ptr::eq(&*c, &**f));
        drop(c);
        let one = f.refs();
        t.note(format_args!("console.files.on_heap: refs() after one clone {two}, after dropping it {one}; the clone shares the open file: {same}"));
        t.expect_eq("refs() after one clone", two as u64, 2);
        t.expect_eq("refs() after the clone dropped", one as u64, 1);
        t.expect(same, "a cloned FileRef reaches the same open file");
    }

    // Every file closed: the same number open again.
    drop(held);
    let mut again = 0u64;
    let mut reopened: Vec<FileRef> = Vec::new();
    for _ in 0..OPEN_FILES {
        if let Ok(f) = FileRef::open(FileObj::CONSOLE, O_RDWR) {
            reopened.push(f);
            again += 1;
        }
    }
    drop(reopened);
    t.note(format_args!("console.files.on_heap: after closing every one, {again} of {OPEN_FILES} opened again"));
    t.expect_eq("console files that opened again after every one was closed", again, OPEN_FILES as u64);

    if t.failed {
        kernel::kprintln!("not ok {n} - {FILES_ID}");
    } else {
        kernel::kprintln!("ok {n} - {FILES_ID}");
    }
}

// ── heap.irq.deadlock.named ──────────────────────────────────────────────

/// The handler's result: the pointer its allocation returned.
static PROBE_PTR: AtomicU64 = AtomicU64::new(0);
static STEPPING: AtomicBool = AtomicBool::new(false);
static STEPS: AtomicU64 = AtomicU64::new(0);
static PM_START: AtomicU64 = AtomicU64::new(0);
static ELAPSED_US: AtomicU64 = AtomicU64::new(0);
/// The row's KTAP number, for the line the `#DB` handler prints.
static ROW_N: AtomicU64 = AtomicU64::new(0);

const DEADLOCK_ID: &str = "heap.irq.deadlock.named";

fn probe_layout() -> Layout {
    Layout::from_size_align(64, 8).expect("64 bytes, 8-aligned")
}

/// `PROBE_VEC`'s handler: one 64-byte allocation, single-stepped. The block
/// is freed by the thread after it lets go of the lock, never here.
fn on_probe(_tf: &mut TrapFrame) {
    STEPS.store(0, Ordering::SeqCst);
    PM_START.store(pm() as u64, Ordering::SeqCst);
    STEPPING.store(true, Ordering::SeqCst);
    // SAFETY: sets RFLAGS.TF; the #DB handler counts and watches each step.
    unsafe { asm!("pushfq", "or qword ptr [rsp], 0x100", "popfq") };
    // SAFETY: a valid, non-zero-sized layout.
    let p = unsafe { alloc::alloc::alloc(probe_layout()) };
    // SAFETY: clears RFLAGS.TF.
    unsafe { asm!("pushfq", "and qword ptr [rsp], ~0x100", "popfq") };
    STEPPING.store(false, Ordering::SeqCst);
    ELAPSED_US.store(pm_us_since(PM_START.load(Ordering::SeqCst) as u32), Ordering::SeqCst);
    PROBE_PTR.store(p as u64, Ordering::SeqCst);
}

/// `#DB`: one single-stepped instruction of the handler's allocation.
fn on_db(tf: &mut TrapFrame) {
    if !STEPPING.load(Ordering::SeqCst) {
        tf.rflags &= !0x100;
        return;
    }
    let n = STEPS.fetch_add(1, Ordering::SeqCst) + 1;
    if n % 64 != 0 {
        return;
    }
    let us = pm_us_since(PM_START.load(Ordering::SeqCst) as u32);
    if us < BOUND_US {
        return;
    }
    STEPPING.store(false, Ordering::SeqCst);
    ktap::note(format_args!(
        "{DEADLOCK_ID}: deadlock — an allocation from an interrupt handler, made while the interrupted code held the heap's lock, had not returned after {} ms ({n} instructions, still running at rip={:#x}): it waits for a lock only the code it interrupted can release",
        us / 1000,
        tf.rip
    ));
    kernel::kprintln!("not ok {} - {DEADLOCK_ID}", ROW_N.load(Ordering::SeqCst));
    ktap::end();
    kernel::panic::halt()
}

/// Raise `PROBE_VEC`; returns (pointer, instructions, µs).
fn raise() -> (u64, u64, u64) {
    PROBE_PTR.store(u64::MAX, Ordering::SeqCst);
    // SAFETY: the gate's handler is `on_probe`, installed by the prelude.
    unsafe { asm!("int {v}", v = const PROBE_VEC) };
    (PROBE_PTR.load(Ordering::SeqCst), STEPS.load(Ordering::SeqCst), ELAPSED_US.load(Ordering::SeqCst))
}

/// RFLAGS.IF, read where the caller stands.
fn irq_enabled() -> bool {
    let rflags: u64;
    // SAFETY: reads RFLAGS through the stack; changes nothing.
    unsafe { asm!("pushfq", "pop {}", out(reg) rflags, options(nomem, preserves_flags)) };
    rflags & (1 << 9) != 0
}

fn free_probe(p: u64) {
    if p != 0 && p != u64::MAX {
        // SAFETY: `p` came from `alloc` with this layout and is freed once.
        unsafe { alloc::alloc::dealloc(p as *mut u8, probe_layout()) };
    }
}

/// An unregistered row's evidence and verdict, printed by this vehicle: the
/// open-files and deadlock rows run after the registered ones (`_start`).
struct Row {
    failed: bool,
}

impl Row {
    fn note(&self, args: core::fmt::Arguments) {
        ktap::note(args);
    }
    fn fail(&mut self, args: core::fmt::Arguments) {
        self.failed = true;
        ktap::note(args);
    }
    fn expect(&mut self, cond: bool, what: &str) {
        if !cond {
            self.fail(format_args!("{what}: observed false, expected true"));
        }
    }
    fn expect_eq(&mut self, what: &str, observed: u64, expected: u64) {
        if observed != expected {
            self.fail(format_args!("{what}: observed {observed:#x}, expected {expected:#x}"));
        }
    }
}

/// `heap.irq.deadlock.named` (plain): the KTAP number `n` is the last.
fn irq_deadlock(n: usize) {
    ROW_N.store(n as u64, Ordering::SeqCst);
    let mut t = Row { failed: false };

    // The lock free: the handler allocates like anyone else.
    let (p, steps, us) = raise();
    t.note(format_args!("{DEADLOCK_ID}: the lock free, the handler's allocation returned {p:#x} after {steps} instructions ({us} us)"));
    if p == 0 {
        t.fail(format_args!("{DEADLOCK_ID}: an allocation from an interrupt handler, the heap's lock free, returned null"));
    }
    free_probe(p);

    // The lock held by the code the interrupt stops — taken with interrupts
    // ON (the 8259s stay masked, so none arrives), because the boot left IF
    // clear and a `lock` that never clears it would otherwise go unseen.
    let if_was_on = irq_enabled();
    // SAFETY: every 8259 line is masked (prelude); `int` ignores IF.
    unsafe { asm!("sti", options(nomem, nostack)) };
    match heap::HEAP.lock() {
        None => t.fail(format_args!("{DEADLOCK_ID}: heap::HEAP.lock() refused the course's test in thread context, with nothing holding the lock")),
        Some(g) => {
            let held_if = irq_enabled();
            let (p, steps, us) = raise();
            drop(g);
            let after_if = irq_enabled();
            t.note(format_args!(
                "{DEADLOCK_ID}: RFLAGS.IF before heap::HEAP.lock() 1, while the lock was held {}, after the guard dropped {}",
                held_if as u8, after_if as u8
            ));
            if held_if {
                t.fail(format_args!(
                    "{DEADLOCK_ID}: heap::HEAP.lock() handed back the lock with interrupts on: an interrupt that allocates can land while it is held, and its allocation is refused (or, with a waiting lock, never returns); `lock` must clear IF and dropping the guard restore it"
                ));
            }
            if !after_if {
                t.fail(format_args!(
                    "{DEADLOCK_ID}: interrupts were on before heap::HEAP.lock() and still off after the guard dropped: dropping the guard must restore IF to what `lock` found"
                ));
            }
            if p == 0 {
                t.note(format_args!(
                    "{DEADLOCK_ID}: deadlock named — the lock held by the interrupted code, the handler's allocation was refused after {steps} instructions ({us} us) instead of waiting"
                ));
            } else {
                t.fail(format_args!(
                    "{DEADLOCK_ID}: the handler's allocation returned {p:#x} while the code it interrupted held the heap's lock: two allocations were inside the heap at once"
                ));
                free_probe(p);
            }
        }
    }
    if !if_was_on {
        // SAFETY: back to the state the boot left (IF clear).
        unsafe { asm!("cli", options(nomem, nostack)) };
    }

    // The lock free again: the heap serves the handler again.
    let (p, steps, _) = raise();
    t.note(format_args!("{DEADLOCK_ID}: the lock released, the handler's allocation returned {p:#x} after {steps} instructions"));
    if p == 0 {
        t.fail(format_args!("{DEADLOCK_ID}: an allocation from an interrupt handler after the lock was released returned null"));
    }
    free_probe(p);

    if t.failed {
        kernel::kprintln!("not ok {n} - {DEADLOCK_ID}");
    } else {
        kernel::kprintln!("ok {n} - {DEADLOCK_ID}");
    }
}

fn prelude() {
    let (k, i, _) = shim::stacks();
    gdt::init(k, i);
    idt::init();
    // The 8259s masked: no IRQ in this run; the probe's interrupt is `int`.
    serial::out8(0x21, 0xff);
    serial::out8(0xa1, 0xff);
    let bi = bootinfo::get().expect("the shim published the BootInfo");
    let mut map = [bi.memmap()[0]; bootinfo::MEMMAP_MAX];
    let n = clipped(bi.memmap(), &mut map);
    frame::init(&map[..n], bi.kernel_range());
    syscall::init();
    syscall::set_kernel_stack(k);
    idt::set_handler(1, on_db, 0);
    idt::set_handler(PROBE_VEC, on_probe, 0);
    let mut bad = None;
    USED_BEFORE_HEAP.store(used_checked(&mut bad), Ordering::SeqCst);
    heap::init(VirtAddr(HEAP_START), HEAP_PAGES);
    INIT_FRAMES.store(
        used_checked(&mut bad).saturating_sub(USED_BEFORE_HEAP.load(Ordering::SeqCst)),
        Ordering::SeqCst,
    );
    if let Some(m) = bad {
        INIT_SAID.store(m.said, Ordering::SeqCst);
        INIT_FOUND.store(m.found, Ordering::SeqCst);
    }
}

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    serial::init();
    // N counts the registered rows plus the two that run after them: the
    // open-files row, then the deadlock row, always last.
    let n = ktap::tests().len() + 2;
    ktap::begin(n);
    prelude();
    ktap::run_all();
    files_on_heap(n - 1);
    irq_deadlock(n);
    ktap::end();
    kernel::hcf()
}
