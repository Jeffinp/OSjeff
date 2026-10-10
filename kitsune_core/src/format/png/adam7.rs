//! adam7 (split out of `png.rs`).

use super::*;

#[derive(Clone, Copy)]
pub(super) struct Pass {
    pub(super) x0: usize,
    pub(super) y0: usize,
    pub(super) dx: usize,
    pub(super) dy: usize,
    pub(super) w: usize,
    pub(super) h: usize,
}

pub(super) const ADAM7: [(usize, usize, usize, usize); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

/// The (up to 7) passes of an image; empty passes have `w == 0 || h == 0`.
pub(super) fn passes(w: usize, h: usize, interlaced: bool) -> ([Pass; 7], usize) {
    let mut out = [Pass {
        x0: 0,
        y0: 0,
        dx: 1,
        dy: 1,
        w,
        h,
    }; 7];
    if !interlaced {
        return (out, 1);
    }
    for (p, &(x0, y0, dx, dy)) in out.iter_mut().zip(ADAM7.iter()) {
        let pw = if w > x0 { (w - x0).div_ceil(dx) } else { 0 };
        let ph = if h > y0 { (h - y0).div_ceil(dy) } else { 0 };
        *p = Pass {
            x0,
            y0,
            dx,
            dy,
            w: pw,
            h: ph,
        };
    }
    (out, 7)
}

/// Bytes in one scanline of `pixels` pixels (without the filter byte).
pub(super) fn row_bytes(pixels: usize, bits_per_pixel: usize) -> usize {
    (pixels * bits_per_pixel).div_ceil(8)
}

/// Exact size of the decompressed scanline stream (filter bytes included).
pub(super) fn raw_size(h: &Header) -> u64 {
    let (ps, n) = passes(h.width as usize, h.height as usize, h.interlaced);
    let bpp = h.bits_per_pixel();
    let mut total = 0u64;
    for p in ps.iter().take(n) {
        if p.w > 0 && p.h > 0 {
            total += p.h as u64 * (1 + row_bytes(p.w, bpp) as u64);
        }
    }
    total
}
