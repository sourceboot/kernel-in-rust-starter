//! Lab 04 — the GDT and the TSS. This file is yours.
//!
//! The layout is the course's, fixed by what lab 09's `sysret` needs:
//! `0x00` null · `0x08` kernel code · `0x10` kernel data · `0x18` user
//! code32 placeholder (never loaded) · `0x20` user data (DPL 3) · `0x28` user
//! code64 (DPL 3) · `0x30` the TSS (a 16-byte descriptor).
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests call them.

use crate::addr::VirtAddr;

/// The selectors, RPL included for the user ones.
pub mod sel {
    pub const KCODE: u16 = 0x08;
    pub const KDATA: u16 = 0x10;
    pub const UDATA: u16 = 0x20 | 3;
    pub const UCODE: u16 = 0x28 | 3;
    pub const TSS: u16 = 0x30;
}

/// The 64-bit task-state segment (SDM Vol. 3A §8.7): 104 bytes, packed, with
/// `rsp0` and the seven `ist` slots where the manual puts them.
#[repr(C, packed)]
pub struct Tss {
    // lab 04 step 2 — lay out the fields.
}

/// Build the table, load it, reload every segment register, load the TSS.
pub fn init(kstack_top: VirtAddr, ist1_top: VirtAddr) {
    let _ = (kstack_top, ist1_top);
    todo!("lab 04 step 3 — init: build the eight slots, lgdt, reload CS and the data segments, ltr")
}

/// The stack the CPU switches to on a ring 3 → 0 transition.
pub fn set_rsp0(top: VirtAddr) {
    let _ = top;
    todo!("lab 04 step 4 — set_rsp0: write the TSS's rsp0")
}
