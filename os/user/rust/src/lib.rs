//! `ulib` — the course's user-mode Rust crate (CONTRACT §A.8; LADDER §1).
//!
//! Course-owned: the user-mode twin of `user/sys.h` and `user/crt0.c`, for the
//! programs you write in `userland/` (lab 28's `init.rs`, lab 29's `sh.rs`).
//! No allocator, no SSE, no libc: everything here is a syscall wrapper, a
//! formatter over a stack buffer, or a pure function.
//!
//! A program on this crate starts like this:
//!
//! ```ignore
//! #![no_std]
//! #![no_main]
//! use ulib::*;
//!
//! entry!(main);
//!
//! fn main(args: Args) -> i32 {
//!     println!("hello from {}", args.str(0));
//!     0
//! }
//! ```
//!
//! `main`'s return value is the exit code. A panic (a `todo!()` included)
//! prints `<argv[0]>: panicked at <file>:<line>:<col>: <message>` on fd 2 and
//! exits with 101, the code a Rust program on Linux exits with.
//!
//! Every wrapper returns `Result<_, Errno>`: the kernel's `-errno` becomes
//! `Err(Errno(n))`, whose `Display` is the name (`ENOENT`) the course's C
//! programs print. Each syscall number carries, in a comment, the lab whose
//! kernel arms it (§A.4); before that lab it answers `-ENOSYS` (`Errno::ENOSYS`).
#![no_std]

use core::arch::asm;
use core::ffi::{c_char, CStr};
use core::fmt;
use core::sync::atomic::{AtomicPtr, Ordering};

// ---- syscall numbers: x86-64 Linux, = CONTRACT §A.4 (the lab that arms each) ----

/// The syscall numbers this crate wraps (Linux's, which are §A.4's), each with
/// the lab whose kernel arms it. Never renumbered; later programs add rows.
pub mod nr {
    pub const READ: usize = 0; // 09: the console returns 0 (EOF) until 23; pipes 15; files 17
    pub const WRITE: usize = 1; // 09
    pub const OPEN: usize = 2; // 17: read flags; 18 adds the write side
    pub const CLOSE: usize = 3; // 09
    pub const STAT: usize = 4; // 17
    pub const FSTAT: usize = 5; // 17
    pub const LSTAT: usize = 6; // 30
    pub const LSEEK: usize = 8; // 17
    pub const MMAP: usize = 9; // 25
    pub const MUNMAP: usize = 11; // 25
    pub const BRK: usize = 12; // 25: answers the current break; never grows (§A.4 row 12)
    pub const RT_SIGACTION: usize = 13; // 25: stored; acted on from 31, handlers run from 32
    pub const RT_SIGPROCMASK: usize = 14; // 25
    pub const IOCTL: usize = 16; // 09: TIOCGWINSZ; TCGETS/TCSETS at 33
    pub const SCHED_YIELD: usize = 24; // 11 (armed at 14)
    pub const DUP: usize = 32; // 15
    pub const DUP2: usize = 33; // 15
    pub const NANOSLEEP: usize = 35; // 25
    pub const GETPID: usize = 39; // 09: the process's pid from 12
    pub const FORK: usize = 57; // 14
    pub const EXECVE: usize = 59; // 13
    pub const EXIT: usize = 60; // 09
    pub const WAIT4: usize = 61; // 14
    pub const KILL: usize = 62; // 31
    pub const FCNTL: usize = 72; // 15
    pub const GETCWD: usize = 79; // 23
    pub const CHDIR: usize = 80; // 23
    pub const RENAME: usize = 82; // 20
    pub const MKDIR: usize = 83; // 20
    pub const RMDIR: usize = 84; // 20
    pub const UNLINK: usize = 87; // 19
    pub const GETPPID: usize = 110; // 14
    pub const MOUNT: usize = 165; // 22: "proc" and "devfs" only
    pub const GETDENTS64: usize = 217; // 20
    pub const EXIT_GROUP: usize = 231; // 13
    pub const OPENAT: usize = 257; // 17: AT_FDCWD only
    pub const DUP3: usize = 292; // 15
    pub const PIPE2: usize = 293; // 15
}

// ---- flags (Linux's values) ----

pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;
pub const O_CREAT: u32 = 0x40; // 18
pub const O_EXCL: u32 = 0x80; // 18
pub const O_TRUNC: u32 = 0x200; // 18
pub const O_APPEND: u32 = 0x400; // 18
pub const O_NONBLOCK: u32 = 0x800;
pub const O_DIRECTORY: u32 = 0x10000;
pub const O_CLOEXEC: u32 = 0x80000;
pub const AT_FDCWD: i32 = -100;
pub const SEEK_SET: u32 = 0;
pub const SEEK_CUR: u32 = 1;
pub const SEEK_END: u32 = 2;
pub const F_DUPFD: u32 = 0;
pub const F_GETFD: u32 = 1;
pub const F_SETFD: u32 = 2;
pub const F_GETFL: u32 = 3;
pub const F_SETFL: u32 = 4;
pub const F_DUPFD_CLOEXEC: u32 = 1030;
pub const FD_CLOEXEC: u32 = 1;
pub const WNOHANG: u32 = 1;
pub const PROT_READ: u32 = 1;
pub const PROT_WRITE: u32 = 2;
pub const MAP_PRIVATE: u32 = 0x02;
pub const MAP_ANONYMOUS: u32 = 0x20;
pub const SIG_BLOCK: u32 = 0;
pub const SIG_UNBLOCK: u32 = 1;
pub const SIG_SETMASK: u32 = 2;
pub const TIOCGWINSZ: u32 = 0x5413;
pub const STDIN: i32 = 0;
pub const STDOUT: i32 = 1;
pub const STDERR: i32 = 2;

// ---- errno ----

/// A refusal: the kernel returned `-errno`. `Display` prints the name the
/// course's C programs print (`ENOENT`), or `-N` for a number it has no name for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Errno(pub i32);

macro_rules! errnos {
    ($($name:ident = $n:expr),* $(,)?) => {
        impl Errno {
            $(pub const $name: Errno = Errno($n);)*
            /// The errno's name (`"ENOENT"`), if it is one the course's refusals use (§A.6).
            pub fn name(self) -> Option<&'static str> {
                match self.0 { $($n => Some(stringify!($name)),)* _ => None }
            }
        }
    };
}
errnos! {
    EPERM = 1, ENOENT = 2, ESRCH = 3, EINTR = 4, EIO = 5, E2BIG = 7, ENOEXEC = 8, EBADF = 9,
    ECHILD = 10, EAGAIN = 11, ENOMEM = 12, EACCES = 13, EFAULT = 14, EBUSY = 16, EEXIST = 17,
    EXDEV = 18, ENODEV = 19, ENOTDIR = 20, EISDIR = 21, EINVAL = 22, EMFILE = 24, ENOTTY = 25,
    ENOSPC = 28, ESPIPE = 29, EROFS = 30, EPIPE = 32, ERANGE = 34, ENAMETOOLONG = 36,
    ENOSYS = 38, ENOTEMPTY = 39, ELOOP = 40,
}

impl fmt::Display for Errno {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(n) => f.write_str(n),
            None => write!(f, "-{}", self.0),
        }
    }
}
impl fmt::Debug for Errno {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// What every wrapper returns.
pub type Result<T> = core::result::Result<T, Errno>;

/// A raw return value: `-4095..=-1` is `-errno`, anything else a result.
#[inline]
pub fn check(r: isize) -> Result<usize> {
    if (-4095..0).contains(&r) {
        Err(Errno(-r as i32))
    } else {
        Ok(r as usize)
    }
}

// ---- the raw syscall instruction ----
// Number in rax; arguments in rdi rsi rdx r10 r8 r9; result in rax; the
// `syscall` instruction clobbers rcx (the return rip) and r11 (rflags).

/// # Safety
/// The kernel reads and writes whatever memory the arguments point at.
#[inline(always)]
pub unsafe fn syscall0(n: usize) -> isize {
    let r: isize;
    asm!("syscall", inlateout("rax") n as isize => r, lateout("rcx") _, lateout("r11") _, options(nostack));
    r
}
/// # Safety
/// As [`syscall0`].
#[inline(always)]
pub unsafe fn syscall1(n: usize, a1: usize) -> isize {
    let r: isize;
    asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a1,
         lateout("rcx") _, lateout("r11") _, options(nostack));
    r
}
/// # Safety
/// As [`syscall0`].
#[inline(always)]
pub unsafe fn syscall2(n: usize, a1: usize, a2: usize) -> isize {
    let r: isize;
    asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a1, in("rsi") a2,
         lateout("rcx") _, lateout("r11") _, options(nostack));
    r
}
/// # Safety
/// As [`syscall0`].
#[inline(always)]
pub unsafe fn syscall3(n: usize, a1: usize, a2: usize, a3: usize) -> isize {
    let r: isize;
    asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a1, in("rsi") a2, in("rdx") a3,
         lateout("rcx") _, lateout("r11") _, options(nostack));
    r
}
/// # Safety
/// As [`syscall0`].
#[inline(always)]
pub unsafe fn syscall4(n: usize, a1: usize, a2: usize, a3: usize, a4: usize) -> isize {
    let r: isize;
    asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a1, in("rsi") a2, in("rdx") a3,
         in("r10") a4, lateout("rcx") _, lateout("r11") _, options(nostack));
    r
}
/// # Safety
/// As [`syscall0`].
#[inline(always)]
pub unsafe fn syscall5(n: usize, a1: usize, a2: usize, a3: usize, a4: usize, a5: usize) -> isize {
    let r: isize;
    asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a1, in("rsi") a2, in("rdx") a3,
         in("r10") a4, in("r8") a5, lateout("rcx") _, lateout("r11") _, options(nostack));
    r
}
/// # Safety
/// As [`syscall0`].
#[inline(always)]
pub unsafe fn syscall6(n: usize, a1: usize, a2: usize, a3: usize, a4: usize, a5: usize, a6: usize) -> isize {
    let r: isize;
    asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a1, in("rsi") a2, in("rdx") a3,
         in("r10") a4, in("r8") a5, in("r9") a6, lateout("rcx") _, lateout("r11") _, options(nostack));
    r
}

fn p(s: &CStr) -> usize {
    s.as_ptr() as usize
}

// ---- files and fds ----

/// `read(fd, buf)` (0): the bytes read; 0 at the end.
pub fn read(fd: i32, buf: &mut [u8]) -> Result<usize> {
    check(unsafe { syscall3(nr::READ, fd as usize, buf.as_mut_ptr() as usize, buf.len()) })
}
/// `write(fd, buf)` (1): the bytes written (maybe fewer than asked).
pub fn write(fd: i32, buf: &[u8]) -> Result<usize> {
    check(unsafe { syscall3(nr::WRITE, fd as usize, buf.as_ptr() as usize, buf.len()) })
}
/// Writes all of `buf`, looping over short writes; the first refusal ends it.
pub fn write_all(fd: i32, mut buf: &[u8]) -> Result<()> {
    while !buf.is_empty() {
        match write(fd, buf)? {
            0 => return Err(Errno::EIO),
            n => buf = &buf[n..],
        }
    }
    Ok(())
}
/// `open(path, flags, mode)` (2): the new fd.
pub fn open(path: &CStr, flags: u32, mode: u32) -> Result<i32> {
    check(unsafe { syscall3(nr::OPEN, p(path), flags as usize, mode as usize) }).map(|fd| fd as i32)
}
/// `openat(dirfd, path, flags, mode)` (257): `AT_FDCWD` is the only `dirfd` the course serves.
pub fn openat(dirfd: i32, path: &CStr, flags: u32, mode: u32) -> Result<i32> {
    check(unsafe { syscall4(nr::OPENAT, dirfd as isize as usize, p(path), flags as usize, mode as usize) })
        .map(|fd| fd as i32)
}
/// `close(fd)` (3).
pub fn close(fd: i32) -> Result<()> {
    check(unsafe { syscall1(nr::CLOSE, fd as usize) }).map(drop)
}

/// x86-64 Linux's `struct stat` (§A.4: 144 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Stat {
    pub dev: u64,
    pub ino: u64,
    pub nlink: u64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    _pad0: u32,
    pub rdev: u64,
    pub size: i64,
    pub blksize: i64,
    pub blocks: i64,
    pub atime: u64,
    pub atime_nsec: u64,
    pub mtime: u64,
    pub mtime_nsec: u64,
    pub ctime: u64,
    pub ctime_nsec: u64,
    _unused: [i64; 3],
}
const _: () = assert!(core::mem::size_of::<Stat>() == 144);
pub const S_IFMT: u32 = 0o170000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFREG: u32 = 0o100000;
impl Stat {
    pub fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }
}

/// `stat(path)` (4).
pub fn stat(path: &CStr) -> Result<Stat> {
    let mut st = Stat::default();
    check(unsafe { syscall2(nr::STAT, p(path), &mut st as *mut Stat as usize) }).map(|_| st)
}
/// `fstat(fd)` (5).
pub fn fstat(fd: i32) -> Result<Stat> {
    let mut st = Stat::default();
    check(unsafe { syscall2(nr::FSTAT, fd as usize, &mut st as *mut Stat as usize) }).map(|_| st)
}
/// `lstat(path)` (6): `stat` without following a final symlink (there are none).
pub fn lstat(path: &CStr) -> Result<Stat> {
    let mut st = Stat::default();
    check(unsafe { syscall2(nr::LSTAT, p(path), &mut st as *mut Stat as usize) }).map(|_| st)
}
/// `lseek(fd, off, whence)` (8): the new offset.
pub fn lseek(fd: i32, off: i64, whence: u32) -> Result<u64> {
    check(unsafe { syscall3(nr::LSEEK, fd as usize, off as usize, whence as usize) }).map(|o| o as u64)
}
/// `getdents64(fd, buf)` (217): bytes of `linux_dirent64` records; 0 at the end.
pub fn getdents64(fd: i32, buf: &mut [u8]) -> Result<usize> {
    check(unsafe { syscall3(nr::GETDENTS64, fd as usize, buf.as_mut_ptr() as usize, buf.len()) })
}
/// `pipe2(flags)` (293): `[read end, write end]`.
pub fn pipe2(flags: u32) -> Result<[i32; 2]> {
    let mut fds = [0i32; 2];
    check(unsafe { syscall2(nr::PIPE2, fds.as_mut_ptr() as usize, flags as usize) }).map(|_| fds)
}
/// `dup(fd)` (32).
pub fn dup(fd: i32) -> Result<i32> {
    check(unsafe { syscall1(nr::DUP, fd as usize) }).map(|fd| fd as i32)
}
/// `dup2(old, new)` (33).
pub fn dup2(old: i32, new: i32) -> Result<i32> {
    check(unsafe { syscall2(nr::DUP2, old as usize, new as usize) }).map(|fd| fd as i32)
}
/// `dup3(old, new, flags)` (292): `flags` is 0 or `O_CLOEXEC`.
pub fn dup3(old: i32, new: i32, flags: u32) -> Result<i32> {
    check(unsafe { syscall3(nr::DUP3, old as usize, new as usize, flags as usize) }).map(|fd| fd as i32)
}
/// `fcntl(fd, cmd, arg)` (72).
pub fn fcntl(fd: i32, cmd: u32, arg: usize) -> Result<usize> {
    check(unsafe { syscall3(nr::FCNTL, fd as usize, cmd as usize, arg) })
}
/// `ioctl(fd, request, arg)` (16).
///
/// # Safety
/// `arg` is whatever the request reads or writes.
pub unsafe fn ioctl(fd: i32, request: u32, arg: usize) -> Result<usize> {
    check(syscall3(nr::IOCTL, fd as usize, request as usize, arg))
}

/// `struct winsize`, `TIOCGWINSZ`'s answer.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Winsize {
    pub rows: u16,
    pub cols: u16,
    pub xpixel: u16,
    pub ypixel: u16,
}
/// The console's size (`ioctl(fd, TIOCGWINSZ)`).
pub fn winsize(fd: i32) -> Result<Winsize> {
    let mut w = Winsize::default();
    unsafe { ioctl(fd, TIOCGWINSZ, &mut w as *mut Winsize as usize) }.map(|_| w)
}

// ---- directories ----

/// `chdir(path)` (80).
pub fn chdir(path: &CStr) -> Result<()> {
    check(unsafe { syscall1(nr::CHDIR, p(path)) }).map(drop)
}
/// `getcwd(buf)` (79): the path, its NUL not included.
pub fn getcwd(buf: &mut [u8]) -> Result<&str> {
    let n = check(unsafe { syscall2(nr::GETCWD, buf.as_mut_ptr() as usize, buf.len()) })?;
    let path = &buf[..n.saturating_sub(1)];
    core::str::from_utf8(path).map_err(|_| Errno::EINVAL)
}
/// `mkdir(path, mode)` (83).
pub fn mkdir(path: &CStr, mode: u32) -> Result<()> {
    check(unsafe { syscall2(nr::MKDIR, p(path), mode as usize) }).map(drop)
}
/// `rmdir(path)` (84).
pub fn rmdir(path: &CStr) -> Result<()> {
    check(unsafe { syscall1(nr::RMDIR, p(path)) }).map(drop)
}
/// `unlink(path)` (87).
pub fn unlink(path: &CStr) -> Result<()> {
    check(unsafe { syscall1(nr::UNLINK, p(path)) }).map(drop)
}
/// `rename(from, to)` (82).
pub fn rename(from: &CStr, to: &CStr) -> Result<()> {
    check(unsafe { syscall2(nr::RENAME, p(from), p(to)) }).map(drop)
}
/// `mount(source, target, fstype, flags, data)` (165): the course's kernel makes
/// `"proc"` and `"devfs"`; `flags` and `data` are ignored.
pub fn mount(source: &CStr, target: &CStr, fstype: &CStr) -> Result<()> {
    check(unsafe { syscall5(nr::MOUNT, p(source), p(target), p(fstype), 0, 0) }).map(drop)
}

// ---- processes ----

/// `getpid()` (39).
pub fn getpid() -> i32 {
    unsafe { syscall0(nr::GETPID) as i32 }
}
/// `getppid()` (110): 0 for a process the kernel started.
pub fn getppid() -> i32 {
    unsafe { syscall0(nr::GETPPID) as i32 }
}
/// `sched_yield()` (24).
pub fn sched_yield() {
    unsafe { syscall0(nr::SCHED_YIELD) };
}
/// `fork()` (57): `Ok(0)` in the child, `Ok(child's pid)` in the parent.
pub fn fork() -> Result<i32> {
    check(unsafe { syscall0(nr::FORK) }).map(|pid| pid as i32)
}

/// The most argv or envp entries [`execve`] passes.
pub const EXEC_MAX_ARGS: usize = 32;

/// `execve(path, argv, envp)` (59). Returns only on failure, with the refusal.
pub fn execve(path: &CStr, argv: &[&CStr], envp: &[&CStr]) -> Errno {
    if argv.len() >= EXEC_MAX_ARGS || envp.len() >= EXEC_MAX_ARGS {
        return Errno::E2BIG;
    }
    let mut a = [core::ptr::null::<c_char>(); EXEC_MAX_ARGS];
    let mut e = [core::ptr::null::<c_char>(); EXEC_MAX_ARGS];
    for (slot, s) in a.iter_mut().zip(argv) {
        *slot = s.as_ptr();
    }
    for (slot, s) in e.iter_mut().zip(envp) {
        *slot = s.as_ptr();
    }
    let r = unsafe { syscall3(nr::EXECVE, p(path), a.as_ptr() as usize, e.as_ptr() as usize) };
    match check(r) {
        Err(e) => e,
        Ok(_) => Errno::EINVAL, // execve never returns success
    }
}
/// `exit(code)` (60): ends this thread — this process, since every course process has one.
pub fn exit(code: i32) -> ! {
    loop {
        unsafe { syscall1(nr::EXIT, code as usize) };
    }
}
/// `exit_group(code)` (231): ends the process.
pub fn exit_group(code: i32) -> ! {
    loop {
        unsafe { syscall1(nr::EXIT_GROUP, code as usize) };
    }
}

/// A `wait4` status, Linux's encoding (§A.6): an exit is `code << 8`, a kill
/// is the signal in the low 7 bits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WaitStatus(pub i32);
impl WaitStatus {
    /// The exit code, if the child exited.
    pub fn code(self) -> Option<i32> {
        (self.0 & 0x7f == 0).then_some((self.0 >> 8) & 0xff)
    }
    /// The signal that killed the child, if one did.
    pub fn signal(self) -> Option<i32> {
        let s = self.0 & 0x7f;
        (s != 0 && s != 0x7f).then_some(s)
    }
}
impl fmt::Display for WaitStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.code(), self.signal()) {
            (Some(c), _) => write!(f, "exit {}", c),
            (_, Some(s)) => write!(f, "signal {}", s),
            _ => write!(f, "status {:#x}", self.0),
        }
    }
}
/// `wait4(pid, &status, options, NULL)` (61): `pid` -1 (or any `pid <= 0`) is any
/// child. `Ok((pid, status))`; `Ok((0, _))` under `WNOHANG` when no child has
/// changed yet; `Err(ECHILD)` when there is no child to wait for.
pub fn wait4(pid: i32, options: u32) -> Result<(i32, WaitStatus)> {
    let mut status: i32 = 0;
    let r = unsafe { syscall4(nr::WAIT4, pid as isize as usize, &mut status as *mut i32 as usize, options as usize, 0) };
    check(r).map(|pid| (pid as i32, WaitStatus(status)))
}
/// `kill(pid, sig)` (62).
pub fn kill(pid: i32, sig: i32) -> Result<()> {
    check(unsafe { syscall2(nr::KILL, pid as isize as usize, sig as usize) }).map(drop)
}

// ---- memory ----

/// `mmap(addr, len, prot, flags, fd, off)` (9): the mapping's address.
///
/// # Safety
/// A mapping at a hint or over a file is the caller's to use soundly.
pub unsafe fn mmap(addr: usize, len: usize, prot: u32, flags: u32, fd: i32, off: u64) -> Result<*mut u8> {
    check(syscall6(nr::MMAP, addr, len, prot as usize, flags as usize, fd as isize as usize, off as usize))
        .map(|a| a as *mut u8)
}
/// `munmap(addr, len)` (11).
///
/// # Safety
/// Nothing may use the range afterwards.
pub unsafe fn munmap(addr: *mut u8, len: usize) -> Result<()> {
    check(syscall2(nr::MUNMAP, addr as usize, len)).map(drop)
}
/// `brk(addr)` (12): the break after the call (unchanged when refused, as on Linux).
pub fn brk(addr: usize) -> usize {
    unsafe { syscall1(nr::BRK, addr) as usize }
}

// ---- time and signals ----

/// `struct timespec`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Timespec {
    pub sec: i64,
    pub nsec: i64,
}
/// `nanosleep(req, rem)` (35).
pub fn nanosleep(req: &Timespec, rem: Option<&mut Timespec>) -> Result<()> {
    let rem = rem.map_or(0, |r| r as *mut Timespec as usize);
    check(unsafe { syscall2(nr::NANOSLEEP, req as *const Timespec as usize, rem) }).map(drop)
}
/// Sleeps `ms` milliseconds.
pub fn sleep_ms(ms: u64) -> Result<()> {
    nanosleep(&Timespec { sec: (ms / 1000) as i64, nsec: ((ms % 1000) * 1_000_000) as i64 }, None)
}

/// The kernel's `struct sigaction` (x86-64): handler, flags, restorer, mask.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct SigAction {
    pub handler: usize,
    pub flags: u64,
    pub restorer: usize,
    pub mask: u64,
}
pub const SIG_DFL: usize = 0;
pub const SIG_IGN: usize = 1;
/// `rt_sigaction(sig, act, old, 8)` (13).
pub fn rt_sigaction(sig: i32, act: Option<&SigAction>, old: Option<&mut SigAction>) -> Result<()> {
    let act = act.map_or(0, |a| a as *const SigAction as usize);
    let old = old.map_or(0, |o| o as *mut SigAction as usize);
    check(unsafe { syscall4(nr::RT_SIGACTION, sig as usize, act, old, 8) }).map(drop)
}
/// `rt_sigprocmask(how, set, old, 8)` (14).
pub fn rt_sigprocmask(how: u32, set: Option<&u64>, old: Option<&mut u64>) -> Result<()> {
    let set = set.map_or(0, |s| s as *const u64 as usize);
    let old = old.map_or(0, |o| o as *mut u64 as usize);
    check(unsafe { syscall4(nr::RT_SIGPROCMASK, how as usize, set, old, 8) }).map(drop)
}

// ---- output: print! and friends ----

/// A line buffer on the stack that writes to one fd: what `print!` formats
/// into, so a line up to 256 bytes leaves in ONE `write` and reaches the
/// console whole even when another process writes at the same time.
pub struct Out {
    fd: i32,
    n: usize,
    buf: [u8; 256],
}
impl Out {
    pub const fn new(fd: i32) -> Out {
        Out { fd, n: 0, buf: [0; 256] }
    }
    /// Sends what the buffer holds; a refusal is dropped (there is nowhere to report it).
    pub fn flush(&mut self) {
        let _ = write_all(self.fd, &self.buf[..self.n]);
        self.n = 0;
    }
}
impl fmt::Write for Out {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &b in s.as_bytes() {
            if self.n == self.buf.len() {
                self.flush();
            }
            self.buf[self.n] = b;
            self.n += 1;
        }
        Ok(())
    }
}
impl Drop for Out {
    fn drop(&mut self) {
        self.flush();
    }
}

#[doc(hidden)]
pub fn _print(fd: i32, args: fmt::Arguments<'_>) {
    let mut out = Out::new(fd);
    let _ = fmt::Write::write_fmt(&mut out, args);
}

/// `print!` to fd 1, in one `write` when it fits 256 bytes.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => { $crate::_print(1, format_args!($($arg)*)) };
}
/// `println!` to fd 1: the line and its newline in one `write`.
#[macro_export]
macro_rules! println {
    () => { $crate::_print(1, format_args!("\n")) };
    ($($arg:tt)*) => { $crate::_print(1, format_args!("{}\n", format_args!($($arg)*))) };
}
/// `eprint!` to fd 2.
#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => { $crate::_print(2, format_args!($($arg)*)) };
}
/// `eprintln!` to fd 2.
#[macro_export]
macro_rules! eprintln {
    () => { $crate::_print(2, format_args!("\n")) };
    ($($arg:tt)*) => { $crate::_print(2, format_args!("{}\n", format_args!($($arg)*))) };
}

// ---- derive (CONTRACT §A.2) ----
// derive(nonce, id) = splitmix64(lo ^ hi ^ fnv1a64(id)): the nonce is 32 hex
// characters = 16 bytes, lo/hi are bytes 0..8 and 8..16 read little-endian.
// The same function as sys.h's derive() and the cockpit's ktap::derive.

fn hexval(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}
/// True if `s` is exactly 32 hex characters (the nonce's only shape).
pub fn nonce_valid(s: &[u8]) -> bool {
    s.len() == 32 && s.iter().all(|&c| hexval(c).is_some())
}
pub fn fnv1a64(s: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in s {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}
pub fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
/// §A.2's derive. Check [`nonce_valid`] first: as in sys.h, a bad digit, or a
/// nonce that ends early, counts as 0.
pub fn derive(nonce: &[u8], id: &str) -> u64 {
    let (mut lo, mut hi) = (0u64, 0u64);
    let mut k = 0;
    let mut next = || {
        let c = nonce.get(k).copied().unwrap_or(0);
        if c == 0 {
            return 0u64;
        }
        k += 1;
        hexval(c).unwrap_or(0) as u64
    };
    for i in 0..16 {
        let h = next();
        let l = next();
        let byte = (h << 4) | l;
        if i < 8 {
            lo |= byte << (8 * i);
        } else {
            hi |= byte << (8 * (i - 8));
        }
    }
    splitmix64(lo ^ hi ^ fnv1a64(id.as_bytes()))
}

// ---- the start: argc, argv, envp off the stack ----

/// The program's arguments and environment, as `_start` found them (§A.5: rsp
/// points at argc, then argv[0..argc], a NULL, envp[..], a NULL).
#[derive(Clone, Copy)]
pub struct Args {
    argc: usize,
    argv: *const *const c_char,
    envp: *const *const c_char,
}
impl Args {
    /// argc.
    pub fn len(&self) -> usize {
        self.argc
    }
    pub fn is_empty(&self) -> bool {
        self.argc == 0
    }
    /// argv[i], if there is one.
    pub fn get(&self, i: usize) -> Option<&'static CStr> {
        (i < self.argc).then(|| unsafe { CStr::from_ptr(*self.argv.add(i)) })
    }
    /// argv[i] as text: `""` when it is missing or not UTF-8.
    pub fn str(&self, i: usize) -> &'static str {
        self.get(i).and_then(|s| s.to_str().ok()).unwrap_or("")
    }
    /// argv[0..argc].
    pub fn iter(&self) -> impl Iterator<Item = &'static CStr> + '_ {
        (0..self.argc).filter_map(move |i| self.get(i))
    }
    /// envp, up to its NULL.
    pub fn env(&self) -> impl Iterator<Item = &'static CStr> + '_ {
        let mut i = 0;
        core::iter::from_fn(move || {
            let e = unsafe { *self.envp.add(i) };
            (!e.is_null()).then(|| {
                i += 1;
                unsafe { CStr::from_ptr(e) }
            })
        })
    }
}

static ARGV0: AtomicPtr<c_char> = AtomicPtr::new(core::ptr::null_mut());

/// What `entry!`'s `_start` calls: reads the stack, runs `main`, exits with its code.
///
/// # Safety
/// `sp` is the stack pointer `_start` was entered with.
#[doc(hidden)]
pub unsafe fn start(sp: *const usize, main: fn(Args) -> i32) -> ! {
    let argc = *sp;
    let argv = sp.add(1) as *const *const c_char;
    let envp = argv.add(argc + 1);
    if argc > 0 {
        ARGV0.store(*argv as *mut c_char, Ordering::Relaxed);
    }
    exit_group(main(Args { argc, argv, envp }))
}

/// Names `main` as the program's entry: defines `_start`, which clears rbp
/// (the end of a frame-pointer walk), hands the stack pointer on, re-aligns
/// rsp to 16 for the call (psABI), and never returns.
#[macro_export]
macro_rules! entry {
    ($main:path) => {
        unsafe extern "C" fn __ulib_start(sp: *const usize) -> ! {
            $crate::start(sp, $main)
        }
        core::arch::global_asm!(
            ".globl _start",
            ".type _start, @function",
            "_start:",
            "    xor ebp, ebp",
            "    mov rdi, rsp",
            "    and rsp, -16",
            "    call {start}",
            "    ud2",
            ".size _start, . - _start",
            start = sym __ulib_start,
        );
    };
}

// ---- panic: the message on fd 2, then exit_group(101) ----

/// The file a panic names, from `userland/` on (the path cargo hands rustc is
/// absolute, and the rest of it is where your clone happens to live).
fn short(file: &str) -> &str {
    match file.rfind("userland/") {
        Some(i) => &file[i..],
        None => file.rsplit('/').next().unwrap_or(file),
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    use core::fmt::Write;
    let me = ARGV0.load(Ordering::Relaxed);
    let me = if me.is_null() { "" } else { unsafe { CStr::from_ptr(me) }.to_str().unwrap_or("") };
    let me = me.rsplit('/').next().unwrap_or(me);
    let mut out = Out::new(STDERR);
    let _ = write!(out, "{}: panicked", if me.is_empty() { "?" } else { me });
    if let Some(at) = info.location() {
        let _ = write!(out, " at {}:{}:{}", short(at.file()), at.line(), at.column());
    }
    let _ = writeln!(out, ": {}", info.message());
    out.flush();
    exit_group(101)
}
