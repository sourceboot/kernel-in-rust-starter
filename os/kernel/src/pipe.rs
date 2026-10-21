//! Lab 15 — pipes: a buffer in the kernel with a reading end and a writing
//! end, each an open file. This file is yours.
//!
//! `pipe2` makes one: a ring of `PIPE_BUF` (4096) bytes and two open files
//! on it, the read end and the write end, installed in the caller's fd
//! table. The pipe counts its open ends — an open file from the moment it
//! is made (`acquire`, from `FileRef::open`) until it drops (`release`),
//! however many fds share it. A read on an empty pipe returns 0 (end of
//! file) only when no write end is open any more; until then it sleeps
//! (`sched::sleep`), and a writer that puts bytes in wakes it
//! (`sched::wakeup`). A writer facing a full ring sleeps the same way until
//! a reader takes bytes out. Check and sleep under the pipe's lock, so no
//! wakeup falls in between and is lost.
//!
//! Linux's rules (pipe(7)): a write of at most `PIPE_BUF` bytes goes in
//! whole or waits; a longer one may go in parts. A write with no read end
//! open is `-EPIPE`. `O_NONBLOCK` on the open file turns every wait into
//! `-EAGAIN`.
//!
//! A pipe end is a `FileObj` (an index — `console.rs`), so keep the pipes
//! in a table here that an end's index finds. The last end to go frees its
//! slot. Never drop a `FileRef` while the task table's lock is held (inside
//! `with_fds`): the drop of a pipe's last write end wakes its reader, and a
//! wakeup takes that lock.
//!
//! Fill in the bodies; keep the signatures — later labs and the course's
//! tests use them. Add private fields and helpers as you need them.

use crate::console::{File, FileObj, FileRef, OpenFile};
use crate::proc::Process;
use crate::syscall::Errno;

/// The ring's size, and the largest write that goes in whole.
pub const PIPE_BUF: usize = 4096;

/// A ring of `PIPE_BUF` bytes (add the bytes and the read and write
/// positions).
pub struct Ring {}

impl Ring {
    /// Bytes waiting.
    pub fn len(&self) -> usize {
        todo!("lab 15 step 5 — Ring::len: the bytes waiting in the ring")
    }

    pub fn is_empty(&self) -> bool {
        todo!("lab 15 step 7 — Ring::is_empty: no bytes waiting")
    }
}

/// What the pipe's lock guards: the bytes and the open ends (add the
/// sleepers you count).
pub struct PipeInner {
    pub ring: Ring,
    pub readers: u32,
    pub writers: u32,
}

/// A pipe (add its `SpinLock<PipeInner>`).
pub struct Pipe {}

/// One end of a pipe, as the object behind an open file (add the pipe it
/// belongs to and which end it is). `Send + Sync`.
pub struct PipeEnd {}

/// The pipe end `obj` names, while its pipe is alive.
pub fn end(obj: FileObj) -> Option<PipeEnd> {
    let _ = obj;
    todo!("lab 15 step 3 — end: the pipe end an open file's FileObj names")
}

/// An open file on `obj` is being made (`FileRef::open`): count it.
pub fn acquire(obj: FileObj) -> Result<(), Errno> {
    let _ = obj;
    todo!("lab 15 step 2 — acquire: one more open reader or writer")
}

/// The open file on `obj` is gone: one end fewer. The last write end gone
/// wakes the readers; the last read end gone wakes the writers; the last
/// end of all frees the slot.
pub fn release(obj: FileObj) {
    let _ = obj;
    todo!("lab 15 step 8 — release: one end fewer, and wake whoever that changes things for")
}

/// How many times a writer has gone to sleep on a full ring since boot.
/// The course's test prints it, so a run whose ring never filled shows.
pub fn full_waits() -> u64 {
    todo!("lab 15 step 9 — full_waits: count every time a writer goes to sleep on a full ring")
}

/// A new pipe and its two open files: (read end, write end). `flags` may
/// carry `O_NONBLOCK`, which both open files get.
pub fn open(flags: u32) -> Result<(FileRef, FileRef), Errno> {
    let _ = flags;
    todo!("lab 15 step 1 — open: a new pipe in the table, and an open file on each end")
}

/// `pipe2` (syscall 293; `pipe`, 22, is `flags` 0): a new pipe, its read
/// end and its write end on the two lowest free fds of the caller's table;
/// returns (read fd, write fd). `O_CLOEXEC` marks both fds close-on-exec,
/// `O_NONBLOCK` makes both open files non-blocking; any other flag is
/// `-EINVAL`; `-EMFILE` with fewer than two fds free.
pub fn pipe2(p: &mut Process, flags: u32) -> Result<(i32, i32), Errno> {
    let _ = (p, flags);
    todo!("lab 15 step 10 — pipe2: a pipe on the two lowest free fds")
}

impl File for PipeEnd {
    /// Up to `buf.len()` bytes, as many as are waiting; with none, 0 when no
    /// write end is open, `-EAGAIN` under `O_NONBLOCK`, else sleep.
    fn read(&self, of: &OpenFile, buf: &mut [u8]) -> Result<usize, Errno> {
        let _ = (of, buf);
        todo!("lab 15 step 6 — PipeEnd::read: take what is waiting, or sleep until a writer puts some in or the last one goes")
    }

    /// All of `buf`, waiting for room while a read end is open; at most
    /// `PIPE_BUF` bytes go in whole. `-EPIPE` with no read end open.
    fn write(&self, of: &OpenFile, buf: &[u8]) -> Result<usize, Errno> {
        let _ = (of, buf);
        todo!("lab 15 step 4 — PipeEnd::write: put it all in, sleeping while the ring is full")
    }
}
