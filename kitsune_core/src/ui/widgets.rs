//! Pure state and geometry of the reusable widgets (the drawing is in the
//! kernel's `desktop/ui.rs`): segmented controls, switches, sliders and the
//! fading overlay scrollbar.
//!
//! The toolkit contract: a widget's geometry comes from one function here, used
//! by both the draw code and the hit test, so they cannot disagree.

use crate::windowing::window::Rect;
use alloc::vec::Vec;

// ------------------------------------------------------------------ segmented

/// Inner padding of a segmented control.
pub const SEG_PAD: i32 = 2;

/// The rectangle of each of `n` equal segments inside `r`.
pub fn segmented_rects(r: Rect, n: usize) -> Vec<Rect> {
    if n == 0 {
        return Vec::new();
    }
    let inner_w = r.w - 2 * SEG_PAD;
    let sw = inner_w / n as i32;
    (0..n as i32)
        .map(|i| {
            // The last segment takes the rounding remainder.
            let w = if i == n as i32 - 1 {
                inner_w - sw * i
            } else {
                sw
            };
            Rect::new(r.x + SEG_PAD + i * sw, r.y + SEG_PAD, w, r.h - 2 * SEG_PAD)
        })
        .collect()
}

/// The segment under `(x, y)`.
pub fn segmented_hit(r: Rect, n: usize, x: i32, y: i32) -> Option<usize> {
    if !r.contains(x, y) {
        return None;
    }
    let rects = segmented_rects(r, n);
    // The padding belongs to the nearest segment: clamp the x into the strip.
    let x = x.clamp(r.x + SEG_PAD, r.right() - SEG_PAD - 1);
    rects.iter().position(|s| x >= s.x && x < s.right())
}

// --------------------------------------------------------------------- switch

pub const SWITCH_W: i32 = 38;
pub const SWITCH_H: i32 = 22;

/// Rectangle of a switch whose top-left is `(x, y)`.
pub fn switch_rect(x: i32, y: i32) -> Rect {
    Rect::new(x, y, SWITCH_W, SWITCH_H)
}

/// The knob of switch `r` at position `t` (0 = off, 1 = on, in 0..=256).
pub fn switch_knob(r: Rect, t256: i32) -> Rect {
    let d = r.h - 4;
    let travel = r.w - 4 - d;
    Rect::new(r.x + 2 + travel * t256.clamp(0, 256) / 256, r.y + 2, d, d)
}

// --------------------------------------------------------------------- slider

pub const SLIDER_KNOB: i32 = 16;

/// The groove of slider `r` (a thin bar centred vertically).
pub fn slider_track(r: Rect) -> Rect {
    Rect::new(
        r.x + SLIDER_KNOB / 2,
        r.y + r.h / 2 - 2,
        (r.w - SLIDER_KNOB).max(1),
        4,
    )
}

/// Value (`min..=max`) for a pointer at `x`.
pub fn slider_value(r: Rect, x: i32, min: i32, max: i32) -> i32 {
    let t = slider_track(r);
    let rel = (x - t.x).clamp(0, t.w) as i64;
    min + ((max - min) as i64 * rel / t.w as i64) as i32
}

/// X of the knob centre for `value`.
pub fn slider_knob_x(r: Rect, value: i32, min: i32, max: i32) -> i32 {
    let t = slider_track(r);
    let span = (max - min).max(1) as i64;
    t.x + (t.w as i64 * (value.clamp(min, max) - min) as i64 / span) as i32
}

// ------------------------------------------------------------------ scrollbar

/// Idle time before an overlay scrollbar starts to fade.
pub const SCROLLBAR_HOLD_MS: u32 = 800;
/// Fade duration.
pub const SCROLLBAR_FADE_MS: u32 = 200;

/// Overlay scrollbar visibility: fully shown while the user scrolls, gone after
/// [`SCROLLBAR_HOLD_MS`] plus [`SCROLLBAR_FADE_MS`] of idleness.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScrollbarFade {
    last_ms: u32,
    touched: bool,
}

impl ScrollbarFade {
    pub const fn new() -> Self {
        Self {
            last_ms: 0,
            touched: false,
        }
    }

    /// The user scrolled (or moved over the list) at `now_ms`.
    pub fn touch(&mut self, now_ms: u32) {
        self.last_ms = now_ms;
        self.touched = true;
    }

    /// Opacity 0..=256 at `now_ms`.
    pub fn alpha(&self, now_ms: u32) -> u32 {
        if !self.touched {
            return 0;
        }
        let idle = now_ms.wrapping_sub(self.last_ms);
        if idle <= SCROLLBAR_HOLD_MS {
            256
        } else if idle >= SCROLLBAR_HOLD_MS + SCROLLBAR_FADE_MS {
            0
        } else {
            256 - 256 * (idle - SCROLLBAR_HOLD_MS) / SCROLLBAR_FADE_MS
        }
    }

    /// Still fading or visible (needs frames).
    pub fn active(&self, now_ms: u32) -> bool {
        self.alpha(now_ms) > 0
    }
}

/// Thumb `(offset, length)` inside a track of `track` pixels for a list showing
/// `rows` of `total` items starting at `top`. Never shorter than `min_len`.
pub fn scroll_thumb(track: i32, total: usize, rows: usize, top: usize, min_len: i32) -> (i32, i32) {
    if total <= rows || track <= 0 {
        return (0, track.max(0));
    }
    let len = ((track as i64 * rows as i64 / total as i64) as i32)
        .max(min_len)
        .min(track);
    let max_top = (total - rows) as i64;
    let off = ((track - len) as i64 * top.min(total - rows) as i64 / max_top) as i32;
    (off, len)
}

#[cfg(test)]
mod tests;
