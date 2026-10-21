//! Lab 22 — `fs::procfs`: `/proc`, a file system with nothing stored. This
//! file is yours.
//!
//! Every file here is text the kernel writes at the moment it is read, from
//! the kernel's own tables (interface 17: `proc::each`, `frame::stats`), so
//! two reads with something in between see different text. The tree:
//!
//! ```text
//! /proc/meminfo       MemTotal and MemFree, in kB (frames × 4)
//! /proc/mounts        one line per mount: source, mount point, type, "rw 0 0"
//! /proc/uptime        seconds since boot, two decimals, then " 0.00"
//! /proc/sboot/nonce   the run's nonce: 32 hex digits and a newline
//! /proc/<pid>/        one directory per process the kernel still has
//!     cmdline         its argv, each string followed by a NUL
//!     status          Name, State (R running, Z ended), Pid, PPid
//! /proc/self          the directory of the process that asks
//! ```
//!
//! A read at an offset renders the whole text again and copies from there.
//! `/proc/mounts` writes a space, a tab, a newline or a backslash inside a
//! field as `\040`, `\011`, `\012`, `\134`, as Linux does, so a mount point
//! with a space in it is still one field.

use crate::fs::vfs::{DirEntry, Inode, InodeRef, Stat};
use crate::syscall::errno::EISDIR;
use crate::syscall::Errno;

/// `/proc` itself: the root of one mounted `/proc` (add its device number;
/// the files and directories under it are types of your own).
pub struct ProcFs {}

impl ProcFs {
    /// A new `/proc`, with a device number of its own (`mount::anon_dev`).
    pub fn new() -> ProcFs {
        todo!("lab 22 step 1 — ProcFs::new: a /proc with a device number of its own")
    }
}

impl Default for ProcFs {
    fn default() -> Self {
        ProcFs::new()
    }
}

impl Inode for ProcFs {
    fn lookup(&self, name: &str) -> Result<InodeRef, Errno> {
        let _ = name;
        todo!("lab 22 step 7 — ProcFs::lookup: meminfo, mounts, uptime, sboot, self, or a live pid")
    }
    fn read_at(&self, _off: u64, _buf: &mut [u8]) -> Result<usize, Errno> {
        Err(EISDIR)
    }
    fn getdents(&self, index: u64) -> Result<Option<DirEntry>, Errno> {
        let _ = index;
        todo!("lab 22 step 10 — ProcFs::getdents: the four names, then one directory per pid")
    }
    fn stat(&self) -> Stat {
        todo!("lab 22 step 4 — ProcFs::stat: a directory, 0555, on this /proc's device")
    }
}
