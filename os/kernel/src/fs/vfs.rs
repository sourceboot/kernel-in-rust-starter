//! Lab 13 — the VFS: one trait every file system implements, a mount
//! table, and the path walk. This file is yours.
//!
//! An `Inode` is a file or a directory of some file system, behind a trait
//! object so the code above it never asks which one: the boot archive
//! (lab 13), ext2 (17–21), /proc and /dev (22). The trait is written whole
//! here, every method a later lab needs; the
//! ones a read-only file system has no use for come with a default that
//! refuses the way Linux does (`-EROFS` for a write, `-ENOTTY` for an
//! `ioctl`, `-ENODEV` for an `mmap`). A later file system overrides what it
//! serves; nothing here changes shape.
//!
//! The mount table maps an absolute path to the root inode of a file system,
//! at most `MAX_MOUNTS` of them. `walk` resolves a path: an absolute one
//! starts at the root of the mount whose path is its longest prefix, decided
//! once at the start; a relative one starts at `cwd`. From there it looks up
//! one component at a time. Until lab 22 the walk never crosses into another
//! mount on the way: with file systems at `/` and `/boot`, `/boot/args` is
//! the second one's `args`, but `boot/args` walked from `/` (or `/x/../boot`)
//! only ever sees the first one's directories. Lab 22 teaches the crossing.
//!
//! The table is touched with interrupts off, like every shared table in this
//! kernel: an `execve` (a syscall, interrupts off) that found it held by a
//! preempted task would spin forever.
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests use them.

extern crate alloc;

use alloc::string::String;
use alloc::sync::Arc;

use crate::addr::VirtAddr;
use crate::paging::AddressSpace;
use crate::syscall::errno::{ENODEV, ENOTTY, EROFS};
use crate::syscall::Errno;

/// `EBUSY`: something is already mounted there.
pub const EBUSY: Errno = 16;
/// `ENAMETOOLONG`: a path component longer than `NAME_MAX`.
pub const ENAMETOOLONG: Errno = 36;
/// The longest name one directory entry may have (Linux's `NAME_MAX`).
pub const NAME_MAX: usize = 255;
/// How many file systems may be mounted at once.
pub const MAX_MOUNTS: usize = 8;

/// A shared handle to an inode.
pub type InodeRef = Arc<dyn Inode>;

/// The file-type bits of `Stat::mode` (Linux's `S_IF*`).
pub mod mode {
    pub const S_IFMT: u32 = 0o170000;
    pub const S_IFREG: u32 = 0o100000;
    pub const S_IFDIR: u32 = 0o040000;
    pub const S_IFCHR: u32 = 0o020000;
    pub const S_IFIFO: u32 = 0o010000;
}

/// A directory entry's type (Linux's `DT_*`, what `getdents64` reports).
pub mod dt {
    pub const DT_FIFO: u8 = 1;
    pub const DT_CHR: u8 = 2;
    pub const DT_DIR: u8 = 4;
    pub const DT_REG: u8 = 8;
}

/// What `stat` reports about an inode: the fields of x86-64 Linux's
/// `struct stat` (times in whole seconds). Build one with
/// `Stat { mode: …, size: …, ..Stat::default() }`.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Stat {
    pub dev: u64,
    pub ino: u64,
    pub mode: u32,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub rdev: u64,
    pub size: u64,
    pub blksize: u64,
    pub blocks: u64,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
}

impl Stat {
    pub fn is_dir(&self) -> bool {
        self.mode & mode::S_IFMT == mode::S_IFDIR
    }
    pub fn is_reg(&self) -> bool {
        self.mode & mode::S_IFMT == mode::S_IFREG
    }
}

/// One entry of a directory, as `getdents` hands it out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub ino: u64,
    /// `dt::*`.
    pub kind: u8,
    pub name: String,
}

/// A file or a directory of some file system (interface 10).
pub trait Inode: Send + Sync {
    /// The entry `name` of this directory. `-ENOENT` when there is none,
    /// `-ENOTDIR` when this is not a directory.
    fn lookup(&self, name: &str) -> Result<InodeRef, Errno>;
    /// Up to `buf.len()` bytes from byte `off`; `Ok(0)` at or past the end.
    /// `-EISDIR` on a directory.
    fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<usize, Errno>;
    /// Up to `buf.len()` bytes at byte `off`.
    fn write_at(&self, _off: u64, _buf: &[u8]) -> Result<usize, Errno> {
        Err(EROFS)
    }
    /// The `index`-th entry of this directory (0, 1, … in a fixed order),
    /// `Ok(None)` past the last. `-ENOTDIR` when this is not a directory.
    fn getdents(&self, index: u64) -> Result<Option<DirEntry>, Errno>;
    fn stat(&self) -> Stat;
    /// A new regular file `name` in this directory.
    fn create(&self, _name: &str, _mode: u32) -> Result<InodeRef, Errno> {
        Err(EROFS)
    }
    fn unlink(&self, _name: &str) -> Result<(), Errno> {
        Err(EROFS)
    }
    fn mkdir(&self, _name: &str, _mode: u32) -> Result<InodeRef, Errno> {
        Err(EROFS)
    }
    fn rmdir(&self, _name: &str) -> Result<(), Errno> {
        Err(EROFS)
    }
    /// Move this directory's entry `old` to `new` in `new_dir`.
    fn rename(&self, _old: &str, _new_dir: &dyn Inode, _new: &str) -> Result<(), Errno> {
        Err(EROFS)
    }
    fn truncate(&self, _len: u64) -> Result<(), Errno> {
        Err(EROFS)
    }
    fn ioctl(&self, _req: u64, _arg: u64) -> Result<i64, Errno> {
        Err(ENOTTY)
    }
    /// Map `len` bytes of this object from byte `off` at `va` in `aspace`.
    fn mmap(&self, _aspace: &mut AddressSpace, _va: VirtAddr, _len: u64, _off: u64) -> Result<(), Errno> {
        Err(ENODEV)
    }
    /// Which of the `poll` events in `mask` are ready now.
    fn poll_ready(&self, mask: u32) -> u32 {
        mask
    }
}

/// Mount the file system whose root is `root` at the absolute `path`.
/// `-EINVAL` for a relative path, `-EBUSY` when something is mounted there
/// already, `-ENOMEM` when the table holds `MAX_MOUNTS` already.
pub fn mount(path: &str, root: InodeRef) -> Result<(), Errno> {
    let _ = (path, root);
    todo!("lab 13 step 2 — mount: a table of (path, root), at most MAX_MOUNTS, interrupts off around it")
}

/// The root of the file system mounted at `/`; `-ENOENT` before one is.
pub fn root() -> Result<InodeRef, Errno> {
    todo!("lab 13 step 6 — root: the root inode of what is mounted at /")
}

/// The inode `path` names. An absolute path starts at the root of the mount
/// whose path is its longest prefix, resolved once, here; a relative one at
/// `cwd`. `.` stays, `..` goes back up — never above where the walk started
/// (until lab 22 a walk does not leave its file system). `-ENOENT` for an
/// empty path, a missing entry or nothing mounted; `-ENOTDIR` when a
/// component before the last, or a last one followed by `/`, is not a
/// directory; `-ENAMETOOLONG` for a component over `NAME_MAX`.
pub fn walk(cwd: &InodeRef, path: &str) -> Result<InodeRef, Errno> {
    let _ = (cwd, path);
    todo!("lab 13 step 7 — walk: the longest-prefix mount (or cwd), then one lookup per component")
}
