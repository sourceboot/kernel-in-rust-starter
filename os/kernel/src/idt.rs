//! Lab 05 — the IDT, its trampolines, and the dispatch. This file is yours.
//!
//! All 256 gates point at trampolines; each leaves a `TrapFrame` on the stack
//! and calls `dispatch`. `#PF` asks `mm::fault` first, whatever is
//! registered: a resolved fault resumes, and only a `Kill` goes on to a
//! handler registered for vector 14 (or, with none, is reported and halts).
//! For every other vector a handler registered with `set_handler` runs
//! instead of the default; the default for `int3` reports and resumes, and
//! for everything else reports and halts.
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests call them.

/// What the trampolines leave on the stack, lowest address first.
/// `regs[0]` = r15 … `regs[14]` = rax
/// (r15, r14, r13, r12, r11, r10, r9, r8, rbp, rdi, rsi, rdx, rcx, rbx, rax).
#[repr(C)]
pub struct TrapFrame {
    pub regs: [u64; 15],
    pub vector: u64,
    pub error: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

// lab 05 step 1 — the trampolines and the common entry go here, above `init`.

/// Fill all 256 gates and load the table.
pub fn init() {
    todo!("lab 05 step 2 — init: point every gate at its trampoline, #DF on IST 1, lidt")
}

/// Route `vector` to `f`, on interrupt stack `ist` (0 = none). For `#PF`
/// (14), `f` runs only for a fault `mm::fault` answers with `Kill`.
pub fn set_handler(vector: u8, f: fn(&mut TrapFrame), ist: u8) {
    let _ = (vector, f, ist);
    todo!("lab 05 step 3 — set_handler: record the handler, set the gate's IST")
}

/// Every trap lands here with its frame.
pub fn dispatch(tf: &mut TrapFrame) {
    let _ = tf;
    todo!("lab 05 step 4 — dispatch: #PF to mm::fault first (a Kill then goes to a registered handler), else a registered handler; int3 resumes, the rest report and halt")
}
