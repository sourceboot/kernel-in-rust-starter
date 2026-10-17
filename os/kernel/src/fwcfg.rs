//! QEMU's fw_cfg device, read over its two I/O ports — the cockpit's nonce
//! channel (CONTRACT §A.2). Course-owned; shipped whole.
//!
//! The port interface is what every QEMU since 2.x has: write a 16-bit
//! selector to 0x510, then read the selected item's bytes one at a time from
//! 0x511. Selector 0x0000 is the signature ("QEMU"); selector 0x0019 is the
//! file directory — a big-endian u32 count, then 64-byte entries
//! `{ size: be32, select: be16, reserved: u16, name: [u8; 56] }`. QEMU's DMA
//! interface is not used. Like `handshake.rs`, this file has its own port
//! I/O and never rides the learner's serial helpers.

const SELECTOR: u16 = 0x510;
const DATA: u16 = 0x511;
const SIG: u16 = 0x0000;
const FILE_DIR: u16 = 0x0019;

fn select(key: u16) {
    // SAFETY: a 16-bit port write to fw_cfg's selector; no memory is touched.
    unsafe {
        core::arch::asm!("out dx, ax", in("dx") SELECTOR, in("ax") key,
            options(nomem, nostack, preserves_flags));
    }
}

fn read8() -> u8 {
    let v: u8;
    // SAFETY: an 8-bit port read from fw_cfg's data port; no memory is touched.
    unsafe {
        core::arch::asm!("in al, dx", out("al") v, in("dx") DATA,
            options(nomem, nostack, preserves_flags));
    }
    v
}

fn read_be32() -> u32 {
    let mut v = 0u32;
    for _ in 0..4 {
        v = (v << 8) | read8() as u32;
    }
    v
}

/// Whether a fw_cfg device answers at all (its signature reads "QEMU").
pub fn present() -> bool {
    select(SIG);
    let mut sig = [0u8; 4];
    for b in sig.iter_mut() {
        *b = read8();
    }
    &sig == b"QEMU"
}

/// Find a named fw_cfg file; returns (selector, size).
pub fn find(name: &str) -> Option<(u16, u32)> {
    if !present() {
        return None;
    }
    select(FILE_DIR);
    let count = read_be32();
    // A sane bound: QEMU's directory holds a few dozen entries.
    if count > 4096 {
        return None;
    }
    let want = name.as_bytes();
    let mut found = None;
    for _ in 0..count {
        let size = read_be32();
        let sel = ((read8() as u16) << 8) | read8() as u16;
        let _reserved = (read8(), read8());
        let mut entry = [0u8; 56];
        for b in entry.iter_mut() {
            *b = read8();
        }
        let len = entry.iter().position(|&b| b == 0).unwrap_or(56);
        if found.is_none() && &entry[..len] == want {
            found = Some((sel, size));
        }
    }
    found
}

/// Read up to `buf.len()` bytes of the file at `sel`; returns the count read.
pub fn read(sel: u16, size: u32, buf: &mut [u8]) -> usize {
    select(sel);
    let n = (size as usize).min(buf.len());
    for b in buf[..n].iter_mut() {
        *b = read8();
    }
    n
}

/// The course's nonce file name (CONTRACT §A.2).
pub const NONCE_FILE: &str = "opt/org.sourceboot/nonce";
