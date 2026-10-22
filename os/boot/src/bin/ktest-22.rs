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
//!     file: `/bin/ls /proc` prints `meminfo mounts sboot uptime` (and
//!     `self`, if the kernel lists it, as Linux does) and one name per pid:
//!     `ls`'s own, every pid `proc::each` has both before `ls` starts and
//!     after it ends, and no number that is neither; and `/bin/ls /dev`
//!     exactly `console null tty zero` — `getdents64` on the mounted roots,
//!     not on ext2's empty directories under them.
//!   2 `proc.pid.cmdline` (U, derived) — `/bin/cat` started with argv
//!     `cat - <1–3 nonce-derived words> t<12 hex digits>` and a pipe as its
//!     fd 0 whose write end this file holds and writes nothing to, so `cat`
//!     sleeps in `read` and stays alive; its pid is what `exec::spawn`
//!     returned. The last word comes from the time-stamp counter and the PM
//!     timer at that moment, so no kernel can derive it; it travels the
//!     same way as the others (exec's argv) and is printed only in a
//!     failure line after the row has read it back. An older `cat -` with
//!     argv `cat - t<12 hex digits>` is started first and lives through the
//!     reads. A second `/bin/cat /proc/<pid>/cmdline` must print the argv,
//!     each string followed by one NUL, and exit 0 — read while the first is
//!     alive (checked before and after); then the same for the older one, so
//!     a cmdline taken from any one live process fails (slice 5b fix round
//!     2). `/proc/<pid>/status` is read as `key: value` lines in any
//!     order with any whitespace; only `Pid` (the pid) and `PPid` (its ppid
//!     in `proc::each`) are graded — `Name` and `State` are not. Then both
//!     write ends are dropped and both `cat`s end. Then `/proc/self`: a
//!     program with argv `probe t<12 hex digits>` forks; fd 0 is a pipe this
//!     file holds the write end of, fd 31 another whose read end it holds.
//!     Each process reads `/proc/self/status` and `/proc/self/cmdline`, then
//!     asks `getpid` and `getppid`, writes a byte to fd 31 and `wait4(-1)`s
//!     (the child's answers `-ECHILD`); before its reads the child waits on
//!     fd 0. The parent reads first; on its byte this file reads the
//!     child's `/proc/<pid>/status` (Pid the child, PPid the parent — a
//!     reader that is not the target); then a second program forks and
//!     waits, and `/bin/cat /proc/<pid>/status` (a process reading another's)
//!     must give the same Pid and PPid (round 3); then this file writes the
//!     child's byte. Each
//!     `/proc/self/status`'s `Pid` and `PPid` must be that process's own
//!     `getpid` and `getppid`; the parent's cmdline its own argv. A `self`
//!     that is the newest pid fails in the parent, one pinned to any one pid
//!     or to the last caller of `getpid` fails in the child, a `PPid` that
//!     is the reader's parent fails from outside, and a status that reports
//!     the reader's own numbers, or a PPid kept only for the newest fork,
//!     fails in cat's read (slice 5b fix rounds 1–3). Last, with the older
//!     `cat` (pid 1) still alive: a program forks child A and waits; `cat`
//!     reads A's status (PPid the program); the program forks child B, so
//!     a pid (that `cat`'s) falls between the forks and B's pid less one is
//!     not its parent, and `cat` reads B's status (PPid the program); B's
//!     own `getppid`, asked while the program waits, must be the program
//!     (round 6); the program ends without waiting; other programs fork
//!     12–20 times (drawn), then one forks and stays alive with its child;
//!     `cat` reads A's and B's status again, and each `PPid` must now be
//!     lab 14's heir (pid 1 while it runs), as must A's and B's `getppid`.
//!     A PPid worked out from the pid, from a list of the last eight forks
//!     or fewer, or from the newest live process that forked fails (rounds
//!     4–6); one from a list of 32 forks, or a table indexed by pid % 64,
//!     passes (no pid read here reaches 64; for the catalogue, G987). A PPid kept
//!     beside the record, or from an earlier read, and handed to pid 1 when
//!     its parent ends behaves correctly, since orphans go only to pid 1 and
//!     pids are never reused: that design is right, and passes. Then (round
//!     7) a program G forks M and waits; M forks O and ends while G runs,
//!     and `cat` reads O's status: PPid must be the heir, not G. (O's
//!     `getppid` is asked only after the pipes close, when G may have ended
//!     too, so only the status read is a check of a live grandparent.)
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
//!     `/dev/null` the `st_dev` of the mount each is in. Then `rename("/tmp/k22
//!     mv a", "/tmp/k22 mv b")` (a file this file made) is 0 and `stat` of
//!     the new name gives ext2's `st_dev`: lab 20's ext2 decides a directory
//!     is its own by comparing stats, so an `st_dev` only some of ext2's
//!     stats carry makes every rename `-EXDEV` (slice 5b fix round 1).
//!   6 `dev.null.zero` (plain) — a program: `/dev/null` reads 0 bytes and
//!     takes a 100-byte write; `/dev/zero` fills a 4096-byte buffer this
//!     file filled with 0xa5 with zeros and takes a write; `stat` gives
//!     `S_IFCHR | 0666` and `st_rdev` 1:3 and 1:5; `open("/dev/null",
//!     O_WRONLY | O_CREAT | O_TRUNC)` (a shell's `>`) succeeds. The program's
//!     fd 0 is the console object itself (`FileRef::open(FileObj::CONSOLE,
//!     O_RDWR | O_NONBLOCK)`, made here): two 256-byte reads drain its queue,
//!     and its `ioctl(TIOCGWINSZ)` is the expected window size (lab 09's
//!     `console.winsz` pins it). `/dev/tty` and `/dev/console`, each opened
//!     `O_RDWR | O_NONBLOCK`, are the console object: a one-byte `read` is
//!     `-EAGAIN` and `ioctl(TIOCGWINSZ)` is 0 with fd 0's 8 bytes, no
//!     literal size (slice 5b fix round 1). Then this file opens each
//!     through devfs's `lookup` and `FileRef::open_inode(.., O_WRONLY)`: the
//!     open file's object must be `FileObj::CONSOLE`, and a `# t<8 hex>`
//!     line written to it must come back over COM1's loopback (rounds 1b, 2).
//!   7 `proc.sboot.nonce` (plain) — `/bin/cat /proc/sboot/nonce` prints the
//!     run's nonce as this file reads it from `ktap::nonce()`, then a
//!     newline.
//!   8 `mount.syscall` (U, plain) — a program: `mount("proc", "/tmp/k22
//!     proc", "proc", 0, 0)` is 0, `/tmp/k22 proc/meminfo` then opens and
//!     reads as `/proc/meminfo` does, and `stat` of the mount point gives a
//!     device unlike every mount's already in the table (the boot's four,
//!     round 4b); `mount("devfs",
//!     "/tmp/k22\td\\ev", "devfs", 0, 0)` (a tab and a backslash in the
//!     name) is 0, `stat` of `<it>/null` gives `st_rdev` 1:3 and `stat` of the
//!     mount point a device unlike every earlier mount's (rounds 2, 4b); two more `devfs` mounts at `/tmp/k22-<8 hex>-a` and `-b`
//!     (drawn for the run), one before the refusals and one after, are 0
//!     and each mount point's `stat` a device unlike every one before it:
//!     the boot's four mounts' and each earlier new mount's
//!     (rounds 3, 4, 4b; with the boot's four that fills lab 13's eight-mount
//!     table, so a device counter that repeats only after four `devfs`
//!     mounts passes); at the program's end each new mount point is
//!     stat'ed again, and a node in it (`meminfo`, `null`), and all three
//!     must give the same device (round 5), as must `fstat` of the new
//!     /proc's meminfo opened (round 6; `null` dropped in round 7, since a
//!     device may open as an object of its own, as the console does); `..`
//!     out of /proc and /dev must be `/`'s file and `..` out of each new
//!     mount `/tmp`'s, `st_dev` and `st_ino` both, in a drawn order (round
//!     7; a device looked up by mount-point name for `stat` and `fstat`
//!     both, with `..` resolved in the name, passes: for the catalogue,
//!     G987); the same proc mount again is `-EBUSY`, type `"ext2"`
//!     and `"nofs"` `-ENODEV`, a relative target `-EINVAL`, a missing one
//!     `-ENOENT`, a file `-ENOTDIR`. Each failure line names the
//!     `mount(...)` call and what it returned. `/proc/mounts` (read through
//!     `/proc`'s root) after the program is exactly its lines before it, then
//!     `proc /tmp/k22\040proc proc rw 0 0`, `devfs /tmp/k22\011d\134ev
//!     devfs rw 0 0` and the two drawn `devfs` lines: what was mounted is
//!     recorded, in order, escaped, and a
//!     refused mount records nothing; the lines before are row 4's business,
//!     so a table that drops a boot-time mount fails row 4 alone (slice 5b
//!     fix round 1).

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
use kernel::console::{Fd, FileObj, FileRef};
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
/// Row 8's second mount point, for a `devfs`.
const DEV_AT_NAME: &str = "k22\td\\ev";
const DEV_AT: &str = "/tmp/k22\td\\ev";
/// Two more new `/dev`s go at names drawn for the run (`more_devs`).
/// Row 5's file, renamed inside ext2's `/tmp`.
const MV_FROM: &str = "k22 mv a";
const MV_TO: &str = "k22 mv b";

/// A probe's data segment, and how much of it a probe may use.
const DATA: u64 = 0x0110_0000;
const MAX_STEPS: usize = 64;
const RESULTS: usize = 8 * MAX_STEPS;
/// `FileObj::as_file` (lab 17 on) and `FileObj::as_pipe` (lab 15 on) where
/// the cut has them: method lookup takes an inherent method of `FileObj`
/// before this trait's, which is on `&FileObj`, so on an earlier cut every
/// object answers `None` and this file still builds (as ktest-17's).
trait SlotOf {
    #[allow(dead_code)]
    fn as_file(self) -> Option<u32>;
    #[allow(dead_code)]
    fn as_pipe(self) -> Option<(u32, bool)>;
}

impl SlotOf for &FileObj {
    fn as_file(self) -> Option<u32> {
        None
    }
    fn as_pipe(self) -> Option<(u32, bool)> {
        None
    }
}

/// `FileRef::open_inode` (lab 17 on) where the cut has it: a path to an
/// associated function takes the inherent one first, so on a cut before lab
/// 17 this answers `-ENOSYS` and this file still builds (as ktest-17's).
trait OpenInodeOf {
    #[allow(dead_code)]
    fn open_inode(ino: InodeRef, flags: u32) -> Result<FileRef, Errno>;
}

impl OpenInodeOf for FileRef {
    fn open_inode(_: InodeRef, _: u32) -> Result<FileRef, Errno> {
        Err(38)
    }
}

/// How long a program may run before its output is taken as it stands.
const RUN_MS: u64 = 10_000;

const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_STAT: u64 = 4;
const SYS_FSTAT: u64 = 5;
const SYS_IOCTL: u64 = 16;
const SYS_DUP2: u64 = 33;
const SYS_GETPID: u64 = 39;
const SYS_FORK: u64 = 57;
const SYS_WAIT4: u64 = 61;
const SYS_RENAME: u64 = 82;
const SYS_GETPPID: u64 = 110;
const SYS_MOUNT: u64 = 165;
/// Row 2's turn pipe: its write end in the forking program's table, at
/// the top (`NFD` = 32); its read end is fd 0.
const TURN_FD: u64 = 31;
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

/// A number no one knows before this moment: the time-stamp counter and
/// the PM timer, mixed. Row 2 puts it in an argv; it is printed only after
/// the row has read it back.
fn unpublished() -> u64 {
    // SAFETY: rdtsc reads a counter; no memory is touched.
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
    ktap::splitmix64(tsc ^ (pm() as u64) << 40) & 0xffff_ffff_ffff
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
        10 => "ECHILD",
        11 => "EAGAIN",
        12 => "ENOMEM",
        13 => "EACCES",
        14 => "EFAULT",
        16 => "EBUSY",
        18 => "EXDEV",
        19 => "ENODEV",
        20 => "ENOTDIR",
        21 => "EISDIR",
        22 => "EINVAL",
        23 => "ENFILE",
        24 => "EMFILE",
        25 => "ENOTTY",
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
        note(format_args!("{what}: pid {} had not closed its output after {ms} ms; the limit is {RUN_MS} ms ({})", pid.0, status_text(exit)));
    }
    Ran { out, exit }
}

/// Run `path` with `argv` and a pipe as fds 1 and 2; what it wrote.
fn run(what: &str, path: &str, argv: &[&str]) -> Result<Ran, String> {
    run_with(what, path, argv, &[])
}

/// `run`, with `fds` in its table as well.
fn run_with(what: &str, path: &str, argv: &[&str], fds: &[(usize, &FileRef)]) -> Result<Ran, String> {
    let (r, w) = out_pipe(what)?;
    let mut all: Vec<(usize, &FileRef)> = alloc::vec![(1, &w), (2, &w)];
    all.extend_from_slice(fds);
    let pid = start_with(what, || exec::spawn(path, argv, &[]), &all);
    drop(all);
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
    run_probe_as(fs, what, p, &["probe"], &[], 1)
}

/// `run_probe` with `argv`, `fds` in its table besides 1 and 2, and
/// `copies` data segments expected back (a probe that forks writes two).
fn run_probe_as(fs: &Fs, what: &str, p: Probe, argv: &[&str], fds: &[(usize, &FileRef)], copies: usize) -> Result<Vec<u8>, String> {
    let (path, len) = install(fs, what, p)?;
    let ran = run_with(what, &path, argv, fds)?;
    ran_whole(what, ran, len * copies)
}

/// A program assembled here, written to ext2's `/tmp/k22-<what>`: its path
/// and the length of one copy of its data segment.
fn install(fs: &Fs, what: &str, p: Probe) -> Result<(String, usize), String> {
    let (elf, len) = p.elf();
    let name = format!("k22-{what}");
    let f = quiet(|| fs.tmp.create(&name, 0o755)).map_err(|e| format!("{what}: create(\"/tmp/{name}\") returned {}", errno_text(e)))?;
    match quiet(|| f.write_at(0, &elf)) {
        Ok(n) if n == elf.len() => Ok((format!("/tmp/{name}"), len)),
        other => Err(format!("{what}: write_at of the program's {} bytes returned {other:?}", elf.len())),
    }
}

/// What a program wrote, if it exited 0 having written `len` bytes.
fn ran_whole(what: &str, ran: Ran, len: usize) -> Result<Vec<u8>, String> {
    if ran.exit != Some(0) || ran.out.len() != len {
        return Err(format!(
            "{what}: the program wrote {} of its {len} bytes and ended {}",
            ran.out.len(),
            status_text(ran.exit)
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

/// `key: value` lines, any order, any whitespace around the value: the
/// value of `key`, if one line has it.
fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k.trim() == key).then(|| v.trim())
    })
}

/// A status file's `Pid` and `PPid` against the two numbers expected.
fn status_check(r: &mut Row, whose: &str, text: &[u8], pid: u64, ppid: u64) {
    let text = String::from_utf8_lossy(text);
    for (key, want) in [("Pid", pid), ("PPid", ppid)] {
        let got = field(&text, key);
        if got.and_then(|v| v.parse::<u64>().ok()) != Some(want) {
            r.fail(format_args!("{CMDLINE_ID}: {whose} reads {text:?}: its {key} line is {got:?}, expected {key}: {want}"));
        }
    }
}

/// `/bin/cat` with `argv`: fd 0 a pipe whose write end comes back (nothing
/// is ever written to it, so cat sleeps in `read`), fds 1 and 2 another
/// pipe. (pid, that write end, the output's read end.)
fn sleeper(argv: &[&str]) -> Result<(Pid, FileRef, FileRef), String> {
    let (stdin_r, stdin_w) = pipe::open(0).map_err(|e| format!("pipe::open(0) returned {}", errno_text(e)))?;
    let (out_r, out_w) = out_pipe("cat -")?;
    let started = start_with("cat -", || exec::spawn("/bin/cat", argv, &[]), &[(0, &stdin_r), (1, &out_w), (2, &out_w)]);
    drop(stdin_r);
    drop(out_w);
    Ok((started?, stdin_w, out_r))
}

/// Each string of `argv`, then a NUL.
fn nul_joined(argv: &[&str]) -> Vec<u8> {
    let mut want = Vec::new();
    for a in argv {
        want.extend_from_slice(a.as_bytes());
        want.push(0);
    }
    want
}

fn cmdline_row(fs: Option<&Fs>) {
    let mut r = Row::new(CMDLINE_ID);
    let d = r.derived(CMDLINE_ID);
    let Some(fs) = fs else { return unmounted(r, 2) };
    let mut words: Vec<String> = (0..1 + d % 3).map(|i| format!("w{:012x}", ktap::splitmix64(d ^ i) & 0xffff_ffff_ffff)).collect();
    // One word more that no one can know before this moment.
    words.push(format!("t{:012x}", unpublished()));
    let mut argv: Vec<&str> = alloc::vec!["cat", "-"];
    argv.extend(words.iter().map(|w| w.as_str()));
    let want = nul_joined(&argv);
    // An older cat first, with a word of its own, alive while the newer
    // one's files are read: a cmdline taken from some other live process
    // than the one named fails on one of the two.
    let other = format!("t{:012x}", unpublished());
    let older_argv = ["cat", "-", other.as_str()];
    let older = match sleeper(&older_argv) {
        Ok(s) => s,
        Err(why) => {
            r.fail(format_args!("{CMDLINE_ID}: an older cat -: {why}"));
            return r.finish(2);
        }
    };
    let (pid, stdin_w, out_r) = match sleeper(&argv) {
        Ok(s) => s,
        Err(why) => {
            r.fail(format_args!("{CMDLINE_ID}: {why}"));
            return r.finish(2);
        }
    };
    let _ = yield_until(50, || false);
    for (who, at, argv) in [("cat -", pid, &argv[..]), ("an older cat -", older.0, &older_argv[..])] {
        let want = nul_joined(argv);
        let alive_before = exit_of(at).is_none();
        let path = format!("/proc/{}/cmdline", at.0);
        let reader = run("cat cmdline", "/bin/cat", &["cat", &path]);
        let alive_after = exit_of(at).is_none();
        r.expect(alive_before && alive_after, &format!("pid {} ({who}) alive while its cmdline was read", at.0));
        match reader {
            Ok(ran) => {
                if ran.exit != Some(0) {
                    r.fail(format_args!("{CMDLINE_ID}: cat {path} ended {}; it wrote {:?}", status_text(ran.exit), String::from_utf8_lossy(&ran.out)));
                }
                if ran.out != want {
                    r.fail(format_args!(
                        "{CMDLINE_ID}: cat {path} ({who}'s) printed {:?} ({} bytes), expected {:?} ({} bytes: each argv string, then a NUL)",
                        String::from_utf8_lossy(&ran.out),
                        ran.out.len(),
                        String::from_utf8_lossy(&want),
                        want.len()
                    ));
                }
            }
            Err(why) => r.fail(format_args!("{CMDLINE_ID}: {why}")),
        }
    }
    // Its status file: Pid, and the parent from the process table.
    let mut ppid = None;
    quiet(|| {
        proc::each(|p| {
            if p.pid == pid {
                ppid = Some(p.ppid.0);
            }
        })
    });
    let spath = format!("/proc/{}/status", pid.0);
    match (quiet(|| vfs::walk(&vfs::root()?, &spath)).and_then(|f| read_all(&f)), ppid) {
        (Ok(s), Some(pp)) => status_check(&mut r, &spath, &s, pid.0, pp),
        (Ok(_), None) => r.fail(format_args!("{CMDLINE_ID}: pid {} is not in proc::each", pid.0)),
        (Err(e), _) => r.fail(format_args!("{CMDLINE_ID}: reading {spath} returned {}", errno_text(e))),
    }
    note(format_args!("{CMDLINE_ID}: argv was {} strings, {} bytes with their NULs", argv.len(), want.len()));
    drop(stdin_w);
    let first = drain("cat -", &out_r, pid);
    note(format_args!("{CMDLINE_ID}: with its stdin closed, cat - (pid {}) ended {}", pid.0, status_text(first.exit)));
    self_check(fs, &mut r);
    // The older cat is still alive: it is the run's first process, pid 1
    // (`NEXT_PID` starts at 1), so an orphan goes to it (lab 14's rule).
    family_check(fs, &mut r);
    chain_check(fs, &mut r);
    let (older_pid, older_in, older_out) = older;
    drop(older_in);
    let _ = drain("an older cat -", &older_out, older_pid);
    r.finish(2);
}

/// An orphan whose grandparent still runs. A program (G) forks M and waits
/// on fd 3; M waits on fd 0 (a pipe only M reads), then moves that pipe to
/// its fd 3 with `dup2`, forks O and ends, while O waits on fd 0. `exit`
/// hands O to lab 14's heir (pid 1 while it runs), not to G, which still
/// runs: `cat` reads O's status, whose PPid must be the heir. O's `getppid`
/// is asked after the pipes close, when G may have ended as well, so it is
/// held to the heir too but checks a live grandparent only when G outlives it.
fn chain_check(fs: &Fs, r: &mut Row) {
    let word = format!("c{:012x}", unpublished());
    let mut p = Probe::new();
    let (tok, _) = p.buf(8);
    let k_m = p.call(SYS_FORK, &[]);
    // M's fork answer is 0, so it reads fd 0 and waits; G's is a pid, not an fd.
    p.call(SYS_READ, &[A::R(k_m), tok, A::I(1)]);
    // In M, fd 3 becomes fd 0's pipe; in G this fails and fd 3 stays its own.
    p.call(SYS_DUP2, &[A::R(k_m), A::I(3)]);
    p.call(SYS_READ, &[A::I(3), tok, A::I(1)]);
    let k_o = p.call(SYS_FORK, &[]);
    p.call(SYS_READ, &[A::R(k_o), tok, A::I(1)]);
    let k_ppid = p.call(SYS_GETPPID, &[]);
    const WHAT: &str = "a program whose child forks and ends while the program waits";
    let (path, len) = match install(fs, "chain", p) {
        Ok(it) => it,
        Err(why) => return r.fail(format_args!("{CMDLINE_ID}: {WHAT}: {why}")),
    };
    let pipes = (pipe::open(0), pipe::open(0), out_pipe("chain"));
    let ((m_r, m_w), (g_r, g_w), (out_r, out_w)) = match pipes {
        (Ok(a), Ok(b), Ok(c)) => (a, b, c),
        (Err(e), _, _) | (_, Err(e), _) => return r.fail(format_args!("{CMDLINE_ID}: pipe::open(0) returned {}", errno_text(e))),
        (_, _, Err(why)) => return r.fail(format_args!("{CMDLINE_ID}: {why}")),
    };
    let started = start_with("chain", || exec::spawn(&path, &["probe", &word], &[]), &[(0, &m_r), (1, &out_w), (2, &out_w), (3, &g_r)]);
    drop((m_r, g_r, out_w));
    let g = match started {
        Ok(g) => g,
        Err(why) => return r.fail(format_args!("{CMDLINE_ID}: {WHAT}: {why}")),
    };
    let mut kids = None;
    let mut heir = 0;
    'run: {
        let mut m = None;
        if !yield_until(RUN_MS, || {
            quiet(|| proc::each(|q| if q.ppid == g { m = Some(q.pid) }));
            m.is_some()
        }) {
            r.fail(format_args!("{CMDLINE_ID}: {WHAT}: pid {} had no fork child within {RUN_MS} ms ({})", g.0, status_text(exit_of(g))));
            break 'run;
        }
        let m = m.unwrap_or(g);
        // Both of M's reads.
        let _ = m_w.write(b"12");
        let mut o = None;
        if !yield_until(RUN_MS, || {
            quiet(|| {
                proc::each(|q| {
                    if q.exit.is_none() && q.pid != g && q.pid != m && q.argv.iter().map(|a| a.as_str()).eq(["probe", word.as_str()]) {
                        o = Some(q.pid);
                    }
                })
            });
            o.is_some() && exit_of(m).is_some()
        }) {
            r.fail(format_args!("{CMDLINE_ID}: {WHAT}: pid {}'s child, pid {}, had not forked and ended within {RUN_MS} ms ({})", g.0, m.0, status_text(exit_of(m))));
            break 'run;
        }
        let o = o.unwrap_or(g);
        kids = Some((m, o));
        if exit_of(g).is_some() {
            r.fail(format_args!("{CMDLINE_ID}: {WHAT}: pid {} ended {} while it waited on fd 3", g.0, status_text(exit_of(g))));
            break 'run;
        }
        quiet(|| {
            proc::each(|q| {
                if q.pid == Pid(1) && q.exit.is_none() {
                    heir = 1;
                }
            })
        });
        note(format_args!("{CMDLINE_ID}: pid {}'s child {} forked {} and ended while pid {} waited; the heir is pid {heir}", g.0, m.0, o.0, g.0));
        let spath = format!("/proc/{}/status", o.0);
        match run("cat status", "/bin/cat", &["cat", &spath]) {
            Ok(ran) if ran.exit == Some(0) => status_check(r, &format!("cat {spath} (pid {}'s child, whose parent pid {} has ended while pid {} runs)", m.0, m.0, g.0), &ran.out, o.0, heir),
            Ok(ran) => r.fail(format_args!("{CMDLINE_ID}: cat {spath} ended {}; it wrote {:?}", status_text(ran.exit), String::from_utf8_lossy(&ran.out))),
            Err(why) => r.fail(format_args!("{CMDLINE_ID}: {why}")),
        }
    }
    // The pipes close: O reads the end of fd 0, G of fd 3 and forks once
    // more. Four copies of the data: G's, M's, O's and G's second child's.
    drop((m_w, g_w));
    let ran = drain("chain", &out_r, g);
    let Some((m, o)) = kids else { return };
    let out = match ran_whole("chain", ran, 4 * len) {
        Ok(out) => out,
        Err(why) => return r.fail(format_args!("{CMDLINE_ID}: {WHAT}: {why} (four copies of its data)")),
    };
    let mut seen = false;
    for seg in out.chunks(len) {
        if res(seg, k_m) == 0 && res(seg, k_o) == 0 {
            seen = true;
            if res(seg, k_ppid) != heir as i64 {
                r.fail(format_args!("{CMDLINE_ID}: {WHAT}: in pid {}, whose parent pid {} ended while pid {} ran, getppid() returned {}; expected {heir}", o.0, m.0, g.0, answer(res(seg, k_ppid))));
            }
        }
    }
    if !seen {
        r.fail(format_args!("{CMDLINE_ID}: {WHAT}: no copy of its data holds fork() answers 0 and 0 (pid {}'s)", o.0));
    }
}

/// Who a fork child's parent is, read by `cat` while the parent runs and
/// after it ends. A program forks child A, which waits on fd 0 (a pipe this
/// file holds the write end of), and itself waits on fd 3 (another such
/// pipe). `cat` reads A's status; that `cat` takes a pid, so the program's
/// second fork, child B, is not one past its parent. Then fd 3's byte: the
/// program forks B, which waits on fd 0, and waits on fd 3 again; `cat`
/// reads B's status (its parent alive, and not B's pid less one). B asks
/// `getppid` and writes a byte to fd 4 before it waits, so it asks while
/// its parent runs. Then fd 3's last byte: the program ends without
/// waiting, and `exit` hands A and B to pid 1 while pid 1 runs (to pid 0
/// otherwise; lab 14's rule). Other programs fork a number of times drawn
/// for the run, and one more forks and stays alive with its child; `cat`
/// reads A's and B's status again: each `PPid` must now be that heir. A
/// parent worked out from the pid, from a list of the last eight forks, or
/// from the newest live process that forked names the wrong process. (A
/// parent kept beside the record, or from an earlier read, and handed to
/// pid 1 when it ends, is correct here: orphans go only to pid 1, and pids
/// are never reused.)
fn family_check(fs: &Fs, r: &mut Row) {
    let word = format!("t{:012x}", unpublished());
    let mut p = Probe::new();
    let (tok, _) = p.buf(8);
    // In a child the fork's answer is 0, so it reads fd 0 and waits; in the
    // program it is the child's pid, an fd it does not have, so it does not.
    let k_f1 = p.call(SYS_FORK, &[]);
    p.call(SYS_READ, &[A::R(k_f1), tok, A::I(1)]);
    p.call(SYS_READ, &[A::I(3), tok, A::I(1)]);
    let k_f2 = p.call(SYS_FORK, &[]);
    // B asks who its parent is while the parent runs, then says so on fd 4
    // (the program writes a byte there too).
    let k_early = p.call(SYS_GETPPID, &[]);
    p.call(SYS_WRITE, &[A::I(4), tok, A::I(1)]);
    p.call(SYS_READ, &[A::R(k_f2), tok, A::I(1)]);
    p.call(SYS_READ, &[A::I(3), tok, A::I(1)]);
    let k_ppid = p.call(SYS_GETPPID, &[]);
    const WHAT: &str = "a program that forks twice and ends while both children wait on a pipe";
    let (path, len) = match install(fs, "family", p) {
        Ok(it) => it,
        Err(why) => return r.fail(format_args!("{CMDLINE_ID}: {WHAT}: {why}")),
    };
    let mut q = Probe::new();
    q.call(SYS_FORK, &[]);
    q.call(SYS_WAIT4, &[A::I(u64::MAX), A::I(0), A::I(0)]);
    let (fpath, flen) = match install(fs, "forks", q) {
        Ok(it) => it,
        Err(why) => return r.fail(format_args!("{CMDLINE_ID}: a program that forks and waits: {why}")),
    };
    // One that forks and stays: its child waits on fd 0, it waits on fd 3.
    let mut h = Probe::new();
    let (htok, _) = h.buf(8);
    let k_h = h.call(SYS_FORK, &[]);
    h.call(SYS_READ, &[A::R(k_h), htok, A::I(1)]);
    h.call(SYS_READ, &[A::I(3), htok, A::I(1)]);
    h.call(SYS_WAIT4, &[A::I(u64::MAX), A::I(0), A::I(0)]);
    let (hpath, hlen) = match install(fs, "holder", h) {
        Ok(it) => it,
        Err(why) => return r.fail(format_args!("{CMDLINE_ID}: a program that forks and stays: {why}")),
    };
    let pipes = (pipe::open(0), pipe::open(0), out_pipe("family"), out_pipe("family's fd 4"));
    let ((g0_r, g0_w), (g3_r, g3_w), (out_r, out_w), (s4_r, s4_w)) = match pipes {
        (Ok(a), Ok(b), Ok(c), Ok(d)) => (a, b, c, d),
        (Err(e), _, _, _) | (_, Err(e), _, _) => return r.fail(format_args!("{CMDLINE_ID}: pipe::open(0) returned {}", errno_text(e))),
        (_, _, Err(why), _) | (_, _, _, Err(why)) => return r.fail(format_args!("{CMDLINE_ID}: {why}")),
    };
    let started = start_with("family", || exec::spawn(&path, &["probe", &word], &[]), &[(0, &g0_r), (1, &out_w), (2, &out_w), (3, &g3_r), (4, &s4_w)]);
    drop((g0_r, g3_r, out_w, s4_w));
    let pid = match started {
        Ok(pid) => pid,
        Err(why) => return r.fail(format_args!("{CMDLINE_ID}: {WHAT}: {why}")),
    };
    // A live process with the program's argv that is none of `not`.
    let find = |not: &[Pid]| {
        let mut c = None;
        quiet(|| {
            proc::each(|q| {
                if q.exit.is_none() && !not.contains(&q.pid) && q.argv.iter().map(|a| a.as_str()).eq(["probe", word.as_str()]) {
                    c = Some(q.pid);
                }
            })
        });
        c
    };
    let cat = |r: &mut Row, c: Pid, ppid: u64, when: &str| {
        let spath = format!("/proc/{}/status", c.0);
        match run("cat status", "/bin/cat", &["cat", &spath]) {
            Ok(ran) if ran.exit == Some(0) => status_check(r, &format!("cat {spath} ({when})"), &ran.out, c.0, ppid),
            Ok(ran) => r.fail(format_args!("{CMDLINE_ID}: cat {spath} ended {}; it wrote {:?}", status_text(ran.exit), String::from_utf8_lossy(&ran.out))),
            Err(why) => r.fail(format_args!("{CMDLINE_ID}: {why}")),
        }
    };
    let hpipes = (pipe::open(0), pipe::open(0), out_pipe("holder"));
    let ((h0_r, h0_w), (h3_r, h3_w), (hout_r, hout_w)) = match hpipes {
        (Ok(a), Ok(b), Ok(c)) => (a, b, c),
        (Err(e), _, _) | (_, Err(e), _) => return r.fail(format_args!("{CMDLINE_ID}: pipe::open(0) returned {}", errno_text(e))),
        (_, _, Err(why)) => return r.fail(format_args!("{CMDLINE_ID}: {why}")),
    };
    let mut holder = None;
    let mut kids = (None, None);
    let mut heir = 0;
    'run: {
        let mut a = None;
        if !yield_until(RUN_MS, || {
            a = find(&[pid]);
            a.is_some()
        }) {
            r.fail(format_args!("{CMDLINE_ID}: {WHAT}: no fork child of pid {} was alive within {RUN_MS} ms ({})", pid.0, status_text(exit_of(pid))));
            break 'run;
        }
        let a = a.unwrap_or(pid);
        kids.0 = Some(a);
        cat(r, a, pid.0, &format!("pid {}'s first fork child, its parent waiting", pid.0));
        let _ = g3_w.write(b"1");
        let mut b = None;
        if !yield_until(RUN_MS, || {
            b = find(&[pid, a]);
            b.is_some()
        }) {
            r.fail(format_args!("{CMDLINE_ID}: {WHAT}: pid {} had no second fork child alive within {RUN_MS} ms of fd 3's byte ({})", pid.0, status_text(exit_of(pid))));
            break 'run;
        }
        let b = b.unwrap_or(pid);
        kids.1 = Some(b);
        // Two bytes on fd 4, the program's and B's: B has asked getppid.
        let mut said = 0;
        let mut byte = [0u8; 8];
        if !yield_until(RUN_MS, || {
            if let Ok(n) = s4_r.read(&mut byte) {
                said += n;
            }
            said >= 2
        }) {
            r.fail(format_args!("{CMDLINE_ID}: {WHAT}: fd 4 had {said} of the 2 bytes the program and its second child write after the second fork within {RUN_MS} ms"));
            break 'run;
        }
        cat(r, b, pid.0, &format!("pid {}'s second fork child, forked after another process started; its parent waiting", pid.0));
        let _ = g3_w.write(b"2");
        if !yield_until(RUN_MS, || exit_of(pid).is_some()) {
            r.fail(format_args!("{CMDLINE_ID}: {WHAT}: pid {} had not ended within {RUN_MS} ms of fd 3's last byte ({})", pid.0, status_text(exit_of(pid))));
            break 'run;
        }
        // Forks the reads have not seen, a number of them drawn for the run.
        let n = 12 + unpublished() % 9;
        for _ in 0..n {
            if let Err(why) = run_with("forks", &fpath, &["probe"], &[]).and_then(|ran| ran_whole("forks", ran, 2 * flen)) {
                r.fail(format_args!("{CMDLINE_ID}: a program that forks and waits: {why} (two copies of its data)"));
                break 'run;
            }
        }
        // Then one that forks and stays alive with its child while cat reads.
        let started = start_with("holder", || exec::spawn(&hpath, &["probe"], &[]), &[(0, &h0_r), (1, &hout_w), (2, &hout_w), (3, &h3_r)]);
        let hpid = match started {
            Ok(hp) => hp,
            Err(why) => {
                r.fail(format_args!("{CMDLINE_ID}: a program that forks and stays: {why}"));
                break 'run;
            }
        };
        holder = Some(hpid);
        let mut forked = false;
        if !yield_until(RUN_MS, || {
            quiet(|| proc::each(|q| forked |= q.ppid == hpid && q.exit.is_none()));
            forked
        }) {
            r.fail(format_args!("{CMDLINE_ID}: a program that forks and stays (pid {}) had no live child within {RUN_MS} ms", hpid.0));
            break 'run;
        }
        quiet(|| {
            proc::each(|q| {
                if q.pid == Pid(1) && q.exit.is_none() {
                    heir = 1;
                }
            })
        });
        note(format_args!("{CMDLINE_ID}: fork children {} and {} of pid {}, read by cat while it waited and after it ended; their heir is pid {heir}", a.0, b.0, pid.0));
        let when = format!("whose parent, pid {}, has ended; pid 1 {}", pid.0, if heir == 1 { "runs" } else { "has ended" });
        cat(r, a, heir, &when);
        cat(r, b, heir, &when);
    }
    // Everyone's turn: the pipes close, every read ends, and A forks once
    // more on its way out. Four copies of the data: the program's, A's, B's
    // and A's child's.
    drop((g0_w, g3_w));
    let ran = drain("family", &out_r, pid);
    drop(s4_r);
    drop((h0_r, h3_r, hout_w, h0_w, h3_w));
    if let Some(hp) = holder {
        if let Err(why) = ran_whole("holder", drain("holder", &hout_r, hp), 2 * hlen) {
            r.fail(format_args!("{CMDLINE_ID}: a program that forks and stays: {why} (two copies of its data)"));
        }
    }
    let (Some(a), Some(b)) = kids else { return };
    let out = match ran_whole("family", ran, 4 * len) {
        Ok(out) => out,
        Err(why) => return r.fail(format_args!("{CMDLINE_ID}: {WHAT}: {why} (four copies of its data: its own, its two children's and the first child's child's)")),
    };
    // Which copy is whose, by its two fork answers.
    for seg in out.chunks(len) {
        let (f1, f2) = (res(seg, k_f1), res(seg, k_f2));
        let (who, want) = if f1 == a.0 as i64 && f2 == b.0 as i64 {
            continue;
        } else if f1 == 0 && f2 > 0 {
            ("its first child", heir as i64)
        } else if f1 == a.0 as i64 && f2 == 0 {
            ("its second child", heir as i64)
        } else if f1 == 0 && f2 == 0 {
            continue;
        } else {
            r.fail(format_args!("{CMDLINE_ID}: {WHAT}: a copy of its data holds fork() answers {} and {}; expected {} and {} in pid {}, 0 and a pid in its first child, {} and 0 in its second", answer(f1), answer(f2), a.0, b.0, pid.0, a.0));
            continue;
        };
        if res(seg, k_ppid) != want {
            r.fail(format_args!("{CMDLINE_ID}: {WHAT}: in {who}, getppid() after its parent ended returned {}; expected {want}", answer(res(seg, k_ppid))));
        }
        if f2 == 0 && f1 != 0 && res(seg, k_early) != pid.0 as i64 {
            r.fail(format_args!("{CMDLINE_ID}: {WHAT}: in its second child, getppid() while pid {} waited returned {}; expected {}", pid.0, answer(res(seg, k_early)), pid.0));
        }
    }
}

/// `/proc` about a process that forked. A program forks; the parent reads
/// `/proc/self/status` and `/proc/self/cmdline` while its child waits on a
/// pipe this file holds the write end of, then asks getpid and getppid and
/// writes a byte to a second pipe. Seeing that byte, this file reads the
/// child's `/proc/<pid>/status` from outside it, then writes the child's
/// byte; the child reads its own `/proc/self/status`, asks getpid and
/// getppid, while the parent waits for it in `wait4`. Each process asks
/// who it is only after its reads.
fn self_check(fs: &Fs, r: &mut Row) {
    let word = format!("t{:012x}", unpublished());
    let mut p = Probe::new();
    let status = p.str("/proc/self/status");
    let cmdline = p.str("/proc/self/cmdline");
    let (sb, sb_at) = p.buf(512);
    let (cb, cb_at) = p.buf(256);
    let (tok, _) = p.buf(8);
    let z = A::I(0);
    // The turn: `read(fork's answer, 1)` — in the child fd 0, the first
    // pipe's read end, which waits for this file's byte; in the parent an
    // fd named by the child's pid, which is not that end and does not wait.
    let k_fork = p.call(SYS_FORK, &[]);
    let k_turn = p.call(SYS_READ, &[A::R(k_fork), tok, A::I(1)]);
    let k_sopen = p.call(SYS_OPEN, &[status, A::I(O_RDONLY), z]);
    let k_sread = p.call(SYS_READ, &[A::R(k_sopen), sb, A::I(512)]);
    let k_copen = p.call(SYS_OPEN, &[cmdline, A::I(O_RDONLY), z]);
    let k_cread = p.call(SYS_READ, &[A::R(k_copen), cb, A::I(256)]);
    let k_pid = p.call(SYS_GETPID, &[]);
    let k_ppid = p.call(SYS_GETPPID, &[]);
    let k_wake = p.call(SYS_WRITE, &[A::I(TURN_FD), tok, A::I(1)]);
    p.call(SYS_WAIT4, &[A::I(u64::MAX), z, z]);
    let seg = p.data.len();
    const WHAT: &str = "a program that forks, then reads /proc/self in the parent while the child waits on a pipe, then in the child";
    let (path, len) = match install(fs, "self", p) {
        Ok(it) => it,
        Err(why) => return r.fail(format_args!("{CMDLINE_ID}: {WHAT}: {why}")),
    };
    let (gate_r, gate_w) = match pipe::open(0) {
        Ok(ends) => ends,
        Err(e) => return r.fail(format_args!("{CMDLINE_ID}: pipe::open(0) returned {}", errno_text(e))),
    };
    let ((sig_r, sig_w), (out_r, out_w)) = match (out_pipe("self"), out_pipe("self")) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(why), _) | (_, Err(why)) => return r.fail(format_args!("{CMDLINE_ID}: {why}")),
    };
    let started = start_with(
        "self",
        || exec::spawn(&path, &["probe", &word], &[]),
        &[(0, &gate_r), (1, &out_w), (2, &out_w), (TURN_FD as usize, &sig_w)],
    );
    drop((gate_r, sig_w, out_w));
    let pid = match started {
        Ok(pid) => pid,
        Err(why) => return r.fail(format_args!("{CMDLINE_ID}: {WHAT}: {why}")),
    };
    // The parent's byte: it has read /proc/self and asked who it is.
    let mut byte = [0u8; 1];
    let signalled = yield_until(RUN_MS, || sig_r.read(&mut byte) == Ok(1));
    let mut child = None;
    quiet(|| {
        proc::each(|q| {
            if q.ppid == pid && q.exit.is_none() {
                child = Some(q.pid);
            }
        })
    });
    match (signalled, child) {
        (true, Some(c)) => {
            let spath = format!("/proc/{}/status", c.0);
            match quiet(|| vfs::walk(&vfs::root()?, &spath)).and_then(|f| read_all(&f)) {
                Ok(text) => status_check(r, &format!("{spath} (pid {}'s fork child, read from outside it)", pid.0), &text, c.0, pid.0),
                Err(e) => r.fail(format_args!("{CMDLINE_ID}: reading {spath} (pid {}'s fork child) returned {}", pid.0, errno_text(e))),
            }
            // Another program forks, so the child is no longer the newest
            // fork; then a process, not this file, reads the child's status.
            let mut q = Probe::new();
            q.call(SYS_FORK, &[]);
            q.call(SYS_WAIT4, &[A::I(u64::MAX), A::I(0), A::I(0)]);
            if let Err(why) = run_probe_as(fs, "fork", q, &["probe"], &[], 2) {
                r.fail(format_args!("{CMDLINE_ID}: a second program that forks and waits: {why} (two copies of its data)"));
            }
            match run("cat status", "/bin/cat", &["cat", &spath]) {
                Ok(ran) if ran.exit == Some(0) => {
                    status_check(r, &format!("cat {spath} (pid {}'s fork child, after another program forked)", pid.0), &ran.out, c.0, pid.0)
                }
                Ok(ran) => r.fail(format_args!("{CMDLINE_ID}: cat {spath} ended {}; it wrote {:?}", status_text(ran.exit), String::from_utf8_lossy(&ran.out))),
                Err(why) => r.fail(format_args!("{CMDLINE_ID}: {why}")),
            }
        }
        (true, None) => r.fail(format_args!("{CMDLINE_ID}: pid {} forked and wrote its byte, but proc::each has no live process whose parent it is", pid.0)),
        (false, _) => r.fail(format_args!("{CMDLINE_ID}: {WHAT}: pid {} wrote no byte to fd {TURN_FD} after its reads in {RUN_MS} ms", pid.0)),
    }
    // The child's turn.
    let _ = gate_w.write(b"g");
    drop(gate_w);
    let ran = drain("self", &out_r, pid);
    drop(sig_r);
    let out = match ran_whole("self", ran, 2 * len) {
        Ok(out) => out,
        Err(why) => return r.fail(format_args!("{CMDLINE_ID}: {WHAT}: {why} (two copies of its data, the child's and the parent's)")),
    };
    let (a, b) = out.split_at(seg);
    // The child's fork() answer is 0, the parent's the child's pid.
    let (kid, parent) = if res(a, k_fork) == 0 { (a, b) } else { (b, a) };
    if res(kid, k_fork) != 0 || res(parent, k_fork) != res(kid, k_pid) || child.is_some_and(|c| res(kid, k_pid) != c.0 as i64) {
        return r.fail(format_args!(
            "{CMDLINE_ID}: fork() in a program returned {} and {}, and getpid() {} and {}; expected 0 in the child, the child's pid in the parent",
            answer(res(a, k_fork)),
            answer(res(b, k_fork)),
            answer(res(a, k_pid)),
            answer(res(b, k_pid))
        ));
    }
    // The child waited for this file's byte; the parent wrote its own.
    if res(kid, k_turn) != 1 || res(parent, k_wake) != 1 {
        return r.fail(format_args!(
            "{CMDLINE_ID}: the child's read of fd 0, a pipe, returned {}, and the parent's write to fd {TURN_FD}, another, {} (expected 1 each)",
            answer(res(kid, k_turn)),
            answer(res(parent, k_wake))
        ));
    }
    for (who, seg) in [("the parent", parent), ("the child", kid)] {
        let n = res(seg, k_sread);
        let text = if (1..=512).contains(&n) { &seg[sb_at..sb_at + n as usize] } else { &[][..] };
        if text.is_empty() {
            r.fail(format_args!("{CMDLINE_ID}: in {who}, open then read of /proc/self/status returned {} and {}", answer(res(seg, k_sopen)), answer(n)));
            continue;
        }
        let whose = format!("/proc/self/status in {who} (pid {})", res(seg, k_pid));
        status_check(r, &whose, text, res(seg, k_pid) as u64, res(seg, k_ppid) as u64);
    }
    let want = nul_joined(&["probe", word.as_str()]);
    let n = res(parent, k_cread);
    let got = if (0..=256).contains(&n) { &parent[cb_at..cb_at + n as usize] } else { &[][..] };
    if got == want.as_slice() {
        note(format_args!(
            "{CMDLINE_ID}: /proc/self read by a parent (pid {}) while its fork child waited on a pipe, the child's status read from outside and by cat after another fork, then /proc/self by the child (pid {}) while the parent waited for it",
            res(parent, k_pid),
            res(kid, k_pid)
        ));
    } else {
        r.fail(format_args!(
            "{CMDLINE_ID}: in the parent (pid {}), read of /proc/self/cmdline returned {} and {:?}, expected its own argv {:?}",
            res(parent, k_pid),
            answer(n),
            String::from_utf8_lossy(got),
            String::from_utf8_lossy(&want)
        ));
    }
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
            bad.push(format!("frame::stats().free fell by {} after the test took {} frames", s1.free as i64 - s2.free as i64, held.len()));
        }
        if m1.1 as i64 - m2.1 as i64 != 4 * held.len() as i64 {
            bad.push(format!("MemFree went {} → {} kB ({:+} kB) after the test took {} frames; expected {} kB less", m1.1, m2.1, m2.1 as i64 - m1.1 as i64, held.len(), 4 * held.len()));
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
    // A rename inside ext2's /tmp: ext2 decides a directory is its own by
    // its stat, st_dev included (lab 20).
    let made = quiet(|| fs.tmp.create(MV_FROM, 0o644)).map(|_| ());
    let (from, to) = (format!("/tmp/{MV_FROM}"), format!("/tmp/{MV_TO}"));
    let (from_a, to_a) = (p.str(&from), p.str(&to));
    let (mv_st, mv_at) = p.buf(144);
    let k_mv = p.call(SYS_RENAME, &[from_a, to_a]);
    let k_mvst = p.call(SYS_STAT, &[to_a, mv_st]);
    if let Err(e) = made {
        r.fail(format_args!("{STDEV_ID}: ext2's /tmp create({MV_FROM:?}) returned {}", errno_text(e)));
    }
    match run_probe(fs, "stat", p) {
        Ok(out) => {
            r.expect_res(&format!("rename({from:?}, {to:?}), both in ext2's /tmp,"), res(&out, k_mv), 0);
            r.expect_res(&format!("stat({to:?}) after it"), res(&out, k_mvst), 0);
            if res(&out, k_mvst) == 0 && le64(&out, mv_at) != fs.ext.stat().dev {
                r.fail(format_args!("{STDEV_ID}: stat({to:?}) gives st_dev {}, ext2's root {}", le64(&out, mv_at), fs.ext.stat().dev));
            }
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
    let (null, zero) = (p.str("/dev/null"), p.str("/dev/zero"));
    let ttys = [p.str("/dev/tty"), p.str("/dev/console")];
    let (b, b_at) = p.filled(4096, 0xa5);
    let (w, _) = p.filled(128, 0x5a);
    let (st1, st1_at) = p.buf(144);
    let (st2, st2_at) = p.buf(144);
    let (one, _) = p.buf(8);
    let (drain_b, _) = p.buf(256);
    let (ws0, ws0_at) = p.buf(8);
    let ws = [p.buf(8), p.buf(8)];
    // fd 0 is the console object itself, opened here O_RDWR|O_NONBLOCK:
    // its queue is drained, and its window size is the one expected.
    let k_drain = [p.call(SYS_READ, &[A::I(0), drain_b, A::I(256)]), p.call(SYS_READ, &[A::I(0), drain_b, A::I(256)])];
    let k_ws0 = p.call(SYS_IOCTL, &[A::I(0), A::I(TIOCGWINSZ), ws0]);
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
    // Both terminals: opened O_RDWR|O_NONBLOCK, a one-byte read (the
    // console object answers -EAGAIN: its queue is empty), then the
    // window size, which must be fd 0's.
    let k_ttys: Vec<(usize, usize, usize)> = (0..2)
        .map(|i| {
            let k_open = p.call(SYS_OPEN, &[ttys[i], A::I(O_RDWR | O_NONBLOCK as u64), A::I(0)]);
            let k_read = p.call(SYS_READ, &[A::R(k_open), one, A::I(1)]);
            let k_win = p.call(SYS_IOCTL, &[A::R(k_open), A::I(TIOCGWINSZ), ws[i].0]);
            (k_open, k_read, k_win)
        })
        .collect();
    let console = match FileRef::open(FileObj::CONSOLE, O_RDWR as u32 | O_NONBLOCK) {
        Ok(f) => f,
        Err(e) => {
            r.fail(format_args!("{DEV_ID}: console::FileRef::open(FileObj::CONSOLE, O_RDWR|O_NONBLOCK) returned {}", errno_text(e)));
            return r.finish(6);
        }
    };
    match run_probe_as(fs, "dev", p, &["probe"], &[(0, &console)], 1) {
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
            let drained = k_drain.map(|k| answer(res(&out, k)));
            r.expect_res(&format!("ioctl(0, TIOCGWINSZ) on the console fd this program was given (its reads returned {} and {})", drained[0], drained[1]), res(&out, k_ws0), 0);
            let want = le64(&out, ws0_at);
            for (i, name) in ["/dev/tty", "/dev/console"].into_iter().enumerate() {
                let (k_open, k_read, k_win) = k_ttys[i];
                let at = ws[i].1;
                r.expect(fd_ok(res(&out, k_open)), &format!("open(\"{name}\", O_RDWR|O_NONBLOCK) returned {}, expected an fd", answer(res(&out, k_open))));
                r.expect_res(&format!("read({name}, 1) with O_NONBLOCK and no input waiting"), res(&out, k_read), -11);
                r.expect_res(&format!("ioctl({name}, TIOCGWINSZ)"), res(&out, k_win), 0);
                let got = le64(&out, at);
                if got != want {
                    r.fail(format_args!(
                        "{DEV_ID}: ioctl({name}, TIOCGWINSZ) gives {} rows, {} columns; the console fd 0 gives {} rows, {} columns: not the console object",
                        got & 0xffff,
                        (got >> 16) & 0xffff,
                        want & 0xffff,
                        (want >> 16) & 0xffff
                    ));
                }
            }
        }
        Err(why) => r.fail(format_args!("{DEV_ID}: {why}")),
    }
    drop(console);
    console_writes(fs, &mut r);
    r.finish(6);
}

/// COM1, the serial line every console in the course writes to (lab 02).
const COM1: u16 = 0x3f8;

fn port_in(port: u16) -> u8 {
    let v: u8;
    // SAFETY: a port read touches no memory.
    unsafe { asm!("in al, dx", out("al") v, in("dx") port, options(nomem, nostack, preserves_flags)) };
    v
}

fn port_out(port: u16, v: u8) {
    // SAFETY: a port write touches no memory; COM1 is the serial line.
    unsafe { asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack, preserves_flags)) };
}

/// Spin until `done` or a bounded number of tries.
fn port_wait(done: impl Fn() -> bool) {
    let mut n = 0u32;
    while !done() && n < 1_000_000 {
        n += 1;
        core::hint::spin_loop();
    }
}

/// `f.write(line)` with COM1 in loopback (MCR bit 4), interrupts off and
/// COM1's received-data interrupt off (IER bit 0, restored after, as the MCR
/// is): the bytes the write puts on the serial line come back in the UART's
/// receive FIFO (16 bytes) instead of leaving the machine; with interrupts
/// off nothing else in the kernel runs to take them unless its own write
/// lets an interrupt in. The IER clear is
/// defensive and unpinned: 8350b9fff's window, without it, already scored
/// an IRQ 4 receive 10/10, also one whose write lets an interrupt in
/// (CONTRACT §B.2-22 row 6). Its answer, and those bytes.
fn looped(f: &FileRef, line: &[u8]) -> (Result<usize, Errno>, Vec<u8>) {
    quiet(|| {
        let mut back = Vec::with_capacity(32);
        let ier = port_in(COM1 + 1);
        port_out(COM1 + 1, ier & !1);
        port_wait(|| port_in(COM1 + 5) & 0x40 != 0);
        let mcr = port_in(COM1 + 4);
        port_out(COM1 + 2, 0xC7);
        while port_in(COM1 + 5) & 1 != 0 && back.len() < 32 {
            back.push(port_in(COM1));
        }
        back.clear();
        port_out(COM1 + 4, mcr | 0x10);
        let w = f.write(line);
        port_wait(|| port_in(COM1 + 5) & 0x40 != 0);
        while port_in(COM1 + 5) & 1 != 0 && back.len() < 32 {
            back.push(port_in(COM1));
        }
        port_out(COM1 + 4, mcr);
        port_out(COM1 + 1, ier);
        (w, back)
    })
}

/// Row 6's last part: `/dev/tty` and `/dev/console`, opened here through
/// devfs, each write a `# t<word>` line with a word from no published
/// source (§A.1-5); the serial line must carry it (a CR before the LF, or
/// not).
fn console_writes(fs: &Fs, r: &mut Row) {
    for name in ["tty", "console"] {
        let line = format!("# t{:08x}\n", unpublished() & 0xffff_ffff);
        let f = match quiet(|| fs.devfs.lookup(name)).and_then(|ino| FileRef::open_inode(ino, O_WRONLY as u32)) {
            Ok(f) => f,
            Err(e) => {
                r.fail(format_args!("{DEV_ID}: /dev/{name}: lookup then FileRef::open_inode(.., O_WRONLY) returned {}", errno_text(e)));
                continue;
            }
        };
        if f.object != FileObj::CONSOLE {
            let what = match (f.object.as_file(), f.object.as_pipe()) {
                (Some(slot), _) => format!("a file slot ({slot})"),
                (_, Some((slot, w))) => format!("a pipe's {} end (slot {slot})", if w { "write" } else { "read" }),
                _ => format!("{:?}", f.object),
            };
            r.fail(format_args!("{DEV_ID}: FileRef::open_inode on /dev/{name} gave an open file on {what}, not the console object"));
        }
        let (w, back) = looped(&f, line.as_bytes());
        drop(f);
        let got: Vec<u8> = back.iter().copied().filter(|&c| c != b'\r').collect();
        if w != Ok(line.len()) || got != line.as_bytes() {
            r.fail(format_args!(
                "{DEV_ID}: a write of {line:?} to /dev/{name} returned {} and the console's serial line carried {:?}; expected the line itself",
                match w {
                    Ok(n) => format!("{n}"),
                    Err(e) => errno_text(e),
                },
                String::from_utf8_lossy(&back)
            ));
        }
    }
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
    // Two more new /dev's at names drawn for this run. With the boot's four
    // mounts, the new /proc and the first new /dev, they fill lab 13's
    // table of MAX_MOUNTS (8), so this is as many as a kernel must hold.
    let tag = unpublished() & 0xffff_ffff;
    let more_names = [format!("k22-{tag:08x}-a"), format!("k22-{tag:08x}-b")];
    let more: Vec<String> = more_names.iter().map(|n| format!("/tmp/{n}")).collect();
    for name in [PROC_AT_NAME, DEV_AT_NAME, &*more_names[0], &*more_names[1]] {
        if let Err(e) = quiet(|| fs.tmp.mkdir(name, 0o755)) {
            r.fail(format_args!("{SYSCALL_ID}: ext2's /tmp mkdir({name:?}) returned {}", errno_text(e)));
            return r.finish(8);
        }
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
    let (devfs_, dev_at) = (p.str("devfs"), p.str(DEV_AT));
    let dev_null = p.str(&format!("{DEV_AT}/null"));
    let (nst, nst_at) = p.buf(144);
    let k_dmount = p.call(SYS_MOUNT, &[devfs_, dev_at, devfs_, z, z]);
    let k_nstat = p.call(SYS_STAT, &[dev_null, nst]);
    let (dst, dst_at) = p.buf(144);
    let k_dstat = p.call(SYS_STAT, &[dev_at, dst]);
    let (d2_at, d3_at) = (p.str(&more[0]), p.str(&more[1]));
    let (d2st, d2st_at) = p.buf(144);
    let (d3st, d3st_at) = p.buf(144);
    let k_d2mount = p.call(SYS_MOUNT, &[devfs_, d2_at, devfs_, z, z]);
    let k_d2stat = p.call(SYS_STAT, &[d2_at, d2st]);
    let refusals: [(String, [A; 3], i64); 6] = [
        (format!("mount(\"proc\", {PROC_AT:?}, \"proc\", 0, 0) a second time"), [proc_, at, proc_], -EBUSY),
        (String::from("mount(\"x\", \"/tmp\", \"ext2\", 0, 0)"), [x, tmp, ext2], -ENODEV),
        (String::from("mount(\"x\", \"/tmp\", \"nofs\", 0, 0)"), [x, tmp, nofs], -ENODEV),
        (String::from("mount(\"proc\", \"tmp/k22 rel\", \"proc\", 0, 0) (a relative target)"), [proc_, rel, proc_], -EINVAL),
        (String::from("mount(\"proc\", \"/tmp/k22-none\", \"proc\", 0, 0) (no such directory)"), [proc_, missing, proc_], -ENOENT),
        (String::from("mount(\"proc\", \"/boot/init\", \"proc\", 0, 0) (a file)"), [proc_, init, proc_], -ENOTDIR),
    ];
    let ks: Vec<usize> = refusals.iter().map(|(_, a, _)| p.call(SYS_MOUNT, &[a[0], a[1], a[2], z, z])).collect();
    // The last new /dev after the refusals, so they meet seven mounts and
    // never a full table.
    let k_d3mount = p.call(SYS_MOUNT, &[devfs_, d3_at, devfs_, z, z]);
    let k_d3stat = p.call(SYS_STAT, &[d3_at, d3st]);
    // Each new mount once more, at the end: its root a second time and a
    // node in it, so a mount must keep one device for all its stats; then
    // the new /proc's meminfo opened and fstat'd, which must give its path's
    // device too (a device node may open as an object of its own, as the
    // console does, so only meminfo is compared).
    let mut steady: Vec<(String, String, usize, usize, usize, usize)> = Vec::new();
    let mut held: Vec<(String, usize, usize, usize, usize, usize)> = Vec::new();
    for (at, inner) in [(String::from(PROC_AT), "meminfo"), (String::from(DEV_AT), "null"), (more[0].clone(), "null"), (more[1].clone(), "null")] {
        let path = format!("{at}/{inner}");
        let ((a2, a2_at), (n2, n2_at)) = (p.buf(144), p.buf(144));
        let (sa, sp) = (p.str(&at), p.str(&path));
        let k1 = p.call(SYS_STAT, &[sa, a2]);
        let k2 = p.call(SYS_STAT, &[sp, n2]);
        if inner == "meminfo" {
            let (f2, f2_at) = p.buf(144);
            let ko = p.call(SYS_OPEN, &[sp, A::I(O_RDONLY), z]);
            let kf = p.call(SYS_FSTAT, &[A::R(ko), f2]);
            held.push((path.clone(), k2, n2_at, ko, kf, f2_at));
        }
        steady.push((at, path, k1, a2_at, k2, n2_at));
    }
    // `..` out of a mount's root is the directory that holds the mount point: /proc's
    // and /dev's give /, each new mount's gives /tmp, in an order drawn for
    // the run; each must be that directory's file, st_dev and st_ino both.
    let mut ups: Vec<(String, &str)> = alloc::vec![(String::from("/proc/.."), "/"), (String::from("/dev/.."), "/")];
    for at in [String::from(PROC_AT), String::from(DEV_AT), more[0].clone(), more[1].clone()] {
        ups.push((format!("{at}/.."), "/tmp"));
    }
    let turn = (unpublished() % ups.len() as u64) as usize;
    ups.rotate_left(turn);
    let mut climbs: Vec<(String, &str, usize, usize, usize, usize)> = Vec::new();
    for (up, base) in ups {
        let ((ub, ub_at), (bb, bb_at)) = (p.buf(144), p.buf(144));
        let (su, sb) = (p.str(&up), p.str(base));
        let ku = p.call(SYS_STAT, &[su, ub]);
        let kb = p.call(SYS_STAT, &[sb, bb]);
        climbs.push((up, base, ku, ub_at, kb, bb_at));
    }
    // /proc/mounts before the program, to compare with after it.
    let table = || quiet(|| fs.procfs.lookup("mounts")).and_then(|f| read_all(&f)).map(|t| String::from(String::from_utf8_lossy(&t)));
    let before = table();
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
            // Every mount in the table holds a file system of its own, and
            // anonymous numbers are global: each new mount's device must be
            // unlike every one mounted before it, the boot's four included
            // (round 4b; the boot's four among themselves are row 5's).
            let mut seen: Vec<(String, u64)> = Vec::new();
            seen.push((String::from("ext2's at /"), fs.ext.stat().dev));
            seen.push((String::from("the boot archive's at /boot"), fs.arch.stat().dev));
            seen.push((String::from("/proc's"), fs.procfs.stat().dev));
            seen.push((String::from("/dev's"), fs.devfs.stat().dev));
            let own = |r: &mut Row, seen: &mut Vec<(String, u64)>, at: &str, what: &str, d: u64| {
                for (whose, other) in seen.iter() {
                    if d == *other {
                        r.fail(format_args!("{SYSCALL_ID}: stat({at:?}) gives st_dev {d}, the same as {whose}; expected a device of its own"));
                    }
                }
                seen.push((format!("the new {what}'s at {at:?}"), d));
            };
            if res(&out, k_stat) == 0 {
                own(&mut r, &mut seen, PROC_AT, "/proc", le64(&out, st_at));
            }
            r.expect_res(&format!("mount(\"devfs\", {DEV_AT:?}, \"devfs\", 0, 0)"), res(&out, k_dmount), 0);
            r.expect_res(&format!("stat({:?}) after it", format!("{DEV_AT}/null")), res(&out, k_nstat), 0);
            if res(&out, k_nstat) == 0 {
                r.expect_eq(&format!("stat({:?}).st_rdev", format!("{DEV_AT}/null")), le64(&out, nst_at + 40), 1 << 8 | 3);
            }
            // The new /dev's: each a device of its own too.
            r.expect_res(&format!("stat({DEV_AT:?})"), res(&out, k_dstat), 0);
            if res(&out, k_dstat) == 0 {
                own(&mut r, &mut seen, DEV_AT, "/dev", le64(&out, dst_at));
            }
            for (at, km, kst, sat) in [(&more[0], k_d2mount, k_d2stat, d2st_at), (&more[1], k_d3mount, k_d3stat, d3st_at)] {
                r.expect_res(&format!("mount(\"devfs\", {at:?}, \"devfs\", 0, 0)"), res(&out, km), 0);
                r.expect_res(&format!("stat({at:?})"), res(&out, kst), 0);
                if res(&out, kst) == 0 {
                    own(&mut r, &mut seen, at, "/dev", le64(&out, sat));
                }
            }
            let firsts = [(k_stat, st_at), (k_dstat, dst_at), (k_d2stat, d2st_at), (k_d3stat, d3st_at)];
            for ((at, path, k1, o1, k2, o2), (k0, o0)) in steady.iter().zip(firsts) {
                r.expect_res(&format!("stat({at:?}) a second time"), res(&out, *k1), 0);
                r.expect_res(&format!("stat({path:?})"), res(&out, *k2), 0);
                if res(&out, k0) == 0 && res(&out, *k1) == 0 && res(&out, *k2) == 0 {
                    let (d0, d1, d2) = (le64(&out, o0), le64(&out, *o1), le64(&out, *o2));
                    if d0 != d1 || d0 != d2 {
                        r.fail(format_args!(
                            "{SYSCALL_ID}: stat({at:?}) gives st_dev {d0}, then {d1} at the program's end, and stat({path:?}) gives {d2}; expected one device for the whole mount"
                        ));
                    }
                }
            }
            for (path, k2, o2, ko, kf, of) in &held {
                let fd = res(&out, *ko);
                if !(3..32).contains(&fd) {
                    r.fail(format_args!("{SYSCALL_ID}: open({path:?}, O_RDONLY) returned {}, expected an fd", answer(fd)));
                    continue;
                }
                r.expect_res(&format!("fstat of {path:?} opened"), res(&out, *kf), 0);
                if res(&out, *kf) == 0 && res(&out, *k2) == 0 {
                    let (dp, df) = (le64(&out, *o2), le64(&out, *of));
                    if dp != df {
                        r.fail(format_args!("{SYSCALL_ID}: stat({path:?}) gives st_dev {dp}, and fstat of it opened gives {df}; expected the same device"));
                    }
                }
            }
            for (up, base, ku, uo, kb, bo) in &climbs {
                r.expect_res(&format!("stat({up:?})"), res(&out, *ku), 0);
                r.expect_res(&format!("stat({base:?})"), res(&out, *kb), 0);
                if res(&out, *ku) == 0 && res(&out, *kb) == 0 {
                    let (ud, ui, bd, bi) = (le64(&out, *uo), le64(&out, uo + 8), le64(&out, *bo), le64(&out, bo + 8));
                    if (ud, ui) != (bd, bi) {
                        r.fail(format_args!("{SYSCALL_ID}: stat({up:?}) gives st_dev {ud} st_ino {ui}, and stat({base:?}) gives st_dev {bd} st_ino {bi}; expected the same file"));
                    }
                }
            }
            for ((what, _, want), k) in refusals.iter().zip(&ks) {
                r.expect_res(what, res(&out, *k), *want);
            }
        }
        Err(why) => r.fail(format_args!("{SYSCALL_ID}: {why}")),
    }
    // The table after: what it read before, then the four mounts that
    // succeeded, in order, a space in a mount point written \040; the
    // refused ones add nothing. What it read before holds the four mounts
    // this file made, so a table that lists only its newest mounts fails.
    match (before, table()) {
        (Ok(b), Ok(a)) => {
            let floor: Vec<String> = MOUNTS.iter().map(|(s, t, f)| format!("{} {} {} rw 0 0", esc(s), esc(t), esc(f))).collect();
            let absent: Vec<&String> = floor.iter().filter(|l| !b.lines().any(|x| x == l.as_str())).collect();
            if !absent.is_empty() {
                r.fail(format_args!("{SYSCALL_ID}: /proc/mounts read {b:?} before these mount calls; expected the four mounts already made among its lines (missing {absent:?})"));
            }
            let mut want: Vec<String> = b.lines().map(String::from).collect();
            want.push(format!("proc {} proc rw 0 0", esc(PROC_AT)));
            want.push(format!("devfs {} devfs rw 0 0", esc(DEV_AT)));
            for at in &more {
                want.push(format!("devfs {} devfs rw 0 0", esc(at)));
            }
            let got: Vec<&str> = a.lines().collect();
            if got != want {
                r.fail(format_args!("{SYSCALL_ID}: /proc/mounts read {b:?} before these mount calls and {a:?} after; expected the lines before, then {:?}", &want[want.len() - 4..]));
            }
        }
        (Err(e), _) | (_, Err(e)) => r.fail(format_args!("{SYSCALL_ID}: /proc's mounts around these mount calls: lookup then read_at returned {}", errno_text(e))),
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
        // The pids the kernel has before ls starts and after it ends: each
        // one in both lists must be in the listing, and no other number but
        // ls's own.
        let mut before = Vec::new();
        quiet(|| proc::each(|p| before.push(format!("{}", p.pid.0))));
        let started = start_with("ls /proc", || exec::spawn("/bin/ls", &["ls", "/proc"], &[]), &[(1, &lw), (2, &lw)]);
        drop(lw);
        match started {
            Ok(pid) => {
                let ran = drain("ls /proc", &lr, pid);
                let mut pids = Vec::new();
                quiet(|| proc::each(|p| pids.push(format!("{}", p.pid.0))));
                let text = String::from_utf8_lossy(&ran.out);
                let names: Vec<&str> = text.lines().collect();
                // procfs's four names must all be there; other names
                // (`self`, as Linux lists it) may be too.
                let absent: Vec<&str> = ["meminfo", "mounts", "sboot", "uptime"].into_iter().filter(|w| !names.contains(w)).collect();
                let numbers = names.iter().filter(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())).count();
                let own = format!("{}", pid.0);
                let stray: Vec<&str> = names
                    .iter()
                    .copied()
                    .filter(|n| n.bytes().all(|c| c.is_ascii_digit()) && *n != own && !pids.iter().chain(&before).any(|p| p == n))
                    .collect();
                let missing: Vec<&str> = before.iter().filter(|b| pids.contains(b) && !names.contains(&b.as_str())).map(|b| b.as_str()).collect();
                if ran.exit != Some(0) || !absent.is_empty() || !names.contains(&own.as_str()) || !stray.is_empty() || !missing.is_empty() {
                    r.fail(format_args!(
                        "{CROSS_ID}: ls /proc printed {text:?} and ended {}; expected meminfo, mounts, sboot, uptime (missing {absent:?}) and one directory per pid the kernel has (ls's own, {own}, among them; missing {missing:?}, no such pid {stray:?}) — getdents64 on /proc lists procfs's, not ext2's empty /proc",
                        status_text(ran.exit)
                    ));
                } else {
                    note(format_args!("{CROSS_ID}: ls /proc: procfs's four names, {numbers} pid directories and {} other names", names.len() - 4 - numbers));
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
    sched::yield_now();
    quiet(ktap::end);
    halt()
}
