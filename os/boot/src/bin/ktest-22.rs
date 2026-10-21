//! Lab 22's vehicle (`22-mounts-proc-dev`): the walk across mount points,
//! `/proc` and `/dev` — `fs/mount.rs`, `fs/procfs.rs`, `fs/devfs.rs`, the
//! crossing in `vfs::walk`, `st_dev` per file system, syscall 165.
//!
//! The disk is `ktest-22.img` as the course's image writer made it (CONTRACT
//! §A.7); the guest writes it (the probes below go into `/tmp`), so the
//! course boots a copy. The vehicle mounts four, through the learner's
//! `mount::mount` and roots it made itself — ext2 (partition 2, through the
//! learner's `Ata`, `partition2` and `Ext2::mount`) at `/` from
//! `/dev/hda2`, the boot archive at `/boot`, a `ProcFs::new()` at `/proc`
//! and a `DevFs::new()` at `/dev` — and keeps each root, so every
//! expectation about where a path lands is one of those roots' own
//! `lookup`s, never the walk under test. Row 8 adds a fifth mount through
//! syscall 165 from a program. Programs are the course's own off ext2's
//! `/bin` (`cat`, `mounts`, `ls`) or assembled here (a straight run of
//! syscalls whose results the program writes back), written to `/tmp` and
//! run through the learner's exec with a pipe as fds 1 and 2. Eight rows;
//! row 1 is printed LAST, after its `getdents64` part, so a `todo!` there
//! costs only row 1 (rows 2–8 have printed by then):
//!
//!   1 `mount.cross.walk` (plain) — `vfs::walk` lands where the roots say,
//!     by `(st_dev, st_ino, st_mode)`: `/boot/init`, `boot/init` from `/`,
//!     `/tmp/../boot/init`, `/boot/..`, `/boot/../tmp`, `/proc/../dev/null`,
//!     `/proc/sboot/nonce`, `/dev/./null`, `/..`, `null` and
//!     `../boot/init` from `/dev`'s root; `/dev/null/` is `-ENOTDIR` and
//!     `/proc/nosuch` `-ENOENT`. Lab 13's walk (the longest-prefix mount,
//!     decided once) fails six of them (measured 2026-10-17). Last in the
//!     file: `/bin/ls /proc` prints `meminfo mounts sboot uptime` and one
//!     name per pid the kernel has, `ls`'s own among them, and `/bin/ls
//!     /dev` exactly `console null tty zero` — `getdents64` on the mounted
//!     roots, not on ext2's empty directories under them.
//!   2 `proc.pid.cmdline` (U, derived) — `/bin/cat` started with argv
//!     `cat - <1–3 nonce-derived words>` and a pipe as its fd 0 whose write
//!     end this file holds and writes nothing to, so `cat` sleeps in `read`
//!     and stays alive; its pid is what `exec::spawn` returned. A second
//!     `/bin/cat /proc/<pid>/cmdline` must print the argv, each string
//!     followed by one NUL, and exit 0 — read while the first is alive
//!     (checked before and after). Then the write end is dropped and the
//!     first `cat` ends.
//!   3 `proc.meminfo.moves` (derived) — `/proc/meminfo` read through the
//!     learner's `read_at`, interrupts off: `MemTotal` and `MemFree` equal
//!     `frame::stats()` × 4 kB; this file takes N frames itself with
//!     `frame::alloc` (N nonce-derived, 16–256) and `MemFree` falls by
//!     exactly 4N kB — N is this file's own count, the control — and comes
//!     back when it frees them. A reading counts only when `frame::stats()`
//!     is the same before and after it.
//!   4 `proc.mounts.lists.all` (plain) — `/proc/mounts` is exactly the four
//!     lines `<source> <mount point> <type> rw 0 0`, in mount order;
//!     `/bin/mounts` lists the four mount points and types and exits 0.
//!   5 `stat.st_dev.differs` (plain) — a program's `stat` of `/`, `/boot`,
//!     `/proc`, `/dev` gives four different `st_dev`s, each the root's own
//!     `stat().dev`; of `/tmp`, `/boot/init`, `/proc/meminfo`, `/proc/sboot`,
//!     `/dev/null` the `st_dev` of the mount each is in.
//!   6 `dev.null.zero` (plain) — a program: `/dev/null` reads 0 bytes and
//!     takes a 100-byte write; `/dev/zero` fills a 4096-byte buffer this
//!     file filled with 0xa5 with zeros and takes a write; `stat` gives
//!     `S_IFCHR | 0666` and `st_rdev` 1:3 and 1:5; `open("/dev/null",
//!     O_WRONLY | O_CREAT | O_TRUNC)` (a shell's `>`) succeeds.
//!   7 `proc.sboot.nonce` (plain) — `/bin/cat /proc/sboot/nonce` prints the
//!     run's nonce as this file reads it from `ktap::nonce()`, then a
//!     newline.
//!   8 `mount.syscall` (U, plain) — a program: `mount("proc", "/tmp/k22
//!     proc", "proc", 0, 0)` is 0, `/tmp/k22 proc/meminfo` then opens and
//!     reads as `/proc/meminfo` does, and `stat` of the mount point gives a
//!     device that is neither ext2's nor `/proc`'s; the same mount again is
//!     `-EBUSY`, type `"ext2"` and `"nofs"` `-ENODEV`, a relative target
//!     `-EINVAL`, a missing one `-ENOENT`, a file `-ENOTDIR`. Each failure
//!     line names the `mount(...)` call and what it returned.

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
use core::sync::atomic::Ordering;

use kernel::addr::VirtAddr;
use kernel::blk::ata::{self, Ata, BlockDevice};
use kernel::console::{Fd, FileRef};
use kernel::exec;
use kernel::fs::archive::Archive;
use kernel::fs::devfs::DevFs;
use kernel::fs::ext2::Ext2;
use kernel::fs::mount;
use kernel::fs::procfs::ProcFs;
use kernel::fs::vfs::{self, InodeRef, Stat};
use kernel::ktap;
use kernel::pipe;
use kernel::proc::{self, Pid};
use kernel::syscall::Errno;
use kernel::task::{self, Tid};
use kernel::{bootinfo, frame, gdt, heap, idt, irq, sched, serial, syscall, time};

const ARCHIVE_LEN: usize = include_bytes!("../../../user/bin/boot.archive").len();

#[link_section = ".sboot_archive"]
static ARCHIVE: [u8; ARCHIVE_LEN] = *include_bytes!("../../../user/bin/boot.archive");

const HEAP_START: u64 = 0xffff_ffff_d000_0000;
const HEAP_PAGES: usize = 16;
const PM_HZ: u64 = 3_579_545;

const PLAN: usize = 8;
const CROSS_ID: &str = "mount.cross.walk";
const CMDLINE_ID: &str = "proc.pid.cmdline";
const MEMINFO_ID: &str = "proc.meminfo.moves";
const MOUNTS_ID: &str = "proc.mounts.lists.all";
const STDEV_ID: &str = "stat.st_dev.differs";
const DEV_ID: &str = "dev.null.zero";
const NONCE_ID: &str = "proc.sboot.nonce";
const SYSCALL_ID: &str = "mount.syscall";

/// The four mounts (source, mount point, type), in the order this file
/// makes them.
const MOUNTS: [(&str, &str, &str); 4] =
    [("/dev/hda2", "/", "ext2"), ("boot.archive", "/boot", "archive"), ("proc", "/proc", "proc"), ("devfs", "/dev", "devfs")];
/// Row 8's mount point, made in ext2's `/tmp`.
const PROC_AT_NAME: &str = "k22 proc";
const PROC_AT: &str = "/tmp/k22 proc";

/// A probe's data segment, and how much of it a probe may use.
const DATA: u64 = 0x0110_0000;
const MAX_STEPS: usize = 64;
const RESULTS: usize = 8 * MAX_STEPS;
/// How long a program may run before its output is taken as it stands.
const RUN_MS: u64 = 10_000;

const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_STAT: u64 = 4;
const SYS_IOCTL: u64 = 16;
const SYS_MOUNT: u64 = 165;
const O_RDONLY: u64 = 0;
const O_WRONLY: u64 = 1;
const O_RDWR: u64 = 2;
const O_CREAT: u64 = 0o100;
const O_TRUNC: u64 = 0o1000;
const O_NONBLOCK: u32 = 0x800;
const TIOCGWINSZ: u64 = 0x5413;
const S_IFCHR: u64 = 0o020000;
const ENOENT: Errno = 2;
const EBUSY: Errno = 16;
const ENODEV: Errno = 19;
const ENOTDIR: Errno = 20;
const EINVAL: Errno = 22;

fn le64(b: &[u8], at: usize) -> u64 {
    let mut v = [0u8; 8];
    v.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(v)
}

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

/// `f` with interrupts off: the learner's drive and cache hold their locks
/// across a read, and a preempted holder would leave a syscall spinning.
fn quiet<R>(f: impl FnOnce() -> R) -> R {
    let on = irq_off();
    let r = f();
    irq_restore(on);
    r
}

fn note(args: core::fmt::Arguments) {
    quiet(|| ktap::note(args));
}

fn halt() -> ! {
    loop {
        // SAFETY: interrupts off, then wait: nothing runs after this.
        unsafe { asm!("cli", "hlt", options(nomem, nostack)) };
    }
}

fn errno_text(e: Errno) -> String {
    let name = match e {
        2 => "ENOENT",
        5 => "EIO",
        8 => "ENOEXEC",
        9 => "EBADF",
        12 => "ENOMEM",
        13 => "EACCES",
        14 => "EFAULT",
        16 => "EBUSY",
        19 => "ENODEV",
        20 => "ENOTDIR",
        21 => "EISDIR",
        22 => "EINVAL",
        23 => "ENFILE",
        24 => "EMFILE",
        29 => "ESPIPE",
        30 => "EROFS",
        36 => "ENAMETOOLONG",
        38 => "ENOSYS",
        _ => "?",
    };
    format!("-{e} ({name})")
}

/// A syscall's answer as a reader wants it: the number, or `-N (ENAME)`.
fn answer(v: i64) -> String {
    if (-4095..0).contains(&v) {
        errno_text(-v)
    } else {
        format!("{v}")
    }
}

// ── One row's recorder (ktap's shapes, printed in this file's order) ─────

struct Row {
    id: &'static str,
    failed: bool,
    used_nonce: bool,
}

impl Row {
    fn new(id: &'static str) -> Row {
        Row { id, failed: false, used_nonce: false }
    }
    fn fail(&mut self, why: core::fmt::Arguments) {
        self.failed = true;
        note(why);
    }
    fn expect(&mut self, cond: bool, what: &str) {
        if !cond {
            let id = self.id;
            self.fail(format_args!("{id}: {what}: observed false, expected true"));
        }
    }
    fn expect_eq(&mut self, what: &str, observed: u64, expected: u64) {
        if observed != expected {
            let id = self.id;
            self.fail(format_args!("{id}: {what}: observed {observed}, expected {expected}"));
        }
    }
    /// A syscall's result against the one expected.
    fn expect_res(&mut self, what: &str, observed: i64, expected: i64) {
        if observed != expected {
            let id = self.id;
            self.fail(format_args!("{id}: {what} returned {}, expected {}", answer(observed), answer(expected)));
        }
    }
    fn derived(&mut self, id: &str) -> u64 {
        self.used_nonce = true;
        ktap::derive(&ktap::nonce().unwrap_or([b'0'; 32]), id)
    }
    fn finish(mut self, n: usize) {
        if self.used_nonce && ktap::nonce().is_none() {
            self.failed = true;
            note(format_args!("{}: derived from the run's nonce, and this run has none", self.id));
        }
        quiet(|| {
            if self.failed {
                kernel::kprintln!("not ok {n} - {}", self.id);
            } else {
                kernel::kprintln!("ok {n} - {}", self.id);
            }
        });
    }
}

// ── Programs: assembled here, run through the learner's exec ─────────────

/// A syscall argument: an immediate, the result of an earlier step, or the
/// address of a place in the data segment.
#[derive(Clone, Copy)]
enum A {
    I(u64),
    R(usize),
    D(usize),
}

/// `mov <arg reg>, imm64` for rdi, rsi, rdx, r10, r8, r9.
const MOV_IMM: [[u8; 2]; 6] = [[0x48, 0xbf], [0x48, 0xbe], [0x48, 0xba], [0x49, 0xba], [0x49, 0xb8], [0x49, 0xb9]];
/// `mov <arg reg>, [disp32]` (SIB, no base, no index).
const MOV_MEM: [[u8; 4]; 6] = [
    [0x48, 0x8b, 0x3c, 0x25],
    [0x48, 0x8b, 0x34, 0x25],
    [0x48, 0x8b, 0x14, 0x25],
    [0x4c, 0x8b, 0x14, 0x25],
    [0x4c, 0x8b, 0x04, 0x25],
    [0x4c, 0x8b, 0x0c, 0x25],
];

/// A straight run of syscalls; step k's `rax` is stored at `DATA + 8k`.
/// The program ends by writing its whole data segment to fd 1, then
/// `exit(0)`.
struct Probe {
    code: Vec<u8>,
    data: Vec<u8>,
    steps: usize,
}

impl Probe {
    fn new() -> Probe {
        Probe { code: Vec::new(), data: alloc::vec![0; RESULTS], steps: 0 }
    }
    fn str(&mut self, s: &str) -> A {
        let at = self.data.len();
        self.data.extend_from_slice(s.as_bytes());
        self.data.push(0);
        self.data.resize(self.data.len().next_multiple_of(8), 0);
        A::D(at)
    }
    fn buf(&mut self, n: usize) -> (A, usize) {
        let at = self.data.len();
        self.data.resize(at + n.next_multiple_of(8), 0);
        (A::D(at), at)
    }
    fn call(&mut self, nr: u64, args: &[A]) -> usize {
        for (i, a) in args.iter().enumerate() {
            match *a {
                A::I(v) => {
                    self.code.extend_from_slice(&MOV_IMM[i]);
                    self.code.extend_from_slice(&v.to_le_bytes());
                }
                A::D(off) => {
                    self.code.extend_from_slice(&MOV_IMM[i]);
                    self.code.extend_from_slice(&(DATA + off as u64).to_le_bytes());
                }
                A::R(k) => {
                    self.code.extend_from_slice(&MOV_MEM[i]);
                    self.code.extend_from_slice(&((DATA + 8 * k as u64) as u32).to_le_bytes());
                }
            }
        }
        self.code.push(0xb8); // mov eax, imm32
        self.code.extend_from_slice(&(nr as u32).to_le_bytes());
        self.code.extend_from_slice(&[0x0f, 0x05]); // syscall
        let k = self.steps;
        assert!(k < MAX_STEPS, "a probe of more than {MAX_STEPS} steps");
        self.steps += 1;
        self.code.extend_from_slice(&[0x48, 0x89, 0x04, 0x25]); // mov [disp32], rax
        self.code.extend_from_slice(&((DATA + 8 * k as u64) as u32).to_le_bytes());
        k
    }
    /// The ELF: text (R X) at `proc::BASE` from file offset 0, the data
    /// segment (RW) at `DATA` from the next page of the file.
    fn elf(mut self) -> (Vec<u8>, usize) {
        let len = self.data.len();
        self.call(SYS_WRITE, &[A::I(1), A::I(DATA), A::I(len as u64)]);
        // xor edi, edi; mov eax, 60 (exit); syscall; ud2
        self.code.extend_from_slice(&[0x31, 0xff, 0xb8, 60, 0, 0, 0, 0x0f, 0x05, 0x0f, 0x0b]);
        let text = 176 + self.code.len();
        let data_off = text.next_multiple_of(0x1000);
        let mut b = alloc::vec![0u8; data_off + len];
        let mut put = |off: usize, v: u64, n: usize| b[off..off + n].copy_from_slice(&v.to_le_bytes()[..n]);
        put(0, 0x0101_0246_4c45_7f, 7); // \x7fELF, 64-bit, little-endian, version 1
        put(16, 2, 2); // ET_EXEC
        put(18, 0x3e, 2); // x86-64
        put(20, 1, 4);
        put(24, proc::BASE + 176, 8); // e_entry
        put(32, 64, 8); // e_phoff
        put(52, 64, 2);
        put(54, 56, 2);
        put(56, 2, 2); // e_phnum
        put(64, 1, 4); // PT_LOAD: the text
        put(68, 5, 4);
        put(80, proc::BASE, 8);
        put(88, proc::BASE, 8);
        put(96, text as u64, 8);
        put(104, text as u64, 8);
        put(112, 0x1000, 8);
        put(120, 1, 4); // PT_LOAD: the data
        put(124, 6, 4);
        put(128, data_off as u64, 8);
        put(136, DATA, 8);
        put(144, DATA, 8);
        put(152, len as u64, 8);
        put(160, len as u64, 8);
        put(168, 0x1000, 8);
        b[176..text].copy_from_slice(&self.code);
        b[data_off..].copy_from_slice(&self.data);
        (b, len)
    }
}

fn task_of(pid: Pid) -> Option<Tid> {
    let mut tid = None;
    proc::each(|p| {
        if p.pid == pid {
            tid = Some(p.task);
        }
    });
    tid
}

fn exit_of(pid: Pid) -> Option<i32> {
    let mut out = None;
    quiet(|| {
        proc::each(|p| {
            if p.pid == pid {
                out = p.exit;
            }
        })
    });
    out
}

fn yield_until(ms: u64, mut done: impl FnMut() -> bool) -> bool {
    let mut c = Clock::new();
    loop {
        if done() {
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
        Some(v) if v & 0x7f == 0 => format!("exit({})", (v >> 8) & 0xff),
        Some(v) => format!("killed by signal {} (status {v:#x})", v & 0x7f),
    }
}

/// What one program did: everything it wrote to fds 1 and 2, how it ended.
struct Ran {
    out: Vec<u8>,
    exit: Option<i32>,
    ms: u64,
}

fn res(out: &[u8], k: usize) -> i64 {
    le64(out, 8 * k) as i64
}


impl Probe {
    /// `n` bytes of the data segment, each `byte`.
    fn filled(&mut self, n: usize, byte: u8) -> (A, usize) {
        let at = self.data.len();
        self.data.resize(at + n.next_multiple_of(8), byte);
        (A::D(at), at)
    }
}

/// Start a program with `start` (interrupts off) and put `fds` in its table
/// before it runs. What the table hands back is dropped after its lock
/// (S4's rule).
fn start_with(what: &str, start: impl FnOnce() -> Result<Pid, Errno>, fds: &[(usize, &FileRef)]) -> Result<Pid, String> {
    let ends: Vec<(usize, Fd)> = fds.iter().map(|&(n, f)| (n, Fd { file: f.clone(), cloexec: false })).collect();
    let started = quiet(|| -> Result<(Pid, Vec<Fd>), (String, Vec<Fd>)> {
        let pid = start().map_err(|e| (format!("{what}: starting it returned {}", errno_text(e)), Vec::new()))?;
        let Some(tid) = task_of(pid) else {
            return Err((format!("{what}: pid {} has no task", pid.0), Vec::new()));
        };
        let back: Vec<Fd> = task::with_table(move |tt| {
            let mut back = Vec::new();
            match tt.get_mut(tid) {
                Some(t) => {
                    for (n, d) in ends {
                        if let Ok(Some(old)) = t.fds.install(n, d) {
                            back.push(old);
                        }
                    }
                }
                None => back.extend(ends.into_iter().map(|(_, d)| d)),
            }
            back
        });
        Ok((pid, back))
    });
    match started {
        Ok((pid, back)) => {
            drop(back);
            Ok(pid)
        }
        Err((why, back)) => {
            drop(back);
            Err(why)
        }
    }
}

/// A pipe whose read end does not block: (read end, write end).
fn out_pipe(what: &str) -> Result<(FileRef, FileRef), String> {
    let (r, w) = pipe::open(0).map_err(|e| format!("{what}: pipe::open(0) returned {}", errno_text(e)))?;
    r.flags.store(r.flags.load(Ordering::SeqCst) | O_NONBLOCK, Ordering::SeqCst);
    Ok((r, w))
}

/// Everything written into `r` until its last write end is gone or `RUN_MS`
/// passes, and how `pid` ended.
fn drain(what: &str, r: &FileRef, pid: Pid) -> Ran {
    let mut out = Vec::new();
    let mut buf = alloc::vec![0u8; 4096];
    let mut c = Clock::new();
    let mut ended = false;
    loop {
        match r.read(&mut buf) {
            Ok(0) => {
                ended = true;
                break;
            }
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(11) => {
                if c.ms() >= RUN_MS {
                    break;
                }
                sched::yield_now();
            }
            Err(e) => {
                note(format_args!("{what}: a read of its pipe returned {}", errno_text(e)));
                break;
            }
        }
    }
    let ms = c.ms();
    let _ = yield_until(500, || exit_of(pid).is_some());
    let exit = exit_of(pid);
    if !ended {
        note(format_args!("{what}: pid {} had not closed its output after {ms} ms ({})", pid.0, status_text(exit)));
    }
    Ran { out, exit, ms }
}

/// Run `path` with `argv` and a pipe as fds 1 and 2; what it wrote.
fn run(what: &str, path: &str, argv: &[&str]) -> Result<Ran, String> {
    let (r, w) = out_pipe(what)?;
    let pid = start_with(what, || exec::spawn(path, argv, &[]), &[(1, &w), (2, &w)]);
    drop(w);
    Ok(drain(what, &r, pid?))
}

/// What this file mounted, and the roots it made.
struct Fs {
    ext: InodeRef,
    arch: InodeRef,
    procfs: InodeRef,
    devfs: InodeRef,
    /// ext2's `/tmp`, where the programs go.
    tmp: InodeRef,
}

/// A program assembled here, written to ext2's `/tmp/k22-<what>` and run;
/// its data segment as it wrote it back.
fn run_probe(fs: &Fs, what: &str, p: Probe) -> Result<Vec<u8>, String> {
    let (elf, len) = p.elf();
    let name = format!("k22-{what}");
    let f = quiet(|| fs.tmp.create(&name, 0o755)).map_err(|e| format!("{what}: create(\"/tmp/{name}\") returned {}", errno_text(e)))?;
    match quiet(|| f.write_at(0, &elf)) {
        Ok(n) if n == elf.len() => {}
        other => return Err(format!("{what}: write_at of the program's {} bytes returned {other:?}", elf.len())),
    }
    let path = format!("/tmp/{name}");
    let ran = run(what, &path, &["probe"])?;
    if ran.exit != Some(0) || ran.out.len() != len {
        return Err(format!(
            "{what}: the program wrote {} of its {len} bytes and ended {} after {} ms",
            ran.out.len(),
            status_text(ran.exit),
            ran.ms
        ));
    }
    Ok(ran.out)
}

/// Read a whole file through its own `read_at`, 256 bytes at a time.
fn read_all(f: &InodeRef) -> Result<Vec<u8>, Errno> {
    let mut out = Vec::new();
    let mut buf = [0u8; 256];
    loop {
        let n = quiet(|| f.read_at(out.len() as u64, &mut buf))?;
        if n == 0 {
            return Ok(out);
        }
        out.extend_from_slice(&buf[..n]);
    }
}

fn show(st: &Stat) -> String {
    format!("st_dev {} st_ino {} st_mode {:#o}", st.dev, st.ino, st.mode)
}

/// `/proc/mounts`' escapes for a space, a tab, a newline and a backslash.
fn esc(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            ' ' => o.push_str("\\040"),
            '\t' => o.push_str("\\011"),
            '\n' => o.push_str("\\012"),
            '\\' => o.push_str("\\134"),
            c => o.push(c),
        }
    }
    o
}

// ── Mounting ─────────────────────────────────────────────────────────────

fn setup() -> Option<Fs> {
    let drive: Arc<dyn BlockDevice> = match quiet(Ata::probe) {
        Ok(a) => Arc::new(a),
        Err(e) => {
            note(format_args!("disk: Ata::probe() returned {}: no drive", errno_text(e)));
            return None;
        }
    };
    let ext2 = match quiet(|| ata::partition2(drive)).and_then(Ext2::mount) {
        Ok(fs) => Arc::new(fs),
        Err(e) => {
            note(format_args!("disk: partition2 then Ext2::mount returned {}", errno_text(e)));
            return None;
        }
    };
    let ext = match quiet(|| ext2.root()) {
        Ok(r) => r,
        Err(e) => {
            note(format_args!("disk: Ext2::root() returned {}", errno_text(e)));
            return None;
        }
    };
    let arch: InodeRef = match Archive::new(&ARCHIVE) {
        Ok(a) => Arc::new(a),
        Err(e) => {
            note(format_args!("vfs: Archive::new over the boot archive returned {}", errno_text(e)));
            return None;
        }
    };
    let procfs: InodeRef = Arc::new(ProcFs::new());
    let devfs: InodeRef = Arc::new(DevFs::new());
    let mut all = true;
    for ((source, target, fstype), root) in MOUNTS.iter().zip([&ext, &arch, &procfs, &devfs]) {
        if let Err(e) = mount::mount(source, target, fstype, root.clone()) {
            note(format_args!("vfs: mount::mount({source:?}, {target:?}, {fstype:?}, its root) returned {}", errno_text(e)));
            all = false;
        }
    }
    let tmp = match quiet(|| ext.lookup("tmp")) {
        Ok(t) => t,
        Err(e) => {
            note(format_args!("disk: ext2's root lookup(\"tmp\") returned {}", errno_text(e)));
            return None;
        }
    };
    note(format_args!(
        "vfs: / ext2 (st_dev {}), /boot the boot archive ({ARCHIVE_LEN} bytes, st_dev {}), /proc (st_dev {}), /dev (st_dev {})",
        ext.stat().dev,
        arch.stat().dev,
        procfs.stat().dev,
        devfs.stat().dev
    ));
    if !all {
        return None;
    }
    Some(Fs { ext, arch, procfs, devfs, tmp })
}

/// A row that cannot run: what was not mounted.
fn unmounted(mut r: Row, n: usize) {
    let id = r.id;
    r.fail(format_args!("{id}: not run: the four mounts did not all succeed"));
    r.finish(n);
}

// ── Row 1: mount.cross.walk ──────────────────────────────────────────────

/// The walks; the row is finished by `dents_check`, last in this file.
fn cross_row(fs: Option<&Fs>) -> Row {
    let mut r = Row::new(CROSS_ID);
    let Some(fs) = fs else {
        r.fail(format_args!("{CROSS_ID}: not run: the four mounts did not all succeed"));
        return r;
    };
    // Where each path must land, from the roots this file made.
    let init = quiet(|| fs.arch.lookup("init")).map(|i| i.stat());
    let null = quiet(|| fs.devfs.lookup("null")).map(|i| i.stat());
    let nonce = quiet(|| fs.procfs.lookup("sboot").and_then(|d| d.lookup("nonce"))).map(|i| i.stat());
    let root = Ok(fs.ext.stat());
    let tmp = Ok(fs.tmp.stat());
    let cases: [(&InodeRef, &str, Result<Stat, Errno>, &str); 13] = [
        (&fs.ext, "/boot/init", init, "the archive's init"),
        (&fs.ext, "boot/init", init, "the archive's init"),
        (&fs.ext, "/tmp/../boot/init", init, "the archive's init"),
        (&fs.ext, "/boot/..", root, "ext2's root"),
        (&fs.ext, "/boot/../tmp", tmp, "ext2's /tmp"),
        (&fs.ext, "/proc/../dev/null", null, "/dev's null"),
        (&fs.ext, "/proc/sboot/nonce", nonce, "/proc's sboot/nonce"),
        (&fs.ext, "/dev/./null", null, "/dev's null"),
        (&fs.ext, "/..", root, "ext2's root"),
        (&fs.devfs, "null", null, "/dev's null"),
        (&fs.devfs, "../boot/init", init, "the archive's init"),
        (&fs.ext, "/dev/null/", Err(ENOTDIR), "-ENOTDIR"),
        (&fs.ext, "/proc/nosuch", Err(ENOENT), "-ENOENT"),
    ];
    let mut wrong = 0;
    for (cwd, path, want, what) in cases.iter() {
        let from = if Arc::ptr_eq(cwd, &fs.ext) { "/" } else { "/dev's root" };
        let got = quiet(|| vfs::walk(cwd, path)).map(|i| i.stat());
        let ok = match (&got, want) {
            (Ok(g), Ok(w)) => (g.dev, g.ino, g.mode) == (w.dev, w.ino, w.mode),
            (Err(g), Err(w)) => g == w,
            _ => false,
        };
        if !ok {
            wrong += 1;
            let got = match got {
                Ok(s) => show(&s),
                Err(e) => errno_text(e),
            };
            let want = match want {
                Ok(s) => format!("{what}: {}", show(s)),
                Err(e) => errno_text(*e),
            };
            r.fail(format_args!("{CROSS_ID}: vfs::walk(cwd {from}, {path:?}) reached {got}, expected {want}"));
        }
    }
    if wrong == 0 {
        note(format_args!("{CROSS_ID}: 13 walks, each where the mounted roots say"));
    }
    r
}

// ── Row 2: proc.pid.cmdline ──────────────────────────────────────────────

fn cmdline_row(fs: Option<&Fs>) {
    let mut r = Row::new(CMDLINE_ID);
    let d = r.derived(CMDLINE_ID);
    let Some(_fs) = fs else { return unmounted(r, 2) };
    let words: Vec<String> = (0..1 + d % 3).map(|i| format!("w{:012x}", ktap::splitmix64(d ^ i) & 0xffff_ffff_ffff)).collect();
    let mut argv: Vec<&str> = alloc::vec!["cat", "-"];
    argv.extend(words.iter().map(|w| w.as_str()));
    let mut want = Vec::new();
    for a in &argv {
        want.extend_from_slice(a.as_bytes());
        want.push(0);
    }
    // The first cat: fd 0 a pipe this file holds the write end of (nothing
    // is ever written: cat sleeps in read), fds 1 and 2 another pipe.
    let (stdin_r, stdin_w) = match pipe::open(0) {
        Ok(p) => p,
        Err(e) => {
            r.fail(format_args!("{CMDLINE_ID}: pipe::open(0) returned {}", errno_text(e)));
            return r.finish(2);
        }
    };
    let (out_r, out_w) = match out_pipe("cat -") {
        Ok(p) => p,
        Err(why) => {
            r.fail(format_args!("{CMDLINE_ID}: {why}"));
            return r.finish(2);
        }
    };
    let started = start_with("cat -", || exec::spawn("/bin/cat", &argv, &[]), &[(0, &stdin_r), (1, &out_w), (2, &out_w)]);
    drop(stdin_r);
    drop(out_w);
    let pid = match started {
        Ok(p) => p,
        Err(why) => {
            r.fail(format_args!("{CMDLINE_ID}: {why}"));
            return r.finish(2);
        }
    };
    let _ = yield_until(50, || false);
    let alive_before = exit_of(pid).is_none();
    let path = format!("/proc/{}/cmdline", pid.0);
    let reader = run("cat cmdline", "/bin/cat", &["cat", &path]);
    let alive_after = exit_of(pid).is_none();
    r.expect(alive_before && alive_after, &format!("pid {} (cat -) alive while its cmdline was read", pid.0));
    match reader {
        Ok(ran) => {
            if ran.exit != Some(0) {
                r.fail(format_args!("{CMDLINE_ID}: cat {path} ended {}; it wrote {:?}", status_text(ran.exit), String::from_utf8_lossy(&ran.out)));
            }
            if ran.out != want {
                r.fail(format_args!(
                    "{CMDLINE_ID}: cat {path} printed {:?} ({} bytes), expected {:?} ({} bytes: each argv string, then a NUL)",
                    String::from_utf8_lossy(&ran.out),
                    ran.out.len(),
                    String::from_utf8_lossy(&want),
                    want.len()
                ));
            }
        }
        Err(why) => r.fail(format_args!("{CMDLINE_ID}: {why}")),
    }
    let status = quiet(|| vfs::walk(&vfs::root()?, &format!("/proc/{}/status", pid.0))).and_then(|f| read_all(&f));
    if let Ok(s) = status {
        note(format_args!("{CMDLINE_ID}: /proc/{}/status reads {:?}", pid.0, String::from_utf8_lossy(&s)));
    }
    note(format_args!("{CMDLINE_ID}: argv was {} strings, {} bytes with their NULs", argv.len(), want.len()));
    drop(stdin_w);
    let first = drain("cat -", &out_r, pid);
    note(format_args!("{CMDLINE_ID}: with its stdin closed, cat - (pid {}) ended {} after {} ms", pid.0, status_text(first.exit), first.ms));
    r.finish(2);
}

// ── Row 3: proc.meminfo.moves ────────────────────────────────────────────

/// (MemTotal, MemFree) in kB from `/proc/meminfo`'s text.
fn parse_meminfo(text: &[u8]) -> Option<(u64, u64)> {
    let text = core::str::from_utf8(text).ok()?;
    let field = |name: &str| {
        text.lines().find_map(|l| {
            let mut w = l.split_whitespace();
            (w.next() == Some(name)).then(|| w.next()?.parse::<u64>().ok().filter(|_| w.next() == Some("kB")))?
        })
    };
    Some((field("MemTotal:")?, field("MemFree:")?))
}

fn meminfo_row(fs: Option<&Fs>) {
    let mut r = Row::new(MEMINFO_ID);
    let d = r.derived(MEMINFO_ID);
    let Some(fs) = fs else { return unmounted(r, 3) };
    let n = 16 + (d % 241) as usize;
    let file = match quiet(|| fs.procfs.lookup("meminfo")) {
        Ok(f) => f,
        Err(e) => {
            r.fail(format_args!("{MEMINFO_ID}: /proc's lookup(\"meminfo\") returned {}", errno_text(e)));
            return r.finish(3);
        }
    };
    let mut held = Vec::with_capacity(n);
    let mut buf = alloc::vec![0u8; 1024];
    // Interrupts off from here to the end: nothing else takes a frame.
    let on = irq_off();
    // A reading the frame allocator agrees with: its stats the same before and after.
    let steady = |buf: &mut Vec<u8>| -> Result<(frame::Stats, (u64, u64)), String> {
        for _ in 0..5 {
            let s1 = frame::stats();
            let got = file.read_at(0, buf);
            let s2 = frame::stats();
            let k = got.map_err(|e| format!("read_at(0, 1024) returned {}", errno_text(e)))?;
            let m = parse_meminfo(&buf[..k]).ok_or_else(|| format!("no `MemTotal: <n> kB` and `MemFree: <n> kB` lines in {:?}", String::from_utf8_lossy(&buf[..k])))?;
            if s1 == s2 {
                return Ok((s1, m));
            }
        }
        Err(String::from("frame::stats() moved across each of five reads"))
    };
    let result = (|| -> Result<(), String> {
        let (s1, m1) = steady(&mut buf)?;
        let mut bad = Vec::new();
        if m1.0 != s1.total as u64 * 4 {
            bad.push(format!("MemTotal {} kB, frame::stats().total {} frames = {} kB", m1.0, s1.total, s1.total * 4));
        }
        if m1.1 != s1.free as u64 * 4 {
            bad.push(format!("MemFree {} kB, frame::stats().free {} frames = {} kB", m1.1, s1.free, s1.free * 4));
        }
        for _ in 0..n {
            match frame::alloc() {
                Some(f) => held.push(f),
                None => break,
            }
        }
        if held.len() != n {
            bad.push(format!("frame::alloc gave {} of {n} frames", held.len()));
        }
        let (s2, m2) = steady(&mut buf)?;
        if s1.free as i64 - s2.free as i64 != held.len() as i64 {
            bad.push(format!("frame::stats().free fell by {} after this file took {} frames", s1.free as i64 - s2.free as i64, held.len()));
        }
        if m1.1 as i64 - m2.1 as i64 != 4 * held.len() as i64 {
            bad.push(format!("MemFree went {} → {} kB ({:+} kB) after this file took {} frames; expected {} kB less", m1.1, m2.1, m2.1 as i64 - m1.1 as i64, held.len(), 4 * held.len()));
        }
        for f in held.drain(..) {
            frame::free(f);
        }
        let (_, m3) = steady(&mut buf)?;
        if m3.1 != m1.1 {
            bad.push(format!("MemFree {} kB after the {n} frames went back, {} kB before they were taken", m3.1, m1.1));
        }
        if bad.is_empty() {
            Ok(())
        } else {
            Err(bad.join("; "))
        }
    })();
    for f in held.drain(..) {
        frame::free(f);
    }
    irq_restore(on);
    match result {
        Ok(()) => note(format_args!("{MEMINFO_ID}: took {n} frames: MemFree fell by {} kB and came back", 4 * n)),
        Err(why) => r.fail(format_args!("{MEMINFO_ID}: {why}")),
    }
    r.finish(3);
}

// ── Row 4: proc.mounts.lists.all ─────────────────────────────────────────

fn mounts_row(fs: Option<&Fs>) {
    let r = Row::new(MOUNTS_ID);
    let Some(fs) = fs else { return unmounted(r, 4) };
    let mut r = r;
    let text = match quiet(|| fs.procfs.lookup("mounts")).and_then(|f| read_all(&f)) {
        Ok(t) => t,
        Err(e) => {
            r.fail(format_args!("{MOUNTS_ID}: /proc's mounts: lookup then read_at returned {}", errno_text(e)));
            return r.finish(4);
        }
    };
    let text = String::from_utf8_lossy(&text);
    let got: Vec<&str> = text.lines().collect();
    let want: Vec<String> = MOUNTS.iter().map(|(s, t, f)| format!("{} {} {} rw 0 0", esc(s), esc(t), esc(f))).collect();
    if got != want || !text.ends_with('\n') {
        r.fail(format_args!("{MOUNTS_ID}: /proc/mounts reads {text:?}; expected the lines {want:?}, in mount order, each ending in a newline"));
    }
    // The course's `mounts`: the table as a program parses it.
    match run("mounts", "/bin/mounts", &["mounts"]) {
        Ok(ran) => {
            let text = String::from_utf8_lossy(&ran.out);
            let got: Vec<(&str, &str)> = text
                .lines()
                .filter_map(|l| {
                    let (rest, _dev) = l.rsplit_once(" st_dev=")?;
                    rest.rsplit_once(' ')
                })
                .collect();
            let want: Vec<(&str, &str)> = MOUNTS.iter().map(|m| (m.1, m.2)).collect();
            if ran.exit != Some(0) || got != want {
                r.fail(format_args!("{MOUNTS_ID}: /bin/mounts printed {text:?} and ended {}; expected each of {want:?} with its st_dev, and exit(0)", status_text(ran.exit)));
            } else {
                note(format_args!("{MOUNTS_ID}: /bin/mounts: {}", text.trim_end().replace('\n', " | ")));
            }
        }
        Err(why) => r.fail(format_args!("{MOUNTS_ID}: {why}")),
    }
    r.finish(4);
}

// ── Row 5: stat.st_dev.differs ───────────────────────────────────────────

fn stdev_row(fs: Option<&Fs>) {
    let r = Row::new(STDEV_ID);
    let Some(fs) = fs else { return unmounted(r, 5) };
    let mut r = r;
    // The four roots, then a path inside each and the root it is in.
    let roots: [(&str, u64); 4] = [("/", fs.ext.stat().dev), ("/boot", fs.arch.stat().dev), ("/proc", fs.procfs.stat().dev), ("/dev", fs.devfs.stat().dev)];
    let inside: [(&str, usize); 5] = [("/tmp", 0), ("/boot/init", 1), ("/proc/meminfo", 2), ("/proc/sboot", 2), ("/dev/null", 3)];
    let paths: Vec<&str> = roots.iter().map(|x| x.0).chain(inside.iter().map(|x| x.0)).collect();
    let mut p = Probe::new();
    let mut at = Vec::new();
    for path in &paths {
        let s = p.str(path);
        let (b, off) = p.buf(144);
        p.call(SYS_STAT, &[s, b]);
        at.push(off);
    }
    match run_probe(fs, "stat", p) {
        Ok(out) => {
            let mut dev = [0u64; 9];
            for (k, path) in paths.iter().enumerate() {
                r.expect_res(&format!("stat({path:?})"), res(&out, k), 0);
                dev[k] = le64(&out, at[k]);
            }
            for i in 0..4 {
                for j in i + 1..4 {
                    if dev[i] == dev[j] {
                        r.fail(format_args!("{STDEV_ID}: stat({:?}) and stat({:?}) both give st_dev {}: two mounts, one device number", paths[i], paths[j], dev[i]));
                    }
                }
                if dev[i] != roots[i].1 {
                    r.fail(format_args!("{STDEV_ID}: stat({:?}) gives st_dev {}, its root's stat().dev is {}", paths[i], dev[i], roots[i].1));
                }
            }
            for (k, (path, g)) in inside.iter().enumerate() {
                if dev[4 + k] != dev[*g] {
                    r.fail(format_args!("{STDEV_ID}: stat({path:?}) gives st_dev {}, the mount it is in ({}) {}", dev[4 + k], roots[*g].0, dev[*g]));
                }
            }
            note(format_args!("{STDEV_ID}: st_dev of / /boot /proc /dev: {:?}", &dev[..4]));
        }
        Err(why) => r.fail(format_args!("{STDEV_ID}: {why}")),
    }
    r.finish(5);
}

// ── Row 6: dev.null.zero ─────────────────────────────────────────────────

fn dev_row(fs: Option<&Fs>) {
    let r = Row::new(DEV_ID);
    let Some(fs) = fs else { return unmounted(r, 6) };
    let mut r = r;
    let mut p = Probe::new();
    let (null, zero, tty) = (p.str("/dev/null"), p.str("/dev/zero"), p.str("/dev/tty"));
    let (b, b_at) = p.filled(4096, 0xa5);
    let (w, _) = p.filled(128, 0x5a);
    let (st1, st1_at) = p.buf(144);
    let (st2, st2_at) = p.buf(144);
    let (ws, ws_at) = p.buf(8);
    let k_null = p.call(SYS_OPEN, &[null, A::I(O_RDWR), A::I(0)]);
    let k_nread = p.call(SYS_READ, &[A::R(k_null), b, A::I(64)]);
    let k_nwrite = p.call(SYS_WRITE, &[A::R(k_null), w, A::I(100)]);
    let k_zero = p.call(SYS_OPEN, &[zero, A::I(O_RDONLY), A::I(0)]);
    let k_zread = p.call(SYS_READ, &[A::R(k_zero), b, A::I(4096)]);
    let k_zw = p.call(SYS_OPEN, &[zero, A::I(O_WRONLY), A::I(0)]);
    let k_zwrite = p.call(SYS_WRITE, &[A::R(k_zw), w, A::I(50)]);
    let k_st1 = p.call(SYS_STAT, &[null, st1]);
    let k_st2 = p.call(SYS_STAT, &[zero, st2]);
    let k_trunc = p.call(SYS_OPEN, &[null, A::I(O_WRONLY | O_CREAT | O_TRUNC), A::I(0o644)]);
    let k_twrite = p.call(SYS_WRITE, &[A::R(k_trunc), w, A::I(7)]);
    let k_tty = p.call(SYS_OPEN, &[tty, A::I(O_WRONLY), A::I(0)]);
    let k_win = p.call(SYS_IOCTL, &[A::R(k_tty), A::I(TIOCGWINSZ), ws]);
    match run_probe(fs, "dev", p) {
        Ok(out) => {
            let fd_ok = |v: i64| (3..32).contains(&v);
            r.expect(fd_ok(res(&out, k_null)), &format!("open(\"/dev/null\", O_RDWR) returned {}, expected an fd", answer(res(&out, k_null))));
            r.expect_res("read(/dev/null, 64)", res(&out, k_nread), 0);
            r.expect_res("write(/dev/null, 100)", res(&out, k_nwrite), 100);
            r.expect(fd_ok(res(&out, k_zero)), &format!("open(\"/dev/zero\", O_RDONLY) returned {}, expected an fd", answer(res(&out, k_zero))));
            r.expect_res("read(/dev/zero, 4096) into 4096 bytes of 0xa5", res(&out, k_zread), 4096);
            if let Some(i) = out[b_at..b_at + 4096].iter().position(|&c| c != 0) {
                r.fail(format_args!("{DEV_ID}: after read(/dev/zero, 4096) byte {i} of the buffer is {:#04x}, expected 0", out[b_at + i]));
            }
            r.expect(fd_ok(res(&out, k_zw)), &format!("open(\"/dev/zero\", O_WRONLY) returned {}, expected an fd", answer(res(&out, k_zw))));
            r.expect_res("write(/dev/zero, 50)", res(&out, k_zwrite), 50);
            for (k, at, name, rdev) in [(k_st1, st1_at, "/dev/null", 1u64 << 8 | 3), (k_st2, st2_at, "/dev/zero", 1 << 8 | 5)] {
                r.expect_res(&format!("stat(\"{name}\")"), res(&out, k), 0);
                r.expect_eq(&format!("stat(\"{name}\").st_mode"), le64(&out, at + 24) & 0xffff_ffff, S_IFCHR | 0o666);
                r.expect_eq(&format!("stat(\"{name}\").st_rdev"), le64(&out, at + 40), rdev);
            }
            r.expect(fd_ok(res(&out, k_trunc)), &format!("open(\"/dev/null\", O_WRONLY|O_CREAT|O_TRUNC) returned {}, expected an fd", answer(res(&out, k_trunc))));
            r.expect_res("write of 7 bytes through it", res(&out, k_twrite), 7);
            note(format_args!(
                "{DEV_ID}: not graded: open(\"/dev/tty\", O_WRONLY) {}, ioctl(TIOCGWINSZ) {} ({} rows, {} columns)",
                answer(res(&out, k_tty)),
                answer(res(&out, k_win)),
                le64(&out, ws_at) & 0xffff,
                (le64(&out, ws_at) >> 16) & 0xffff
            ));
        }
        Err(why) => r.fail(format_args!("{DEV_ID}: {why}")),
    }
    r.finish(6);
}

// ── Row 7: proc.sboot.nonce ──────────────────────────────────────────────

fn nonce_row(fs: Option<&Fs>) {
    let r = Row::new(NONCE_ID);
    let Some(_fs) = fs else { return unmounted(r, 7) };
    let mut r = r;
    let Some(hex) = ktap::nonce() else {
        r.fail(format_args!("{NONCE_ID}: this run has no nonce to compare /proc/sboot/nonce with"));
        return r.finish(7);
    };
    let mut want = Vec::from(&hex[..]);
    want.push(b'\n');
    match run("cat nonce", "/bin/cat", &["cat", "/proc/sboot/nonce"]) {
        Ok(ran) => {
            r.expect_eq("cat /proc/sboot/nonce's exit status", ran.exit.map_or(u64::MAX, |s| s as u64), 0);
            if ran.out != want {
                r.fail(format_args!(
                    "{NONCE_ID}: cat /proc/sboot/nonce printed {:?}, expected the nonce {:?} and a newline",
                    String::from_utf8_lossy(&ran.out),
                    String::from_utf8_lossy(&hex)
                ));
            }
        }
        Err(why) => r.fail(format_args!("{NONCE_ID}: {why}")),
    }
    r.finish(7);
}

// ── Row 8: mount.syscall ─────────────────────────────────────────────────

fn syscall_row(fs: Option<&Fs>) {
    let r = Row::new(SYSCALL_ID);
    let Some(fs) = fs else { return unmounted(r, 8) };
    let mut r = r;
    if let Err(e) = quiet(|| fs.tmp.mkdir(PROC_AT_NAME, 0o755)) {
        r.fail(format_args!("{SYSCALL_ID}: ext2's /tmp mkdir({PROC_AT_NAME:?}) returned {}", errno_text(e)));
        return r.finish(8);
    }
    let mut p = Probe::new();
    let (proc_, x, tmp, ext2, nofs, missing, init, rel) =
        (p.str("proc"), p.str("x"), p.str("/tmp"), p.str("ext2"), p.str("nofs"), p.str("/tmp/k22-none"), p.str("/boot/init"), p.str("tmp/k22 rel"));
    let at = p.str(PROC_AT);
    let meminfo = p.str(&format!("{PROC_AT}/meminfo"));
    let (st, st_at) = p.buf(144);
    let (b, b_at) = p.buf(256);
    let z = A::I(0);
    let k_mount = p.call(SYS_MOUNT, &[proc_, at, proc_, z, z]);
    let k_open = p.call(SYS_OPEN, &[meminfo, A::I(O_RDONLY), z]);
    let k_read = p.call(SYS_READ, &[A::R(k_open), b, A::I(256)]);
    let k_stat = p.call(SYS_STAT, &[at, st]);
    let refusals: [(String, [A; 3], i64); 6] = [
        (format!("mount(\"proc\", {PROC_AT:?}, \"proc\", 0, 0) a second time"), [proc_, at, proc_], -EBUSY),
        (String::from("mount(\"x\", \"/tmp\", \"ext2\", 0, 0)"), [x, tmp, ext2], -ENODEV),
        (String::from("mount(\"x\", \"/tmp\", \"nofs\", 0, 0)"), [x, tmp, nofs], -ENODEV),
        (String::from("mount(\"proc\", \"tmp/k22 rel\", \"proc\", 0, 0) (a relative target)"), [proc_, rel, proc_], -EINVAL),
        (String::from("mount(\"proc\", \"/tmp/k22-none\", \"proc\", 0, 0) (no such directory)"), [proc_, missing, proc_], -ENOENT),
        (String::from("mount(\"proc\", \"/boot/init\", \"proc\", 0, 0) (a file)"), [proc_, init, proc_], -ENOTDIR),
    ];
    let ks: Vec<usize> = refusals.iter().map(|(_, a, _)| p.call(SYS_MOUNT, &[a[0], a[1], a[2], z, z])).collect();
    match run_probe(fs, "mount", p) {
        Ok(out) => {
            r.expect_res(&format!("mount(\"proc\", {PROC_AT:?}, \"proc\", 0, 0)"), res(&out, k_mount), 0);
            let fd = res(&out, k_open);
            if !(3..32).contains(&fd) {
                r.fail(format_args!("{SYSCALL_ID}: open(\"{PROC_AT}/meminfo\", O_RDONLY) after that mount returned {}, expected an fd", answer(fd)));
            } else {
                let n = res(&out, k_read);
                let text = if (1..=256).contains(&n) { &out[b_at..b_at + n as usize] } else { &[][..] };
                if parse_meminfo(text).is_none() {
                    r.fail(format_args!(
                        "{SYSCALL_ID}: read of \"{PROC_AT}/meminfo\" returned {} and {:?}; expected /proc's meminfo through the new mount",
                        answer(n),
                        String::from_utf8_lossy(text)
                    ));
                }
            }
            r.expect_res(&format!("stat({PROC_AT:?})"), res(&out, k_stat), 0);
            let dev = le64(&out, st_at);
            if dev == fs.ext.stat().dev || dev == fs.procfs.stat().dev {
                r.fail(format_args!("{SYSCALL_ID}: stat({PROC_AT:?}) gives st_dev {dev}: ext2's or /proc's, not a new /proc's"));
            }
            for ((what, _, want), k) in refusals.iter().zip(&ks) {
                r.expect_res(what, res(&out, *k), *want);
            }
        }
        Err(why) => r.fail(format_args!("{SYSCALL_ID}: {why}")),
    }
    r.finish(8);
}

// ── Row 1's last part: getdents64 on /proc and /dev (last in this file) ──

fn dents_check(fs: Option<&Fs>, mut r: Row) {
    if fs.is_some() {
        let (lr, lw) = match out_pipe("ls /proc") {
            Ok(p) => p,
            Err(why) => {
                r.fail(format_args!("{CROSS_ID}: {why}"));
                return r.finish(1);
            }
        };
        let started = start_with("ls /proc", || exec::spawn("/bin/ls", &["ls", "/proc"], &[]), &[(1, &lw), (2, &lw)]);
        drop(lw);
        match started {
            Ok(pid) => {
                let ran = drain("ls /proc", &lr, pid);
                let mut pids = Vec::new();
                quiet(|| proc::each(|p| pids.push(format!("{}", p.pid.0))));
                let text = String::from_utf8_lossy(&ran.out);
                let names: Vec<&str> = text.lines().collect();
                let words: Vec<&str> = names.iter().copied().filter(|n| !n.bytes().all(|c| c.is_ascii_digit())).collect();
                let stray: Vec<&str> = names.iter().copied().filter(|n| n.bytes().all(|c| c.is_ascii_digit()) && !pids.iter().any(|p| p == n)).collect();
                let own = format!("{}", pid.0);
                if ran.exit != Some(0) || words != ["meminfo", "mounts", "sboot", "uptime"] || !names.contains(&own.as_str()) || !stray.is_empty() {
                    r.fail(format_args!(
                        "{CROSS_ID}: ls /proc printed {text:?} and ended {}; expected meminfo, mounts, sboot, uptime and one directory per pid the kernel has (ls's own, {own}, among them) — getdents64 on /proc lists procfs's, not ext2's empty /proc",
                        status_text(ran.exit)
                    ));
                } else {
                    note(format_args!("{CROSS_ID}: ls /proc: procfs's four names and {} pid directories", names.len() - 4));
                }
            }
            Err(why) => r.fail(format_args!("{CROSS_ID}: {why}")),
        }
        match run("ls /dev", "/bin/ls", &["ls", "/dev"]) {
            Ok(ran) => {
                let want = "console\nnull\ntty\nzero\n";
                if ran.exit != Some(0) || ran.out != want.as_bytes() {
                    r.fail(format_args!(
                        "{CROSS_ID}: ls /dev printed {:?} and ended {}; expected {want:?} (getdents64 on /dev lists devfs's four) and exit(0)",
                        String::from_utf8_lossy(&ran.out),
                        status_text(ran.exit)
                    ));
                } else {
                    note(format_args!("{CROSS_ID}: ls /dev: devfs's four devices"));
                }
            }
            Err(why) => r.fail(format_args!("{CROSS_ID}: {why}")),
        }
    }
    r.finish(1);
}

// ── The machine ──────────────────────────────────────────────────────────

/// CR0.EM = 0, CR0.TS = 0, CR0.MP = 1; CR4.OSFXSR = 1, CR4.OSXMMEXCPT = 1;
/// `fninit`; MXCSR = 0x1F80 — lab 15's prelude, unchanged.
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
    frame::init(bi.memmap(), bi.kernel_range());
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
    let mut c = Clock::new();
    let fs = setup();
    let walks = cross_row(fs.as_ref());
    cmdline_row(fs.as_ref());
    meminfo_row(fs.as_ref());
    mounts_row(fs.as_ref());
    stdev_row(fs.as_ref());
    dev_row(fs.as_ref());
    nonce_row(fs.as_ref());
    syscall_row(fs.as_ref());
    dents_check(fs.as_ref(), walks);
    note(format_args!("the rows took {} ms", c.ms()));
    sched::yield_now();
    quiet(ktap::end);
    halt()
}
