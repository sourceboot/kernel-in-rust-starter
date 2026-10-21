//! Lab 04's vehicle: the GDT and the TSS, decoded from what the CPU holds.
//! `gdt.layout` reads the table back with `sgdt` and checks CONTRACT §A.9
//! S1 slot by slot; `df.on.ist` installs this vehicle's own minimal IDT (no
//! learner IDT exists before lab 05) and proves a `#DF` lands on IST1.

#![no_std]
#![no_main]

#[path = "../shim.rs"]
mod shim;

use core::arch::{asm, naked_asm};
use core::sync::atomic::{AtomicU64, Ordering};

use kernel::addr::VirtAddr;
use kernel::gdt::{self, sel};
use kernel::ktest;

#[repr(C, packed)]
struct Dtr {
    limit: u16,
    base: u64,
}

fn sgdt() -> (u64, u16) {
    let mut d = Dtr { limit: 0, base: 0 };
    // SAFETY: `sgdt` stores 10 bytes into `d`.
    unsafe { asm!("sgdt [{}]", in(reg) &mut d, options(nostack, preserves_flags)) };
    (d.base, d.limit)
}

fn desc(i: usize) -> u64 {
    let (base, _) = sgdt();
    // SAFETY: the GDTR names a live table the CPU uses; reading a slot below
    // its limit has no effect.
    unsafe { core::ptr::read_volatile((base as *const u64).add(i)) }
}

fn access(d: u64) -> u64 {
    (d >> 40) & 0xff
}
fn present(d: u64) -> bool {
    access(d) & 0x80 != 0
}
fn dpl(d: u64) -> u64 {
    (access(d) >> 5) & 3
}
fn is_code(d: u64) -> bool {
    access(d) & 0x18 == 0x18
}
fn is_data_rw(d: u64) -> bool {
    access(d) & 0x1a == 0x12
}
fn long(d: u64) -> bool {
    (d >> 53) & 1 == 1
}

fn tss_base() -> u64 {
    let (lo, hi) = (desc(6), desc(7));
    ((lo >> 16) & 0xff_ffff) | (((lo >> 56) & 0xff) << 24) | ((hi & 0xffff_ffff) << 32)
}

// Bracket the whole image (boot/linker.ld); only their addresses matter.
extern "C" {
    static __kernel_start: u8;
    static __kernel_end: u8;
}

/// The learner's TSS is a `static`, so the descriptor's base (all three base
/// fields, the high half's bits 63:32 included) must land inside the kernel
/// image. Checked before any row reads the TSS through it: a wrong base reads
/// unmapped memory, which with no IDT is a reset (sk04-ladder "outside",
/// TPM ruling 2026-10-18).
fn tss_in_image() -> bool {
    let (lo, hi) = (core::ptr::addr_of!(__kernel_start) as u64, core::ptr::addr_of!(__kernel_end) as u64);
    let b = tss_base();
    b >= lo && b.checked_add(104).is_some_and(|e| e <= hi)
}

fn tss_base_fail(t: &mut kernel::ktap::TestCtx) -> bool {
    if tss_in_image() {
        return false;
    }
    t.fail(format_args!("the TSS descriptor's base: observed {:#x}, expected the address of a TSS inside the kernel", tss_base()));
    true
}

fn segs() -> (u16, u16, u16, u16) {
    let (cs, ss, ds, tr): (u16, u16, u16, u16);
    // SAFETY: reading segment registers and the task register has no effect.
    unsafe {
        asm!("mov {0:x}, cs", "mov {1:x}, ss", "mov {2:x}, ds", "str {3:x}",
            out(reg) cs, out(reg) ss, out(reg) ds, out(reg) tr, options(nomem, nostack, preserves_flags));
    }
    (cs, ss, ds, tr)
}

ktest!("gdt.layout", fn layout(t: &mut TestCtx) {
    let (_, limit) = sgdt();
    t.expect_eq("GDT limit (six slots + a 16-byte TSS, minus one)", limit as u64, 0x3f);
    t.expect_eq("slot 0x00 is null", desc(0), 0);
    let kc = desc(1);
    t.expect(present(kc) && is_code(kc) && long(kc) && dpl(kc) == 0, "0x08 is a present 64-bit DPL0 code segment");
    t.expect((kc >> 54) & 1 == 0, "0x08 has D=0 (required with L=1)");
    let kd = desc(2);
    t.expect(present(kd) && is_data_rw(kd) && dpl(kd) == 0, "0x10 is a present writable DPL0 data segment");
    let u32c = desc(3);
    t.expect(!(present(u32c) && is_code(u32c) && long(u32c)), "0x18 is not a 64-bit code segment (sysret's CS is 0x18 + 16)");
    let td = desc(6);
    t.expect(present(td) && access(td) & 0x1f == 0x0b, "0x30 is a present, busy 64-bit TSS (ltr ran)");
    t.expect((td & 0xffff) >= 103, "the TSS limit covers 104 bytes");
    tss_base_fail(t);
    let (cs, ss, ds, tr) = segs();
    t.expect_eq("CS after init", cs as u64, sel::KCODE as u64);
    t.expect_eq("SS after init", ss as u64, sel::KDATA as u64);
    t.expect_eq("DS after init", ds as u64, sel::KDATA as u64);
    t.expect_eq("TR after init", tr as u64, sel::TSS as u64);
    t.expect_eq("sel::TSS", sel::TSS as u64, 0x30);
});

ktest!("gdt.user.dpl", fn user_dpl(t: &mut TestCtx) {
    let ud = desc(4);
    t.expect(present(ud) && is_data_rw(ud) && dpl(ud) == 3, "0x20 is a present writable DPL3 data segment");
    let uc = desc(5);
    t.expect(present(uc) && is_code(uc) && long(uc) && dpl(uc) == 3, "0x28 is a present 64-bit DPL3 code segment");
    t.expect_eq("sel::UDATA", sel::UDATA as u64, 0x23);
    t.expect_eq("sel::UCODE", sel::UCODE as u64, 0x2b);
    t.expect_eq("sel::KCODE", sel::KCODE as u64, 0x08);
    t.expect_eq("sel::KDATA", sel::KDATA as u64, 0x10);
});

fn tss_u64(off: u64) -> u64 {
    // SAFETY: `off` < 104 lies inside the TSS the GDT names.
    unsafe { core::ptr::read_unaligned((tss_base() + off) as *const u64) }
}

ktest!("tss.rsp0", fn rsp0(t: &mut TestCtx) {
    let (k, _, _) = shim::stacks();
    if tss_base_fail(t) {
        return;
    }
    t.expect_eq("TSS.rsp0 (offset 4) after init", tss_u64(4), k.0);
    gdt::set_rsp0(VirtAddr(k.0 - 0x100));
    t.expect_eq("TSS.rsp0 after set_rsp0", tss_u64(4), k.0 - 0x100);
    gdt::set_rsp0(k);
});

ktest!("tss.ist1", fn ist1(t: &mut TestCtx) {
    let (_, i, _) = shim::stacks();
    if tss_base_fail(t) {
        return;
    }
    t.expect_eq("TSS.ist[0] (offset 0x24) after init", tss_u64(0x24), i.0);
});

// ── df.on.ist: this vehicle's own one-gate IDT ─────────────────────────────

static IDT: [[AtomicU64; 2]; 256] = [const { [AtomicU64::new(0), AtomicU64::new(0)] }; 256];
static DF_RSP: AtomicU64 = AtomicU64::new(0);
static DF_RESUME: AtomicU64 = AtomicU64::new(0);
static DF_HITS: AtomicU64 = AtomicU64::new(0);

/// `#DF` on IST1: record the stack it arrived on, then resume at the
/// continuation the trigger stored (the saved rip of a `#DF` is undefined).
#[unsafe(naked)]
extern "C" fn df_entry() {
    naked_asm!(
        "mov [rip + {rsp}], rsp",
        "lock inc qword ptr [rip + {hits}]",
        "push rax",
        "mov rax, [rip + {resume}]",
        "mov [rsp + 16], rax",
        "pop rax",
        "add rsp, 8",
        "iretq",
        rsp = sym DF_RSP,
        hits = sym DF_HITS,
        resume = sym DF_RESUME,
    )
}

ktest!("df.on.ist", fn df_on_ist(t: &mut TestCtx) {
    let (_, ist1, size) = shim::stacks();
    let h = df_entry as extern "C" fn() as usize as u64;
    let lo = (h & 0xffff) | ((sel::KCODE as u64) << 16) | (1u64 << 32) | (0x8e << 40) | (((h >> 16) & 0xffff) << 48);
    IDT[8][0].store(lo, Ordering::Relaxed);
    IDT[8][1].store(h >> 32, Ordering::Relaxed);
    let mut old = Dtr { limit: 0, base: 0 };
    let new = Dtr { limit: 256 * 16 - 1, base: IDT.as_ptr() as u64 };
    let before = DF_HITS.load(Ordering::Relaxed);
    // SAFETY: the vehicle's IDT has one present gate, #DF on IST1. Loading
    // selector 0x58 (past the GDT's limit) into DS raises #GP; its gate is
    // absent, so delivering it faults again and the CPU raises #DF, which
    // lands on IST1 and resumes at label 2. DS is never changed (the load
    // faulted). The old IDTR is restored after.
    unsafe {
        asm!(
            "sidt [{old}]",
            "lidt [{new}]",
            "lea {t}, [rip + 2f]",
            "mov [{resume}], {t}",
            "mov {t:e}, 0x58",
            "mov ds, {t:e}",
            "2:",
            "lidt [{old}]",
            old = in(reg) &mut old,
            new = in(reg) &new,
            resume = in(reg) DF_RESUME.as_ptr(),
            t = out(reg) _,
        );
    }
    let rsp = DF_RSP.load(Ordering::Relaxed);
    t.expect_eq("#DF deliveries", DF_HITS.load(Ordering::Relaxed) - before, 1);
    t.note(format_args!("#DF arrived with rsp = IST1 top - {:#x}", ist1.0.wrapping_sub(rsp)));
    t.expect(rsp < ist1.0 && rsp >= ist1.0 - size, "#DF ran on the IST1 stack the TSS names");
});

fn prelude() {
    let (k, i, _) = shim::stacks();
    gdt::init(k, i);
}

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    kernel::ktap::run_and_halt_after(prelude)
}
