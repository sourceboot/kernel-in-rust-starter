//! Lab 05's vehicle: the IDT, its trampolines and the fault seam. `int3`
//! goes through the default dispatch and must resume with every register
//! intact; `#PF` and `#GP` go through a handler this vehicle registers with
//! `idt::set_handler`, which records the frame the trampoline built and
//! skips the faulting instruction. A `#PF` reaches that handler only after
//! the learner's dispatch has handed CR2 to `mm::fault` and got `Kill`, so
//! `pf.cr2` reads the address back from `mm::last_kill`. The `#PF` addresses
//! are chosen per run from the nonce; `pf.error_code` faults twice, a read and
//! a write, in a nonce-chosen order (2026-10-18, BUILD-STATUS F3), and each of
//! its faults also reads `mm::last_kill().addr` back, so an address handed to
//! `mm::fault` on one access kind only is red here. `fault.seam`'s case
//! addresses are not page-aligned, so a `Kill.addr` rounded to its page is red.
//! `idt.gates` reads the learner's table back with `sidt` (lab 04's `sgdt`
//! shape) and checks all 256 gates' shape and `#DF` on IST 1, which no run
//! of this lab can observe by faulting (2026-10-18, TPM class B ruling).
//! `idt.vector` (fix round, 2026-10-18, sk05-forger-1) registers a handler
//! with `set_handler(v, f, 1)` for a nonce-chosen vector v in 32..=255 and
//! raises it with `int v`: the gate's IST must read back 1, the frame must sit
//! on the IST 1 stack and carry vector v, and every caller-saved register the
//! handler overwrites must come back as it was — so a vector the run never
//! named in advance needs real entry code, `set_handler`'s IST and a common
//! entry that saves what a Rust handler may clobber. `fault.seam` has six
//! cases, two pairs sharing an error code across the address halves.

#![no_std]
#![no_main]

#[path = "../shim.rs"]
mod shim;

use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

use kernel::addr::VirtAddr;
use kernel::idt::{self, TrapFrame};
use kernel::ktap::TestCtx;
use kernel::ktest;
use kernel::mm::{self, FaultOutcome, KillReason};

static HITS: AtomicU64 = AtomicU64::new(0);
static VEC: AtomicU64 = AtomicU64::new(0);
static ERR: AtomicU64 = AtomicU64::new(0);
static RIP: AtomicU64 = AtomicU64::new(0);
static CR2: AtomicU64 = AtomicU64::new(0);
static RESUME: AtomicU64 = AtomicU64::new(0);
static AT: AtomicU64 = AtomicU64::new(0);
static FRAME: AtomicU64 = AtomicU64::new(0);
/// The local APIC's EOI register while `idt.vector` has the APIC mapped, else 0.
static APIC_EOI: AtomicU64 = AtomicU64::new(0);
/// The span of instructions `idt.vector`'s interrupt may arrive in.
static WIN_LO: AtomicU64 = AtomicU64::new(0);
static WIN_HI: AtomicU64 = AtomicU64::new(0);

// Port I/O of the vehicle's own: the UART checks read COM1 without going
// through the learner's serial.rs.
fn inb(port: u16) -> u8 {
    let v: u8;
    // SAFETY: reading a 16550 register has no effect beyond the UART.
    unsafe { asm!("in al, dx", out("al") v, in("dx") port, options(nomem, nostack, preserves_flags)) };
    v
}
fn outb(port: u16, v: u8) {
    // SAFETY: the callers write only COM1's MCR and the 8259s' mask registers.
    unsafe { asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack, preserves_flags)) };
}
const COM1: u16 = 0x3f8;

/// A page of the vehicle's own whose mapping `idt.vector` points at the local
/// APIC for the length of the row: the loader's direct map leaves MMIO out.
#[repr(C, align(4096))]
struct Window(core::cell::UnsafeCell<[u8; 4096]>);
// SAFETY: only `idt.vector` touches it, through volatile MMIO accesses.
unsafe impl Sync for Window {}
static WINDOW: Window = Window(core::cell::UnsafeCell::new([0; 4096]));

/// The leaf page-table entry that maps `va` in the running tables, or None if
/// a level is absent or maps a large page.
fn leaf_pte(va: u64) -> Option<*mut u64> {
    let hhdm = kernel::bootinfo::hhdm_offset();
    let cr3: u64;
    // SAFETY: reading CR3 has no effect.
    unsafe { asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags)) };
    let mut table = cr3 & 0x000f_ffff_ffff_f000;
    for level in (1..=4).rev() {
        let idx = (va >> (12 + 9 * (level - 1))) & 0x1ff;
        let e = (hhdm + table + idx * 8) as *mut u64;
        if level == 1 {
            return Some(e);
        }
        // SAFETY: `table` is a live page table the CPU walks; the direct map covers it.
        let v = unsafe { core::ptr::read_volatile(e) };
        if v & 1 == 0 || v & 0x80 != 0 {
            return None;
        }
        table = v & 0x000f_ffff_ffff_f000;
    }
    None
}

/// Record the frame and continue at the label the trigger stored.
fn record(tf: &mut TrapFrame) {
    let cr2: u64;
    // SAFETY: reading CR2 is side-effect free at CPL 0.
    unsafe { asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags)) };
    HITS.fetch_add(1, Ordering::SeqCst);
    VEC.store(tf.vector, Ordering::SeqCst);
    ERR.store(tf.error, Ordering::SeqCst);
    RIP.store(tf.rip, Ordering::SeqCst);
    CR2.store(cr2, Ordering::SeqCst);
    tf.rip = RESUME.load(Ordering::SeqCst);
}

/// Fault once at `addr` (a write if `write`), through the vehicle's #PF
/// handler; returns how many traps arrived.
fn page_fault(addr: u64, write: bool) -> u64 {
    idt::set_handler(14, record, 0);
    let before = HITS.load(Ordering::SeqCst);
    // SAFETY: `addr` is an unmapped user-half address, so the access traps;
    // the recorded handler resumes at label 2. The stored labels are this
    // block's own instructions.
    unsafe {
        if write {
            asm!("lea {t}, [rip + 2f]", "mov [{r}], {t}", "lea {t}, [rip + 3f]", "mov [{at}], {t}",
                "3: mov qword ptr [{a}], {t}", "2:",
                t = out(reg) _, r = in(reg) RESUME.as_ptr(), at = in(reg) AT.as_ptr(), a = in(reg) addr);
        } else {
            asm!("lea {t}, [rip + 2f]", "mov [{r}], {t}", "lea {t}, [rip + 3f]", "mov [{at}], {t}",
                "3: mov {t}, qword ptr [{a}]", "2:",
                t = out(reg) _, r = in(reg) RESUME.as_ptr(), at = in(reg) AT.as_ptr(), a = in(reg) addr);
        }
    }
    HITS.load(Ordering::SeqCst) - before
}

/// A nonce-chosen, 8-aligned address in [2^44, 2^45): user half, unmapped.
fn user_addr(t: &TestCtx, id: &str) -> u64 {
    0x0000_1000_0000_0000 | (t.derived(id) & 0x0000_0fff_ffff_fff8)
}

/// The 13 registers `int3.resume` and `idt.vector` set, in `want`'s order.
const REG_NAMES: [&str; 13] = ["rax", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15"];

ktest!("int3.resume", fn int3_resume(t: &mut TestCtx) {
    let mut v = [0u64; 13];
    let (s0, s1): (u64, u64);
    // SAFETY: `int3` traps to the learner's default dispatch, which must
    // resume at the next instruction with every register restored.
    unsafe {
        asm!("mov {s0}, rsp", "int3", "mov {s1}, rsp",
            s0 = out(reg) s0, s1 = out(reg) s1,
            inout("rax") 0x1111_0000_0000_00a0u64 => v[0], inout("rcx") 0x1111_0000_0000_00c0u64 => v[1],
            inout("rdx") 0x1111_0000_0000_00d0u64 => v[2], inout("rsi") 0x1111_0000_0000_0050u64 => v[3],
            inout("rdi") 0x1111_0000_0000_00d1u64 => v[4], inout("r8") 0x1111_0000_0000_0008u64 => v[5],
            inout("r9") 0x1111_0000_0000_0009u64 => v[6], inout("r10") 0x1111_0000_0000_0010u64 => v[7],
            inout("r11") 0x1111_0000_0000_0011u64 => v[8], inout("r12") 0x1111_0000_0000_0012u64 => v[9],
            inout("r13") 0x1111_0000_0000_0013u64 => v[10], inout("r14") 0x1111_0000_0000_0014u64 => v[11],
            inout("r15") 0x1111_0000_0000_0015u64 => v[12]);
    }
    let want = [0xa0, 0xc0, 0xd0, 0x50, 0xd1, 0x08, 0x09, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15];
    for ((got, w), name) in v.iter().zip(want).zip(REG_NAMES) {
        let w = 0x1111_0000_0000_0000 | w;
        if *got != w {
            t.fail(format_args!("{name} after int3: observed {got:#x}, expected {w:#x}"));
        }
    }
    t.expect_eq("rsp after int3", s1, s0);
    // The default for #BP reports a line. A second `int3`, once the first has
    // come back whole, runs with COM1 in loopback (MCR bit 4): what the kernel
    // sends comes back into the UART's receive buffer instead of onto the
    // wire, where this row counts it. (The first runs with the line on the
    // wire, so a run that dies in it still shows its report.) First, let what
    // was already sent leave (LSR TEMT).
    let mut spins = 0u32;
    while inb(COM1 + 5) & 0x40 == 0 && spins < 10_000_000 {
        spins += 1;
    }
    let mcr = inb(COM1 + 4);
    outb(COM1 + 4, mcr | 0x10);
    while inb(COM1 + 5) & 1 != 0 {
        inb(COM1);
    }
    // SAFETY: as above; the first `int3` came back with every marked register.
    unsafe { asm!("int3", clobber_abi("C")) };
    let (mut n, mut head) = (0usize, [b'.'; 16]);
    while inb(COM1 + 5) & 1 != 0 && n < 4096 {
        let b = inb(COM1);
        if n < 16 {
            head[n] = if (0x20..0x7f).contains(&b) { b } else { b'.' };
        }
        n += 1;
    }
    outb(COM1 + 4, mcr);
    if n == 0 {
        t.fail(format_args!("the default for vector 3 sent nothing to the serial line during the int3: no breakpoint line"));
    } else {
        t.note(format_args!("int3.resume: during the int3 the kernel sent \"{}\"... to the serial line", core::str::from_utf8(&head[..n.min(16)]).unwrap_or("?")));
    }
});

ktest!("pf.cr2", fn pf_cr2(t: &mut TestCtx) {
    let addr = user_addr(t, "pf.cr2");
    t.note(format_args!("pf.cr2: reading 0x{addr:016x} (chosen by this run's nonce)"));
    t.expect_eq("traps delivered", page_fault(addr, false), 1);
    t.expect_eq("vector", VEC.load(Ordering::SeqCst), 14);
    t.note(format_args!("pf.cr2: CR2 read by the cockpit's own handler 0x{:016x}", CR2.load(Ordering::SeqCst)));
    match mm::last_kill() {
        Some(k) => t.expect_eq("the fault address in mm::last_kill(): what idt::dispatch handed mm::fault from CR2, as mm::fault recorded it (neither may change it)", k.addr, addr),
        None => t.fail(format_args!("mm::last_kill is None after a page fault: mm::fault was never asked, or did not record its Kill")),
    }
    t.expect_eq("the frame's rip (the faulting instruction)", RIP.load(Ordering::SeqCst), AT.load(Ordering::SeqCst));
});

// ── idt.vector: a vector nobody named in advance, through set_handler ──────

/// The handler `idt.vector` registers: record the frame, then overwrite every
/// caller-saved register, as any Rust handler is free to.
fn on_vector(tf: &mut TrapFrame) {
    HITS.fetch_add(1, Ordering::SeqCst);
    VEC.store(tf.vector, Ordering::SeqCst);
    RIP.store(tf.rip, Ordering::SeqCst);
    FRAME.store(tf as *const TrapFrame as u64, Ordering::SeqCst);
    let eoi = APIC_EOI.load(Ordering::SeqCst);
    if eoi != 0 {
        // SAFETY: while `idt.vector` runs, `eoi` is the mapped APIC's EOI register.
        unsafe { core::ptr::write_volatile(eoi as *mut u32, 0) };
    }
    // SAFETY: only writes registers it declares clobbered.
    unsafe {
        asm!("mov rax, -1", "mov rcx, -1", "mov rdx, -1", "mov rsi, -1", "mov rdi, -1",
            "mov r8, -1", "mov r9, -1", "mov r10, -1", "mov r11, -1",
            out("rax") _, out("rcx") _, out("rdx") _, out("rsi") _, out("rdi") _,
            out("r8") _, out("r9") _, out("r10") _, out("r11") _, options(nomem, nostack));
    }
}

ktest!("idt.vector", fn idt_vector(t: &mut TestCtx) {
    let v = 32 + t.pick("idt.vector", 224);
    t.note(format_args!("idt.vector: set_handler({v}, f, 1), then a fixed interrupt on vector {v} sent by the local APIC to itself (vector chosen by this run's nonce)"));
    idt::set_handler(v as u8, on_vector, 1);
    let mut d = Dtr { limit: 0, base: 0 };
    // SAFETY: `sidt` stores 10 bytes into `d`.
    unsafe { asm!("sidt [{}]", in(reg) &mut d, options(nostack, preserves_flags)) };
    let (base, limit) = (d.base, d.limit as u64);
    if limit < v * 16 + 15 {
        t.fail(format_args!("the IDT register's limit {limit:#x} stops before gate {v}"));
        return;
    }
    // SAFETY: the IDTR names a live table that holds gate v; reading it has no effect.
    let lo = unsafe { core::ptr::read_volatile((base as *const u64).add(2 * v as usize)) };
    t.expect_eq("the gate's IST index after set_handler(v, f, 1)", (lo >> 32) & 7, 1);

    // The vector is raised by the local APIC, not by `int`: nothing in the
    // instruction stream names it, so only the gate's own entry knows it.
    // SAFETY: reading IA32_APIC_BASE has no effect.
    let apic_base = unsafe {
        let (lo, hi): (u32, u32);
        asm!("rdmsr", in("ecx") 0x1bu32, out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags));
        ((hi as u64) << 32) | lo as u64
    };
    let win = WINDOW.0.get() as u64;
    let Some(pte) = leaf_pte(win) else {
        t.fail(format_args!("the test could not map the local APIC (its own page is not mapped 4 KiB)"));
        return;
    };
    // SAFETY: `pte` maps the vehicle's own page, which nothing else uses; it
    // is pointed at the APIC's registers (uncached) and restored below.
    let old_pte = unsafe {
        let old = core::ptr::read_volatile(pte);
        core::ptr::write_volatile(pte, (apic_base & 0x000f_ffff_ffff_f000) | 0x1b | (old & (1 << 63)));
        asm!("invlpg [{}]", in(reg) win, options(nostack, preserves_flags));
        old
    };
    let reg = |off: u64| (win + off) as *mut u32;
    // SAFETY: the window maps the local APIC; these are its documented registers.
    let (svr, mask1, mask2) = unsafe { (core::ptr::read_volatile(reg(0xf0)), inb(0x21), inb(0xa1)) };
    outb(0x21, 0xff);
    outb(0xa1, 0xff);
    // SAFETY: as above: enable the APIC, then send vector v to this CPU only
    // (fixed delivery, the "self" shorthand); interrupts are off, so it waits
    // in the IRR until the `sti` below.
    unsafe {
        core::ptr::write_volatile(reg(0xf0), svr | 0x100);
        core::ptr::write_volatile(reg(0x310), 0);
        core::ptr::write_volatile(reg(0x300), 0x0004_4000 | v as u32);
        let mut spins = 0u32;
        while core::ptr::read_volatile(reg(0x300)) & (1 << 12) != 0 && spins < 1_000_000 {
            spins += 1;
        }
    }
    APIC_EOI.store(win + 0xb0, Ordering::SeqCst);
    let before = HITS.load(Ordering::SeqCst);
    let mut r = [0u64; 13];
    let (s0, s1): (u64, u64);
    // SAFETY: the pending interrupt arrives inside the window of 3-byte NOPs
    // (whose last byte, 0x00, names no vector 32..=255), runs the handler
    // registered above and returns; interrupts are off again after. Every
    // register it may touch is named.
    unsafe {
        asm!("lea {s1}, [rip + 2f]", "mov [rip + {lo}], {s1}", "lea {s1}, [rip + 3f]", "mov [rip + {hi}], {s1}",
            "mov {s0}, rsp", "sti", "2:",
            ".byte 0x0f, 0x1f, 0x00", ".byte 0x0f, 0x1f, 0x00", ".byte 0x0f, 0x1f, 0x00", ".byte 0x0f, 0x1f, 0x00",
            "3:", "cli", "mov {s1}, rsp",
            lo = sym WIN_LO, hi = sym WIN_HI, s0 = out(reg) s0, s1 = out(reg) s1,
            inout("rax") 0x2222_0000_0000_00a0u64 => r[0], inout("rcx") 0x2222_0000_0000_00c0u64 => r[1],
            inout("rdx") 0x2222_0000_0000_00d0u64 => r[2], inout("rsi") 0x2222_0000_0000_0050u64 => r[3],
            inout("rdi") 0x2222_0000_0000_00d1u64 => r[4], inout("r8") 0x2222_0000_0000_0008u64 => r[5],
            inout("r9") 0x2222_0000_0000_0009u64 => r[6], inout("r10") 0x2222_0000_0000_0010u64 => r[7],
            inout("r11") 0x2222_0000_0000_0011u64 => r[8], inout("r12") 0x2222_0000_0000_0012u64 => r[9],
            inout("r13") 0x2222_0000_0000_0013u64 => r[10], inout("r14") 0x2222_0000_0000_0014u64 => r[11],
            inout("r15") 0x2222_0000_0000_0015u64 => r[12]);
    }
    APIC_EOI.store(0, Ordering::SeqCst);
    // SAFETY: put back what this row changed: the APIC's enable, the 8259
    // masks and the window's own mapping.
    unsafe {
        core::ptr::write_volatile(reg(0xf0), svr);
        outb(0x21, mask1);
        outb(0xa1, mask2);
        core::ptr::write_volatile(pte, old_pte);
        asm!("invlpg [{}]", in(reg) win, options(nostack, preserves_flags));
    }
    t.expect_eq("interrupts delivered to the registered handler", HITS.load(Ordering::SeqCst) - before, 1);
    t.expect_eq("the frame's vector (what the entry for this gate pushed)", VEC.load(Ordering::SeqCst), v);
    let (rip, w0, w1) = (RIP.load(Ordering::SeqCst), WIN_LO.load(Ordering::SeqCst), WIN_HI.load(Ordering::SeqCst));
    if !(rip >= w0 && rip <= w1) {
        t.fail(format_args!("the frame's rip {rip:#x} is outside the instructions the interrupt arrived in [{w0:#x}, {w1:#x}]"));
    }
    let (_, ist_top, size) = shim::stacks();
    let frame = FRAME.load(Ordering::SeqCst);
    if !(frame < ist_top.0 && frame >= ist_top.0 - size) {
        t.fail(format_args!("the frame the handler got is at {frame:#x}, not on the IST 1 stack [{:#x}, {:#x}): the gate's IST index did not switch stacks", ist_top.0 - size, ist_top.0));
    }
    let want = [0xa0, 0xc0, 0xd0, 0x50, 0xd1, 0x08, 0x09, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15];
    for ((got, w), name) in r.iter().zip(want).zip(REG_NAMES) {
        let w = 0x2222_0000_0000_0000 | w;
        if *got != w {
            t.fail(format_args!("{name} after a handler that overwrote it: observed {got:#x}, expected {w:#x} (the entry code saves and restores every general register)"));
        }
    }
    t.expect_eq("rsp after the handler", s1, s0);
});

// ── idt.gates: the learner's table, decoded from what the CPU holds ────────
// (2026-10-18, TPM class B ruling: nothing in this lab double-faults, so
// without this row a `#DF` gate on IST 0 scored full marks.) Lab 04's `sgdt`
// decode, for the IDT: `sidt` names the table `idt::init` loaded, and every
// one of its 256 gates is read back as the CPU will read it.

#[repr(C, packed)]
struct Dtr {
    limit: u16,
    base: u64,
}

// Bracket the whole image (boot/linker.ld); only their addresses matter.
extern "C" {
    static __kernel_start: u8;
    static __kernel_end: u8;
}

ktest!("idt.gates", fn idt_gates(t: &mut TestCtx) {
    let mut d = Dtr { limit: 0, base: 0 };
    // SAFETY: `sidt` stores 10 bytes into `d`.
    unsafe { asm!("sidt [{}]", in(reg) &mut d, options(nostack, preserves_flags)) };
    let (base, limit) = (d.base, d.limit as u64);
    if limit < 256 * 16 - 1 {
        t.fail(format_args!("the IDT register's limit: observed {limit:#x}, expected at least 0xfff (256 gates of 16 bytes, minus one)"));
        return;
    }
    let (lo_img, hi_img) = (
        core::ptr::addr_of!(__kernel_start) as u64,
        core::ptr::addr_of!(__kernel_end) as u64,
    );
    let mut off = [0u64; 256];
    let mut bad = 0u64;
    for v in 0..256usize {
        // SAFETY: the IDTR names a live table of at least 256 gates the CPU
        // uses; reading a gate has no effect.
        let (lo, hi) = unsafe {
            let g = (base as *const u64).add(2 * v);
            (core::ptr::read_volatile(g), core::ptr::read_volatile(g.add(1)))
        };
        let ty = (lo >> 40) & 0xff;
        let sel = (lo >> 16) & 0xffff;
        off[v] = (lo & 0xffff) | (((lo >> 48) & 0xffff) << 16) | ((hi & 0xffff_ffff) << 32);
        let shaped = ty & 0x80 != 0 && ty & 0x1f == 0x0e && sel == kernel::gdt::sel::KCODE as u64;
        let inside = off[v] >= lo_img && off[v] < hi_img;
        if !(shaped && inside) {
            if bad == 0 {
                t.fail(format_args!("gate {v}: type byte {ty:#04x}, selector {sel:#x}, handler {:#018x} — expected a present interrupt gate (type 0xE) on the kernel code selector, its handler inside the kernel", off[v]));
            }
            bad += 1;
        }
        if v == 8 {
            t.note(format_args!("idt.gates: gate 8 (#DF) has type byte {ty:#04x}, selector {sel:#x}, IST index {}, handler {:#018x}", (lo >> 32) & 7, off[v]));
            t.expect_eq("gate 8 (#DF)'s IST index (1 = the TSS's first IST stack, the one lab 04 set aside)", (lo >> 32) & 7, 1);
        }
    }
    t.expect_eq("gates not shaped as above", bad, 0);
    let mut sorted = off;
    sorted.sort_unstable();
    let shared = sorted.windows(2).filter(|w| w[0] == w[1]).count() as u64;
    t.expect_eq("gates whose handler address another gate also uses (each vector needs its own entry, or dispatch cannot know the vector)", shared, 0);
});

ktest!("pf.error_code", fn pf_error_code(t: &mut TestCtx) {
    let read = user_addr(t, "pf.error_code/read");
    let mut write = user_addr(t, "pf.error_code/write");
    if write >> 12 == read >> 12 {
        write ^= 0x0000_0000_0010_0000; // another page, still user half and 8-aligned
    }
    let faults = if t.derived("pf.error_code/rw") & 1 == 1 {
        [(write, true), (read, false)]
    } else {
        [(read, false), (write, true)]
    };
    for (addr, is_write) in faults {
        let want = if is_write { mm::pf::WRITE } else { 0 };
        let kind = if is_write { "write" } else { "read" };
        t.note(format_args!("pf.error_code: a {kind} of 0x{addr:016x} (chosen by this run's nonce); the CPU's code is 0x{want:x}"));
        t.expect_eq("traps delivered", page_fault(addr, is_write), 1);
        t.expect_eq("vector", VEC.load(Ordering::SeqCst), 14);
        let err = ERR.load(Ordering::SeqCst);
        if err != want {
            t.fail(format_args!("the frame's error code after the {kind}: observed {err:#x}, expected {want:#x} (bit 1 is set by the CPU on a write and clear on a read; the frame must carry the code the CPU pushed)"));
        }
        match mm::last_kill() {
            Some(k) if k.addr == addr => {}
            Some(k) => t.fail(format_args!("the fault address in mm::last_kill() after the {kind}: observed {:#x}, expected {addr:#x} — idt::dispatch hands mm::fault CR2 on every #PF, a read or a write, and mm::fault records it unchanged", k.addr)),
            None => t.fail(format_args!("mm::last_kill is None after the {kind} fault: mm::fault was never asked, or did not record its Kill")),
        }
    }
});

ktest!("gp.reported", fn gp_reported(t: &mut TestCtx) {
    idt::set_handler(13, record, 0);
    let before = HITS.load(Ordering::SeqCst);
    // SAFETY: selector 0x58 lies past the GDT's limit, so loading it into DS
    // raises #GP(0x58) and DS is unchanged; the handler resumes at label 2.
    unsafe {
        asm!("lea {t}, [rip + 2f]", "mov [{r}], {t}", "lea {t}, [rip + 3f]", "mov [{at}], {t}",
            "mov {t:e}, 0x58", "3: mov ds, {t:e}", "2:",
            t = out(reg) _, r = in(reg) RESUME.as_ptr(), at = in(reg) AT.as_ptr());
    }
    t.expect_eq("traps delivered", HITS.load(Ordering::SeqCst) - before, 1);
    t.expect_eq("vector", VEC.load(Ordering::SeqCst), 13);
    t.expect_eq("the frame's error code (the selector)", ERR.load(Ordering::SeqCst), 0x58);
    t.expect_eq("the frame's rip", RIP.load(Ordering::SeqCst), AT.load(Ordering::SeqCst));
});

ktest!("fault.seam", fn fault_seam(t: &mut TestCtx) {
    const F: fn(VirtAddr, u64, &TrapFrame) -> FaultOutcome = mm::fault;
    let mut tf = TrapFrame { regs: [0; 15], vector: 14, error: 0, rip: 0xffff_ffff_8000_1234, cs: 0x08, rflags: 0x2, rsp: 0, ss: 0x10 };
    // No case address is page-aligned (2026-10-18): Kill.addr is the faulting
    // byte's address, not its page's.
    // Fix round (2026-10-18, sk05-forger-1, sk05-ladder-6): cases 2 and 4 share
    // an error code across the address halves, case 5 makes the kernel-half
    // test win over the present and write bits, and case 6 is a kernel-mode
    // access to the kernel half, which is not pf-kernel-addr.
    let cases = [
        (0x0000_0000_0040_0a38u64, 0x4u64, KillReason::PfUnmapped),
        (0xffff_8000_0000_1ff8, 0x5, KillReason::PfKernelAddr),
        (0x0000_0000_0040_0001, 0x7, KillReason::PfWriteRo),
        (0x0000_0000_0040_2b10, 0x5, KillReason::PfUnmapped),
        (0xffff_8000_0000_2a38, 0x6, KillReason::PfKernelAddr),
        (0xffff_8000_0000_3001, 0x2, KillReason::PfUnmapped),
    ];
    // Two more cases a run's nonce picks (2026-10-18, sk05-forger-2), from the
    // ten (error code, address half) pairs the six above leave out, each with
    // the lesson's reason, at a nonce-chosen byte of that half: one whose
    // reason is pf-unmapped and one whose reason is not. A table fitted to the
    // fixed six, whatever it answers for the rest, gets one of them wrong.
    const OPEN_UNMAPPED: [(u64, bool); 6] = [(0x0, false), (0x1, false), (0x2, false), (0x6, false), (0x0, true), (0x1, true)];
    const OPEN_OTHER: [(u64, bool, KillReason); 4] = [
        (0x3, false, KillReason::PfWriteRo), (0x3, true, KillReason::PfWriteRo),
        (0x4, true, KillReason::PfKernelAddr), (0x7, true, KillReason::PfKernelAddr),
    ];
    let (e1, h1) = OPEN_UNMAPPED[t.pick("fault.seam/a", 6) as usize];
    let picked = [(e1, h1, KillReason::PfUnmapped, "fault.seam/a"), {
        let (e, h, r) = OPEN_OTHER[t.pick("fault.seam/b", 4) as usize];
        (e, h, r, "fault.seam/b")
    }];
    let mut all = [(0u64, 0u64, KillReason::PfUnmapped); 8];
    all[..6].copy_from_slice(&cases);
    // The nonce also picks which of the two goes first.
    let order = if t.derived("fault.seam/order") & 1 == 1 { [1, 0] } else { [0, 1] };
    for (slot, k) in all[6..].iter_mut().zip(order) {
        let (err, kernel_half, reason, id) = picked[k];
        let off = t.derived(id) >> 8;
        let mut addr = if kernel_half {
            0xffff_8000_0000_0000 | (off & 0x0000_7fff_ffff_ffff)
        } else {
            0x0000_1000_0000_0000 | (off & 0x0000_0fff_ffff_ffff)
        };
        if addr & 0xfff == 0 {
            addr |= 0x8;
        }
        *slot = (addr, err, reason);
    }
    t.note(format_args!("fault.seam: cases 7 and 8 are mm::fault({:#x}, error {:#x}) and mm::fault({:#x}, error {:#x}) (chosen by this run's nonce)", all[6].0, all[6].1, all[7].0, all[7].1));
    for (addr, err, reason) in all {
        tf.error = err;
        match F(VirtAddr(addr), err, &tf) {
            FaultOutcome::Kill(k) => {
                if k.reason != reason {
                    t.fail(format_args!("mm::fault({addr:#x}, error {err:#x}): reason {}, expected {}", k.reason.as_str(), reason.as_str()));
                }
                t.expect_eq("Kill.rip", k.rip, tf.rip);
                t.expect_eq("Kill.addr (the address mm::fault was given, to the byte)", k.addr, addr);
            }
            FaultOutcome::Resolved => t.fail(format_args!("mm::fault resolved a fault nothing can map yet")),
        }
    }
});

fn prelude() {
    let (k, i, _) = shim::stacks();
    kernel::gdt::init(k, i);
    idt::init();
}

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    kernel::ktap::run_and_halt_after(prelude)
}
