//! Lab 07's vehicle: the frame allocator. The prelude hands `frame::init` the
//! BootInfo's memory map and kernel range. Two rows share one survey: it
//! allocates until the pool is empty, checks every frame it got against the
//! memory map, dirties each one, and frees them all — so a later `alloc`
//! that does not zero shows up in `frames.alloc.count`. `frames.kernel_image`
//! also holds the drain to the pool's own counts: it must hand out exactly the
//! free count `stats()` reported, and at most a sixteenth of the total may be
//! held back (the count array's frames) — a pool that keeps a few dozen
//! frames and reports the map's recount as its total is refused (sk07-forger-1,
//! 2026-10-18). The survey drains while it holds one frame, after freeing
//! another, so a pool that reuses a freed frame only once nothing is held
//! comes up short (sk07-forger-5, 2026-10-18). It then gives back the lower
//! half of what it took, by address, while the rest is still held, and drains
//! again: every one must come back, so a pool that recycles freed frames
//! through a small cache is refused (sk07-forger-6, 2026-10-18). A cache of
//! half the pool or more is the catalogued residual: at that size it is a
//! working free list for everything this course asks of it.

#![no_std]
#![no_main]

#[path = "../shim.rs"]
mod shim;

use kernel::addr::{PhysAddr, PAGE_SIZE};
use kernel::bootinfo::{self, BootInfo};
use kernel::frame::{self, Frame};
use kernel::ktest;
use kernel::sync::SpinLock;

const ONE_MIB: u64 = 0x10_0000;
/// Frames the survey can track: physical memory below 256 MiB.
const TRACK: usize = 1 << 16;

fn bi() -> &'static BootInfo {
    bootinfo::get().expect("the shim published the BootInfo")
}

fn ptr(f: Frame) -> *mut u64 {
    (f.0 .0 + bootinfo::hhdm_offset()) as *mut u64
}

fn kind_at(pa: u64) -> Option<u32> {
    bi().memmap().iter().find(|r| pa >= r.base && pa < r.base + r.len).map(|r| r.kind)
}

#[derive(Clone, Copy)]
struct Survey {
    got: u64,
    below_1m: u64,
    in_kernel: u64,
    not_usable: u64,
    untracked: u64,
    dups: u64,
    free_before: u64,
    free_after: u64,
    total: u64,
    redrain: u64,
    half: u64,
}

static SEEN: SpinLock<[u64; TRACK / 64]> = SpinLock::new([0; TRACK / 64]);
static SURVEY: SpinLock<Option<Survey>> = SpinLock::new(None);

fn survey() -> Survey {
    let mut g = SURVEY.lock();
    if let Some(s) = *g {
        return s;
    }
    let k = bi().kernel_range();
    let mut seen = SEEN.lock();
    // Hold one frame across a freed one, so the drain sees whether a frame
    // freed while another is held goes back to the pool.
    let held = frame::alloc();
    if let Some(a) = frame::alloc() { frame::free(a); }
    let mut s = Survey { got: 0, below_1m: 0, in_kernel: 0, not_usable: 0, untracked: 0, dups: 0, free_before: frame::stats().free as u64, free_after: 0, redrain: 0, half: 0, total: frame::stats().total as u64 };
    while let Some(f) = frame::alloc() {
        let pa = f.0 .0;
        s.got += 1;
        if pa < ONE_MIB {
            s.below_1m += 1;
        }
        if k.contains(f.0) {
            s.in_kernel += 1;
        }
        if kind_at(pa) != Some(0) || kind_at(pa + PAGE_SIZE - 1) != Some(0) {
            s.not_usable += 1;
        }
        let i = (pa / PAGE_SIZE) as usize;
        if i >= TRACK {
            s.untracked += 1;
            continue;
        }
        if seen[i / 64] & 1 << (i % 64) != 0 {
            s.dups += 1;
        }
        seen[i / 64] |= 1 << (i % 64);
        if s.below_1m + s.in_kernel + s.not_usable == 0 {
            // SAFETY: a frame the allocator just handed us, below 256 MiB,
            // reached through the HHDM: ours to write until we free it.
            unsafe {
                ptr(f).write_volatile(0x5b00_7b00_dead_f00d);
                ptr(f).add(511).write_volatile(0x5b00_7b00_dead_f00d);
            }
        }
    }
    // Give back the lower half of what the drain took (by address) while
    // `held` and the upper half are still out, then drain again: every one of
    // them, freed while frames were held, must come back.
    let mut left = s.got / 2;
    s.half = left;
    for (w, word) in seen.iter_mut().enumerate() {
        let mut bits = *word;
        while bits != 0 && left > 0 {
            let b = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            *word &= !(1u64 << b);
            left -= 1;
            frame::free(Frame(PhysAddr(((w * 64 + b) as u64) * PAGE_SIZE)));
        }
        if left == 0 {
            break;
        }
    }
    while let Some(f) = frame::alloc() {
        s.redrain += 1;
        let i = (f.0 .0 / PAGE_SIZE) as usize;
        if i < TRACK {
            seen[i / 64] |= 1 << (i % 64);
        }
    }
    for (w, word) in seen.iter_mut().enumerate() {
        let mut bits = *word;
        while bits != 0 {
            let b = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            frame::free(Frame(PhysAddr(((w * 64 + b) as u64) * PAGE_SIZE)));
        }
        *word = 0;
    }
    s.free_after = frame::stats().free as u64;
    if let Some(h) = held { frame::free(h); }
    kernel::ktap::note(format_args!(
        "frames survey: {} frames allocated before the pool ran dry; {} below 1 MiB, {} inside the kernel image, {} outside usable memory, {} handed out twice",
        s.got, s.below_1m, s.in_kernel, s.not_usable, s.dups
    ));
    *g = Some(s);
    s
}

ktest!("frames.total", fn frames_total(t: &mut TestCtx) {
    let bi = bi();
    let k = bi.kernel_range();
    let mut want = 0u64;
    let mut kinds = [0u32; 8];
    for r in bi.memmap() {
        kinds[(r.kind as usize).min(7)] += 1;
        if r.kind != 0 {
            continue;
        }
        let s = PhysAddr(r.base.max(ONE_MIB)).align_up().0;
        let e = PhysAddr(r.base + r.len).align_down().0;
        let mut pa = s;
        while pa < e {
            if !k.contains(PhysAddr(pa)) {
                want += 1;
            }
            pa += PAGE_SIZE;
        }
    }
    t.note(format_args!("memmap: {} entries; by kind 0..7: {:?}; kernel image [{:#x}, {:#x})", bi.memmap().len(), kinds, k.start.0, k.end.0));
    let st = frame::stats();
    t.note(format_args!("frames.total: the pool says {}, the memory map says {}", st.total, want));
    t.expect_eq("total frames (usable, at or above 1 MiB, outside the kernel image)", st.total as u64, want);
    t.expect_eq("free + used", (st.free + st.used) as u64, st.total as u64);
});

ktest!("frames.below_1m", fn frames_below_1m(t: &mut TestCtx) {
    let s = survey();
    t.expect(s.got > 0, "the pool hands out frames");
    t.expect_eq("frames handed out below 1 MiB", s.below_1m, 0);
    t.expect_eq("frames handed out twice", s.dups, 0);
    t.expect_eq("frames handed out above 256 MiB on a 128 MiB machine", s.untracked, 0);
});

ktest!("frames.kernel_image", fn frames_kernel_image(t: &mut TestCtx) {
    let s = survey();
    t.expect(s.got > 0, "the pool hands out frames");
    t.expect_eq("frames handed out inside the kernel image", s.in_kernel, 0);
    t.expect_eq("frames handed out outside usable memory", s.not_usable, 0);
    t.expect_eq("free frames after every surveyed frame was freed", s.free_after, s.free_before);
    t.expect_eq("frames the survey drained, against the free count it started from", s.got, s.free_before);
    t.expect_eq("frames a second drain got back, of the half given back while the rest was held", s.redrain, s.half);
    t.expect(s.got >= s.total - s.total / 16, "the survey drained the pool: at most a sixteenth of the total held back");
});

ktest!("frames.alloc.count", fn frames_alloc_count(t: &mut TestCtx) {
    survey();
    let n = 8 + t.pick("frames.alloc.count", 56) as usize;
    let before = frame::stats();
    let mut got = [Frame(PhysAddr(0)); 64];
    let mut k = 0;
    let mut dirty = 0;
    let mut not_one = 0;
    while k < n {
        let Some(f) = frame::alloc() else { break };
        got[k] = f;
        k += 1;
        if frame::refs(f) != 1 {
            not_one += 1;
        }
        // SAFETY: a frame just handed to us, reached through the HHDM.
        if (0..512).any(|w| unsafe { ptr(f).add(w).read_volatile() } != 0) {
            dirty += 1;
        }
    }
    let mut dups = 0;
    for i in 0..k {
        for j in 0..i {
            if got[i] == got[j] {
                dups += 1;
            }
        }
    }
    let mid = frame::stats();
    for f in &got[..k] {
        frame::free(*f);
    }
    t.note(format_args!("frames.alloc.count: {n} allocations (chosen by this run's nonce); used went {} -> {}", before.used, mid.used));
    t.expect_eq("frames allocated", k as u64, n as u64);
    t.expect_eq("frames not zeroed on alloc", dirty, 0);
    t.expect_eq("frames whose count is not 1 after alloc", not_one, 0);
    t.expect_eq("the same frame handed out twice", dups, 0);
    t.expect_eq("used after the allocations", mid.used as u64, (before.used + n) as u64);
});

ktest!("frames.free.drop", fn frames_free_drop(t: &mut TestCtx) {
    let extra = 1 + t.pick("frames.free.drop", 6) as u32;
    let before = frame::stats();
    let Some(f) = frame::alloc() else {
        t.fail(format_args!("alloc returned None with {} frames free", before.free));
        return;
    };
    for _ in 0..extra {
        frame::inc(f);
    }
    t.expect_eq("count after alloc and inc", frame::refs(f) as u64, 1 + extra as u64);
    for i in (0..extra).rev() {
        frame::free(f);
        t.expect_eq("count after one free", frame::refs(f) as u64, 1 + i as u64);
        t.expect_eq("used while references remain", frame::stats().used as u64, before.used as u64 + 1);
    }
    frame::free(f);
    t.note(format_args!("frames.free.drop: {} references (chosen by this run's nonce), dropped one at a time", 1 + extra));
    t.expect_eq("count after the last free", frame::refs(f) as u64, 0);
    t.expect_eq("free after the last free", frame::stats().free as u64, before.free as u64);
});

ktest!("frames.stats.moves", fn frames_stats_moves(t: &mut TestCtx) {
    let n = 1 + t.pick("frames.stats.moves", 40) as usize;
    let s0 = frame::stats();
    let mut got = [Frame(PhysAddr(0)); 40];
    for g in got.iter_mut().take(n) {
        match frame::alloc() {
            Some(f) => *g = f,
            None => {
                t.fail(format_args!("alloc returned None with {} frames free", frame::stats().free));
                return;
            }
        }
    }
    let s1 = frame::stats();
    for f in &got[..n] {
        frame::free(*f);
    }
    let s2 = frame::stats();
    t.note(format_args!("frames.stats.moves: {n} frames (chosen by this run's nonce): free {} -> {} -> {}", s0.free, s1.free, s2.free));
    t.expect_eq("free after the allocations", s1.free as u64, (s0.free - n) as u64);
    t.expect_eq("used after the allocations", s1.used as u64, (s0.used + n) as u64);
    t.expect_eq("total after the allocations", s1.total as u64, s0.total as u64);
    t.expect_eq("free after the frees", s2.free as u64, s0.free as u64);
    t.expect_eq("used after the frees", s2.used as u64, s0.used as u64);
});

fn prelude() {
    let (k, i, _) = shim::stacks();
    kernel::gdt::init(k, i);
    kernel::idt::init();
    let bi = bi();
    frame::init(bi.memmap(), bi.kernel_range());
}

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    kernel::ktap::run_and_halt_after(prelude)
}
