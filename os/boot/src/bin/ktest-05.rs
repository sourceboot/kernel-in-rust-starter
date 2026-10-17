//! Lab 05's vehicle: the IDT, its trampolines and the fault seam. `int3`
//! goes through the default dispatch and must resume with every register
//! intact; `#PF` and `#GP` go through a handler this vehicle registers with
//! `idt::set_handler`, which records the frame the trampoline built and
//! skips the faulting instruction. The `#PF` address (and read vs write) is
//! chosen per run from the nonce.

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
    for (got, w) in v.iter().zip(want) {
        t.expect_eq("a register after int3", *got, 0x1111_0000_0000_0000 | w);
    }
    t.expect_eq("rsp after int3", s1, s0);
});

ktest!("pf.cr2", fn pf_cr2(t: &mut TestCtx) {
    let addr = user_addr(t, "pf.cr2");
    t.note(format_args!("pf.cr2: reading 0x{addr:016x} (chosen by this run's nonce)"));
    t.expect_eq("traps delivered", page_fault(addr, false), 1);
    t.expect_eq("vector", VEC.load(Ordering::SeqCst), 14);
    t.expect_eq("CR2 in the #PF handler", CR2.load(Ordering::SeqCst), addr);
    t.expect_eq("the frame's rip (the faulting instruction)", RIP.load(Ordering::SeqCst), AT.load(Ordering::SeqCst));
});

ktest!("pf.error_code", fn pf_error_code(t: &mut TestCtx) {
    let addr = user_addr(t, "pf.error_code");
    let write = t.derived("pf.error_code/rw") & 1 == 1;
    let want = if write { mm::pf::WRITE } else { 0 };
    t.note(format_args!("pf.error_code: a {} of 0x{addr:016x}; the CPU's code is 0x{want:x}", if write { "write" } else { "read" }));
    t.expect_eq("traps delivered", page_fault(addr, write), 1);
    t.expect_eq("vector", VEC.load(Ordering::SeqCst), 14);
    t.expect_eq("the frame's error code", ERR.load(Ordering::SeqCst), want);
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
    let cases = [
        (0x0000_0000_0040_0000u64, 0x4u64, KillReason::PfUnmapped),
        (0xffff_8000_0000_1000, 0x5, KillReason::PfKernelAddr),
        (0x0000_0000_0040_0000, 0x7, KillReason::PfWriteRo),
    ];
    for (addr, err, reason) in cases {
        tf.error = err;
        match F(VirtAddr(addr), err, &tf) {
            FaultOutcome::Kill(k) => {
                t.expect(k.reason == reason, "the kill reason for this error code");
                t.expect_eq("Kill.rip", k.rip, tf.rip);
                t.expect_eq("Kill.addr", k.addr, addr);
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
