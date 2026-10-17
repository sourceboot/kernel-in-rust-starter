//! lab 09 — `syscall`. Not written yet: a later slice of the
//! reference fills this file, and its stub arrives with lab 09.
//!
//! Published surface (CONTRACT §B.2-09): `syscall::init()` (STAR/LSTAR/SFMASK), `syscall::dispatch(nr, a: [u64; 6], f: &mut SyscallFrame) -> i64` (one `match`, `_ => -ENOSYS` counted), `syscall::enosys_count() -> u64`, `mod errno`.
