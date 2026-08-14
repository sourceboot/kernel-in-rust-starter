//! Minimal framebuffer access. Lab 01's raw material: `Fb` hands you the
//! screen; what you draw on it is yours.

use crate::bootinfo::FramebufferInfo;

pub struct Fb {
    addr: *mut u8,
    pub width: usize,
    pub height: usize,
    pitch: usize,
}

impl Fb {
    /// The scaffold only supports 32bpp modes (QEMU's default). A learner on
    /// exotic hardware is out of scope: QEMU is the course's one target.
    pub fn new(info: &FramebufferInfo) -> Option<Fb> {
        if info.bpp != 32 || info.addr == 0 {
            return None;
        }
        Some(Fb {
            addr: info.addr as *mut u8,
            width: info.width as usize,
            height: info.height as usize,
            pitch: info.pitch as usize,
        })
    }

    /// Write one pixel. 0x00RRGGBB.
    pub fn plot(&mut self, x: usize, y: usize, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        // pitch is in bytes and rows may carry padding, so the offset is
        // computed in bytes and only then treated as a u32 cell.
        unsafe {
            let p = self.addr.add(y * self.pitch + x * 4) as *mut u32;
            p.write_volatile(color);
        }
    }
}
