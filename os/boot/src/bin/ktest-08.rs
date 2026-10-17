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

/// The vehicle's own walk of the running tables: (physical address, page size in bytes).
fn walk(va: u64) -> Option<(u64, u64)> {
    let mut t = cr3() & ADDR;
    for (level, shift) in [39u32, 30, 21, 12].into_iter().enumerate() {
        // SAFETY: `t` is a page-table frame of the running tables, read through the HHDM.
        let e: u64 = unsafe { hhdm::<u64>(t).add((va >> shift) as usize & 511).read_volatile() };
        if e & 1 == 0 {
            return None;
        }
        let size = 1u64 << shift;
        if level == 3 || (level > 0 && e & (1 << 7) != 0) {
            return Some(((e & ADDR & !(size - 1)) | (va & (size - 1)), size));
        }
        t = e & ADDR;
    }
    None
}

struct Shown(Option<(u64, u64)>);

impl core::fmt::Display for Shown {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self.0 {
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
        ("the framebuffer", bi.fb.addr + 0x1008),
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
    frame::free(Frame(a.root()));
});

ktest!("pml4.fresh.alive", fn pml4_fresh_alive(t: &mut TestCtx) {
    let old = cr3();
    let a = AddressSpace::new_kernel();
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
    let (Some(f), Some(g)) = (frame::alloc(), frame::alloc()) else {
        t.fail(format_args!("no frames to map"));
        return;
    };
    // SAFETY: a frame just handed to us, written through the HHDM.
    unsafe { hhdm::<u64>(f.0 .0).add(3).write_volatile(pattern) };
    t.note(format_args!("map.user.readback: {:#x} (chosen by this run's nonce) -> frame {:#x}", va.0, f.0 .0));
    let r = a.map(va, f, flags::PRESENT | flags::WRITE | flags::USER);
    t.expect(r.is_ok(), "map returns Ok");
    t.expect(a.map(va, g, flags::PRESENT | flags::USER) == Err(MapError::AlreadyMapped), "mapping the same page twice returns AlreadyMapped");
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
    t.expect(a.map(va, f, flags::PRESENT | flags::USER | flags::COW).is_ok(), "map with COW returns Ok");
    let e = a.entry_mut(va).map(|e| e.0);
    t.note(format_args!("pte.cow.reserved: the leaf entry reads {:#x}", e.unwrap_or(0)));
    t.expect(e.is_some_and(|e| e & flags::COW != 0 && e & flags::WRITE == 0), "the leaf keeps COW and stays read-only");
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
