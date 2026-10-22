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

#[repr(C)]
struct ExecAddrResponse {
    revision: u64,
    physical_base: u64,
    virtual_base: u64,
}

#[used]
#[link_section = ".limine_requests"]
static EXEC_ADDR_REQ: ReqCell<Request<ExecAddrResponse>> = ReqCell(UnsafeCell::new(Request {
    id: [LIMINE_COMMON_MAGIC[0], LIMINE_COMMON_MAGIC[1], 0x71ba76863cc55f63, 0xb2644a48c516a487],
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
    kernel_phys: 0,
    kernel_virt: 0,
    kernel_len: 0,
}));

// Bracket the whole image (boot/linker.ld).
extern "C" {
    static __kernel_start: u8;
    static __kernel_end: u8;
}

fn response<R>(req: &ReqCell<Request<R>>) -> Option<&'static R> {
    // SAFETY: the request is a 'static the bootloader filled before entry;
    // nothing writes it after, and a volatile read keeps the compiler from
    // assuming the null it was initialised with.
    let ptr = unsafe { core::ptr::read_volatile(&(*req.0.get()).response) };
    if ptr.is_null() {
        None
    } else {
        // SAFETY: a non-null response points into bootloader-reclaimable
        // memory Limine filled, which nothing reclaims in this course.
        Some(unsafe { &*ptr })
    }
}

/// The line the grader reads as "this kernel reached its entry point" (its
/// boot marker). A failing check whose serial output lacks it is reported
/// as a kernel that never started, so it is printed once, first.
const BOOT_MARKER: &str = "[BOOT] reached kernel_entry";

/// Print the boot marker, then validate the Limine handoff and fill BootInfo.
/// Halts (never returns) if the bootloader gave us nothing to stand on.
///
/// Every binary's `_start` calls this before anything else, so the marker is
/// the kernel's first line on COM1 and precedes all of your code. It goes out
/// through the provided handshake writer (kernel/src/handshake.rs): its own
/// port I/O, no UART set-up, nothing of your serial.rs, so it is there at
/// every lab, before lab 02's driver exists, and leaves the device as it
/// found it.
///
/// # Safety
/// Call once, from `_start`, before anything reads BOOT_INFO.
pub unsafe fn boot_info() -> &'static BootInfo {
    kernel::handshake::line(BOOT_MARKER);

    let supported = core::ptr::read_volatile(&(*BASE_REVISION.0.get())[2]) == 0;
    if !supported {
        kernel::hcf();
    }

    let bi = &mut *BOOT_INFO.0.get();

    let (Some(fb), Some(hhdm), Some(mm), Some(exec)) = (
        response(&FB_REQ),
        response(&HHDM_REQ),
        response(&MEMMAP_REQ),
        response(&EXEC_ADDR_REQ),
    ) else {
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

    bi.kernel_phys = exec.physical_base;
    bi.kernel_virt = exec.virtual_base;
    bi.kernel_len = core::ptr::addr_of!(__kernel_end) as u64 - core::ptr::addr_of!(__kernel_start) as u64;

    bi.magic = BOOT_MAGIC;

    let bi: &'static BootInfo = &*BOOT_INFO.0.get();
    kernel::bootinfo::publish(bi);
    bi
}

// ── Stacks the vehicles hand to `gdt::init` ─────────────────────────────────

const STACK_BYTES: usize = 16 * 1024;

#[repr(C, align(16))]
struct Stack(UnsafeCell<[u8; STACK_BYTES]>);
// SAFETY: each stack is used only by the CPU, as the stack a TSS field names.
unsafe impl Sync for Stack {}

static KSTACK: Stack = Stack(UnsafeCell::new([0; STACK_BYTES]));
static IST1: Stack = Stack(UnsafeCell::new([0; STACK_BYTES]));

/// The tops of two 16 KiB stacks in .bss: the kernel stack for `rsp0` and the
/// IST1 stack for `#DF`, plus their size.
#[allow(dead_code)]
pub fn stacks() -> (kernel::addr::VirtAddr, kernel::addr::VirtAddr, u64) {
    let top = |s: &Stack| kernel::addr::VirtAddr(s.0.get() as u64 + STACK_BYTES as u64);
    (top(&KSTACK), top(&IST1), STACK_BYTES as u64)
}
