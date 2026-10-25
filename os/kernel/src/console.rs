//! Lab 09 — the console, and the file-descriptor table. This file is yours.
//!
//! Three layers, Linux's: the fd table (32 slots of `Option<Fd>`), the open
//! file (`OpenFile`: the object, the offset, the flags — shared by fds that
//! share it), and the object (`FileObj`; in lab 09 only the console). There
//! is no heap until lab 10, so an open file lives in a fixed pool and a
//! `FileRef` is a pool index with a hand-kept reference count; lab 10 makes
//! it an `Arc<OpenFile>`. fds 0, 1 and 2 are the console when a program
//! starts.
//!
//! Fill in the bodies; keep the signatures, the types and the constants —
//! later labs and the course's tests use them.

use core::ops::Deref;
use core::sync::atomic::{AtomicU32, AtomicU64};

use crate::sync::SpinLock;
use crate::syscall::errno::{EBADF, ENOTTY};
use crate::syscall::Errno;

/// `ioctl` request: the terminal's size, into a `struct winsize`.
pub const TIOCGWINSZ: u64 = 0x5413;
/// Lab 33: `ioctl` requests: the console's termios, read and replaced.
pub const TCGETS: u64 = 0x5401;
pub const TCSETS: u64 = 0x5402;
/// Lab 33: the bits of a termios this console honours, and the `c_cc`
/// slots it reads (Linux's numbers, asm-generic/termbits.h).
pub mod tc {
    pub const ICRNL: u32 = 0o400;
    pub const ISIG: u32 = 0o1;
    pub const ICANON: u32 = 0o2;
    pub const ECHO: u32 = 0o10;
    pub const VTIME: usize = 5;
    pub const VMIN: usize = 6;
    /// Linux's cooked defaults (`tty_std_termios`): `c_iflag` ICRNL|IXON,
    /// `c_oflag` OPOST|ONLCR, `c_cflag` B38400|CS8|CREAD|HUPCL, `c_lflag`
    /// ISIG|ICANON|ECHO|ECHOE|ECHOK|ECHOCTL|ECHOKE|IEXTEN, `c_line` 0, and
    /// `c_cc` INIT_C_CC (^C, ^\\, DEL, ^U, ^D, VTIME 0, VMIN 1, …), as the
    /// 36 bytes `TCGETS` writes.
    pub const COOKED: [u8; 36] = [
        0x00, 0x05, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0xbf, 0x04, 0x00, 0x00, 0x3b, 0x8a, 0x00, 0x00, 0x00, 0x03, 0x1c, 0x7f,
        0x15, 0x04, 0x00, 0x01, 0x00, 0x11, 0x13, 0x1a, 0x00, 0x12, 0x0f, 0x17, 0x16, 0x00, 0x00, 0x00,
    ];
}
/// The console's size, as `TIOCGWINSZ` reports it.
pub const ROWS: u16 = 25;
pub const COLS: u16 = 80;
/// Slots in an fd table.
pub const NFD: usize = 32;

/// What an object behind a file descriptor can be asked. A method an object
/// has no meaning for keeps the default refusal.
pub trait File: Sync {
    fn read(&self, _of: &OpenFile, _buf: &mut [u8]) -> Result<usize, Errno> {
        Err(EBADF)
    }
    fn write(&self, _of: &OpenFile, _buf: &[u8]) -> Result<usize, Errno> {
        Err(EBADF)
    }
    fn ioctl(&self, _of: &OpenFile, _req: u64, _arg: u64) -> Result<i64, Errno> {
        Err(ENOTTY)
    }
}

/// The console: the serial line, as a file.
pub struct Console {
    lock: SpinLock<()>,
}

pub static CONSOLE: Console = Console { lock: SpinLock::new(()) };

impl File for Console {
    fn read(&self, _of: &OpenFile, _buf: &mut [u8]) -> Result<usize, Errno> {
        todo!("lab 09 step 3 — Console::read: end of file until lab 23")
    }
    fn write(&self, _of: &OpenFile, buf: &[u8]) -> Result<usize, Errno> {
        let _ = (buf, &self.lock);
        todo!("lab 09 step 3 — Console::write: under the lock, every byte to the serial line")
    }
    fn ioctl(&self, _of: &OpenFile, req: u64, arg: u64) -> Result<i64, Errno> {
        let _ = (req, arg);
        todo!("lab 09 step 3 — Console::ioctl: TIOCGWINSZ writes 25 rows, 80 cols; anything else ENOTTY")
    }
}

/// The object an open file refers to. Opaque: in lab 09 a small index.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileObj(u32);

impl FileObj {
    pub const CONSOLE: FileObj = FileObj(0);
}

/// An open file description: what `open` makes and `dup` shares.
pub struct OpenFile {
    pub object: FileObj,
    pub offset: AtomicU64,
    pub flags: AtomicU32,
}

impl OpenFile {
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, Errno> {
        let _ = buf;
        todo!("lab 09 step 3 — OpenFile::read: ask the object")
    }
    pub fn write(&self, buf: &[u8]) -> Result<usize, Errno> {
        let _ = buf;
        todo!("lab 09 step 3 — OpenFile::write: ask the object")
    }
    pub fn ioctl(&self, req: u64, arg: u64) -> Result<i64, Errno> {
        let _ = (req, arg);
        todo!("lab 09 step 3 — OpenFile::ioctl: ask the object")
    }
}

/// A counted reference to an open file. `clone` adds a reference; dropping
/// the last one frees the open file.
pub struct FileRef(u16);

impl FileRef {
    /// A new open file on `object` with `flags`, holding one reference.
    pub fn open(object: FileObj, flags: u32) -> Result<FileRef, Errno> {
        let _ = (object, flags);
        todo!("lab 09 step 3 — FileRef::open: claim a free pool slot (count 0 -> 1), fill it")
    }
    /// How many references this open file has.
    pub fn refs(&self) -> u32 {
        let _ = self.0;
        todo!("lab 09 step 3 — FileRef::refs: the slot's count")
    }
}

impl Clone for FileRef {
    fn clone(&self) -> FileRef {
        todo!("lab 09 step 3 — FileRef::clone: one more reference")
    }
}

impl Drop for FileRef {
    fn drop(&mut self) {
        todo!("lab 09 step 3 — FileRef::drop: one reference fewer")
    }
}

impl Deref for FileRef {
    type Target = OpenFile;
    fn deref(&self) -> &OpenFile {
        todo!("lab 09 step 3 — FileRef::deref: the open file in the slot")
    }
}

/// One fd-table slot's contents.
#[derive(Clone)]
pub struct Fd {
    pub file: FileRef,
    pub cloexec: bool,
}

/// A process's file descriptors.
pub struct FdTable {
    slots: [Option<Fd>; NFD],
}

impl FdTable {
    pub const fn new() -> FdTable {
        FdTable { slots: [const { None }; NFD] }
    }
    /// The open file behind `fd` (a new reference), or `EBADF`.
    pub fn get(&self, fd: u64) -> Result<FileRef, Errno> {
        let _ = (fd, &self.slots);
        todo!("lab 09 step 3 — FdTable::get")
    }
    /// Put `d` in the lowest free slot; returns its number.
    pub fn alloc(&mut self, d: Fd) -> Result<usize, Errno> {
        let _ = d;
        todo!("lab 09 step 3 — FdTable::alloc: the lowest free slot, or EMFILE")
    }
    /// Put `d` in slot `n`, returning what was there.
    pub fn install(&mut self, n: usize, d: Fd) -> Result<Option<Fd>, Errno> {
        let _ = (n, d);
        todo!("lab 09 step 3 — FdTable::install")
    }
    pub fn close(&mut self, fd: u64) -> Result<(), Errno> {
        let _ = fd;
        todo!("lab 09 step 3 — FdTable::close: EBADF for a slot that is empty")
    }
    /// Close every fd.
    pub fn clear(&mut self) {
        todo!("lab 09 step 3 — FdTable::clear")
    }
    /// fds 0, 1 and 2 on one open console (read-write).
    pub fn open_std(&mut self) -> Result<(), Errno> {
        todo!("lab 09 step 3 — FdTable::open_std: one console open file, in slots 0, 1 and 2")
    }
}

impl Default for FdTable {
    fn default() -> FdTable {
        FdTable::new()
    }
}

/// Run `f` on the running program's fd table.
pub fn with_fds<R>(f: impl FnOnce(&mut FdTable) -> R) -> R {
    let _ = f;
    todo!("lab 09 step 3 — with_fds: one table behind a SpinLock, for now")
}

/// Lab 32 step 5: ctrl+c, from the keyboard interrupt. Every process
/// holding the console open but init (pid 1) is sent SIGINT, the input
/// waiting is thrown away and `^C` is echoed; `true`. With no process
/// holding the console, nothing, and `false` (ctrl+c is then a key).
pub fn interrupt() -> bool {
    todo!("lab 32 step 5 — interrupt: the input discarded, SIGINT to every process with the console open but pid 1")
}

impl Console {
    /// Lab 33 step 1: the termios `TCGETS` writes — Linux's 36-byte
    /// `struct termios` (`c_iflag`, `c_oflag`, `c_cflag`, `c_lflag`,
    /// `c_line`, `c_cc[19]`), `tc::COOKED` until a `TCSETS` replaced it.
    pub fn tcgets(&self) -> [u8; 36] {
        todo!("lab 33 step 1 — Console::tcgets: the console's termios, Linux's 36 bytes, cooked until TCSETS changed it")
    }

    /// Lab 33 step 2: `TCSETS` — the console's termios is now `t`, for
    /// every open file on it, from the next key read.
    pub fn tcsets(&self, t: &[u8; 36]) {
        let _ = t;
        todo!("lab 33 step 2 — Console::tcsets: the 36 bytes are the console's termios now")
    }

    /// Lab 33 step 3: `read` with `ICANON` off — keys as they come, no line
    /// and no editing: until `VMIN` bytes are there (what there is when it
    /// is 0), CR made NL under `ICRNL`, echoed under `ECHO`; keys typed
    /// beyond what the read takes stay in the ring.
    pub fn read_raw(&self, of: &OpenFile, buf: &mut [u8]) -> Result<usize, Errno> {
        let _ = (of, buf);
        todo!("lab 33 step 3 — Console::read_raw: keys as they come, until VMIN bytes; ICRNL, ECHO; the rest stays in the ring")
    }
}

impl OpenFile {
    /// Lab 33 step 6: `ftruncate` (77) — the file is now `len` bytes, cut
    /// or grown with zeros; only a regular file opened for writing,
    /// `-EINVAL` for anything else.
    pub fn truncate(&self, len: u64) -> Result<(), Errno> {
        let _ = len;
        todo!("lab 33 step 6 — OpenFile::truncate: a regular file open for writing is len bytes now; anything else EINVAL")
    }
}
