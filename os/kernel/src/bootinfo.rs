//! BootInfo v2 — the frozen seam between any bootloader and this kernel.
//!
//! Flat, C-ABI, no pointers to bootloader-owned memory except where stated.
//! Nothing here references Limine types on purpose: the shim in `boot/` is the
//! only writer today, and a writer in another language (or a learner's own
//! loader, should a later course add one) must be able to produce it
//! byte-for-byte from this file alone.

/// "SB" + boot + version 2. The magic names the handoff protocol, not the
/// struct's length: fields appended at the end (as `kernel_phys`/`kernel_virt`/
/// `kernel_len` were, 2026-10-16) do not bump it. The kernel refuses to run on
/// a mismatch rather than trust a pointer nobody vouched for.
pub const BOOT_MAGIC: u64 = 0x5B00_7B00_7000_0002;

pub const MEMMAP_MAX: usize = 128;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FramebufferInfo {
    /// Virtual address, already mapped and writable when kernel_main runs.
    pub addr: u64,
    pub width: u64,
    pub height: u64,
    /// Bytes per row — never assume width * 4.
    pub pitch: u64,
    pub bpp: u16,
    pub _pad: [u8; 6],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MemRegion {
    pub base: u64,
    pub len: u64,
    /// Limine memmap type values (0 = usable). The numbering is part of the
    /// BootInfo contract now, whatever bootloader fills it in.
    pub kind: u32,
    pub _pad: u32,
}

#[repr(C)]
pub struct BootInfo {
    pub magic: u64,
    /// Higher-half direct-map offset: phys + hhdm_offset = virt.
    pub hhdm_offset: u64,
    pub fb: FramebufferInfo,
    pub memmap_len: u64,
    pub memmap: [MemRegion; MEMMAP_MAX],
    /// Where the bootloader put this kernel's image (appended 2026-10-16 for
    /// lab 07's `frame::init(memmap, kernel)`): its physical base, the
    /// virtual base it is linked at, and its length in bytes, page-rounded.
    pub kernel_phys: u64,
    pub kernel_virt: u64,
    pub kernel_len: u64,
}

impl BootInfo {
    /// The filled part of the memory map.
    pub fn memmap(&self) -> &[MemRegion] {
        &self.memmap[..(self.memmap_len as usize).min(MEMMAP_MAX)]
    }

    /// The kernel image's physical range, `[base, base + len)`.
    pub fn kernel_range(&self) -> PhysRange {
        PhysRange {
            start: PhysAddr(self.kernel_phys),
            end: PhysAddr(self.kernel_phys + self.kernel_len),
        }
    }
}

use crate::addr::{PhysAddr, PhysRange};
use core::sync::atomic::{AtomicPtr, Ordering};

/// The handoff, published once by the boot shim so any module can reach it
/// (the HHDM offset is the one most of them need).
static CURRENT: AtomicPtr<BootInfo> = AtomicPtr::new(core::ptr::null_mut());

/// Called by the boot shim after it has filled the BootInfo.
pub fn publish(bi: &'static BootInfo) {
    CURRENT.store(bi as *const BootInfo as *mut BootInfo, Ordering::Release);
}

/// The published handoff, if the shim has published one.
pub fn get() -> Option<&'static BootInfo> {
    let p = CURRENT.load(Ordering::Acquire);
    // SAFETY: `publish` only ever stores a pointer derived from a
    // `&'static BootInfo`, which stays valid and unmutated for the run.
    unsafe { p.as_ref() }
}

/// Higher-half direct-map offset (phys + offset = virt); 0 before `publish`.
pub fn hhdm_offset() -> u64 {
    get().map_or(0, |bi| bi.hhdm_offset)
}

// The seam is only a seam if the layout can't drift silently.
const _: () = assert!(core::mem::size_of::<FramebufferInfo>() == 40);
const _: () = assert!(core::mem::size_of::<MemRegion>() == 24);
const _: () = assert!(core::mem::size_of::<BootInfo>() == 8 + 8 + 40 + 8 + 24 * MEMMAP_MAX + 24);
