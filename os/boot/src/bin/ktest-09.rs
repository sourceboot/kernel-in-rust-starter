//! Lab 09's vehicle: ring 3 and the Linux syscall ABI. Two halves (CONTRACT
//! §A.3): the course's program `hi` reports `ring3.cs.rpl` itself, through
//! the learner's `write(1)`; the seven kernel rows run small ring-3 probes
//! this vehicle carries (`PROBE`, below) and read back what each one stored.
//!
//! Every program runs in a fresh address space built by this vehicle's own
//! flat loader (the learner's loader is lab 12's): the file's bytes copied to
//! `0x1000000` in frames mapped USER (read-write: W^X is lab 13's), a
//! 16-page stack under `0x7fff_ffff_f000` with an unmapped guard page below
//! it, and CONTRACT §A.5's initial stack (argc, argv, envp, auxv). The
//! learner's `user::run` enters it and returns when `exit` — or this
//! vehicle's trap handler, on a fault from ring 3 — calls `user::leave`.
//! IRQs stay off: the PIC is masked, so nothing preempts a probe, and every
//! probe is straight-line code that ends in `exit` or a fault.
//!
//! Three rows reach past what a good program shows (TPM ruling 2026-10-18,
//! slice 3 unit vrow09): `sys.write.efault` passes pointers the program may
//! not use and checks each is refused; `sys.entry.flags` reads RFLAGS inside
//! the kernel's own copy of a `write` buffer, through a hardware data
//! breakpoint this vehicle arms on the buffer (the vehicle's `#DB` handler
//! is the vehicle's path inside the syscall), and SFMASK's TF bit;
//! `sse.usable` reads RFLAGS at a program's entry, runs a program that
//! dirties the x87/SSE state first, and leaves CR0.TS set before the next.

#![no_std]
#![no_main]

#[path = "../shim.rs"]
mod shim;

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use kernel::addr::{PhysAddr, VirtAddr};
use kernel::frame::{self, Frame};
use kernel::idt::{self, TrapFrame};
use kernel::ktap::{self, TestCtx};
use kernel::ktest;
use kernel::paging::{self, flags, AddressSpace};
use kernel::{bootinfo, gdt, serial, syscall, user};

/// The course's program (CONTRACT §A.8), built by the pinned toolchain.
static HI: &[u8] = include_bytes!("../../../user/bin/hi");

const BASE: u64 = 0x100_0000;
const STACK_TOP: u64 = 0x7fff_ffff_f000;
const STACK_PAGES: u64 = 16;
/// The row `hi` reports, and the KTAP number it is handed (`argv[2]`).
const HI_ID: &str = "ring3.cs.rpl";
const HI_INDEX: &str = "1";
/// Filled into the stack below the initial vector, to see how much `hi` used.
const POISON: u8 = 0xa5;

// ── The probes: one page of position-independent ring-3 code ─────────────

/// The two lines the write probe sends (each in one call).
macro_rules! msg1 { () => { "# sys.write.console: this line left ring 3 through write(1)" } }
macro_rules! msg2 { () => { "# sys.write.console: and this one through write(2)" } }
const N1: usize = concat!(msg1!(), "\n").len();
const N2: usize = concat!(msg2!(), "\n").len();
/// The line the flags probe writes with DF set.
macro_rules! msg3 { () => { "# sys.entry.flags: this line was written with DF set in ring 3" } }
const N3: usize = concat!(msg3!(), "\n").len();
//
// Each entry does one thing, stores what the kernel answered in the result
// words at the end of the blob (its own page, mapped read-write), and exits
// with `exit(0)` — except `gp`, which never gets that far.
core::arch::global_asm!(
    ".section .rodata.ktest09_probe, \"a\"",
    ".balign 16",
    ".globl ktest09_probe_start",
    "ktest09_probe_start:",
    // write(1, m1, n1); write(2, m2, n2); write(9, m1, 1) — fd 9 is not open
    ".globl ktest09_p_write",
    "ktest09_p_write:",
    "lea rsi, [rip + 81f]",
    "mov edi, 1",
    "mov edx, {n1}",
    "mov eax, 1",
    "syscall",
    "mov [rip + 90f], rax",
    "lea rsi, [rip + 83f]",
    "mov edi, 2",
    "mov edx, {n2}",
    "mov eax, 1",
    "syscall",
    "mov [rip + 90f + 8], rax",
    "lea rsi, [rip + 81f]",
    "mov edi, 9",
    "mov edx, 1",
    "mov eax, 1",
    "syscall",
    "mov [rip + 90f + 16], rax",
    // CS and SS as this code sees them after three `sysret`s
    "mov eax, cs",
    "mov [rip + 90f + 120], rax",
    "mov eax, ss",
    "mov [rip + 90f + 128], rax",
    "jmp 70f",
    // syscall(1000); syscall(-1); getpid()
    ".globl ktest09_p_enosys",
    "ktest09_p_enosys:",
    "mov eax, 1000",
    "syscall",
    "mov [rip + 90f + 24], rax",
    "mov rax, -1",
    "syscall",
    "mov [rip + 90f + 32], rax",
    "mov eax, 39",
    "syscall",
    "mov [rip + 90f + 40], rax",
    "jmp 70f",
    // ioctl(1, TIOCGWINSZ, &ws)
    ".globl ktest09_p_winsz",
    "ktest09_p_winsz:",
    "mov edi, 1",
    "mov esi, 0x5413",
    "lea rdx, [rip + 90f + 56]",
    "mov eax, 16",
    "syscall",
    "mov [rip + 90f + 48], rax",
    "jmp 70f",
    // MXCSR and the x87 control word as the program finds them; 1.5 + 1.5
    // in a scalar; {1,2,3,4} + itself packed through an aligned load/store.
    ".globl ktest09_p_sse",
    "ktest09_p_sse:",
    "stmxcsr [rip + 90f + 64]",
    "fnstcw [rip + 90f + 72]",
    "mov eax, 0x3fc00000",
    "movd xmm0, eax",
    "addss xmm0, xmm0",
    "movd eax, xmm0",
    "mov [rip + 90f + 80], eax",
    "movaps xmm1, [rip + 85f]",
    "addps xmm1, xmm1",
    "movaps [rip + 90f + 96], xmm1",
    "mov qword ptr [rip + 90f + 88], 1",
    "jmp 70f",
    // a privileged instruction at CPL 3: #GP(0), never the exit below
    ".globl ktest09_p_gp",
    "ktest09_p_gp:",
    "mov qword ptr [rip + 90f + 112], 1",
    ".globl ktest09_p_gp_insn",
    "ktest09_p_gp_insn:",
    "hlt",
    "70:",
    "xor edi, edi",
    "mov eax, 60",
    "syscall",
    "ud2",
    // (after the shared exit, so the probes above keep their addresses)
    // the program before the SSE probe: leaves the x87 control word, the
    // x87 stack and MXCSR changed, for the next program's `enter` to reset
    ".globl ktest09_p_dirty",
    "ktest09_p_dirty:",
    // first, RFLAGS as `enter` left it
    "pushfq",
    "pop rax",
    "mov [rip + 90f + {s_flags}], rax",
    "fldcw [rip + 88f]",
    "fld1",
    "ldmxcsr [rip + 89f]",
    "jmp 70b",
    // write(1, line, n) with DF set: the vehicle's breakpoint on the line
    // reads RFLAGS where the kernel copies it (DF is noted only, see the row)
    ".globl ktest09_p_flags",
    "ktest09_p_flags:",
    "std",
    "pushfq",
    "pop rax",
    "mov [rip + 90f + {f_user}], rax",
    "lea rsi, [rip + 86f]",
    "mov edi, 1",
    "mov edx, {n3}",
    "mov eax, 1",
    "syscall",
    "cld",
    "mov [rip + 90f + {f_write}], rax",
    "jmp 70b",
    // pointers the program may not pass (the vehicle fills the parameter
    // words): write(1, <HHDM>, 16), write(1, <HHDM>, 0), ioctl(1,
    // TIOCGWINSZ, <a kernel variable>), write(1, <runs off the stack's top>,
    // 4096), write(1, NULL, 16), write(1, <unmapped user half>, 16)
    ".globl ktest09_p_badptr",
    "ktest09_p_badptr:",
    "mov edi, 1",
    "mov rsi, [rip + 90f + {p_hhdm}]",
    "mov edx, 16",
    "mov eax, 1",
    "syscall",
    "mov [rip + 90f + {b_hhdm}], rax",
    "mov edi, 1",
    "mov rsi, [rip + 90f + {p_hhdm}]",
    "xor edx, edx",
    "mov eax, 1",
    "syscall",
    "mov [rip + 90f + {b_hhdm0}], rax",
    "mov edi, 1",
    "mov esi, 0x5413",
    "mov rdx, [rip + 90f + {p_canary}]",
    "mov eax, 16",
    "syscall",
    "mov [rip + 90f + {b_ioctl}], rax",
    "mov edi, 1",
    "mov rsi, [rip + 90f + {p_straddle}]",
    "mov edx, 4096",
    "mov eax, 1",
    "syscall",
    "mov [rip + 90f + {b_straddle}], rax",
    "mov edi, 1",
    "xor esi, esi",
    "mov edx, 16",
    "mov eax, 1",
    "syscall",
    "mov [rip + 90f + {b_null}], rax",
    "mov edi, 1",
    "mov rsi, [rip + 90f + {p_unmapped}]",
    "mov edx, 16",
    "mov eax, 1",
    "syscall",
    "mov [rip + 90f + {b_unmapped}], rax",
    "jmp 70b",
    concat!("81: .ascii \"", msg1!(), "\\n\""),
    concat!("83: .ascii \"", msg2!(), "\\n\""),
    // 8-aligned, so one 8-byte breakpoint covers its first word
    ".balign 8",
    ".globl ktest09_flags_line",
    "ktest09_flags_line:",
    concat!("86: .ascii \"", msg3!(), "\\n\""),
    ".balign 16",
    "85: .float 1.0, 2.0, 3.0, 4.0",
    "88: .short 0x027f",
    ".balign 4",
    "89: .long 0x7f80",
    ".balign 16",
    ".globl ktest09_probe_results",
    "ktest09_probe_results:",
    "90: .zero {len}",
    ".globl ktest09_probe_end",
    "ktest09_probe_end:",
    ".previous",
    n1 = const N1,
    n2 = const N2,
    n3 = const N3,
    s_flags = const res::SSE_RFLAGS,
    f_user = const res::FLAGS_USER,
    f_write = const res::FLAGS_WRITE,
    p_hhdm = const res::P_HHDM,
    p_canary = const res::P_CANARY,
    p_straddle = const res::P_STRADDLE,
    p_unmapped = const res::P_UNMAPPED,
    b_hhdm = const res::B_HHDM,
    b_hhdm0 = const res::B_HHDM0,
    b_ioctl = const res::B_IOCTL,
    b_straddle = const res::B_STRADDLE,
    b_null = const res::B_NULL,
    b_unmapped = const res::B_UNMAPPED,
    len = const res::LEN,
);

extern "C" {
    static ktest09_probe_start: u8;
    static ktest09_probe_end: u8;
    static ktest09_probe_results: u8;
    static ktest09_p_write: u8;
    static ktest09_p_enosys: u8;
    static ktest09_p_winsz: u8;
    static ktest09_p_sse: u8;
    static ktest09_p_dirty: u8;
    static ktest09_p_flags: u8;
    static ktest09_p_badptr: u8;
    static ktest09_flags_line: u8;
    static ktest09_p_gp: u8;
    static ktest09_p_gp_insn: u8;
}

/// Result words (offsets into the results area, in bytes).
mod res {
    pub const WRITE1: usize = 0;
    pub const WRITE2: usize = 8;
    pub const WRITE_BADFD: usize = 16;
    pub const NOSYS_1000: usize = 24;
    pub const NOSYS_M1: usize = 32;
    pub const GETPID: usize = 40;
    pub const IOCTL: usize = 48;
    pub const WINSZ: usize = 56;
    pub const MXCSR: usize = 64;
    pub const FCW: usize = 72;
    pub const SCALAR: usize = 80;
    pub const SSE_DONE: usize = 88;
    pub const PACKED: usize = 96;
    pub const GP_REACHED: usize = 112;
    pub const SYSRET_CS: usize = 120;
    pub const SYSRET_SS: usize = 128;
    pub const SSE_RFLAGS: usize = 136;
    pub const FLAGS_USER: usize = 144;
    pub const FLAGS_WRITE: usize = 152;
    // parameter words the vehicle fills before the badptr probe runs
    pub const P_HHDM: usize = 160;
    pub const P_CANARY: usize = 168;
    pub const P_STRADDLE: usize = 176;
    pub const P_UNMAPPED: usize = 184;
    // the badptr probe's answers (pre-filled with NOT_REACHED)
    pub const B_HHDM: usize = 192;
    pub const B_HHDM0: usize = 200;
    pub const B_IOCTL: usize = 208;
    pub const B_STRADDLE: usize = 216;
    pub const B_NULL: usize = 224;
    pub const B_UNMAPPED: usize = 232;
    pub const LEN: usize = 240;
}

fn sym(s: &u8) -> u64 {
    s as *const u8 as u64
}

fn probe() -> &'static [u8] {
    // SAFETY: the two labels bracket the blob in .rodata.
    unsafe {
        let (a, b) = (sym(&ktest09_probe_start), sym(&ktest09_probe_end));
        core::slice::from_raw_parts(a as *const u8, (b - a) as usize)
    }
}

/// Where a probe label lands once the blob sits at BASE.
fn at(label: &u8) -> u64 {
    // SAFETY: only the label's address is used.
    BASE + sym(label) - unsafe { sym(&ktest09_probe_start) }
}

// ── The vehicle's flat loader ────────────────────────────────────────────

fn hhdm<T>(pa: u64) -> *mut T {
    paging::hhdm(PhysAddr(pa)).as_mut_ptr()
}

fn rd<const N: usize>(b: &[u8], off: usize) -> u64 {
    let mut v = [0u8; 8];
    v[..N].copy_from_slice(&b[off..off + N]);
    u64::from_le_bytes(v)
}

/// A program placed and ready to enter.
struct Proc {
    aspace: AddressSpace,
    entry: u64,
    rsp: u64,
    image_pages: u64,
    /// The lowest byte the initial vector occupies (the stack-use
    /// measurement scans below it).
    vec_low: u64,
}

struct Elf {
    entry: u64,
    phoff: u64,
    phnum: u64,
}

/// A lab 09–12 program: ET_EXEC x86-64, exactly one PT_LOAD, file offset 0
/// at BASE, filesz == memsz (user/README.md, layout `flat`).
fn parse_flat(img: &[u8]) -> Result<Elf, &'static str> {
    if img.len() < 64 || img[0..4] != *b"\x7fELF" || img[4] != 2 || img[5] != 1 {
        return Err("not a 64-bit little-endian ELF");
    }
    if rd::<2>(img, 16) != 2 || rd::<2>(img, 18) != 0x3e {
        return Err("not ET_EXEC x86-64");
    }
    let (entry, phoff, phent, phnum) = (rd::<8>(img, 24), rd::<8>(img, 32), rd::<2>(img, 54), rd::<2>(img, 56));
    let mut loads = 0;
    for i in 0..phnum {
        let p = (phoff + i * phent) as usize;
        if p + 56 > img.len() {
            return Err("program headers past the end of the file");
        }
        if rd::<4>(img, p) == 1 {
            loads += 1;
            let (off, va, filesz, memsz) = (rd::<8>(img, p + 8), rd::<8>(img, p + 16), rd::<8>(img, p + 32), rd::<8>(img, p + 40));
            if off != 0 || va != BASE || filesz != memsz || filesz > img.len() as u64 {
                return Err("the PT_LOAD is not file offset 0 -> 0x1000000 with filesz == memsz");
            }
        }
    }
    if loads != 1 {
        return Err("not exactly one PT_LOAD");
    }
    Ok(Elf { entry, phoff, phnum })
}

/// Map `n` fresh frames at `va..` in `a`, USER and writable.
fn map_pages(a: &mut AddressSpace, va: u64, n: u64) -> Result<(), &'static str> {
    for i in 0..n {
        let f = frame::alloc().ok_or("no frame")?;
        a.map(VirtAddr(va + i * 4096), f, flags::PRESENT | flags::WRITE | flags::USER).map_err(|_| "map failed")?;
    }
    Ok(())
}

/// The physical frame behind a page this vehicle mapped.
fn frame_of(a: &AddressSpace, va: u64) -> u64 {
    a.translate(VirtAddr(va)).map_or(0, |(pa, _)| pa.0 & !0xfff)
}

/// Load a flat image (with `entry_override` for a probe's entry label) and
/// write §A.5's initial stack for `argv`.
fn load(img: &[u8], entry_override: Option<u64>, argv: &[&[u8]]) -> Result<Proc, &'static str> {
    let elf = if entry_override.is_some() { Elf { entry: 0, phoff: 0, phnum: 0 } } else { parse_flat(img)? };
    let mut aspace = AddressSpace::new_kernel();
    let pages = (img.len() as u64).div_ceil(4096);
    map_pages(&mut aspace, BASE, pages)?;
    for i in 0..pages {
        let pa = frame_of(&aspace, BASE + i * 4096);
        let chunk = &img[(i * 4096) as usize..img.len().min(((i + 1) * 4096) as usize)];
        // SAFETY: a frame just allocated for this page, written through the HHDM.
        unsafe { core::ptr::copy_nonoverlapping(chunk.as_ptr(), hhdm::<u8>(pa), chunk.len()) };
    }
    let low = STACK_TOP - STACK_PAGES * 4096;
    map_pages(&mut aspace, low, STACK_PAGES)?;
    // Poison every stack page, so what the program touched shows afterwards.
    for i in 0..STACK_PAGES {
        // SAFETY: a stack frame just allocated, written through the HHDM.
        unsafe { core::ptr::write_bytes(hhdm::<u8>(frame_of(&aspace, low + i * 4096)), POISON, 4096) };
    }

    // The initial stack (CONTRACT §A.5), built in the top page through the HHDM.
    let top_page = STACK_TOP - 4096;
    let top_frame = frame_of(&aspace, top_page);
    let put = |va: u64, bytes: &[u8]| {
        // SAFETY: `va..va+len` lies in the top stack page (checked by the
        // layout below: strings + vector < 1 KiB), reached through the HHDM.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), hhdm::<u8>(top_frame + (va - top_page)), bytes.len()) };
    };
    let mut sp = STACK_TOP;
    // 16 bytes for AT_RANDOM, from the nonce when there is one.
    let seed = ktap::nonce().map_or(0x5b00_7b00_5eed_0000, |n| ktap::derive(&n, "exec.random"));
    sp -= 16;
    let random = sp;
    put(random, &seed.to_le_bytes());
    put(random + 8, &ktap::splitmix64(seed).to_le_bytes());
    // The strings: argv[0] doubles as AT_EXECFN.
    let mut ptrs = [0u64; 4];
    debug_assert!(argv.len() <= ptrs.len());
    for (i, s) in argv.iter().enumerate().rev() {
        sp -= s.len() as u64 + 1;
        put(sp, s);
        put(sp + s.len() as u64, &[0]);
        ptrs[i] = sp;
    }
    let execfn = ptrs[0];
    let phdr = BASE + elf.phoff;
    // A probe gets only AT_NULL; a program gets every pair §A.5 lists.
    let full: [(u64, u64); 13] = [
        (3, phdr),       // AT_PHDR
        (4, 56),         // AT_PHENT
        (5, elf.phnum),  // AT_PHNUM
        (6, 4096),       // AT_PAGESZ
        (9, elf.entry),  // AT_ENTRY
        (11, 0),         // AT_UID
        (12, 0),         // AT_EUID
        (13, 0),         // AT_GID
        (14, 0),         // AT_EGID
        (23, 0),         // AT_SECURE
        (25, random),    // AT_RANDOM
        (31, execfn),    // AT_EXECFN
        (0, 0),          // AT_NULL
    ];
    let aux = if entry_override.is_some() { &full[12..] } else { &full[..] };
    let words = 1 + argv.len() as u64 + 1 + 1 + 2 * aux.len() as u64;
    sp = (sp - words * 8) & !15;
    let rsp = sp;
    let mut w = sp;
    let mut word = |v: u64| {
        put(w, &v.to_le_bytes());
        w += 8;
    };
    word(argv.len() as u64);
    for p in &ptrs[..argv.len()] {
        word(*p);
    }
    word(0); // argv's NULL
    word(0); // envp: empty, its NULL
    for &(k, v) in aux {
        word(k);
        word(v);
    }
    Ok(Proc {
        aspace,
        entry: entry_override.unwrap_or(elf.entry),
        rsp,
        image_pages: pages,
        vec_low: rsp,
    })
}

/// Unmap and free the program's pages and its PML4. (The intermediate
/// tables are not reclaimed: `AddressSpace` has no teardown before lab 12.)
fn unload(mut p: Proc) {
    let low = STACK_TOP - STACK_PAGES * 4096;
    for va in (0..p.image_pages).map(|i| BASE + i * 4096).chain((0..STACK_PAGES).map(|i| low + i * 4096)) {
        if let Some(f) = p.aspace.unmap(VirtAddr(va)) {
            frame::free(f);
        }
    }
    frame::free(Frame(p.aspace.root()));
}

/// How many bytes below the initial vector the program wrote: the distance
/// from `vec_low` down to the lowest byte that is no longer POISON.
fn stack_used(p: &Proc) -> u64 {
    let low = STACK_TOP - STACK_PAGES * 4096;
    let mut lowest = p.vec_low;
    let mut va = low;
    while va < p.vec_low {
        let pa = frame_of(&p.aspace, va & !0xfff) + (va & 0xfff);
        // SAFETY: a stack frame of this program, read through the HHDM.
        if unsafe { hhdm::<u8>(pa).read_volatile() } != POISON {
            lowest = va;
            break;
        }
        va += 1;
    }
    p.vec_low - lowest
}

// ── Traps from ring 3 ────────────────────────────────────────────────────

static TRAPS: AtomicU64 = AtomicU64::new(0);
static T_VEC: AtomicU64 = AtomicU64::new(0);
static T_ERR: AtomicU64 = AtomicU64::new(0);
static T_RIP: AtomicU64 = AtomicU64::new(0);
static T_CS: AtomicU64 = AtomicU64::new(0);
static T_RSP: AtomicU64 = AtomicU64::new(0);
static T_SS: AtomicU64 = AtomicU64::new(0);

/// The status `run` returns when this handler ended the program.
const TRAPPED: i64 = -1000;

/// sse.usable: CR0.TS is left set for the program's `enter` to clear.
static TS_ARMED: AtomicBool = AtomicBool::new(false);
static KERNEL_NM: AtomicU64 = AtomicU64::new(0);
static KERNEL_NM_RIP: AtomicU64 = AtomicU64::new(0);

/// sys.write.efault: a kernel #PF ends the program instead of the machine.
static KFAULT_ARMED: AtomicBool = AtomicBool::new(false);
static KFAULTS: AtomicU64 = AtomicU64::new(0);
static KFAULT_ADDR: AtomicU64 = AtomicU64::new(0);
static KFAULT_RIP: AtomicU64 = AtomicU64::new(0);
/// The status `run` returns when the kernel faulted on a program's pointer.
const KFAULTED: i64 = -2000;

fn cr0_ts(set: bool) {
    // SAFETY: CR0.TS only decides whether the next x87/SSE instruction
    // raises #NM; this kernel is built without them (soft-float), and the
    // vehicle's handler above clears it again on a kernel #NM.
    unsafe {
        if set {
            core::arch::asm!("mov {t}, cr0", "or {t}, 8", "mov cr0, {t}", t = out(reg) _, options(nomem, nostack));
        } else {
            core::arch::asm!("clts", options(nomem, nostack));
        }
    }
}

fn rdmsr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    // SAFETY: reading an architectural MSR (SFMASK, 0xC000_0084) has no side effect.
    unsafe { core::arch::asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags)) };
    (hi as u64) << 32 | lo as u64
}

/// The hardware data breakpoints sys.entry.flags arms: DR0 and DR1 each
/// watch 8 bytes for any read or write, and the `#DB` they raise (a trap,
/// after the access) lands in `on_db`, which records the interrupted
/// context's RFLAGS and disarms.
mod dr {
    use core::sync::atomic::AtomicU64;

    pub static KHITS: AtomicU64 = AtomicU64::new(0);
    pub static UHITS: AtomicU64 = AtomicU64::new(0);
    pub static RFLAGS: AtomicU64 = AtomicU64::new(0);
    pub static RIP: AtomicU64 = AtomicU64::new(0);
    pub static ADDR: AtomicU64 = AtomicU64::new(0);

    /// L0, L1, LE, GE, bit 10 (reserved, 1); RW0 = RW1 = 0b11 (read or
    /// write), LEN0 = LEN1 = 0b10 (8 bytes) — SDM Vol. 3B §18.2.4.
    const DR7_ON: u64 = 1 | (1 << 2) | (1 << 8) | (1 << 9) | (1 << 10) | (0b11 << 16) | (0b10 << 18) | (0b11 << 20) | (0b10 << 22);

    /// Watch `a` and `b` (each 8-aligned).
    pub fn arm(a: u64, b: u64) {
        // SAFETY: the debug registers only decide when #DB is raised; the
        // vehicle routes vector 1 to `on_db` before any probe runs.
        unsafe {
            core::arch::asm!(
                "mov dr0, {a}", "mov dr1, {b}", "xor {t:e}, {t:e}", "mov dr6, {t}", "mov dr7, {c}",
                a = in(reg) a, b = in(reg) b, c = in(reg) DR7_ON, t = out(reg) _, options(nomem, nostack, preserves_flags)
            )
        };
    }

    pub fn disarm() {
        // SAFETY: as `arm`; DR7 = 0 turns every breakpoint off.
        unsafe { core::arch::asm!("xor {t:e}, {t:e}", "mov dr7, {t}", "mov dr6, {t}", t = out(reg) _, options(nomem, nostack, preserves_flags)) };
    }

    pub fn dr6() -> u64 {
        let v: u64;
        // SAFETY: reading DR6 has no side effect.
        unsafe { core::arch::asm!("mov {}, dr6", out(reg) v, options(nomem, nostack, preserves_flags)) };
        v
    }
}

/// #DB: a breakpoint `dr::arm` set. The first hit at CPL 0 records the
/// RFLAGS of the kernel code that touched the watched bytes; then every
/// breakpoint is off, and the kernel resumes unchanged.
fn on_db(tf: &mut TrapFrame) {
    let dr6 = dr::dr6();
    if tf.cs & 3 == 0 {
        if dr::KHITS.fetch_add(1, Ordering::SeqCst) == 0 {
            dr::RFLAGS.store(tf.rflags, Ordering::SeqCst);
            dr::RIP.store(tf.rip, Ordering::SeqCst);
            dr::ADDR.store(dr6 & 3, Ordering::SeqCst);
        }
    } else {
        dr::UHITS.fetch_add(1, Ordering::SeqCst);
    }
    dr::disarm();
}

/// Whether the word the frame holds as `cs` could only be an RFLAGS image:
/// RFLAGS always has bit 1 set and bits 3 and 5 clear (reserved), and no
/// code selector this kernel loads (0x08, 0x2b) passes that test.
fn looks_like_rflags(v: u64) -> bool {
    v & 0x2a == 0x02
}

/// Whether the CPU pushed no error code for this vector-13/14 entry. The
/// CPU's own #GP and #PF always push one, and the learner's trampoline for
/// those vectors expects it; a software `int 13` (or `int 14`) pushes none,
/// so every slot reads one word off: `error` holds the rip, `rip` the CS,
/// `cs` the RFLAGS, `rflags` the rsp. Decided from the frame alone: the
/// `rflags` slot is not an RFLAGS image, and the `cs` slot is one.
fn no_error_code(tf: &TrapFrame) -> bool {
    matches!(tf.vector, 13 | 14) && !looks_like_rflags(tf.rflags) && looks_like_rflags(tf.cs) && tf.rip <= 0xffff
}

/// On #UD, #NM, #GP and #PF: from ring 3, record the frame and end the
/// program; from ring 0, report and halt (a kernel bug, not the probe's).
fn on_trap(tf: &mut TrapFrame) {
    if no_error_code(tf) {
        // Every gate is DPL 0, so only ring-0 code can issue `int 13`; the
        // CPL is the shifted reading's CS RPL either way.
        kernel::kprintln!(
            "# ktest-09: vector {} at CPL {} with no error code pushed (a software `int {}`; the CPU's own #GP/#PF always pushes one): rip={:#x} cs={:#x} rflags={:#x}; halting",
            tf.vector, tf.rip & 3, tf.vector, tf.error, tf.rip, tf.cs
        );
        ktap::end();
        kernel::panic::halt();
    }
    if tf.cs & 3 == 0 && tf.vector == 7 && TS_ARMED.load(Ordering::SeqCst) {
        // #NM at CPL 0 while this vehicle left CR0.TS set for `enter` to
        // clear: an x87/SSE instruction of the kernel's ran with TS still
        // set. Recorded for sse.usable; TS cleared so it can retry.
        KERNEL_NM.fetch_add(1, Ordering::SeqCst);
        KERNEL_NM_RIP.store(tf.rip, Ordering::SeqCst);
        cr0_ts(false);
        return;
    }
    if tf.cs & 3 == 0 && tf.vector == 14 && KFAULT_ARMED.load(Ordering::SeqCst) {
        // #PF at CPL 0 during the bad-pointer probe: the kernel touched a
        // pointer the program passed without refusing it. Recorded for
        // sys.write.efault; the program is ended here, as `exit` would.
        let cr2: u64;
        // SAFETY: reading CR2 has no side effect.
        unsafe { core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags)) };
        KFAULTS.fetch_add(1, Ordering::SeqCst);
        KFAULT_ADDR.store(cr2, Ordering::SeqCst);
        KFAULT_RIP.store(tf.rip, Ordering::SeqCst);
        user::leave(KFAULTED)
    }
    if tf.cs & 3 != 3 {
        kernel::kprintln!(
            "# ktest-09: vector {} at CPL 0, rip={:#x} cs={:#x} error={:#x}; halting",
            tf.vector, tf.rip, tf.cs, tf.error
        );
        ktap::end();
        kernel::panic::halt();
    }
    TRAPS.fetch_add(1, Ordering::SeqCst);
    T_VEC.store(tf.vector, Ordering::SeqCst);
    T_ERR.store(tf.error, Ordering::SeqCst);
    T_RIP.store(tf.rip, Ordering::SeqCst);
    T_CS.store(tf.cs, Ordering::SeqCst);
    T_RSP.store(tf.rsp, Ordering::SeqCst);
    T_SS.store(tf.ss, Ordering::SeqCst);
    user::leave(TRAPPED - tf.vector as i64)
}

/// What one run left behind.
struct Ran {
    status: i64,
    traps: u64,
    results: [u8; res::LEN],
    stack_used: u64,
}

/// The physical address of byte `off` of a placed probe's results area.
fn results_pa(p: &Proc, off: usize) -> u64 {
    // SAFETY: only the address of the label is used.
    let o = unsafe { sym(&ktest09_probe_results) - sym(&ktest09_probe_start) } + off as u64;
    frame_of(&p.aspace, BASE + (o & !0xfff)) + (o & 0xfff)
}

/// Fill one result/parameter word of a placed probe (before it runs).
fn poke(p: &Proc, off: usize, v: u64) {
    // SAFETY: a word of the probe's own page (results_pa), written through
    // the HHDM before the program runs; `off` is 8-aligned and < res::LEN.
    unsafe { hhdm::<u64>(results_pa(p, off)).write_volatile(v) }
}

/// Load, run to the end, read the probe's results, unload.
fn run(img: &[u8], entry: Option<u64>, argv: &[&[u8]]) -> Result<Ran, &'static str> {
    run_with(img, entry, argv, &|_| {})
}

/// `run`, with `before` called on the placed program just before it is
/// entered (to fill parameter words or arm a breakpoint). Any breakpoint is
/// disarmed as soon as `run` returns.
fn run_with(img: &[u8], entry: Option<u64>, argv: &[&[u8]], before: &dyn Fn(&Proc)) -> Result<Ran, &'static str> {
    let p = load(img, entry, argv)?;
    before(&p);
    let traps = TRAPS.load(Ordering::SeqCst);
    let status = user::run(VirtAddr(p.entry), VirtAddr(p.rsp), &p.aspace);
    dr::disarm();
    let mut results = [0u8; res::LEN];
    if entry.is_some() {
        let pa = results_pa(&p, 0);
        // SAFETY: the probe's own page, read through the HHDM (the results
        // area is 16-aligned and res::LEN bytes, inside one page: `probe()` < 4 KiB).
        unsafe { core::ptr::copy_nonoverlapping(hhdm::<u8>(pa), results.as_mut_ptr(), res::LEN) };
    }
    let used = stack_used(&p);
    unload(p);
    Ok(Ran { status, traps: TRAPS.load(Ordering::SeqCst) - traps, results, stack_used: used })
}

fn word(r: &Ran, off: usize) -> u64 {
    rd::<8>(&r.results, off)
}

/// Run one probe entry for a row; `None` (with the reason printed) if it
/// could not be placed.
fn run_probe(t: &mut TestCtx, label: &u8) -> Option<Ran> {
    run_probe_with(t, label, &|_| {})
}

fn run_probe_with(t: &mut TestCtx, label: &u8, before: &dyn Fn(&Proc)) -> Option<Ran> {
    if probe().len() > 4096 {
        t.fail(format_args!("the course's probe is {} bytes; its results must sit in its one page", probe().len()));
        return None;
    }
    match run_with(probe(), Some(at(label)), &[b"probe"], before) {
        Ok(r) => Some(r),
        Err(why) => {
            t.fail(format_args!("the test could not place its probe: {why}"));
            None
        }
    }
}

fn describe(t: &TestCtx, r: &Ran) {
    if r.traps > 0 {
        t.note(format_args!(
            "the probe trapped: vector {} error={:#x} rip={:#x} cs={:#x}",
            T_VEC.load(Ordering::SeqCst), T_ERR.load(Ordering::SeqCst), T_RIP.load(Ordering::SeqCst), T_CS.load(Ordering::SeqCst)
        ));
    }
}

// ── hi (the user half) ───────────────────────────────────────────────────

static HI_STATUS: AtomicU64 = AtomicU64::new(u64::MAX);
static HI_ENOSYS: AtomicU64 = AtomicU64::new(u64::MAX);

/// Run `hi` with `argv = ["hi", <nonce>, "1"]`; it prints its own row.
fn run_hi() {
    let nonce = ktap::nonce();
    let nonce_arg: &[u8] = match &nonce {
        Some(n) => n,
        None => b"none",
    };
    if let Some(n) = &nonce {
        ktap::note(format_args!("hi: ktap::derive(nonce, \"{HI_ID}\") = {:016x}", ktap::derive(n, HI_ID)));
    }
    let before = syscall::enosys_count();
    match run(HI, None, &[b"hi", nonce_arg, HI_INDEX.as_bytes()]) {
        Ok(r) => {
            let moved = syscall::enosys_count() - before;
            ktap::note(format_args!(
                "hi: exited with status {}, used {} bytes of stack below its initial rsp, and issued {moved} unknown syscall numbers",
                r.status, r.stack_used
            ));
            if r.traps > 0 {
                ktap::note(format_args!("hi: trapped: vector {} rip={:#x}", T_VEC.load(Ordering::SeqCst), T_RIP.load(Ordering::SeqCst)));
            }
            HI_STATUS.store(r.status as u64, Ordering::SeqCst);
            HI_ENOSYS.store(moved, Ordering::SeqCst);
        }
        Err(why) => ktap::note(format_args!("hi: the test could not place it: {why}")),
    }
}

// ── The kernel half's rows ───────────────────────────────────────────────

ktest!("sys.write.console", fn sys_write_console(t: &mut TestCtx) {
    t.expect_eq("hi's exit status", HI_STATUS.load(Ordering::SeqCst), 0);
    let Some(r) = run_probe(t, unsafe { &ktest09_p_write }) else { return };
    describe(t, &r);
    // sysret's selectors are asserted, not only noted: STAR's user base is
    // what puts the probe back on 0x2b/0x23 (a wrong base — 0x13, 0x28 — ran
    // every row green while this was a note; skeptic, 2026-10-17).
    t.expect_eq("after sysret the probe reads CS", word(&r, res::SYSRET_CS) & 0xffff, 0x2b);
    t.expect_eq("after sysret the probe reads SS", word(&r, res::SYSRET_SS) & 0xffff, 0x23);
    let kcs: u16;
    // SAFETY: reading a segment register has no side effect.
    unsafe { core::arch::asm!("mov {0:x}, cs", out(reg) kcs, options(nomem, nostack, preserves_flags)) };
    t.expect_eq("the kernel's own CS after run returned (STAR's kernel base is 0x08)", kcs as u64, 0x08);
    t.expect_eq("the probe's exit status", r.status as u64, 0);
    let (n1, n2) = (N1 as u64, N2 as u64);
    t.expect_eq("write(1, line, n) returned", word(&r, res::WRITE1), n1);
    t.expect_eq("write(2, line, n) returned", word(&r, res::WRITE2), n2);
    t.expect_eq("write(9, ..) on a closed fd returned", word(&r, res::WRITE_BADFD), (-syscall::errno::EBADF) as u64);
});

ktest!("sys.enosys", fn sys_enosys(t: &mut TestCtx) {
    let hi = HI_ENOSYS.load(Ordering::SeqCst);
    t.note(format_args!("sys.enosys: hi's write/getpid/ioctl/exit moved the count by {hi}"));
    t.expect_eq("enosys_count() moved by hi's calls, all in the table", hi, 0);
    let before = syscall::enosys_count();
    let Some(r) = run_probe(t, unsafe { &ktest09_p_enosys }) else { return };
    let moved = syscall::enosys_count() - before;
    describe(t, &r);
    t.note(format_args!("sys.enosys: the probe issued 1000, -1, getpid and exit; the count moved by {moved}"));
    t.expect_eq("the probe's exit status", r.status as u64, 0);
    t.expect_eq("syscall 1000 returned", word(&r, res::NOSYS_1000), (-syscall::errno::ENOSYS) as u64);
    t.expect_eq("syscall -1 returned", word(&r, res::NOSYS_M1), (-syscall::errno::ENOSYS) as u64);
    t.expect_eq("getpid returned", word(&r, res::GETPID), 1);
    t.expect_eq("enosys_count() moved by the probe's two unknown numbers", moved, 2);
});

ktest!("console.winsz", fn console_winsz(t: &mut TestCtx) {
    let Some(r) = run_probe(t, unsafe { &ktest09_p_winsz }) else { return };
    describe(t, &r);
    let ws = word(&r, res::WINSZ);
    let (rows, cols) = (ws & 0xffff, (ws >> 16) & 0xffff);
    t.note(format_args!("console.winsz: ioctl(1, TIOCGWINSZ) = {} with rows={rows} cols={cols}", word(&r, res::IOCTL) as i64));
    t.expect_eq("the probe's exit status", r.status as u64, 0);
    t.expect_eq("ioctl(TIOCGWINSZ) returned", word(&r, res::IOCTL), 0);
    // CONTRACT §A.4 row 16: 80×25, fixed — the learner's constants are not the oracle.
    t.expect_eq("ws_row", rows, 25);
    t.expect_eq("ws_col", cols, 80);
});

/// What sys.write.efault's ioctl must leave alone: a kernel variable (the
/// kernel image, present, U = 0).
static CANARY: [AtomicU64; 2] = [AtomicU64::new(0x5b00_ca4a_4157_0001), AtomicU64::new(0x5b00_ca4a_4157_0002)];
/// A badptr answer word the probe never wrote.
const NOT_REACHED: u64 = 0x5eed_0000_dead_0000;
/// What sys.write.efault's kernel-half write points at, through its HHDM
/// alias: whole text lines, so a kernel that wrongly sends them prints a
/// readable line, never raw bytes on the console.
#[repr(align(16))]
struct Lines([u8; 16]);
static KERNEL_LINE: Lines = Lines(*b"# kernel memory\n");
/// What fills the straddle buffer's mapped half (the top of the program's
/// stack) for the same reason; 16 bytes, so every whole chunk ends a line.
const PROGRAM_LINE: &[u8; 16] = b"# program bytes\n";

ktest!("sys.write.efault", fn sys_write_efault(t: &mut TestCtx) {
    // The HHDM alias of KERNEL_LINE, read off the program's own tables
    // (they share the kernel half) once it is placed.
    let hhdm_at = core::cell::Cell::new(0u64);
    let canary = CANARY.as_ptr() as u64;
    let straddle = STACK_TOP - 0x800;
    let unmapped: u64 = 0x0000_1000_0000_0000;
    let faults = KFAULTS.load(Ordering::SeqCst);
    KFAULT_ARMED.store(true, Ordering::SeqCst);
    let r = run_probe_with(t, unsafe { &ktest09_p_badptr }, &|p| {
        let va = KERNEL_LINE.0.as_ptr() as u64;
        let hhdm_ptr = paging::hhdm(PhysAddr(frame_of(&p.aspace, va & !0xfff) + (va & 0xfff))).0;
        hhdm_at.set(hhdm_ptr);
        poke(p, res::P_HHDM, hhdm_ptr);
        // The stack's top 2 KiB, initial vector included (this probe never
        // reads its stack), as text lines.
        let top = frame_of(&p.aspace, STACK_TOP - 4096);
        for i in (0x800..0x1000).step_by(16) {
            // SAFETY: bytes of the stack's top frame, reached through the HHDM
            // before the program runs.
            unsafe { core::ptr::copy_nonoverlapping(PROGRAM_LINE.as_ptr(), hhdm::<u8>(top + i), 16) };
        }
        poke(p, res::P_CANARY, canary);
        poke(p, res::P_STRADDLE, straddle);
        poke(p, res::P_UNMAPPED, unmapped);
        for b in [res::B_HHDM, res::B_HHDM0, res::B_IOCTL, res::B_STRADDLE, res::B_NULL, res::B_UNMAPPED] {
            poke(p, b, NOT_REACHED);
        }
    });
    KFAULT_ARMED.store(false, Ordering::SeqCst);
    let Some(r) = r else { return };
    describe(t, &r);
    let hhdm_ptr = hhdm_at.get();
    let cases: [(&str, u64, usize); 6] = [
        ("write(1, <HHDM>, 16)", hhdm_ptr, res::B_HHDM),
        ("write(1, <HHDM>, 0)", hhdm_ptr, res::B_HHDM0),
        ("ioctl(1, TIOCGWINSZ, <a kernel variable>)", canary, res::B_IOCTL),
        ("write(1, <4096 bytes from 2 KiB below the stack's top>, 4096)", straddle, res::B_STRADDLE),
        ("write(1, NULL, 16)", 0, res::B_NULL),
        ("write(1, <unmapped user half>, 16)", unmapped, res::B_UNMAPPED),
    ];
    for (what, ptr, off) in cases {
        let got = word(&r, off);
        if got == NOT_REACHED {
            t.fail(format_args!("{what} with pointer {ptr:#x}: never answered (the program had ended)"));
        } else if got as i64 != -syscall::errno::EFAULT {
            t.fail(format_args!("{what} with pointer {ptr:#x}: returned {}, expected -14 (-EFAULT)", got as i64));
        } else {
            t.note(format_args!("sys.write.efault: {what} with pointer {ptr:#x} = -14"));
        }
    }
    let kf = KFAULTS.load(Ordering::SeqCst) - faults;
    if kf > 0 {
        t.fail(format_args!(
            "the kernel page-faulted at rip={:#x} on address {:#x} while serving the program; the program was ended there",
            KFAULT_RIP.load(Ordering::SeqCst), KFAULT_ADDR.load(Ordering::SeqCst)
        ));
    } else {
        t.expect_eq("the probe's exit status", r.status as u64, 0);
    }
    t.expect_eq("the kernel variable the ioctl named, word 0, afterwards", CANARY[0].load(Ordering::SeqCst), 0x5b00_ca4a_4157_0001);
    t.expect_eq("the kernel variable the ioctl named, word 1, afterwards", CANARY[1].load(Ordering::SeqCst), 0x5b00_ca4a_4157_0002);
});

ktest!("sys.entry.flags", fn sys_entry_flags(t: &mut TestCtx) {
    // SAFETY: only the label's address is used.
    let line = at(unsafe { &ktest09_flags_line });
    let k0 = dr::KHITS.load(Ordering::SeqCst);
    let r = run_probe_with(t, unsafe { &ktest09_p_flags }, &|p| {
        // The line's first 8 bytes, at the address the program passed and
        // at the same bytes' HHDM alias: a kernel copies from one of them.
        let pa = frame_of(&p.aspace, line & !0xfff) + (line & 0xfff);
        dr::arm(line, paging::hhdm(PhysAddr(pa)).0);
    });
    let Some(r) = r else { return };
    describe(t, &r);
    let hits = dr::KHITS.load(Ordering::SeqCst) - k0;
    let kflags = dr::RFLAGS.load(Ordering::SeqCst);
    let sfmask = rdmsr(0xC000_0084);
    t.note(format_args!(
        "sys.entry.flags: the program's rflags at the syscall {:#x}; inside the kernel, where it read the line ({}, rip={:#x}), rflags {kflags:#x}; SFMASK {sfmask:#x}",
        word(&r, res::FLAGS_USER),
        match dr::ADDR.load(Ordering::SeqCst) { 1 => "at the program's address", 2 => "at its HHDM alias", _ => "not seen" },
        dr::RIP.load(Ordering::SeqCst)
    ));
    t.expect_eq("the probe's exit status", r.status as u64, 0);
    t.expect_eq("write(1, line, n) with DF set returned", word(&r, res::FLAGS_WRITE), N3 as u64);
    if hits == 0 {
        t.fail(format_args!("no read of the program's line by the kernel was seen (at its address or its HHDM alias)"));
        return;
    }
    t.expect_eq("RFLAGS.IF inside the syscall (interrupts off)", (kflags >> 9) & 1, 0);
    // TF from SFMASK itself: a program that sets TF before `syscall` would
    // single-step the kernel, and only SFMASK can clear it in time. DF is
    // noted, not asserted: QEMU's TCG `syscall` clears DF whatever SFMASK
    // says (measured 2026-10-18, QEMU 11.1.1: SFMASK 0 and the program's DF
    // set still read DF clear here), so no kernel could turn it red.
    t.expect_eq("SFMASK's TF bit (no single-step into the kernel)", (sfmask >> 8) & 1, 1);
});

ktest!("sse.usable", fn sse_usable(t: &mut TestCtx) {
    // A program first leaves the x87 control word 0x027f, one value on the
    // x87 stack and MXCSR 0x7f80; then CR0.TS is set, as a kernel that
    // switched tasks lazily would leave it. The next `enter` must hand the
    // probe a clean unit either way.
    let Some(d) = run_probe(t, unsafe { &ktest09_p_dirty }) else { return };
    describe(t, &d);
    t.expect_eq("the state-changing program's exit status", d.status as u64, 0);
    let nm = KERNEL_NM.load(Ordering::SeqCst);
    cr0_ts(true);
    TS_ARMED.store(true, Ordering::SeqCst);
    let r = run_probe(t, unsafe { &ktest09_p_sse });
    TS_ARMED.store(false, Ordering::SeqCst);
    cr0_ts(false);
    let Some(r) = r else { return };
    describe(t, &r);
    // RFLAGS as the first program found it at entry.
    let flags = word(&d, res::SSE_RFLAGS);
    t.note(format_args!(
        "sse.usable: rflags at a program's entry {flags:#x}; after a program that changed FCW and MXCSR, and with CR0.TS set before enter: MXCSR={:#x} FCW={:#x} 1.5+1.5={:#x} packed={:#x}:{:#x}",
        word(&r, res::MXCSR) & 0xffff_ffff, word(&r, res::FCW) & 0xffff, word(&r, res::SCALAR) & 0xffff_ffff,
        word(&r, res::PACKED), word(&r, res::PACKED + 8)
    ));
    let nm = KERNEL_NM.load(Ordering::SeqCst) - nm;
    if nm > 0 {
        t.fail(format_args!(
            "#NM (vector 7) in the kernel at rip={:#x}: an x87/SSE instruction ran before CR0.TS was cleared",
            KERNEL_NM_RIP.load(Ordering::SeqCst)
        ));
    }
    t.expect_eq("the program's RFLAGS.IF at entry (interrupts on)", (flags >> 9) & 1, 1);
    t.expect_eq("the program's RFLAGS.IOPL at entry", (flags >> 12) & 3, 0);
    t.expect_eq("traps during the SSE probe", r.traps, 0);
    t.expect_eq("the probe's exit status", r.status as u64, 0);
    t.expect_eq("the probe reached its end", word(&r, res::SSE_DONE), 1);
    t.expect_eq("MXCSR at entry", word(&r, res::MXCSR) & 0xffff_ffff, 0x1f80);
    t.expect_eq("x87 control word at entry (fninit's)", word(&r, res::FCW) & 0xffff, 0x037f);
    t.expect_eq("addss 1.5 + 1.5", word(&r, res::SCALAR) & 0xffff_ffff, 0x4040_0000);
    t.expect_eq("addps {1,2} + itself", word(&r, res::PACKED), 0x4080_0000_4000_0000);
    t.expect_eq("addps {3,4} + itself", word(&r, res::PACKED + 8), 0x4100_0000_40c0_0000);
});

ktest!("gp.user.u1", fn gp_user_u1(t: &mut TestCtx) {
    let Some(r) = run_probe(t, unsafe { &ktest09_p_gp }) else { return };
    let want_rip = at(unsafe { &ktest09_p_gp_insn });
    // What this vehicle's trap handler hands `user::leave` for a #GP: the
    // way back carries data, not only control, so `run` must return it.
    let want_status = TRAPPED - 13;
    t.note(format_args!(
        "gp.user.u1: `hlt` at CPL 3 ({want_rip:#x}) -> vector {} error={:#x} rip={:#x} cs={:#x} ss={:#x} rsp={:#x}; run returned {}; the kernel is still running",
        T_VEC.load(Ordering::SeqCst), T_ERR.load(Ordering::SeqCst), T_RIP.load(Ordering::SeqCst),
        T_CS.load(Ordering::SeqCst), T_SS.load(Ordering::SeqCst), T_RSP.load(Ordering::SeqCst), r.status
    ));
    t.expect_eq("the probe ran up to the instruction", word(&r, res::GP_REACHED), 1);
    t.expect_eq("traps", r.traps, 1);
    t.expect_eq("the trap's vector", T_VEC.load(Ordering::SeqCst), 13);
    t.expect_eq("#GP's error code", T_ERR.load(Ordering::SeqCst), 0);
    t.expect_eq("the frame's CS", T_CS.load(Ordering::SeqCst), gdt::sel::UCODE as u64);
    t.expect_eq("the frame's CS RPL", T_CS.load(Ordering::SeqCst) & 3, 3);
    t.expect_eq("the frame's SS", T_SS.load(Ordering::SeqCst), gdt::sel::UDATA as u64);
    t.expect_eq("the frame's rip (the hlt)", T_RIP.load(Ordering::SeqCst), want_rip);
    if r.status != want_status {
        t.fail(format_args!(
            "run's return value, the status the trap handler passed to user::leave: observed {}, expected {want_status}",
            r.status
        ));
    }
});

fn prelude() {
    let (k, i, _) = shim::stacks();
    gdt::init(k, i);
    idt::init();
    // The 8259s masked: no IRQ reaches ring 3 or the kernel in this run.
    serial::out8(0x21, 0xff);
    serial::out8(0xa1, 0xff);
    let bi = bootinfo::get().expect("the shim published the BootInfo");
    frame::init(bi.memmap(), bi.kernel_range());
    syscall::init();
    syscall::set_kernel_stack(k);
    for v in [6u8, 7, 13, 14] {
        idt::set_handler(v, on_trap, 0);
    }
    // #DB: sys.entry.flags' data breakpoints (only ever armed around its probe).
    idt::set_handler(1, on_db, 0);
}

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    serial::init();
    // N counts both halves: `hi`'s row plus the seven registered here.
    ktap::begin(ktap::tests().len() + 1);
    prelude();
    run_hi();
    ktap::run_all();
    ktap::end();
    kernel::hcf()
}
