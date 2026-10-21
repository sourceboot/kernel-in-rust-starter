//! Lab 13 — the ELF loader: from a file's bytes to a program's memory.
//! This file is yours.
//!
//! An ELF64 executable starts with a 64-byte header (`e_type`, `e_machine`,
//! `e_entry`, where the program headers are) and carries a table of program
//! headers; each `PT_LOAD` says "bytes `[p_offset, p_offset + p_filesz)` of
//! this file belong at `p_vaddr`, in `p_memsz` bytes of memory — the rest
//! zero — with these permissions". Loading is doing what the table says in
//! an address space: map pages, copy the file's bytes in, leave the tail
//! zero, record a VMA per segment.
//!
//! The rules this kernel holds a file to: ELF64, little-endian, `ET_EXEC`,
//! x86-64 — anything else is `-ENOEXEC` (an `ET_DYN` would need relocating
//! and a dynamic linker, which this kernel does not have). A segment's
//! offset and address need only agree modulo the page size: the page that
//! holds `p_vaddr` takes the file's bytes from `p_offset` on. The bytes
//! from `p_filesz` to `p_memsz` read as zero, including the ones that share
//! the last file page (`.bss` straight after `.data`). `PF_X` makes a
//! segment executable, `!PF_W` read-only. The program headers themselves
//! must be inside a loaded segment, because `AT_PHDR` hands their address
//! to the program. A `PT_TLS` is recorded and nothing more: a static musl
//! program builds its own thread block from it (lab 25).
//!
//! The reader is anything with `read_at`: a byte slice (a flat image the
//! kernel already holds), a file of the boot archive (lab 13), an ext2
//! file (lab 17).
//!
//! Fill in the body; keep the signatures — later labs and the course's
//! tests use them.

use crate::addr::VirtAddr;
use crate::paging::AddressSpace;
use crate::proc::VmaList;
use crate::syscall::Errno;

/// Something that can hand out its bytes by offset (interface 9).
pub trait ReadAt {
    /// Up to `buf.len()` bytes from byte `off`; `Ok(0)` at or past the end.
    fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<usize, Errno>;
}

/// A byte slice is a file: `&image` reads as one.
impl ReadAt for &[u8] {
    fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<usize, Errno> {
        let len = self.len() as u64;
        if off >= len {
            return Ok(0);
        }
        let n = buf.len().min((len - off) as usize);
        buf[..n].copy_from_slice(&self[off as usize..off as usize + n]);
        Ok(n)
    }
}

/// A program's `PT_TLS`: where its initial thread data is and how big.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tls {
    pub vaddr: VirtAddr,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
}

/// What `load` placed, for the initial stack's auxv.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Loaded {
    /// `e_entry` (`AT_ENTRY`).
    pub entry: VirtAddr,
    /// Where the program headers are in the program's memory (`AT_PHDR`).
    pub phdr: VirtAddr,
    /// `AT_PHNUM`.
    pub phnum: u16,
    /// `AT_PHENT`: 56 for ELF64.
    pub phent: u16,
    /// The first page above the highest segment: where a break would start.
    pub brk: VirtAddr,
    /// The `PT_TLS` segment, if the program has one.
    pub tls: Option<Tls>,
}

pub const ET_EXEC: u16 = 2;
pub const ET_DYN: u16 = 3;
pub const EM_X86_64: u16 = 0x3e;
pub const PT_LOAD: u32 = 1;
pub const PT_INTERP: u32 = 3;
pub const PT_PHDR: u32 = 6;
pub const PT_TLS: u32 = 7;
pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
/// The size of one ELF64 program header.
pub const PHENT: u16 = 56;

/// Load the ELF executable `r` into `aspace` and record a VMA per `PT_LOAD`
/// in `vmas`. `-ENOEXEC` for a file that is not an x86-64 `ET_EXEC` this
/// kernel can place, `-ENOMEM` when frames run out. On an error the pages
/// mapped so far stay in `aspace`: the caller throws the space away.
pub fn load(r: &dyn ReadAt, aspace: &mut AddressSpace, vmas: &mut VmaList) -> Result<Loaded, Errno> {
    let _ = (r, aspace, vmas);
    todo!("lab 13 step 9 — load: check the header, map each PT_LOAD (the tail past p_filesz zero), a VMA per segment, AT_PHDR's address")
}
