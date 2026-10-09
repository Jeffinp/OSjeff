//! Small drawing helpers of the panel.

use crate::desktop::*;
use crate::text::{self, Weight};

pub(super) fn tween_at(on: bool) -> kitsune_core::anim::Tween {
    kitsune_core::anim::Tween::at(if on { 1.0 } else { 0.0 })
}

/// Draw `label` left-aligned in `r`, cut with an ellipsis if it does not fit.
pub(super) fn draw_fit(c: &mut Canvas, r: Rect, label: &str, px: u16, w: Weight, col: Color) {
    let t = text::ellipsize(label, px, w, r.w);
    text::draw_left(c, r, &t, px, w, col);
}

pub(super) fn pack(c: Color) -> u32 {
    ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32
}

/// A horizontally mirrored copy of a surface (for the "previous" chevron).
pub(super) fn mirror_x(s: &kitsune_core::raster::Surface) -> kitsune_core::raster::Surface {
    let mut m = kitsune_core::raster::Surface::new(s.w, s.h);
    for y in 0..s.h {
        for x in 0..s.w {
            m.px[y * s.w + x] = s.px[y * s.w + (s.w - 1 - x)];
        }
    }
    m
}
