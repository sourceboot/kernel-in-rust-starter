//! The provided boot shim: Limine protocol in, BootInfo v2 out.
//!
//! Hand-rolled — no `limine` crate — so the scaffold has zero dependencies.
//! Constants transcribed from limine-protocol include/limine.h, commit pinned
//! in boot/limine/PIN.md. Arc B replaces this whole crate (and Limine) with
//! the learner's own bootloader producing the same BootInfo.
//!
//! Shared by every bin target in this package (`#[path]`-included, so each
//! binary carries its own copy of the request statics — Limine scans the
//! loaded executable, and there is exactly one per boot).

use kernel::bootinfo::{BootInfo, FramebufferInfo, MemRegion, BOOT_MAGIC, MEMMAP_MAX};

// ── Limine request plumbing ─────────────────────────────────────────────────
// The bootloader finds these by scanning the .limine_requests section and
// WRITES the response pointers into them before _start runs. `UnsafeCell`
// makes that external mutation sound; reads go through read_volatile.

use core::cell::UnsafeCell;

#[repr(transparent)]
struct ReqCell<T>(UnsafeCell<T>);
// Statics require Sync. Single-threaded at this point by definition: the
// bootloader wrote before entry, and no second CPU exists yet.
unsafe impl<T> Sync for ReqCell<T> {}

const LIMINE_COMMON_MAGIC: [u64; 2] = [0xc7b1dd30df4c8b88, 0x0a82e883a194f07b];

#[used]
#[link_section = ".limine_requests_start"]
static REQUESTS_START: [u64; 4] =
    [0xf6b8f4b39de7d1ae, 0xfab91a6940fcb9cf, 0x785c6ed015d3e316, 0x181e920a7852b9d9];

#[used]
#[link_section = ".limine_requests_end"]
static REQUESTS_END: [u64; 2] = [0xadc0e0531bb10d03, 0x9572709f31764c62];

// Base revision 3; the loader zeroes the last word when it supports it.
#[used]
#[link_section = ".limine_requests"]
static BASE_REVISION: ReqCell<[u64; 3]> =
    ReqCell(UnsafeCell::new([0xf9562b2d5c95a6c8, 0x6a7b384944536bdc, 3]));

#[repr(C)]
struct Request<R> {
    id: [u64; 4],
    revision: u64,
    response: *mut R,
}

#[repr(C)]
struct FbResponse {
    revision: u64,
    framebuffer_count: u64,
    framebuffers: *mut *mut LimineFramebuffer,
}

#[repr(C)]
struct LimineFramebuffer {
    address: *mut u8,
    width: u64,
    height: u64,
    pitch: u64,
    bpp: u16,
    memory_model: u8,
    red_mask_size: u8,
    red_mask_shift: u8,
    green_mask_size: u8,
    green_mask_shift: u8,
    blue_mask_size: u8,
    blue_mask_shift: u8,
    unused: [u8; 7],
    edid_size: u64,
    edid: *mut u8,
    // response revision 1 fields (mode_count, modes) exist past here; the
    // shim never reads them, so they are deliberately not declared.
}

#[repr(C)]
struct HhdmResponse {
    revision: u64,
    offset: u64,
}

#[repr(C)]
struct MemmapEntry {
    base: u64,
    length: u64,
    kind: u64,
}

#[repr(C)]
struct MemmapResponse {
    revision: u64,
    entry_count: u64,
    entries: *mut *mut MemmapEntry,
}

#[used]
#[link_section = ".limine_requests"]
static FB_REQ: ReqCell<Request<FbResponse>> = ReqCell(UnsafeCell::new(Request {
    id: [LIMINE_COMMON_MAGIC[0], LIMINE_COMMON_MAGIC[1], 0x9d5827dcd881dd75, 0xa3148604f6fab11b],
    revision: 0,
    response: core::ptr::null_mut(),
}));

#[used]
#[link_section = ".limine_requests"]
static HHDM_REQ: ReqCell<Request<HhdmResponse>> = ReqCell(UnsafeCell::new(Request {
    id: [LIMINE_COMMON_MAGIC[0], LIMINE_COMMON_MAGIC[1], 0x48dcf1cb8ad2b852, 0x63984e959a98244b],
    revision: 0,
    response: core::ptr::null_mut(),
}));

#[used]
#[link_section = ".limine_requests"]
static MEMMAP_REQ: ReqCell<Request<MemmapResponse>> = ReqCell(UnsafeCell::new(Request {
    id: [LIMINE_COMMON_MAGIC[0], LIMINE_COMMON_MAGIC[1], 0x67cf3d9d378a806f, 0xe304acdfc50c3c62],
    revision: 0,
    response: core::ptr::null_mut(),
}));

// ── The handoff ─────────────────────────────────────────────────────────────

// Zeroed BootInfo in .bss; filled once, then handed to the kernel by pointer.
static BOOT_INFO: ReqCell<BootInfo> = ReqCell(UnsafeCell::new(BootInfo {
    magic: 0,
    hhdm_offset: 0,
    fb: FramebufferInfo { addr: 0, width: 0, height: 0, pitch: 0, bpp: 0, _pad: [0; 6] },
    memmap_len: 0,
    memmap: [MemRegion { base: 0, len: 0, kind: 0, _pad: 0 }; MEMMAP_MAX],
}));

fn response<R>(req: &ReqCell<Request<R>>) -> Option<&'static R> {
    let ptr = unsafe { core::ptr::read_volatile(&(*req.0.get()).response) };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

/// Validate the Limine handoff and fill BootInfo. Halts (never returns) if
/// the bootloader gave us nothing to stand on.
///
/// # Safety
/// Call once, from `_start`, before anything reads BOOT_INFO.
pub unsafe fn boot_info() -> &'static BootInfo {
    let supported = core::ptr::read_volatile(&(*BASE_REVISION.0.get())[2]) == 0;
    if !supported {
        kernel::hcf();
    }

    let bi = &mut *BOOT_INFO.0.get();

    let (Some(fb), Some(hhdm), Some(mm)) =
        (response(&FB_REQ), response(&HHDM_REQ), response(&MEMMAP_REQ))
    else {
        kernel::hcf();
    };

    if fb.framebuffer_count == 0 {
        kernel::hcf();
    }
    let f = &**fb.framebuffers;
    bi.fb = FramebufferInfo {
        addr: f.address as u64,
        width: f.width,
        height: f.height,
        pitch: f.pitch,
        bpp: f.bpp,
        _pad: [0; 6],
    };

    bi.hhdm_offset = hhdm.offset;

    let n = (mm.entry_count as usize).min(MEMMAP_MAX);
    for i in 0..n {
        let e = &**mm.entries.add(i);
        bi.memmap[i] = MemRegion { base: e.base, len: e.length, kind: e.kind as u32, _pad: 0 };
    }
    bi.memmap_len = n as u64;

    bi.magic = BOOT_MAGIC;

    &*BOOT_INFO.0.get()
}
