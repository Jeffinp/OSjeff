//! The painted page area, kept so a scroll repaints only the rows that came into view.

use crate::desktop::*;

/// The page area as last painted, in the framebuffer's own pixel format (rows copied with
/// `copy_from_slice`, no per-pixel conversion).
#[derive(Default)]
pub(crate) struct PaintCache {
    pub valid: bool,
    pub key: u64,
    pub scroll: i32,
    pub w: usize,
    pub h: usize,
    pub bpp: usize,
    rows: Vec<u8>,
}

impl PaintCache {
    /// Start over for a page area of `w` x `h` pixels.
    pub(super) fn begin(&mut self, key: u64, w: usize, h: usize, bpp: usize, scroll: i32) {
        self.valid = true;
        self.key = key;
        self.scroll = scroll;
        self.w = w;
        self.h = h;
        self.bpp = bpp;
        self.rows.clear();
        self.rows.resize(w * h * bpp, 0);
    }

    pub(crate) fn invalidate(&mut self) {
        self.valid = false;
        self.rows = Vec::new();
    }

    fn row_len(&self) -> usize {
        self.w * self.bpp
    }

    /// Copy the whole cache back to `area` of the framebuffer.
    pub(super) fn restore(&self, fb: &mut [u8], stride: usize, area: Rect) {
        self.restore_rows(fb, stride, area, 0, 0, self.h as i32);
    }

    /// Copy `n` cached rows starting at `from` to the framebuffer rows starting at `to` of `area`.
    pub(super) fn restore_rows(
        &self,
        fb: &mut [u8],
        stride: usize,
        area: Rect,
        to: i32,
        from: i32,
        n: i32,
    ) {
        let rl = self.row_len();
        for r in 0..n.max(0) as usize {
            let src = &self.rows[(from as usize + r) * rl..][..rl];
            let o = ((area.y as usize + to as usize + r) * stride + area.x as usize) * self.bpp;
            if let Some(dst) = fb.get_mut(o..o + rl) {
                dst.copy_from_slice(src);
            }
        }
    }

    /// Copy framebuffer rows `y0..y1` of `area` into the cache.
    pub(super) fn store_rows(&mut self, fb: &[u8], stride: usize, area: Rect, y0: i32, y1: i32) {
        let rl = self.row_len();
        for y in y0.max(0) as usize..(y1.max(0) as usize).min(self.h) {
            let o = ((area.y as usize + y) * stride + area.x as usize) * self.bpp;
            if let Some(src) = fb.get(o..o + rl) {
                self.rows[y * rl..(y + 1) * rl].copy_from_slice(src);
            }
        }
    }

    /// Move the cached rows by `dy` (the page moved up by `dy` when positive); the rows that
    /// came into view are left stale for the caller to paint.
    pub(super) fn shift(&mut self, dy: i32) {
        let rl = self.row_len();
        let n = (self.h as i32 - dy.abs()).max(0) as usize;
        if dy > 0 {
            self.rows
                .copy_within(dy as usize * rl..(dy as usize + n) * rl, 0);
        } else {
            self.rows.copy_within(0..n * rl, (-dy) as usize * rl);
        }
    }
}

/// Everything that changes how the page area looks except the scroll position.
pub(super) fn page_key(bs: &BrowserState, content: Rect) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x0100_0000_01b3);
    };
    mix(bs.rev);
    mix(u64::from(bs.tabs.active().id));
    mix(content.w as u64);
    mix(content.h as u64);
    mix(bs.hover_link.map_or(u64::MAX, |l| l as u64));
    mix(u64::from(theme::dark()));
    h
}
