//! Lab 08's vehicle: page tables. The prelude brings up the frame pool; each
//! row builds its own `AddressSpace`, and any row that switches CR3 puts the
//! bootloader's tables back before it returns. The vehicle walks the running
//! tables itself, through the HHDM, to have an answer `translate` must match.

#![no_std]
#![no_main]

#[path = "../shim.rs"]
mod shim;

use core::arch::asm;

use kernel::addr::{PhysAddr, VirtAddr};
use kernel::bootinfo;
use kernel::frame::{self, Frame};
use kernel::ktest;
use kernel::paging::{self, flags, AddressSpace, MapError, PageSize};

fn cr3() -> u64 {
    let v: u64;
    // SAFETY: reading CR3 at CPL 0 has no side effect.
    unsafe { asm!("mov {}, cr3", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

fn set_cr3(v: u64) {
    // SAFETY: only ever called with the CR3 this vehicle read at the start of
    // the same row, whose tables are untouched.
    unsafe { asm!("mov cr3, {}", in(reg) v, options(nostack, preserves_flags)) };
}

fn hhdm<T>(pa: u64) -> *mut T {
    (pa + bootinfo::hhdm_offset()) as *mut T
}

const ADDR: u64 = 0x000f_ffff_ffff_f000;

/// The vehicle's own walk of the tables rooted at `root`: (physical address,
/// page size in bytes), and the entry it read at each level (PML4E, PDPTE,
/// PDE, PTE; 0 past the level where the walk stopped).
fn walk_in(root: u64, va: u64) -> (Option<(u64, u64)>, [u64; 4]) {
    let mut seen = [0u64; 4];
    let mut t = root & ADDR;
    for (level, shift) in [39u32, 30, 21, 12].into_iter().enumerate() {
        // SAFETY: `t` is a page-table frame of a live table tree, read through the HHDM.
        let e: u64 = unsafe { hhdm::<u64>(t).add((va >> shift) as usize & 511).read_volatile() };
        seen[level] = e;
        if e & 1 == 0 {
            return (None, seen);
        }
        let size = 1u64 << shift;
        if level == 3 || (level > 0 && e & (1 << 7) != 0) {
            return (Some(((e & ADDR & !(size - 1)) | (va & (size - 1)), size)), seen);
        }
        t = e & ADDR;
    }
    (None, seen)
}

/// The vehicle's own walk of the running tables.
fn walk(va: u64) -> Option<(u64, u64)> {
    walk_in(cr3(), va).0
}

struct Shown(Option<(u64, u64)>);

impl core::fmt::Display for Shown {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self.0 {
            Some((pa, s)) if s == 1 << 30 => write!(f, "{pa:#x} in a 1 GiB page"),
            Some((pa, s)) => write!(f, "{pa:#x} in a {} KiB page", s >> 10),
            None => write!(f, "unmapped"),
        }
    }
}

fn bytes(s: PageSize) -> u64 {
    match s {
        PageSize::Size4K => 4096,
        PageSize::Size2M => 2 << 20,
        PageSize::Size1G => 1 << 30,
    }
}

ktest!("translate.huge", fn translate_huge(t: &mut TestCtx) {
    let a = AddressSpace::new_kernel();
    let bi = bootinfo::get().expect("the shim published the BootInfo");
    let stack = 0u8;
    let probes = [
        ("the HHDM at 1 MiB + 0x123", paging::hhdm(PhysAddr(0x10_0123)).0),
        ("the HHDM at 64 MiB + 0x7654", paging::hhdm(PhysAddr(0x400_7654)).0),
        ("the kernel's code", translate_huge as fn(&mut kernel::ktap::TestCtx) as usize as u64),
        ("the stack", &stack as *const u8 as u64),
        ("the framebuffer", bi.fb.addr + 0x2008),
    ];
    let mut huge = 0;
    for (what, va) in probes {
        let want = walk(va);
        let got = a.translate(VirtAddr(va)).map(|(pa, s)| (pa.0, bytes(s)));
        t.note(format_args!("translate {what} {va:#x}: the tables say {}, translate says {}", Shown(want), Shown(got)));
        if got != want {
            t.fail(format_args!("translate({va:#x}) disagrees with the page tables"));
        }
        if matches!(want, Some((_, s)) if s > 4096) {
            huge += 1;
        }
    }
    t.expect(huge > 0, "at least one probe lands in a huge page");
    t.expect(a.translate(VirtAddr(0x1234_5000)).is_none(), "an address in the empty user half translates to None");
    // The bootloader maps no 1 GiB page, so the row builds one: PML4[1] of the
    // fresh space names a PDPT whose entry 0 is PRESENT|PS at physical 0 with
    // bit 12 (PAT; SDM Vol. 3A, 4-Level Paging and 5-Level Paging) set — a valid 1 GiB entry whose address
    // bits read 0x1000. A walk that ignores PS reads physical page 1 as a PD,
    // so the probe picks a PD index whose entry there is not present: that walk
    // answers None instead of wandering. The space is never loaded.
    if let Some(p) = frame::alloc() {
        // SAFETY: physical page 1 is in the HHDM (measured), read only.
        let slot = (1..512usize).find(|&i| unsafe { hhdm::<u64>(0x1000).add(i).read_volatile() } & 1 == 0).unwrap_or(1);
        // SAFETY: `p` is ours and zeroed; `a`'s PML4 frame is ours; neither is live.
        unsafe {
            hhdm::<u64>(p.0 .0).write_volatile(0x1000 | 1 << 7 | 1);
            hhdm::<u64>(a.root().0).add(1).write_volatile(p.0 .0 | 1);
        }
        let va = 1u64 << 39 | (slot as u64) << 21 | 0x2234;
        let want = walk_in(a.root().0, va).0;
        let got = a.translate(VirtAddr(va)).map(|(pa, s)| (pa.0, bytes(s)));
        t.note(format_args!("translate a 1 GiB page {va:#x}: the tables say {}, translate says {}", Shown(want), Shown(got)));
        if got != want {
            t.fail(format_args!("translate({va:#x}) disagrees with the page tables"));
        }
        // SAFETY: as above.
        unsafe { hhdm::<u64>(a.root().0).add(1).write_volatile(0) };
        frame::free(p);
    } else {
        t.fail(format_args!("no frame for a PDPT"));
    }
    frame::free(Frame(a.root()));
});

ktest!("pml4.fresh.alive", fn pml4_fresh_alive(t: &mut TestCtx) {
    let old = cr3();
    // A present marker in the running PML4's user half for the length of
    // new_kernel: entry 255 names a zeroed frame of ours, an empty PDPT, so it
    // maps nothing; a copy of the user half, present entries or all, carries
    // it across. Cleared after, with CR3 reloaded so no walk keeps it cached.
    let Some(mk) = frame::alloc() else {
        t.fail(format_args!("no frame for the marker"));
        return;
    };
    // SAFETY: the running PML4 through the HHDM; entry 255 is empty (the
    // bootloader maps nothing below the upper half) and `mk` is ours, zeroed.
    unsafe { hhdm::<u64>(old & ADDR).add(255).write_volatile(mk.0 .0 | 1) };
    let a = AddressSpace::new_kernel();
    // SAFETY: as above.
    unsafe { hhdm::<u64>(old & ADDR).add(255).write_volatile(0) };
    set_cr3(old);
    frame::free(mk);
    let root = a.root().0;
    t.expect(root != old & ADDR, "the new PML4 is a new frame");
    let mut user = 0;
    let mut differ = 0;
    for i in 0..512 {
        // SAFETY: both are PML4 frames read through the HHDM.
        let (n, o) = unsafe { (hhdm::<u64>(root).add(i).read_volatile(), hhdm::<u64>(old & ADDR).add(i).read_volatile()) };
        if i < 256 && n != 0 {
            user += 1;
        }
        if i >= 256 && n != o {
            differ += 1;
        }
    }
    t.expect_eq("user-half PML4 entries present in a fresh space", user, 0);
    t.expect_eq("kernel-half PML4 entries that differ from the running ones", differ, 0);
    a.activate();
    let now = cr3();
    t.note(format_args!("pml4.fresh.alive: CR3 {old:#x} -> {now:#x}, and this line printed from the new space"));
    let alive = 0x5b00_7b00u64;
    // SAFETY: a read of a local through the stack, with the new tables live.
    let seen = unsafe { core::ptr::read_volatile(&alive) };
    set_cr3(old);
    t.expect_eq("CR3 after activate", now & ADDR, root);
    t.expect_eq("a stack value read in the new space", seen, alive);
    frame::free(Frame(PhysAddr(root)));
});

ktest!("map.user.readback", fn map_user_readback(t: &mut TestCtx) {
    let va = VirtAddr(0x2000_0000_0000 + (t.derived("map.user.readback") & 0x0fff_ffff_f000));
    let pattern = 0x5b00_7b00_0000_0000 | va.0 >> 12;
    let old = cr3();
    let mut a = AddressSpace::new_kernel();
    let (Some(f), Some(g), Some(k)) = (frame::alloc(), frame::alloc(), frame::alloc()) else {
        t.fail(format_args!("no frames to map"));
        return;
    };
    // SAFETY: a frame just handed to us, written through the HHDM.
    unsafe { hhdm::<u64>(f.0 .0).add(3).write_volatile(pattern) };
    t.note(format_args!("map.user.readback: {:#x} (chosen by this run's nonce) -> frame {:#x}", va.0, f.0 .0));
    let r = a.map(va, f, flags::PRESENT | flags::WRITE | flags::USER);
    t.expect(r.is_ok(), "map returns Ok");
    // A ring-3 access needs USER at every level of the walk, not only in the
    // leaf; read each level's entry back out of the new space's own tables,
    // right after the first map (a second map may repair what the first left).
    let (_, levels) = walk_in(a.root().0, va.0);
    for (level, e) in ["PML4E", "PDPTE", "PDE", "PTE"].into_iter().zip(levels) {
        if e & flags::PRESENT == 0 {
            // The levels below this one were never read, so say nothing
            // about them.
            t.fail(format_args!("the {level} on the walk to {:#x} is not present ({e:#x}); the walk stops here", va.0));
            break;
        } else if e & flags::USER == 0 {
            t.fail(format_args!("the {level} on the walk to {:#x} lacks USER, bit 2 ({e:#x}): ring 3 could not touch this page", va.0));
        }
    }
    t.expect(a.map(va, g, flags::PRESENT | flags::USER) == Err(MapError::AlreadyMapped), "mapping the same page twice returns AlreadyMapped");
    // Tables that exist before a user page needs them: a kernel page at
    // PML4[3] first, whose tables lack USER, then a user page beside it.
    let (kva, uva) = (VirtAddr(3 << 39), VirtAddr(3 << 39 | 0x1000));
    t.expect(a.map(kva, k, flags::PRESENT | flags::WRITE).is_ok(), "map of a kernel page returns Ok");
    t.expect(a.map(uva, g, flags::PRESENT | flags::WRITE | flags::USER).is_ok(), "map of a user page beside it returns Ok");
    let (_, levels) = walk_in(a.root().0, uva.0);
    if levels.iter().any(|e| e & flags::USER == 0) {
        t.fail(format_args!("a user page mapped through tables a kernel page made lacks USER on its walk ({:#x} {:#x} {:#x} {:#x})", levels[0], levels[1], levels[2], levels[3]));
    }
    a.unmap(uva);
    a.unmap(kva);
    // Huge pages at a PML4 index this run picks in 4..64 (below `va`'s 64..96,
    // clear of 1 and 3): a 2 MiB page at PDPT[0] -> PD[0] and a 1 GiB page at
    // PDPT[1], built by the row in this space before it is ever loaded, and
    // taken down before it is. Each PS entry names a frame of the row's own,
    // so a map that walks through it as if it were a table writes only there;
    // each such frame holds one entry that reads like a real table entry (it
    // names a frame of the row's, `q`), at an index this run picks clear of the
    // indices a map here would read, so it is not empty.
    let hi = 4 + t.pick("map.user.readback/huge", 60);
    let j = 6 + t.pick("map.user.readback/huge-data", 500) as usize;
    if let (Some(p), Some(d), Some(s), Some(s1), Some(q)) = (frame::alloc(), frame::alloc(), frame::alloc(), frame::alloc(), frame::alloc()) {
        // SAFETY: five zeroed frames of ours and `a`'s PML4, none of them live.
        unsafe {
            hhdm::<u64>(s.0 .0).add(j).write_volatile(q.0 .0 | 0b11);
            hhdm::<u64>(s1.0 .0).add(j).write_volatile(q.0 .0 | 0b11);
            hhdm::<u64>(d.0 .0).write_volatile(s.0 .0 | 1 << 7 | 0b11);
            hhdm::<u64>(p.0 .0).write_volatile(d.0 .0 | 0b11);
            hhdm::<u64>(p.0 .0).add(1).write_volatile(s1.0 .0 | 1 << 7 | 0b11);
            hhdm::<u64>(a.root().0).add(hi as usize).write_volatile(p.0 .0 | 0b11);
        }
        let r = a.map(VirtAddr(hi << 39 | 0x5000), g, flags::PRESENT | flags::WRITE);
        t.expect(r == Err(MapError::HugePage), "mapping a 4 KiB page inside a 2 MiB page returns HugePage");
        let r = a.map(VirtAddr(hi << 39 | 1 << 30 | 0x5000), g, flags::PRESENT | flags::WRITE);
        t.expect(r == Err(MapError::HugePage), "mapping a 4 KiB page inside a 1 GiB page returns HugePage");
        // Beside them, under the same PML4 entry (present, no USER), a kernel
        // page maps: HugePage is the PS entry's answer, not the PML4 entry's.
        let kb = VirtAddr(hi << 39 | 2 << 30 | 0x5000);
        t.expect(a.map(kb, g, flags::PRESENT | flags::WRITE).is_ok(), "a kernel page beside the huge pages maps");
        a.unmap(kb);
        // And one under the 2 MiB page's own PDPT entry, through its PD.
        let kb2 = VirtAddr(hi << 39 | 1 << 21 | 0x5000);
        t.expect(a.map(kb2, g, flags::PRESENT | flags::WRITE).is_ok(), "a kernel page beside the 2 MiB page, under its PDPT entry, maps");
        a.unmap(kb2);
        // SAFETY: as above.
        unsafe { hhdm::<u64>(a.root().0).add(hi as usize).write_volatile(0) };
        frame::free(p);
        frame::free(d);
        frame::free(s);
        frame::free(s1);
        frame::free(q);
    } else {
        t.fail(format_args!("no frames for the huge pages"));
    }
    t.expect(a.translate(va) == Some((f.0, PageSize::Size4K)), "translate finds the frame as a 4 KiB page");
    a.activate();
    let p = va.as_mut_ptr::<u64>();
    // SAFETY: `va` is mapped writable in the live space; SMAP is off on this
    // machine (CR4 = 0x20, measured), so CPL 0 may touch a user page.
    let (seen, wrote) = unsafe {
        let s = p.add(3).read_volatile();
        p.add(7).write_volatile(!pattern);
        (s, hhdm::<u64>(f.0 .0).add(7).read_volatile())
    };
    set_cr3(old);
    t.expect_eq("the word read through the new mapping", seen, pattern);
    t.expect_eq("a word written through the mapping, read through the HHDM", wrote, !pattern);
    t.expect(a.unmap(va) == Some(f), "unmap returns the mapped frame");
    t.expect(a.translate(va).is_none(), "translate after unmap returns None");
    frame::free(f);
    frame::free(g);
    frame::free(k);
});

ktest!("pte.cow.reserved", fn pte_cow_reserved(t: &mut TestCtx) {
    t.expect_eq("flags::COW", flags::COW, 1 << 9);
    let hw = flags::PRESENT | flags::WRITE | flags::USER | flags::NX | 1 << 3 | 1 << 4 | 1 << 5 | 1 << 6 | 1 << 7 | 1 << 8;
    t.expect_eq("COW's overlap with bits the CPU reads", flags::COW & hw, 0);
    let mut a = AddressSpace::new_kernel();
    let va = VirtAddr(0x10_0000_0000);
    let Some(f) = frame::alloc() else {
        t.fail(format_args!("no frame to map"));
        return;
    };
    let want = flags::PRESENT | flags::USER | flags::COW | flags::NX;
    t.expect(a.map(va, f, want).is_ok(), "map with COW and NX returns Ok");
    let e = a.entry_mut(va).map(|e| e.0);
    t.note(format_args!("pte.cow.reserved: the leaf entry reads {:#x}", e.unwrap_or(0)));
    t.expect(e.is_some_and(|e| e & flags::COW != 0 && e & flags::WRITE == 0), "the leaf keeps COW and stays read-only");
    t.expect(e.is_some_and(|e| e & flags::NX != 0), "the leaf keeps NX, bit 63");
    t.expect(e.is_some_and(|e| paging::Pte(e).flags() == want), "Pte::flags of the leaf is every bit but the address");
    t.expect(a.translate(va) == Some((f.0, PageSize::Size4K)), "translate finds the frame of a page with NX set");
    let old = cr3();
    a.activate();
    // SAFETY: `va` is mapped present (read-only) in the live space; SMAP off.
    let v = unsafe { va.as_ptr::<u64>().read_volatile() };
    set_cr3(old);
    t.expect_eq("a read through a COW-marked page", v, 0);
    a.unmap(va);
    frame::free(f);
});

fn prelude() {
    let (k, i, _) = shim::stacks();
    kernel::gdt::init(k, i);
    kernel::idt::init();
    let bi = bootinfo::get().expect("the shim published the BootInfo");
    frame::init(bi.memmap(), bi.kernel_range());
}

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    kernel::ktap::run_and_halt_after(prelude)
}
