//! BootInfo v2 — the frozen seam between any bootloader and this kernel.
//!
//! Flat, C-ABI, no pointers to bootloader-owned memory except where stated.
//! Arc B's hand-written bootloader must be able to produce this struct
//! byte-for-byte, which is why nothing here references Limine types.

/// "SB" + boot + version 2. The kernel refuses to run on a mismatch rather
/// than trust a pointer nobody vouched for.
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
}

// The seam is only a seam if the layout can't drift silently.
const _: () = assert!(core::mem::size_of::<FramebufferInfo>() == 40);
const _: () = assert!(core::mem::size_of::<MemRegion>() == 24);
const _: () = assert!(core::mem::size_of::<BootInfo>() == 8 + 8 + 40 + 8 + 24 * MEMMAP_MAX);
