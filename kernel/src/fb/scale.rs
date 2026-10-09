//! Scaled, faded, corner-masked blit of a window texture (the open / close /
//! minimise / zoom animations).
//!
//! The texture is a window drawn once into an offscreen buffer in the framebuffer's
//! own pixel layout; each animation frame resamples it into its on-screen
//! rectangle with bilinear filtering, an overall opacity and the window's rounded
//! corners (scaled with it). Integer arithmetic only.

use super::Canvas;
use kitsune_core::Rect;
use kitsune_core::raster::{Corner, rrect_cov};

impl Canvas<'_> {
    /// Draw the `sw x sh` texture `src` (same pixel layout as this canvas, rows
    /// tightly packed) stretched into `dest`, blended with `alpha` (0..=256) and
    /// clipped to the rounded rectangle of corner radius `radius` (given for the
    /// texture's own size and scaled with the rectangle).
    pub fn blit_scaled(
        &mut self,
        src: &[u8],
        sw: usize,
        sh: usize,
        dest: Rect,
        alpha: u32,
        radius: i32,
    ) {
        let t0 = crate::trace::t();
        self.blit_scaled_inner(src, sw, sh, dest, alpha.min(256), radius);
        crate::trace::prim(crate::trace::Prim::Fade, t0);
    }

    fn blit_scaled_inner(
        &mut self,
        src: &[u8],
        sw: usize,
        sh: usize,
        dest: Rect,
        alpha: u32,
        radius: i32,
    ) {
        let Some((bpp, _)) = self.order() else {
            return;
        };
        if alpha == 0 || sw == 0 || sh == 0 || dest.w <= 0 || dest.h <= 0 {
            return;
        }
        if src.len() < sw * sh * bpp {
            return;
        }
        let clip = self.clip_rect();
        let Some(vis) = dest.intersection(&clip) else {
            return;
        };
        let (dw, dh) = (dest.w as usize, dest.h as usize);
        // Corner radius of the destination, and its coverage mask.
        let rd = ((radius.max(0) as usize * dw) / sw.max(1))
            .min(dw / 2)
            .min(dh / 2)
            .min(super::shapes::LIVE_MAX_RADIUS);
        let mask = super::shapes::masks().get(Corner::Circle, rd);
        // Source x for every visible destination column (Q8 index + fraction).
        let mut xs: alloc::vec::Vec<(u32, u32)> = alloc::vec::Vec::with_capacity(vis.w as usize);
        for x in vis.x..vis.right() {
            let lx = (x - dest.x) as usize;
            let q = ((2 * lx + 1) * sw * 128 / dw) as i32 - 128; // Q8 of (x+.5)*s-.5
            let q = q.clamp(0, ((sw - 1) * 256) as i32) as u32;
            xs.push((q >> 8, q & 255));
        }
        let stride = self.info.stride;
        for y in vis.y..vis.bottom() {
            let ly = (y - dest.y) as usize;
            let qy = ((2 * ly + 1) * sh * 128 / dh) as i32 - 128;
            let qy = qy.clamp(0, ((sh - 1) * 256) as i32) as u32;
            let (iy, fy) = ((qy >> 8) as usize, qy & 255);
            let iy1 = (iy + 1).min(sh - 1);
            let r0 = &src[iy * sw * bpp..(iy + 1) * sw * bpp];
            let r1 = &src[iy1 * sw * bpp..(iy1 + 1) * sw * bpp];
            let band = rd > 0 && (ly < rd || ly + rd >= dh);
            let o = (y as usize * stride + vis.x as usize) * bpp;
            let row = &mut self.buf[o..o + vis.w as usize * bpp];
            for (i, &(ix, fx)) in xs.iter().enumerate() {
                let lx = (vis.x - dest.x) as usize + i;
                let mut a = alpha;
                if band || lx < rd || lx + rd >= dw {
                    let cov = if rd > 0 && (band || lx < rd || lx + rd >= dw) {
                        rrect_cov(mask, rd, dw, dh, lx, ly) as u32
                    } else {
                        255
                    };
                    if cov == 0 {
                        continue;
                    }
                    a = (a * (cov + (cov >> 7))) >> 8;
                    if a == 0 {
                        continue;
                    }
                }
                let ix = ix as usize;
                let ix1 = (ix + 1).min(sw - 1);
                let d = &mut row[i * bpp..i * bpp + 3];
                for ch in 0..3 {
                    let p00 = r0[ix * bpp + ch] as i32;
                    let p01 = r0[ix1 * bpp + ch] as i32;
                    let p10 = r1[ix * bpp + ch] as i32;
                    let p11 = r1[ix1 * bpp + ch] as i32;
                    let top = p00 + (((p01 - p00) * fx as i32) >> 8);
                    let bot = p10 + (((p11 - p10) * fx as i32) >> 8);
                    let s = top + (((bot - top) * fy as i32) >> 8);
                    let dv = d[ch] as i32;
                    d[ch] = (dv + (((s - dv) * a as i32) >> 8)) as u8;
                }
            }
        }
    }
}
