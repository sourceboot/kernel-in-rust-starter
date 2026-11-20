//! Lab 01 — first light. This file is yours: draw on the screen.
//!
//! `Fb::plot(x, y, color)` (in fb.rs, provided) puts one pixel on the screen.
//! Build fill and rect on top of it, then draw the three panels from the brief.
//! `sboot test 01-first-light` boots it headless and samples your pixels.

use crate::fb::Fb;

pub fn main(_fb: &mut Fb) {
    // Your kernel. Your screen. Draw something.
}

/// Paint every pixel of the screen `color`.
pub fn fill(_fb: &mut Fb, _color: u32) {
    todo!("lab 01 task 1 — fill: plot every pixel of the screen")
}

/// Paint the `w`×`h` rectangle whose top-left corner is `(x, y)`.
pub fn rect(_fb: &mut Fb, _x: usize, _y: usize, _w: usize, _h: usize, _color: u32) {
    todo!("lab 01 task 2 — rect: plot every pixel of the rectangle")
}
