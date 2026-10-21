//! The log's spy and controls, shared by `ktest-18`, `ktest-19` and `ktest-20`
//! (each includes this file with `#[path]`, as every vehicle includes
//! `shim.rs`), so the three `txn.*.metadata.write` rows classify writes with
//! one piece of code: the drive under the mount (`Spy`, every write recorded
//! with whether the learner's `log::logged()` listed it), the log's control
//! before any mount (`log_control` over `MemDev`), and `watched` (one call into
//! the learner's file system between two walks of the tree; each metadata write
//! it made must have been listed). Course-owned (`boot/`); learners never edit it.
//! Shared code by ruling: TPM, 2026-10-19, B2 (a) condition 3.

use super::*;

/// Every write that reached the drive since the mount: (fs block, whether
/// `log::logged()` listed it then).
pub(super) static WRITES: SpinLock<Vec<(u64, bool)>> = SpinLock::new(Vec::new());

pub(super) fn writes() -> Vec<(u64, bool)> {
    quiet(|| WRITES.lock().clone())
}

/// The drive, watched.
pub(super) struct Spy {
    pub(super) inner: Arc<dyn BlockDevice>,
    pub(super) reads: AtomicU64,
}

impl BlockDevice for Spy {
    fn read_block(&self, n: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), Errno> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.inner.read_block(n, buf)
    }
    fn write_block(&self, n: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), Errno> {
        let b = n.wrapping_sub(P2_BLOCK0);
        let listed = log::logged().contains(&b);
        let r = self.inner.write_block(n, buf);
        if r.is_ok() {
            quiet(|| WRITES.lock().push((b, listed)));
        }
        r
    }
    fn blocks(&self) -> u64 {
        self.inner.blocks()
    }
    fn arm_fail_after(&self, k: u32) {
        self.inner.arm_fail_after(k)
    }
    fn disarm(&self) {
        self.inner.disarm()
    }
    fn refused_writes(&self) -> u32 {
        self.inner.refused_writes()
    }
}

/// The control's device: zeros, and a record of each write (block, listed).
pub(super) struct MemDev {
    seen: SpinLock<Vec<(u64, bool)>>,
}

impl BlockDevice for MemDev {
    fn read_block(&self, _n: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), Errno> {
        buf.fill(0);
        Ok(())
    }
    fn write_block(&self, n: u64, _buf: &[u8; BLOCK_SIZE]) -> Result<(), Errno> {
        let listed = log::logged().contains(&n);
        quiet(|| self.seen.lock().push((n, listed)));
        Ok(())
    }
    fn blocks(&self) -> u64 {
        64
    }
    fn arm_fail_after(&self, _k: u32) {}
    fn disarm(&self) {}
    fn refused_writes(&self) -> u32 {
        0
    }
}

/// The log's control, before any mount: a cache of this file's over a
/// memory device, attached; what is wrong with how the log answered.
pub(super) fn log_control() -> Vec<String> {
    let mut bad = Vec::new();
    let dev = Arc::new(MemDev { seen: SpinLock::new(Vec::new()) });
    let cache = Arc::new(Cache::new(dev.clone(), 4));
    log::attach(cache.clone());
    log::begin_op();
    let take = |n: u64| cache.get(n).map_err(|e| format!("the control's own Cache::get({n}) returned {}", errno_text(e)));
    let (a, b, c) = match (take(7), take(9), take(11)) {
        (Ok(a), Ok(b), Ok(c)) => (a, b, c),
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => return alloc::vec![e],
    };
    a.data()[0] = 0x18;
    log::log_write(&a);
    b.data()[0] = 0x19;
    log::log_write(&b);
    log::log_write(&a);
    let l = log::logged();
    if l != [7, 9] {
        bad.push(format!("logged() inside the operation is {l:?} after log_write of blocks 7, 9, 7; expected [7, 9]"));
    }
    if !quiet(|| dev.seen.lock().is_empty()) {
        bad.push(String::from("a block reached the device before end_op"));
    }
    drop((a, b));
    if let Err(e) = log::end_op() {
        bad.push(format!("end_op() returned {}", errno_text(e)));
    }
    let seen = quiet(|| dev.seen.lock().clone());
    for n in [7, 9] {
        if !seen.contains(&(n, true)) {
            bad.push(format!("end_op did not write block {n} while logged() listed it (the device saw {seen:?})"));
        }
    }
    let l = log::logged();
    if !l.is_empty() {
        bad.push(format!("logged() after end_op is {l:?}, expected []"));
    }
    // The negative control: a write outside any operation is seen unlisted.
    c.data()[0] = 0x11;
    cache.write(&c);
    drop(c);
    let _ = cache.sync();
    if !quiet(|| dev.seen.lock().contains(&(11, false))) {
        bad.push(String::from("a write outside any operation was not seen unlisted (this file's classifier)"));
    }
    bad
}

pub(super) const KINDS: [&str; 7] = ["the superblock", "the group descriptor", "the block bitmap", "the inode bitmap", "the inode table", "a directory block", "a pointer block"];

/// `f`, one call into the learner's file system, between two walks of the
/// tree; every metadata write it made must have been listed.
pub(super) fn watched(r: &mut Row, raw: &Raw, seen: &mut [bool; 7], what: &str, f: impl FnOnce() -> Result<(), Errno>) {
    let Some(before) = got(r, scan(raw)) else { return };
    let from = quiet(|| WRITES.lock().len());
    let res = f();
    let Some(after) = got(r, scan(raw)) else { return };
    if let Err(e) = res {
        r.fail(format_args!("{TXN_ID}: {what} returned {}", errno_text(e)));
    }
    let w = writes();
    let mut unlisted = Vec::new();
    for &(b, listed) in &w[from..] {
        let k = match b {
            0 => 0,
            1 => 1,
            b if b == raw.block_bitmap => 2,
            b if b == raw.inode_bitmap => 3,
            b if b < raw.meta_end() => 4,
            b if before.dir_blocks.contains(&b) || after.dir_blocks.contains(&b) => 5,
            b if before.ptr_blocks.contains(&b) || after.ptr_blocks.contains(&b) => 6,
            _ => continue,
        };
        if listed {
            seen[k] = true;
        } else {
            unlisted.push(format!("{b} ({})", KINDS[k]));
        }
    }
    if !unlisted.is_empty() {
        r.fail(format_args!("{TXN_ID}: {what}: fs blocks {} reached the disk while logged() did not list them", unlisted.join(", ")));
    }
}
