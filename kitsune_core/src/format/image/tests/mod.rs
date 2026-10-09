use super::*;
use alloc::vec;
use alloc::vec::Vec;

const RED: u32 = 0xFFFF_0000;
const GREEN: u32 = 0xFF00_FF00;
const BLUE: u32 = 0xFF00_00FF;
const WHITE: u32 = 0xFFFF_FFFF;
const BLACK: u32 = 0xFF00_0000;

/// A deterministic non-trivial image (opaque unless `alpha` is set).
fn pattern(w: usize, h: usize, alpha: bool) -> Image {
    let mut px = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let v = (x * 31 + y * 17 + x * y) as u32;
            let a = if alpha { (v * 7 % 256) as u8 } else { 255 };
            px.push(rgba(
                (v * 3) as u8,
                (v * 5 + 11) as u8,
                (v * 13 + 29) as u8,
                a,
            ));
        }
    }
    Image::from_pixels(w, h, px).unwrap()
}

fn img(w: usize, h: usize, px: &[u32]) -> Image {
    Image::from_pixels(w, h, px.to_vec()).unwrap()
}

mod basics;
mod bilinear;
mod box_filter;
mod compositing;
mod crop;
mod fit;
mod format_detection_decode;
mod nearest;
mod orientation;
