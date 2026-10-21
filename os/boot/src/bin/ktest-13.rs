//! Lab 13's vehicle: exec. The prelude is lab 12's (the GDT, the IDT, the
//! frame pool, the syscall MSRs, the heap, `sse_on`, `proc::init`, task 0,
//! the PIC and the PIT).
//!
//! What is mounted where is this vehicle's decision (CONTRACT §A.7): the boot
//! archive — linked into this image in its own section, `.sboot_archive` —
//! served by the learner's `fs::archive::Archive` and mounted at `/` through
//! the learner's `vfs::mount`; and a directory of this vehicle's own at
//! `/ktest` (its own `Inode`), through the same `vfs::mount`, holding a
//! small program, `exec-probe`, and three copies of `args` each broken one
//! way.
//!
//! `exec-probe` does one thing: `execve(argv[1], &argv[2], envp)`, and if
//! that returns, `exit(100 + errno)`. Every program in this run is started
//! by the learner's `exec::spawn` as `exec-probe`, which then replaces
//! itself through the learner's syscall 59 — the copy-in of the path, argv
//! and envp, the walk, the loader, the initial stack and the swap are all
//! the learner's. The vehicle (task 0) only starts the probe and waits for
//! it to end (`proc::each`, the ACPI PM timer for the bound — never the
//! learner's tick). Rows, in the order they print:
//!
//!   1–5  `exec.argc_argv`, `exec.envp`, `exec.rsp.aligned`,
//!        `exec.auxv.phdr_pagesz`, `exec.auxv.random.readable` — printed by
//!        `args` itself, which the probe exec'd from `/args`
//!   6    `exec.reject.dyn`   (this vehicle)
//!   7    `exec.reject.class` (this vehicle)
//!   8    `exec.reject.magic` (this vehicle)
//!   9    `exec.refuse.frees` (this vehicle)
//!   10   `exec.execve.frees` (this vehicle)
//!   11   `exec.execve.refuses` (this vehicle)
//!   12   `exec.execve.replaces` (this vehicle)
//!
//! `args`' environment carries one more string, `SBOOT_PAD=…`, whose length
//! the vehicle picks so that the vector has an odd number of words and the
//! strings, with the path and the 16 random bytes, come to 15 mod 16 bytes:
//! then a stack pointer rounded down to 8 rather than 16 lands 8 off the
//! boundary, whether the strings are packed from `STACK_TOP` or their end is
//! aligned to 16 first (and with or without `AT_EXECFN`'s copy of the path).
//!
//! A refusal is checked twice: `exec::spawn` of the broken file must return
//! `-ENOEXEC` and leave no process behind, and a probe's `execve` of it
//! must return `-ENOEXEC` with the probe still there to `exit(108)`. That
//! probe is `refuse-probe`, which runs `ud2` just before its `execve` and
//! again just after; while it runs, this vehicle's #UD handler records the
//! caller each time (its argv, VMAs, root, its task's space and CR3), and
//! the two must be equal: a refused `sys_execve` leaves its caller as it was. The
//! broken copies start with `exit(99)` in place of `args`' first
//! instructions, so a loader that wrongly accepts one runs nothing that
//! prints. The refusals come last: a loader that panics on a broken header
//! ends the run after `args`' rows have printed.
//!
//! Rows 9 and 10 read the frames in use from `frame::stats()` and check each
//! reading against a drain of `frame::alloc` (every frame taken, counted and
//! freed back): a count that disagrees fails the row.
//!
//! Row 9 counts frames around refusals: after one refusal of each kind to
//! warm up, eight `exec::spawn`s of `/ktest/machine` (`args` built for
//! another machine, `e_machine` = EM_386: refused at the header, before
//! anything is mapped) and eight of `/ktest/exec-probe` with an argv
//! larger than the whole 16-page stack (refused after the segments and the
//! stack pages are mapped) must leave the frames in use grown by less than
//! the number of refusals: a refusal that keeps even one frame grows the
//! count by at least one per refusal.
//!
//! Row 10 counts frames around successful execs: one process that the probe
//! takes through 1 + 8 `execve`s (itself 8 times, then `/ktest/done`, which
//! exits 0), against one taken through a single `execve` (after one of each
//! to warm up). Whatever a process keeps until it is collected is the same
//! in both; each successful exec must give back the address space it
//! replaced, so the long chain may grow the frames in use by less than 8
//! more than the short one.
//!
//! Row 11 calls the learner's `exec::execve` itself, on a running process of
//! the vehicle's: `/ktest/execve-ud` starts with `ud2`, and while it runs this
//! vehicle routes #UD to a handler of its own, which, on that process's task,
//! calls `execve(p, "/ktest/machine", …)` and `execve(p, "/ktest/exec-probe",
//! <the oversized argv>, …)`. The first must return `-ENOEXEC`, the second
//! `-E2BIG` (Linux's 7: the strings and the vector do not fit the initial
//! stack; the vehicle holds Linux's number, not the learner's constant), and each must leave the process as it was — its argv, its VMAs, its root, the
//! address space its task runs on, CR3, and the bytes of every page its VMAs
//! cover — and the program must go on to `exit(argc)` on its own stack.
//! Rows 6–8's ring-3 half holds `refuse-probe`'s caller to the same, between
//! a `ud2` on each side of its `execve`.
//!
//! Row 12 is `execve`'s other half: a second `execve-ud` process, whose #UD
//! handler calls `execve(p, "/ktest/landed", ["landed"], [])`. The call must
//! not return. `landed` starts with `ud2`, so the vehicle sees the new
//! program run: a trap at its entry, in the same pid, with argv `["landed"]`,
//! on one space that the record, the task and CR3 agree on and that is not
//! the old one. Then it must `exit(0)`, and the record's argv stay
//! `["landed"]`.
//! Rows 11 and 12 come last: a stubbed `execve` panics.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../shim.rs"]
mod shim;

use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

use kernel::addr::VirtAddr;
use kernel::exec;
use kernel::fs::archive::Archive;
use kernel::fs::vfs::{self, dt, mode, DirEntry, Inode, InodeRef, Stat};
use kernel::idt::{self, TrapFrame};
use kernel::ktap;
use kernel::proc::{self, Pid, VmaKind};
use kernel::sync::SpinLock;
use kernel::syscall::errno::{EISDIR, ENOENT, ENOEXEC, ENOTDIR};
use kernel::syscall::Errno;
use kernel::{bootinfo, frame, gdt, heap, irq, sched, serial, syscall, task, time};

/// Linux's `E2BIG` (`errno-base.h`), held here rather than read from the
/// learner's `exec.rs`, so the row grades the number Linux returns.
const LINUX_E2BIG: Errno = 7;

const ARCHIVE_LEN: usize = include_bytes!("../../../user/bin/boot.archive").len();
/// The boot archive, in the kernel image as a section of its own (§A.7).
#[link_section = ".sboot_archive"]
static ARCHIVE: [u8; ARCHIVE_LEN] = *include_bytes!("../../../user/bin/boot.archive");
/// `args` once more, as the raw material for the three broken copies.
static ARGS: &[u8] = include_bytes!("../../../user/bin/args");

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 16;
const PM_HZ: u64 = 3_579_545;

const PLAN: usize = 12;
const ARGV_ID: &str = "exec.argc_argv";
const ARGS_IDS: [&str; 5] = [ARGV_ID, "exec.envp", "exec.rsp.aligned", "exec.auxv.phdr_pagesz", "exec.auxv.random.readable"];
/// (row, id, the file under /rejects, what is broken)
const REJECTS: [(usize, &str, &str, &str); 3] = [
    (6, "exec.reject.dyn", "dyn", "e_type = ET_DYN (3)"),
    (7, "exec.reject.class", "class", "e_ident[EI_CLASS] = ELFCLASS32 (1)"),
    (8, "exec.reject.magic", "magic", "e_ident[3] = 'G' (the magic reads \\x7fELG)"),
];
/// What a broken copy exits with if a loader runs it anyway.
const RAN_ANYWAY: i32 = 99;
/// What `exec-probe` adds to the errno of an `execve` that returned.
const PROBE_FAILED: i32 = 100;
const PROBE: &str = "/ktest/exec-probe";
/// `exec-probe` with a `ud2` on each side of its `execve` (rows 6–8).
const REFUSE_PROBE: &str = "/ktest/refuse-probe";
const FREES_ID: &str = "exec.refuse.frees";
const FREES_ROW: usize = 9;
/// Refusals of each kind row 9 counts (after one of each to warm up).
const FREES_EACH: usize = 8;
const EXECVE_ID: &str = "exec.execve.refuses";
const EXECVE_ROW: usize = 11;
const REPLACES_ID: &str = "exec.execve.replaces";
const REPLACES_ROW: usize = 12;
const REPLACE_ID: &str = "exec.execve.frees";
const REPLACE_ROW: usize = 10;
/// The successful execs row 10's long chain makes beyond the short one's.
const CHAIN: usize = 8;
/// `exit(0)`: where every chain of row 10 ends.
const DONE: &str = "/ktest/done";
const EXECVE_UD: &str = "/ktest/execve-ud";
/// Row 12's target: `ud2` at its entry (the vehicle sees it run), then
/// `exit(0)`.
const LANDED: &str = "/ktest/landed";
/// `args` for another machine: what rows 9 and 10 have refused at the header
/// (none of rows 6–8's broken copies, so a loader that wrongly accepts one of
/// those still refuses this).
const MACHINE: &str = "/ktest/machine";
/// The argv strings of the oversized exec: 32 of 4000 bytes, twice what the
/// whole 16-page initial stack holds, so no layout fits it.
const BIG_STRS: usize = 32;
const BIG_LEN: usize = 4000;

// ── Time that does not depend on the learner's tick ──────────────────────

fn pm() -> u32 {
    let v: u32;
    // SAFETY: a 32-bit read of the ACPI PM timer port; no memory is touched.
    unsafe { asm!("in eax, dx", out("eax") v, in("dx") 0x608u16, options(nomem, nostack, preserves_flags)) };
    v & 0xff_ffff
}

/// Microseconds since `new`, from the 24-bit PM timer (it wraps every
/// 4.7 s; `us` must be called more often than that).
struct Clock {
    last: u32,
    acc: u64,
}

impl Clock {
    fn new() -> Clock {
        Clock { last: pm(), acc: 0 }
    }
    fn us(&mut self) -> u64 {
        let now = pm();
        self.acc += (now.wrapping_sub(self.last) & 0xff_ffff) as u64;
        self.last = now;
        self.acc * 1_000_000 / PM_HZ
    }
    fn ms(&mut self) -> u64 {
        self.us() / 1000
    }
}

fn ticks() -> u64 {
    time::TICKS.load(Ordering::Relaxed)
}

// ── Interrupts off around what must not be cut ───────────────────────────

fn irq_off() -> bool {
    let rflags: u64;
    // SAFETY: reads RFLAGS through the stack and clears IF.
    unsafe { asm!("pushfq", "pop {}", "cli", out(reg) rflags) };
    rflags & (1 << 9) != 0
}

fn irq_restore(on: bool) {
    if on {
        // SAFETY: setting IF touches no memory.
        unsafe { asm!("sti", options(nomem, nostack)) };
    }
}

fn quiet<R>(f: impl FnOnce() -> R) -> R {
    let on = irq_off();
    let r = f();
    irq_restore(on);
    r
}

/// One `#` line, whole: a tick between two of its bytes would let a
/// program's line land inside it.
fn note(args: core::fmt::Arguments) {
    quiet(|| ktap::note(args));
}

fn row(ok: bool, n: usize, id: &str) {
    quiet(|| {
        if ok {
            kernel::kprintln!("ok {n} - {id}");
        } else {
            kernel::kprintln!("not ok {n} - {id}");
        }
    });
}

fn halt() -> ! {
    loop {
        // SAFETY: interrupts off, then wait: nothing runs after this.
        unsafe { asm!("cli", "hlt", options(nomem, nostack)) };
    }
}

// ── Watching processes ───────────────────────────────────────────────────

fn exit_of(pid: Pid) -> Option<i32> {
    let mut e = None;
    proc::each(|p| {
        if p.pid == pid {
            e = p.exit;
        }
    });
    e
}

fn nprocs() -> usize {
    let mut n = 0;
    proc::each(|_| n += 1);
    n
}

/// Yield until `pid` has ended (at most `ms`).
fn wait_exit(pid: Pid, ms: u64) -> bool {
    let mut c = Clock::new();
    loop {
        if exit_of(pid).is_some() {
            return true;
        }
        if c.ms() >= ms {
            return false;
        }
        sched::yield_now();
    }
}

fn status_text(s: Option<i32>) -> String {
    match s {
        None => String::from("still running"),
        Some(v) if v & 0x7f == 0 => format!("exit({}) (status {v:#x})", (v >> 8) & 0xff),
        Some(v) => format!("killed by signal {} (status {v:#x})", v & 0x7f),
    }
}

fn errno_text(e: Errno) -> String {
    let name = match e {
        2 => "ENOENT",
        7 => "E2BIG",
        8 => "ENOEXEC",
        12 => "ENOMEM",
        13 => "EACCES",
        14 => "EFAULT",
        20 => "ENOTDIR",
        22 => "EINVAL",
        38 => "ENOSYS",
        _ => "?",
    };
    format!("-{e} ({name})")
}

fn nonce_str() -> String {
    match ktap::nonce() {
        Some(n) => String::from(core::str::from_utf8(&n).unwrap_or("none")),
        None => String::from("none"),
    }
}

/// What `frames_used` found wrong with `frame::stats()`, since the row that
/// is measuring cleared it.
static FRAME_LIES: SpinLock<Vec<String>> = SpinLock::new(Vec::new());
/// The pool's size as `frame::stats()` first gave it.
static FRAME_TOTAL: SpinLock<Option<usize>> = SpinLock::new(None);

/// The usable memory at or above 1 MiB that this vehicle hands
/// `frame::init`: far more than any row here takes, far less than the
/// machine has (128 MiB). Each frame-count verdict drains the pool through
/// `frame::alloc`, and an allocator that searches from the bottom every time
/// takes time that grows with the square of the pool for each drain.
const POOL_BYTES: u64 = 16 << 20;
const ONE_MIB: u64 = 1 << 20;

/// `map` with its usable memory at or above 1 MiB cut to `POOL_BYTES`, in
/// address order: a region past the budget is cut short or dropped. The rest
/// of the map is passed as it is.
fn clipped(map: &[bootinfo::MemRegion], out: &mut [bootinfo::MemRegion; bootinfo::MEMMAP_MAX]) -> usize {
    let mut left = POOL_BYTES;
    let mut n = 0;
    for r in map {
        let mut r = *r;
        if r.kind == 0 && r.base + r.len > ONE_MIB {
            let low = ONE_MIB.saturating_sub(r.base).min(r.len);
            let high = (r.len - low).min(left);
            left -= high;
            r.len = low + high;
            if r.len == 0 {
                continue;
            }
        }
        if n < out.len() {
            out[n] = r;
            n += 1;
        }
    }
    n
}

/// Frames in use, as `frame::stats()` reports them, cross-checked by a
/// drain: with interrupts off, `frame::alloc` until `None` (each frame
/// linked through its first word, via the HHDM), then every one freed back.
/// The free count must be what the drain found, used + free the total, the
/// total what it was the first time, and `stats()` after the refill what it
/// was before; a mismatch is kept in `FRAME_LIES` for the row to fail on.
/// An `alloc` that gives up early finds fewer free frames, so the frames in
/// use read higher: a bound row can only fail on it.
fn frames_used() -> usize {
    let (st, drained, after) = quiet(|| {
        let st = frame::stats();
        let mut head = 0u64;
        let mut n = 0usize;
        while let Some(f) = frame::alloc() {
            // SAFETY: a frame this loop holds, through the HHDM: its first
            // word links the drained list.
            unsafe { (kernel::paging::hhdm(f.0).0 as *mut u64).write(head) };
            head = f.0 .0;
            n += 1;
        }
        while head != 0 {
            // SAFETY: as above.
            let next = unsafe { (kernel::paging::hhdm(kernel::addr::PhysAddr(head)).0 as *const u64).read() };
            frame::free(frame::Frame(kernel::addr::PhysAddr(head)));
            head = next;
        }
        (st, n, frame::stats())
    });
    let mut why: Vec<String> = Vec::new();
    if st.free != drained || st.used + drained != st.total {
        why.push(format!(
            "frame::stats() says {} in use and {} free of {}; a drain of frame::alloc finds {} free, so {} in use",
            st.used,
            st.free,
            st.total,
            drained,
            st.total.saturating_sub(drained)
        ));
    }
    if (after.used, after.free, after.total) != (st.used, st.free, st.total) {
        why.push(format!(
            "with every drained frame freed back, frame::stats() says {} in use and {} free, where it said {} and {} before the drain",
            after.used, after.free, st.used, st.free
        ));
    }
    {
        let mut t = FRAME_TOTAL.lock();
        match *t {
            None => *t = Some(st.total),
            Some(t0) if t0 != st.total => {
                why.push(format!("frame::stats() says the pool holds {} frames, where it said {t0} earlier", st.total))
            }
            Some(_) => {}
        }
    }
    if !why.is_empty() {
        FRAME_LIES.lock().push(why.join("; "));
    }
    st.used
}

/// The frame counts a row read were honest: notes each mismatch `frames_used`
/// found since the row began, and says whether there were none.
fn frames_honest(id: &str) -> bool {
    let lies = core::mem::take(&mut *FRAME_LIES.lock());
    for l in lies.iter().take(3) {
        note(format_args!("{id}: {l}"));
    }
    if lies.len() > 3 {
        note(format_args!("{id}: … and {} more counts like these", lies.len() - 3));
    }
    lies.is_empty()
}

// ── The archive at / ─────────────────────────────────────────────────────

/// Mount the boot archive at `/`; a `#` line lists what `getdents` and
/// `stat` say is in it.
fn mount_archive() -> bool {
    let a = match Archive::new(&ARCHIVE) {
        Ok(a) => a,
        Err(e) => {
            note(format_args!("vfs: Archive::new over the boot archive ({ARCHIVE_LEN} bytes) returned {}", errno_text(e)));
            return false;
        }
    };
    let root: InodeRef = Arc::new(a);
    if let Err(e) = vfs::mount("/", root.clone()) {
        note(format_args!("vfs: mount(\"/\", the archive) returned {}", errno_text(e)));
        return false;
    }
    let mut listing = String::new();
    let mut n = 0u64;
    loop {
        match root.getdents(n) {
            Ok(Some(d)) => {
                let size = root.lookup(&d.name).map(|f| f.stat().size);
                match size {
                    Ok(s) => listing.push_str(&format!(" {}({s})", d.name)),
                    Err(e) => listing.push_str(&format!(" {}(lookup {})", d.name, errno_text(e))),
                }
                n += 1;
            }
            Ok(None) => break,
            Err(e) => {
                listing.push_str(&format!(" [getdents({n}) returned {}]", errno_text(e)));
                break;
            }
        }
        if n > 64 {
            break;
        }
    }
    note(format_args!(
        "vfs: / is the boot archive, linked at {:#x} ({ARCHIVE_LEN} bytes): {n} entries:{listing}",
        ARCHIVE.as_ptr() as u64
    ));
    true
}

// ── The vehicle's directory: /ktest ──────────────────────────────────────

/// A directory of this vehicle's, served through the learner's VFS.
struct KtestDir {
    files: Vec<(&'static str, InodeRef)>,
}

struct MemFile {
    bytes: Vec<u8>,
    ino: u64,
}

impl Inode for MemFile {
    fn lookup(&self, _name: &str) -> Result<InodeRef, Errno> {
        Err(ENOTDIR)
    }
    fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<usize, Errno> {
        let len = self.bytes.len() as u64;
        if off >= len {
            return Ok(0);
        }
        let n = buf.len().min((len - off) as usize);
        buf[..n].copy_from_slice(&self.bytes[off as usize..off as usize + n]);
        Ok(n)
    }
    fn getdents(&self, _index: u64) -> Result<Option<DirEntry>, Errno> {
        Err(ENOTDIR)
    }
    fn stat(&self) -> Stat {
        Stat { ino: self.ino, mode: mode::S_IFREG | 0o555, nlink: 1, size: self.bytes.len() as u64, ..Stat::default() }
    }
}

impl Inode for KtestDir {
    fn lookup(&self, name: &str) -> Result<InodeRef, Errno> {
        self.files.iter().find(|(n, _)| *n == name).map(|(_, f)| f.clone()).ok_or(ENOENT)
    }
    fn read_at(&self, _off: u64, _buf: &mut [u8]) -> Result<usize, Errno> {
        Err(EISDIR)
    }
    fn getdents(&self, index: u64) -> Result<Option<DirEntry>, Errno> {
        Ok(self.files.get(index as usize).map(|(n, f)| DirEntry { ino: f.stat().ino, kind: dt::DT_REG, name: String::from(*n) }))
    }
    fn stat(&self) -> Stat {
        Stat { ino: 1, mode: mode::S_IFDIR | 0o555, nlink: 2, ..Stat::default() }
    }
}

/// `args` with its first instructions replaced by `exit(99)` (so a loader
/// that accepts a broken copy runs nothing that prints) and then broken
/// the one way `which` names.
fn broken(which: &str) -> Vec<u8> {
    let mut b = Vec::from(ARGS);
    let le = |b: &[u8], off: usize| u64::from_le_bytes(b[off..off + 8].try_into().unwrap_or([0; 8]));
    // args is one PT_LOAD at file offset 0, placed at proc::BASE.
    let at = (le(&b, 24) - proc::BASE) as usize;
    // mov edi, 99; mov eax, 60; syscall
    let quit = [0xbf, RAN_ANYWAY as u8, 0, 0, 0, 0xb8, 60, 0, 0, 0, 0x0f, 0x05];
    b[at..at + quit.len()].copy_from_slice(&quit);
    match which {
        "dyn" => b[16] = 3,
        "class" => b[4] = 1,
        "machine" => b[18] = 3,
        _ => b[3] = b'G',
    }
    b
}

/// `exec-probe`: `execve(argv[1], &argv[2], envp)`; if that returns,
/// `exit(100 + errno)`. A static ELF of one PT_LOAD, placed at `proc::BASE`.
fn exec_probe() -> Vec<u8> {
    const CODE: [u8; 42] = [
        0x48, 0x8b, 0x7c, 0x24, 0x10, // mov rdi, [rsp+16]          argv[1]: the path
        0x48, 0x8d, 0x74, 0x24, 0x18, // lea rsi, [rsp+24]          &argv[2]: the new argv
        0x48, 0x8b, 0x0c, 0x24, //       mov rcx, [rsp]             argc
        0x48, 0x8d, 0x54, 0xcc, 0x10, // lea rdx, [rsp+rcx*8+16]    envp
        0xb8, 59, 0, 0, 0, //            mov eax, 59 (execve)
        0x0f, 0x05, //                   syscall
        0xf7, 0xd8, //                   neg eax
        0x83, 0xc0, PROBE_FAILED as u8, // add eax, 100
        0x89, 0xc7, //                   mov edi, eax
        0xb8, 60, 0, 0, 0, //            mov eax, 60 (exit)
        0x0f, 0x05, //                   syscall
        0x0f, 0x0b, //                   ud2
    ];
    let len = 120 + CODE.len();
    let mut b = alloc::vec![0u8; len];
    let mut put = |off: usize, v: u64, n: usize| b[off..off + n].copy_from_slice(&v.to_le_bytes()[..n]);
    put(0, 0x0101_0246_4c45_7f, 7); // \x7fELF, 64-bit, little-endian, version 1
    put(16, 2, 2); // ET_EXEC
    put(18, 0x3e, 2); // x86-64
    put(20, 1, 4);
    put(24, proc::BASE + 120, 8); // e_entry
    put(32, 64, 8); // e_phoff
    put(52, 64, 2); // e_ehsize
    put(54, 56, 2); // e_phentsize
    put(56, 1, 2); // e_phnum
    put(64, 1, 4); // PT_LOAD
    put(68, 5, 4); // R X
    put(80, proc::BASE, 8); // p_vaddr
    put(88, proc::BASE, 8); // p_paddr
    put(96, len as u64, 8); // p_filesz
    put(104, len as u64, 8); // p_memsz
    put(112, 0x1000, 8);
    b[120..].copy_from_slice(&CODE);
    b
}

/// `refuse-probe`: `ud2`; `execve(argv[1], &argv[2], envp)`; `ud2`; then
/// `exit(100 + errno)`. The vehicle's #UD handler records the caller at each
/// `ud2` and steps over it.
fn refuse_probe() -> Vec<u8> {
    tiny_elf(&[
        0x0f, 0x0b, //                   ud2                        the caller, before
        0x48, 0x8b, 0x7c, 0x24, 0x10, // mov rdi, [rsp+16]          argv[1]: the path
        0x48, 0x8d, 0x74, 0x24, 0x18, // lea rsi, [rsp+24]          &argv[2]: the new argv
        0x48, 0x8b, 0x0c, 0x24, //       mov rcx, [rsp]             argc
        0x48, 0x8d, 0x54, 0xcc, 0x10, // lea rdx, [rsp+rcx*8+16]    envp
        0xb8, 59, 0, 0, 0, //            mov eax, 59 (execve)
        0x0f, 0x05, //                   syscall
        0x0f, 0x0b, //                   ud2                        the caller, after
        0xf7, 0xd8, //                   neg eax
        0x83, 0xc0, PROBE_FAILED as u8, // add eax, 100
        0x89, 0xc7, //                   mov edi, eax
        0xb8, 60, 0, 0, 0, //            mov eax, 60 (exit)
        0x0f, 0x05, //                   syscall
        0x0f, 0x0b, //                   ud2
    ])
}

/// A static ELF of one PT_LOAD (R X) at `proc::BASE` around `code`.
fn tiny_elf(code: &[u8]) -> Vec<u8> {
    let len = 120 + code.len();
    let mut b = alloc::vec![0u8; len];
    let mut put = |off: usize, v: u64, n: usize| b[off..off + n].copy_from_slice(&v.to_le_bytes()[..n]);
    put(0, 0x0101_0246_4c45_7f, 7); // \x7fELF, 64-bit, little-endian, version 1
    put(16, 2, 2); // ET_EXEC
    put(18, 0x3e, 2); // x86-64
    put(20, 1, 4);
    put(24, proc::BASE + 120, 8); // e_entry
    put(32, 64, 8); // e_phoff
    put(52, 64, 2); // e_ehsize
    put(54, 56, 2); // e_phentsize
    put(56, 1, 2); // e_phnum
    put(64, 1, 4); // PT_LOAD
    put(68, 5, 4); // R X
    put(80, proc::BASE, 8); // p_vaddr
    put(88, proc::BASE, 8); // p_paddr
    put(96, len as u64, 8); // p_filesz
    put(104, len as u64, 8); // p_memsz
    put(112, 0x1000, 8);
    b[120..].copy_from_slice(code);
    b
}

/// `execve-ud`: `ud2` (the vehicle's #UD handler calls `execve` on this
/// process there and steps over it), then `exit(argc)`, read off its own
/// stack.
fn execve_ud() -> Vec<u8> {
    tiny_elf(&[
        0x0f, 0x0b, //                   ud2
        0x48, 0x8b, 0x3c, 0x24, //       mov rdi, [rsp]             argc
        0xb8, 60, 0, 0, 0, //            mov eax, 60 (exit)
        0x0f, 0x05, //                   syscall
        0x0f, 0x0b, //                   ud2
    ])
}

/// `done`: `exit(0)`.
fn done_elf() -> Vec<u8> {
    tiny_elf(&[
        0x31, 0xff, //                   xor edi, edi
        0xb8, 60, 0, 0, 0, //            mov eax, 60 (exit)
        0x0f, 0x05, //                   syscall
        0x0f, 0x0b, //                   ud2
    ])
}

/// `landed`: `ud2` (row 12's handler records the process there and steps
/// over it), then `exit(0)`.
fn landed_elf() -> Vec<u8> {
    tiny_elf(&[
        0x0f, 0x0b, //                   ud2                        the new program, running
        0x31, 0xff, //                   xor edi, edi
        0xb8, 60, 0, 0, 0, //            mov eax, 60 (exit)
        0x0f, 0x05, //                   syscall
        0x0f, 0x0b, //                   ud2
    ])
}

fn mount_ktest() -> bool {
    let mut files: Vec<(&'static str, InodeRef)> = Vec::new();
    files.push(("exec-probe", Arc::new(MemFile { bytes: exec_probe(), ino: 2 })));
    files.push(("execve-ud", Arc::new(MemFile { bytes: execve_ud(), ino: 6 })));
    files.push(("machine", Arc::new(MemFile { bytes: broken("machine"), ino: 7 })));
    files.push(("done", Arc::new(MemFile { bytes: done_elf(), ino: 8 })));
    files.push(("refuse-probe", Arc::new(MemFile { bytes: refuse_probe(), ino: 9 })));
    files.push(("landed", Arc::new(MemFile { bytes: landed_elf(), ino: 10 })));
    for (i, (_, _, name, _)) in REJECTS.iter().enumerate() {
        files.push((name, Arc::new(MemFile { bytes: broken(name), ino: 3 + i as u64 })));
    }
    match vfs::mount("/ktest", Arc::new(KtestDir { files })) {
        Ok(()) => true,
        Err(e) => {
            note(format_args!("vfs: mount(\"/ktest\", the test's directory) returned {}", errno_text(e)));
            false
        }
    }
}

/// Start `exec-probe` to exec `path` as `argv` with `envp`, and wait for it
/// to end. `None` when `exec::spawn` of the probe itself failed (noted).
fn probe(tag: &str, path: &str, argv: &[&str], envp: &[&str]) -> Option<(Pid, bool, Option<i32>)> {
    probe_with(PROBE, tag, path, argv, envp)
}

fn probe_with(probe: &str, tag: &str, path: &str, argv: &[&str], envp: &[&str]) -> Option<(Pid, bool, Option<i32>)> {
    let mut pv: Vec<&str> = alloc::vec!["exec-probe", path];
    pv.extend_from_slice(argv);
    let pid = match exec::spawn(probe, &pv, envp) {
        Ok(p) => p,
        Err(e) => {
            note(format_args!("{tag}: exec::spawn(\"{probe}\", …) returned {}; nothing ran", errno_text(e)));
            return None;
        }
    };
    let ended = wait_exit(pid, 5000);
    Some((pid, ended, exit_of(pid)))
}

/// The errno a probe's failed `execve` reported, from its status.
fn probe_errno(st: Option<i32>) -> Option<i32> {
    let code = (st? >> 8) & 0xff;
    (st? & 0x7f == 0 && code > PROBE_FAILED && code <= PROBE_FAILED + 133).then_some(code - PROBE_FAILED)
}

fn record_of(pid: Pid) -> (String, String) {
    let (mut argv, mut vmas) = (String::new(), String::new());
    proc::each(|p| {
        if p.pid == pid {
            argv = format!("{:?}", p.argv);
            for v in p.vmas.iter() {
                let kind = if v.kind == VmaKind::Text {
                    "text"
                } else if v.kind == VmaKind::Stack {
                    "stack"
                } else {
                    "data"
                };
                let r = if v.prot & proc::prot::READ != 0 { 'r' } else { '-' };
                let w = if v.prot & proc::prot::WRITE != 0 { 'w' } else { '-' };
                let x = if v.prot & proc::prot::EXEC != 0 { 'x' } else { '-' };
                vmas.push_str(&format!(" {kind} {:#x}-{:#x} {r}{w}{x};", v.start.0, v.end.0));
            }
        }
    });
    (argv, vmas)
}

// ── args: rows 1–5 ───────────────────────────────────────────────────────

/// Prints every row `args` would have printed as `not ok`. Only for when
/// `args` never started, so no line of its own can duplicate one.
fn args_never_ran() {
    for (i, id) in ARGS_IDS.iter().enumerate() {
        row(false, i + 1, id);
    }
}

fn run_args(nonce: &str, mounted: bool) {
    let d = ktap::nonce().map_or(0, |n| ktap::derive(&n, ARGV_ID));
    let derived = format!("{d:016x}");
    note(format_args!("{ARGV_ID}: derive(nonce, \"{ARGV_ID}\") = {derived}; args gets it as argv[3]"));
    if !mounted {
        note(format_args!("args: not run, the file systems are not mounted"));
        return args_never_ran();
    }
    let envp = format!("SBOOT_NONCE={nonce}");
    let argv = ["args", nonce, "1", derived.as_str()];
    // The strings, NUL-terminated: the path, argv, envp and the 16 random
    // bytes. The pad brings them to 15 mod 16, and with 4 + 2 pointers the
    // vector's word count is odd (module comment).
    let base = 16 + "/args".len() + 1 + argv.iter().map(|a| a.len() + 1).sum::<usize>() + envp.len() + 1;
    let pre = "SBOOT_PAD=";
    let k = (1..=16).find(|k| (base + pre.len() + k + 1) % 16 == 15).unwrap_or(1);
    let pad = format!("{pre}{}", "x".repeat(k));
    note(format_args!(
        "exec.rsp.aligned: args gets envp [\"{pad}\", \"SBOOT_NONCE=…\"]: {} bytes of strings with the path and the 16 random bytes, and an odd number of words below them",
        base + pad.len() + 1
    ));
    let Some((pid, ended, st)) = probe("args", "/args", &argv, &[pad.as_str(), envp.as_str()]) else {
        return args_never_ran();
    };
    let (argv, vmas) = record_of(pid);
    note(format_args!(
        "args: pid {} (started as exec-probe, execve(\"/args\")): {}; its record now: argv {argv}, VMAs:{vmas}",
        pid.0,
        if ended { status_text(st) } else { String::from("did not end within 5 s") }
    ));
    if let Some(e) = probe_errno(st) {
        note(format_args!("args: execve(\"/args\", …) returned {} in pid {}; args never ran", errno_text(e as Errno), pid.0));
        return args_never_ran();
    }
    if st.is_some_and(|s| s & 0x7f != 0) {
        note(format_args!("args: pid {} was killed, so any row it had not printed yet is missing", pid.0));
    }
}

// ── The refusals: rows 6–8 ───────────────────────────────────────────────

fn reject(n: usize, id: &str, name: &str, what: &str, mounted: bool) {
    let path = format!("/ktest/{name}");
    note(format_args!("{id}: {path} is args with {what}"));
    if !mounted {
        note(format_args!("{id}: /ktest is not mounted"));
        return row(false, n, id);
    }
    let mut ok = true;
    // 1. The kernel's own exec of it.
    let (r, procs, frames) = quiet(|| {
        let (p0, f0) = (nprocs(), frame::stats().used);
        let r = exec::spawn(&path, &[name], &[]);
        (r, (p0, nprocs()), (f0, frame::stats().used))
    });
    match r {
        Err(e) if e == ENOEXEC => {}
        Err(e) => {
            ok = false;
            note(format_args!("{id}: exec::spawn(\"{path}\") returned {}, not -8 (ENOEXEC)", errno_text(e)));
        }
        Ok(pid) => {
            ok = false;
            note(format_args!("{id}: exec::spawn(\"{path}\") accepted it as pid {}, where it must return -8 (ENOEXEC)", pid.0));
            wait_exit(pid, 2000);
        }
    }
    note(format_args!(
        "{id}: exec::spawn: processes {} before, {} after; frames in use {} before, {} after",
        procs.0, procs.1, frames.0, frames.1
    ));
    // 2. A running program's execve of it: refused, the caller as it was,
    // and the program goes on.
    REFUSE_SEEN.lock().clear();
    REFUSE_MODE.store(true, Ordering::Relaxed);
    idt::set_handler(6, on_ud, 0);
    let probed = probe_with(REFUSE_PROBE, id, &path, &[name], &[]);
    REFUSE_MODE.store(false, Ordering::Relaxed);
    // Back to the learner's routing of ring-3 #UD and #GP.
    proc::init();
    let Some((pid, ended, st)) = probed else {
        return row(false, n, id);
    };
    let seen = core::mem::take(&mut *REFUSE_SEEN.lock());
    match seen.as_slice() {
        [Some(before), Some(after)] if before == after => {}
        [Some(before), Some(after)] => {
            ok = false;
            note(format_args!(
                "{id}: pid {} after its refused execve(\"{path}\"): {} changed — a refused execve must leave the caller exactly as it was",
                pid.0,
                caller_diffs(before, after).join(", ")
            ));
        }
        [_, _] => {
            ok = false;
            note(format_args!("{id}: pid {}: no running process was found around its execve(\"{path}\")", pid.0));
        }
        [_] => {
            ok = false;
            note(format_args!("{id}: pid {} never came back from its refused execve(\"{path}\")", pid.0));
        }
        _ => {
            ok = false;
            note(format_args!("{id}: pid {} never reached its execve(\"{path}\")", pid.0));
        }
    }
    if probe_errno(st) == Some(ENOEXEC as i32) {
        note(format_args!("{id}: pid {}'s execve(\"{path}\") returned -8 (ENOEXEC) and the program went on to exit({})", pid.0, PROBE_FAILED + 8));
    } else {
        ok = false;
        let why = match (st, probe_errno(st)) {
            _ if !ended => String::from("did not end within 5 s"),
            (_, Some(e)) => format!("execve returned {} where -8 (ENOEXEC) was due", errno_text(e as Errno)),
            (Some(s), _) if s == RAN_ANYWAY << 8 => String::from("execve accepted it: the broken image ran and exit(99)"),
            (other, _) => format!("{} — not the probe's exit({}) after a refused execve", status_text(other), PROBE_FAILED + 8),
        };
        note(format_args!("{id}: pid {} (execve of {path} from ring 3): {why}", pid.0));
    }
    row(ok, n, id);
}

// ── Row 9: a refused exec gives its frames back ───────────────────────────

/// One `exec::spawn` that should be refused: `Ok(errno)` when it was, or
/// `Err(pid)` when it started a process (waited for until it ends).
fn refuse_once(path: &str, argv: &[&str]) -> Result<Errno, Pid> {
    match quiet(|| exec::spawn(path, argv, &[])) {
        Err(e) => Ok(e),
        Ok(pid) => {
            wait_exit(pid, 2000);
            Err(pid)
        }
    }
}

fn refuse_frees(mounted: bool, big: &[&str]) {
    if !mounted {
        note(format_args!("{FREES_ID}: /ktest is not mounted"));
        return row(false, FREES_ROW, FREES_ID);
    }
    let machine_argv = ["machine"];
    let mut big_argv: Vec<&str> = alloc::vec!["exec-probe", "/ktest/nothing"];
    big_argv.extend_from_slice(big);
    FRAME_LIES.lock().clear();
    // Warm up: one of each, so a first-time allocation is not counted.
    let _ = refuse_once(MACHINE, &machine_argv);
    let _ = refuse_once(PROBE, &big_argv);
    sched::yield_now();
    let f0 = frames_used();
    let (mut refused, mut accepted) = (0usize, Vec::new());
    let mut errs: Vec<Errno> = Vec::new();
    for _ in 0..FREES_EACH {
        for (path, argv) in [(MACHINE, &machine_argv[..]), (PROBE, &big_argv[..])] {
            match refuse_once(path, argv) {
                Ok(e) => {
                    refused += 1;
                    if !errs.contains(&e) {
                        errs.push(e);
                    }
                }
                Err(pid) => accepted.push((path, pid)),
            }
        }
    }
    sched::yield_now();
    let f1 = frames_used();
    let grown = f1 as i64 - f0 as i64;
    let errs: Vec<String> = errs.into_iter().map(errno_text).collect();
    note(format_args!(
        "{FREES_ID}: {refused} refused exec::spawn calls ({FREES_EACH} of {MACHINE}, e_machine EM_386; {FREES_EACH} of {PROBE} with an argv of {} bytes, more than the whole stack; errors {errs:?}): frames in use {f0} before, {f1} after ({grown:+})",
        BIG_STRS * (BIG_LEN + 1)
    ));
    for (path, pid) in &accepted {
        note(format_args!("{FREES_ID}: exec::spawn(\"{path}\") was not refused: it started pid {}, so that call is not counted", pid.0));
    }
    let honest = frames_honest(FREES_ID);
    let ok = refused > 0 && grown < refused as i64 && honest;
    if refused == 0 {
        note(format_args!("{FREES_ID}: no exec::spawn was refused, so nothing was counted"));
    } else if grown >= refused as i64 {
        note(format_args!(
            "{FREES_ID}: the frames in use grew by {grown} over {refused} refusals: a refused exec must give back every frame it took — the new address space's tables and PML4, the segments' pages, the stack pages — before it returns the error"
        ));
    }
    row(ok, FREES_ROW, FREES_ID);
}

// ── Row 10: a successful exec gives back the space it replaced ────────────

/// One process: the probe exec'd `k` more times, then `/ktest/done` —
/// `k + 1` successful execs. The frames in use it leaves grown, or why
/// it did not end with `exit(0)`. A warm-up (`measure` false) reads
/// `frame::stats()` without the drain.
fn chain(k: usize, measure: bool) -> Result<i64, String> {
    let count = || if measure { frames_used() } else { quiet(|| frame::stats().used) };
    let mut argv: Vec<&str> = alloc::vec!["exec-probe"];
    for _ in 0..k {
        argv.push(PROBE);
        argv.push("exec-probe");
    }
    argv.push(DONE);
    argv.push("done");
    sched::yield_now();
    let f0 = count();
    let pid = match quiet(|| exec::spawn(PROBE, &argv, &[])) {
        Ok(p) => p,
        Err(e) => return Err(format!("exec::spawn(\"{PROBE}\", …) returned {}", errno_text(e))),
    };
    let ended = wait_exit(pid, 5000);
    let st = exit_of(pid);
    sched::yield_now();
    let f1 = count();
    if st == Some(0) {
        return Ok(f1 as i64 - f0 as i64);
    }
    Err(match probe_errno(st) {
        _ if !ended => format!("pid {} did not end within 5 s", pid.0),
        Some(e) => format!("pid {}: an execve in the chain returned {}", pid.0, errno_text(e as Errno)),
        None => format!("pid {} {}, not the exit(0) at the chain's end", pid.0, status_text(st)),
    })
}

fn execve_frees(mounted: bool) {
    if !mounted {
        note(format_args!("{REPLACE_ID}: /ktest is not mounted"));
        return row(false, REPLACE_ROW, REPLACE_ID);
    }
    // Warm up: one of each, so a first-time allocation is not counted.
    let _ = chain(0, false);
    let _ = chain(CHAIN, false);
    FRAME_LIES.lock().clear();
    let (one, many) = (chain(0, true), chain(CHAIN, true));
    let honest = frames_honest(REPLACE_ID);
    let ok = match (&one, &many) {
        (Ok(a), Ok(b)) => {
            note(format_args!(
                "{REPLACE_ID}: one process exec'd once grew the frames in use by {a:+}; one exec'd {} times grew them by {b:+}",
                CHAIN + 1
            ));
            let ok = b - a < CHAIN as i64;
            if !ok {
                note(format_args!(
                    "{REPLACE_ID}: {} more successful execs kept {} more frames: a successful execve must give back the address space it replaced — its tables and PML4, the segments' pages, the stack pages",
                    CHAIN,
                    b - a
                ));
            }
            ok
        }
        _ => {
            for r in [&one, &many] {
                if let Err(why) = r {
                    note(format_args!("{REPLACE_ID}: {why}"));
                }
            }
            false
        }
    };
    row(ok && honest, REPLACE_ROW, REPLACE_ID);
}

// ── Row 11: execve itself, refused, leaves the caller as it was ──────────

/// What the vehicle can see of a process and the space its task runs on.
#[derive(PartialEq, Eq, Clone)]
struct Caller {
    pid: u64,
    argv: Vec<String>,
    vmas: Vec<(u64, u64, u8, u8)>,
    root: u64,
    space: Option<u64>,
    cr3: u64,
    /// FNV-1a over the words of every page of every VMA, read through the task's space
    /// and the HHDM (an unmapped page hashes as a marker).
    mem: u64,
}

/// What differs between two views of the same caller.
fn caller_diffs(b: &Caller, a: &Caller) -> Vec<&'static str> {
    let mut diffs: Vec<&str> = Vec::new();
    if b.argv != a.argv {
        diffs.push("its argv");
    }
    if b.vmas != a.vmas {
        diffs.push("its VMA list");
    }
    if b.root != a.root {
        diffs.push("its record's root");
    }
    if b.space != a.space {
        diffs.push("the address space its task runs on");
    }
    if b.cr3 != a.cr3 {
        diffs.push("CR3");
    }
    if b.mem != a.mem {
        diffs.push("the bytes of its memory (its text and stack pages)");
    }
    diffs
}

fn caller() -> Option<Caller> {
    let space = task::with_current(|t| t.aspace.as_ref().map(|a| a.root().0));
    let cr3: u64;
    // SAFETY: reading CR3 has no side effect.
    unsafe { asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags)) };
    let ranges: Vec<(u64, u64)> = proc::with_current(|p| p.vmas.iter().map(|v| (v.start.0, v.end.0)).collect())?;
    let mem = task::with_current(|t| t.aspace.as_ref().map(|a| memory_hash(a, &ranges))).unwrap_or(0);
    proc::with_current(|p| Caller {
        mem,
        pid: p.pid.0,
        argv: p.argv.clone(),
        vmas: p
            .vmas
            .iter()
            .map(|v| (v.start.0, v.end.0, v.prot, if v.kind == VmaKind::Text { 0 } else if v.kind == VmaKind::Stack { 1 } else { 2 }))
            .collect(),
        root: p.root.0,
        space,
        cr3: cr3 & 0x000f_ffff_ffff_f000,
    })
}

/// FNV-1a, a word at a time, over the pages of `ranges` in `a`, each read
/// through the HHDM.
fn memory_hash(a: &kernel::paging::AddressSpace, ranges: &[(u64, u64)]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |w: u64| h = (h ^ w).wrapping_mul(0x100_0000_01b3);
    for &(start, end) in ranges {
        let mut va = start & !0xfff;
        while va < end {
            match a.translate(VirtAddr(va)) {
                Some((pa, _)) => {
                    // SAFETY: `pa` is a present page of this space, and the
                    // HHDM maps every physical page; the page is only read.
                    let page = unsafe { core::slice::from_raw_parts(kernel::paging::hhdm(pa).as_ptr::<u64>(), 512) };
                    page.iter().for_each(|&w| eat(w));
                }
                None => eat(0xa5a5_a5a5_a5a5_a5a5),
            }
            va += 4096;
        }
    }
    h
}

/// What the handler saw: the caller before, the two `execve` results and
/// the caller after each, frames around both.
struct Seen {
    before: Option<Caller>,
    calls: [(Option<Errno>, Option<Caller>); 2],
    frames: (usize, usize),
}

static UD_SEEN: SpinLock<Option<Seen>> = SpinLock::new(None);
/// Rows 6–8: the handler only records the caller, at each of `refuse-probe`'s
/// two `ud2`s.
static REFUSE_MODE: AtomicBool = AtomicBool::new(false);
static REFUSE_SEEN: SpinLock<Vec<Option<Caller>>> = SpinLock::new(Vec::new());
/// Row 12: the handler calls the successful `execve` instead.
static REPLACE_MODE: AtomicBool = AtomicBool::new(false);
/// Row 12: set just before the call.
static REPLACE_CALLED: AtomicBool = AtomicBool::new(false);
/// Row 12: what the call returned, if it returned at all.
static REPLACE_RET: SpinLock<Option<Option<Errno>>> = SpinLock::new(None);
/// Row 12: the caller just before the call.
static REPLACE_BEFORE: SpinLock<Option<Caller>> = SpinLock::new(None);
/// Row 12: each later ring-3 `ud2` of that process — its rip and the process
/// there (`landed`'s entry, once the new program runs).
static REPLACE_LANDED: SpinLock<Vec<(u64, Option<Caller>)>> = SpinLock::new(Vec::new());
static BIG_ARGV: SpinLock<Vec<String>> = SpinLock::new(Vec::new());

/// The vehicle's #UD handler while `execve-ud` runs: on that process's task,
/// in ring 0, interrupts off — what a syscall handler is.
fn on_ud(tf: &mut TrapFrame) {
    if REFUSE_MODE.load(Ordering::Relaxed) && tf.cs & 3 == 3 {
        let c = caller();
        REFUSE_SEEN.lock().push(c);
        tf.rip += 2;
        return;
    }
    if REPLACE_MODE.load(Ordering::Relaxed) && tf.cs & 3 == 3 {
        if REPLACE_CALLED.load(Ordering::Relaxed) {
            let c = caller();
            REPLACE_LANDED.lock().push((tf.rip, c));
        } else {
            *REPLACE_BEFORE.lock() = caller();
            REPLACE_CALLED.store(true, Ordering::Relaxed);
            let e = proc::with_current(|p| exec::execve(p, LANDED, &["landed"], &[]));
            *REPLACE_RET.lock() = Some(e);
        }
        tf.rip += 2;
        return;
    }
    if tf.cs & 3 != 3 || UD_SEEN.lock().is_some() {
        panic!("ktest-13: an unexpected #UD at rip={:#x} cs={:#x}", tf.rip, tf.cs);
    }
    let big = BIG_ARGV.lock();
    let mut argv: Vec<&str> = alloc::vec!["exec-probe", "/ktest/nothing"];
    argv.extend(big.iter().map(|s| s.as_str()));
    let before = caller();
    let f0 = frame::stats().used;
    let mut calls: [(Option<Errno>, Option<Caller>); 2] = [(None, None), (None, None)];
    let e = proc::with_current(|p| exec::execve(p, MACHINE, &["machine"], &[]));
    calls[0] = (e, caller());
    let e = proc::with_current(|p| exec::execve(p, PROBE, &argv, &[]));
    calls[1] = (e, caller());
    let f1 = frame::stats().used;
    drop(argv);
    drop(big);
    *UD_SEEN.lock() = Some(Seen { before, calls, frames: (f0, f1) });
    tf.rip += 2;
}

fn execve_refuses(mounted: bool, big: &[&str]) {
    if !mounted {
        note(format_args!("{EXECVE_ID}: /ktest is not mounted"));
        return row(false, EXECVE_ROW, EXECVE_ID);
    }
    *BIG_ARGV.lock() = big.iter().map(|s| String::from(*s)).collect();
    let argv = ["execve-ud", "a", "b"];
    idt::set_handler(6, on_ud, 0);
    let spawned = quiet(|| exec::spawn(EXECVE_UD, &argv, &[]));
    let (pid, ended, st) = match spawned {
        Ok(pid) => {
            let ended = wait_exit(pid, 5000);
            (Some(pid), ended, exit_of(pid))
        }
        Err(e) => {
            note(format_args!("{EXECVE_ID}: exec::spawn(\"{EXECVE_UD}\") returned {}; nothing ran", errno_text(e)));
            (None, false, None)
        }
    };
    // Back to the learner's routing of ring-3 #UD and #GP.
    proc::init();
    let Some(pid) = pid else { return row(false, EXECVE_ROW, EXECVE_ID) };
    let seen = UD_SEEN.lock().take();
    let mut ok = true;
    match seen {
        None => {
            ok = false;
            note(format_args!(
                "{EXECVE_ID}: pid {} ({}) never came back from execve: it {}",
                pid.0,
                EXECVE_UD,
                if ended { status_text(st) } else { String::from("did not end within 5 s") }
            ));
        }
        Some(s) => {
            let what = ["execve(p, \"/ktest/machine\", [\"machine\"], []) (args with e_machine = EM_386)", "execve(p, \"/ktest/exec-probe\", <an argv larger than the whole stack>, [])"];
            for (i, (e, after)) in s.calls.iter().enumerate() {
                let e = e.unwrap_or(0);
                let errno_ok = if i == 0 { e == ENOEXEC } else { e == LINUX_E2BIG };
                note(format_args!("{EXECVE_ID}: pid {}: {} returned {}", pid.0, what[i], if e > 0 { errno_text(e) } else { format!("{e}") }));
                if !errno_ok {
                    ok = false;
                    note(format_args!("{EXECVE_ID}: {} must return {}", what[i], if i == 0 { "-8 (ENOEXEC)" } else { "-7 (E2BIG): the strings and the vector do not fit the initial stack" }));
                }
                if *after != s.before {
                    ok = false;
                    let diffs = match (&s.before, after) {
                        (Some(b), Some(a)) => caller_diffs(b, a),
                        _ => alloc::vec!["its record (no running process found)"],
                    };
                    note(format_args!(
                        "{EXECVE_ID}: after the refused {}, the caller changed: {} — a failed execve must leave the caller exactly as it was, so the new image is built beside the old one and swapped in only when it is whole",
                        what[i],
                        diffs.join(", ")
                    ));
                }
            }
            note(format_args!("{EXECVE_ID}: frames in use {} before the two calls, {} after", s.frames.0, s.frames.1));
            let want = (argv.len() as i32) << 8;
            if st != Some(want) {
                ok = false;
                note(format_args!(
                    "{EXECVE_ID}: pid {} then {}, where the program goes on to exit({}) (its argc, read off its own stack)",
                    pid.0,
                    if ended { status_text(st) } else { String::from("did not end within 5 s") },
                    argv.len()
                ));
            }
        }
    }
    row(ok, EXECVE_ROW, EXECVE_ID);
}

// ── Row 12: execve itself, succeeding, replaces the caller ───────────────

fn execve_replaces(mounted: bool) {
    if !mounted {
        note(format_args!("{REPLACES_ID}: /ktest is not mounted"));
        return row(false, REPLACES_ROW, REPLACES_ID);
    }
    let argv = ["execve-ud", "replace"];
    REPLACE_CALLED.store(false, Ordering::Relaxed);
    *REPLACE_RET.lock() = None;
    *REPLACE_BEFORE.lock() = None;
    REPLACE_LANDED.lock().clear();
    REPLACE_MODE.store(true, Ordering::Relaxed);
    idt::set_handler(6, on_ud, 0);
    let spawned = quiet(|| exec::spawn(EXECVE_UD, &argv, &[]));
    let (pid, ended, st) = match spawned {
        Ok(pid) => {
            let ended = wait_exit(pid, 5000);
            (Some(pid), ended, exit_of(pid))
        }
        Err(e) => {
            note(format_args!("{REPLACES_ID}: exec::spawn(\"{EXECVE_UD}\") returned {}; nothing ran", errno_text(e)));
            (None, false, None)
        }
    };
    REPLACE_MODE.store(false, Ordering::Relaxed);
    // Back to the learner's routing of ring-3 #UD and #GP.
    proc::init();
    let Some(pid) = pid else { return row(false, REPLACES_ROW, REPLACES_ID) };
    let what = "execve(p, \"/ktest/landed\", [\"landed\"], [])";
    let (rec_argv, vmas) = record_of(pid);
    let status = if ended { status_text(st) } else { String::from("did not end within 5 s") };
    note(format_args!("{REPLACES_ID}: pid {} ({EXECVE_UD}, argc {}) called {what}; it then {status}; its record now: argv {rec_argv}, VMAs:{vmas}", pid.0, argv.len()));
    let mut ok = true;
    if !REPLACE_CALLED.load(Ordering::Relaxed) {
        ok = false;
        note(format_args!("{REPLACES_ID}: pid {} never reached its ud2, so {what} was not called", pid.0));
    }
    if let Some(e) = REPLACE_RET.lock().take() {
        ok = false;
        note(format_args!(
            "{REPLACES_ID}: {what} returned {} — an execve that succeeds does not return: the caller's program is replaced and the CPU enters the new one",
            match e {
                Some(e) if e > 0 => errno_text(e),
                Some(e) => format!("{e}"),
                None => String::from("with no running process found"),
            }
        ));
    }
    // The new program must have run: `landed` traps at its entry, in the
    // process that called execve, on a space that is not the old one.
    let before = REPLACE_BEFORE.lock().take();
    let landed = core::mem::take(&mut *REPLACE_LANDED.lock());
    let entry = proc::BASE + 120;
    match (before, landed.as_slice()) {
        (Some(b), [(rip, Some(a))]) if *rip == entry => {
            let mut wrong: Vec<String> = Vec::new();
            if a.pid != b.pid {
                wrong.push(format!("it ran as pid {}, not the caller's pid {}", a.pid, b.pid));
            }
            if a.argv != ["landed"] {
                wrong.push(format!("its record's argv was {:?}, not [\"landed\"]", a.argv));
            }
            if a.space != Some(a.cr3) || a.root != a.cr3 {
                wrong.push(format!(
                    "the record's space {:#x}, the task's {:?} and CR3 {:#x} were not one space",
                    a.root, a.space.map(|x| format!("{x:#x}")), a.cr3
                ));
            }
            if a.cr3 == b.cr3 || a.root == b.root {
                wrong.push(format!("it ran on the caller's old space {:#x}", b.cr3));
            }
            if !wrong.is_empty() {
                ok = false;
                note(format_args!("{REPLACES_ID}: pid {}: when /ktest/landed began to run, {}", pid.0, wrong.join("; ")));
            }
        }
        (None, _) => {
            ok = false;
            note(format_args!("{REPLACES_ID}: pid {}: no running process was found at its ud2", pid.0));
        }
        (Some(_), []) => {
            ok = false;
            note(format_args!(
                "{REPLACES_ID}: pid {}: no instruction of /ktest/landed ever ran — the program an execve succeeds with must be the one the process runs next",
                pid.0
            ));
        }
        (Some(_), more) => {
            ok = false;
            let rips: Vec<String> = more.iter().map(|(r, _)| format!("{r:#x}")).collect();
            note(format_args!(
                "{REPLACES_ID}: pid {} trapped at {} after the call, where /ktest/landed traps once, at its entry {entry:#x}",
                pid.0,
                rips.join(", ")
            ));
        }
    }
    if st != Some(0) {
        ok = false;
        note(format_args!("{REPLACES_ID}: pid {} {status}, where /ktest/landed, once it runs, exits 0", pid.0));
    }
    if rec_argv != format!("{:?}", ["landed"]) {
        ok = false;
        note(format_args!("{REPLACES_ID}: pid {}'s record keeps argv {rec_argv}, where the program it was given runs as [\"landed\"]", pid.0));
    }
    row(ok, REPLACES_ROW, REPLACES_ID);
}

// ── The machine ──────────────────────────────────────────────────────────

/// CR0.EM = 0, CR0.TS = 0, CR0.MP = 1; CR4.OSFXSR = 1, CR4.OSXMMEXCPT = 1;
/// `fninit`; MXCSR = 0x1F80 — so the switch's FXSAVE saves xmm before the
/// first program's `user::enter` turns SSE on.
fn sse_on() {
    let mxcsr: u32 = 0x1F80;
    // SAFETY: these bits change only how x87/SSE instructions behave; the
    // kernel is soft-float and uses neither.
    unsafe {
        asm!(
            "mov {t}, cr0",
            "and {t}, {clear0}",
            "or {t}, 2",
            "mov cr0, {t}",
            "mov {t}, cr4",
            "or {t}, {set4}",
            "mov cr4, {t}",
            "fninit",
            "ldmxcsr [{m}]",
            t = out(reg) _,
            clear0 = in(reg) !((1u64 << 2) | (1 << 3)),
            set4 = const (1u64 << 9) | (1 << 10),
            m = in(reg) &mxcsr,
            options(nostack),
        )
    };
}

fn prelude() {
    let (k, i, _) = shim::stacks();
    gdt::init(k, i);
    idt::init();
    let bi = bootinfo::get().expect("the shim published the BootInfo");
    let mut map = [bi.memmap()[0]; bootinfo::MEMMAP_MAX];
    let n = clipped(bi.memmap(), &mut map);
    frame::init(&map[..n], bi.kernel_range());
    syscall::init();
    syscall::set_kernel_stack(k);
    heap::init(VirtAddr(HEAP_START), HEAP_PAGES);
    sse_on();
    proc::init();
    sched::init(k);
    sched::yield_now();
    irq::init();
    let t0 = ticks();
    let mut c = Clock::new();
    while ticks() < t0 + 2 && c.ms() < 100 {
        core::hint::spin_loop();
    }
}

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    let _bi = shim::boot_info();
    serial::init();
    ktap::begin(PLAN);
    prelude();
    let nonce = nonce_str();
    let frames0 = frames_used();

    let mounted = mount_archive() && mount_ktest();
    run_args(&nonce, mounted);

    let frames1 = frames_used();
    for (n, id, name, what) in REJECTS {
        reject(n, id, name, what, mounted);
    }
    note(format_args!(
        "frames in use: {frames0} before the mounts, {frames1} after args ended, {} after the three refusals",
        frames_used()
    ));
    let big_strs: Vec<String> = (0..BIG_STRS).map(|i| alloc::vec![b'a' + (i % 26) as u8; BIG_LEN]).map(|v| String::from_utf8(v).unwrap_or_default()).collect();
    let big: Vec<&str> = big_strs.iter().map(|s| s.as_str()).collect();
    refuse_frees(mounted, &big);
    execve_frees(mounted);
    execve_refuses(mounted, &big);
    execve_replaces(mounted);
    quiet(ktap::end);
    halt()
}
