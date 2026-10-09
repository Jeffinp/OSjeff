//! Small drawing helpers of the overlays.

use crate::desktop::*;
use crate::text::{self, Weight};

/// Draw `label` left-aligned in `r`, cut with an ellipsis if it does not fit.
pub(super) fn draw_fit(c: &mut Canvas, r: Rect, label: &str, px: u16, w: Weight, col: Color) {
    let t = text::ellipsize(label, px, w, r.w);
    text::draw_centered(c, r, &t, px, w, col);
}

pub(super) fn pack_rgb(c: Color) -> u32 {
    ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32
}
