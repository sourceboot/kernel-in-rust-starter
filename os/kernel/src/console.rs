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
