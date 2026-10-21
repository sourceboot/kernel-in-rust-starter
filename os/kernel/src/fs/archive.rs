//! Lab 13 — the boot archive, the first file system: read-only, one flat
//! directory, linked into the kernel image. This file is yours.
//!
//! The course's programs ship as one archive (`user/README.md` "The boot
//! archive"), every number little-endian:
//!
//! ```text
//! offset 0   u64  magic 0x5B007B00A2C4
//! offset 8   u32  count
//! offset 12  count × { name: [u8; 56] (UTF-8, NUL-padded), offset: u64, size: u64 }   72 bytes each
//!            then each file's bytes at its offset
//! ```
//!
//! The table starts at byte 12, so its fields sit at addresses no `#[repr(C)]`
//! struct would put them: read every field out of the byte slice with
//! `from_le_bytes`. An `Archive` is the archive's root directory; `lookup`
//! hands out one inode per file, which reads straight out of the slice. No
//! copy is made: the bytes live as long as the kernel image does.
//!
//! The archive serves the read half of the `Inode` trait — `lookup`,
//! `read_at`, `getdents`, `stat` — and every write is the trait's default
//! refusal (`-EROFS`).
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests use them.

use crate::fs::vfs::{DirEntry, Inode, InodeRef, Stat};
use crate::syscall::errno::EISDIR;
use crate::syscall::Errno;

/// The archive's magic number (its first eight bytes).
pub const MAGIC: u64 = 0x5B00_7B00_A2C4;
/// Bytes per table entry: a 56-byte name, then the offset and the size.
pub const ENTRY: usize = 72;

/// The boot archive's root directory. Add the fields you need.
pub struct Archive {
    bytes: &'static [u8],
}

impl Archive {
    /// Check `bytes` is an archive — the magic, a table that fits, every
    /// name non-empty UTF-8 without a `/`, every file inside the slice — and
    /// serve it. `-EINVAL` for anything else.
    pub fn new(bytes: &'static [u8]) -> Result<Archive, Errno> {
        let _ = bytes;
        todo!("lab 13 step 1 — Archive::new: the magic, the count, every table entry checked")
    }

    /// How many files the archive holds.
    pub fn len(&self) -> usize {
        let _ = self.bytes;
        todo!("lab 13 step 13 — len: the count from the header")
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Inode for Archive {
    /// One inode per file, reading straight out of the archive's bytes.
    fn lookup(&self, name: &str) -> Result<InodeRef, Errno> {
        let _ = name;
        todo!("lab 13 step 4 — lookup: the entry named `name`, as an inode of its own (-ENOENT when none is)")
    }

    fn read_at(&self, _off: u64, _buf: &mut [u8]) -> Result<usize, Errno> {
        Err(EISDIR)
    }

    fn getdents(&self, index: u64) -> Result<Option<DirEntry>, Errno> {
        let _ = index;
        todo!("lab 13 step 3 — getdents: entry `index` of the table, Ok(None) past the last")
    }

    fn stat(&self) -> Stat {
        todo!("lab 13 step 11 — stat: a directory (S_IFDIR), read-only")
    }
}
