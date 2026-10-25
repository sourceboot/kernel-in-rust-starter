//! Lab 22 — `fs::devfs`: `/dev`, devices as files. This file is yours.
//!
//! Four character devices, each with Linux's device number (`st_rdev`,
//! `major << 8 | minor`):
//!
//! ```text
//! /dev/null      1:3   reads end at once (0 bytes); writes take every byte
//! /dev/zero      1:5   reads fill the whole buffer with zeros; writes take every byte
//! /dev/tty       5:0   the console (interface 18)
//! /dev/console   5:1   the console
//! ```
//!
//! `null` and `zero` are served here, through `read_at`/`write_at`. `tty`
//! and `console` are not: `open` on either gives an open file on the console
//! object itself (`console::FileRef::open_inode` knows them by their device
//! numbers), so a write through `/dev/console` and one through fd 1 are the
//! same object's, with its `O_NONBLOCK` and its `ioctl`. Lab 26 adds `fb0`
//! (29:0, the framebuffer) and `input/event0` (13:64, the keyboard), each
//! listed only once `probe` has found the hardware behind it.

use crate::fs::vfs::{DirEntry, Inode, InodeRef, Stat};
use crate::syscall::errno::EISDIR;
use crate::syscall::Errno;

/// `/dev/tty`'s and `/dev/console`'s device numbers: what makes `open`
/// hand out the console object.
pub const TTY: u64 = 5 << 8;
pub const CONSOLE: u64 = 5 << 8 | 1;

/// `/dev` itself: the root of one mounted `/dev` (add its device number;
/// the devices under it are a type of your own).
pub struct DevFs {}

impl DevFs {
    /// A new `/dev`, with a device number of its own (`mount::anon_dev`).
    pub fn new() -> DevFs {
        todo!("lab 22 step 2 — DevFs::new: a /dev with a device number of its own")
    }
}

impl Default for DevFs {
    fn default() -> Self {
        DevFs::new()
    }
}

impl Inode for DevFs {
    fn lookup(&self, name: &str) -> Result<InodeRef, Errno> {
        let _ = name;
        todo!("lab 22 step 6 — DevFs::lookup: null, zero, tty, console, each a character device")
    }
    fn read_at(&self, _off: u64, _buf: &mut [u8]) -> Result<usize, Errno> {
        Err(EISDIR)
    }
    fn getdents(&self, index: u64) -> Result<Option<DirEntry>, Errno> {
        let _ = index;
        todo!("lab 22 step 11 — DevFs::getdents: the four devices, in order")
    }
    fn stat(&self) -> Stat {
        todo!("lab 22 step 5 — DevFs::stat: a directory, 0755, on this /dev's device")
    }
}

/// Lab 26: look for the hardware behind `/dev/fb0` (the bootloader's
/// framebuffer, 32 bits a pixel) and `/dev/input/event0` (the keyboard),
/// once at boot. A `/dev` lists them, and `lookup` finds them, only after
/// this.
pub fn probe() {
    todo!("lab 26 step 2 — probe: which of fb0 and the input device this machine has")
}

/// Lab 26: `kbd::push` hands every scancode here first, in the keyboard
/// interrupt. While anyone has `event0` open the key becomes an `EV_KEY`
/// event and an `EV_SYN` in the device's queue; `true` when a reader holds
/// the device grabbed (`EVIOCGRAB`), so the console never sees the key.
pub fn feed(sc: u8) -> bool {
    let _ = sc;
    todo!("lab 26 step 8 — feed: the scancode as input events; whether the device is grabbed")
}
