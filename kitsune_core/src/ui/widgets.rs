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
mod tests {
    use super::*;

    #[test]
    fn segments_tile_the_strip_exactly() {
        for n in 1..8usize {
            let r = Rect::new(10, 20, 203, 28);
            let segs = segmented_rects(r, n);
            assert_eq!(segs.len(), n);
            assert_eq!(segs[0].x, r.x + SEG_PAD);
            assert_eq!(segs.last().unwrap().right(), r.right() - SEG_PAD);
            for w in segs.windows(2) {
                assert_eq!(w[0].right(), w[1].x);
            }
            assert!(segs.iter().all(|s| s.h == r.h - 2 * SEG_PAD));
        }
        assert!(segmented_rects(Rect::new(0, 0, 10, 10), 0).is_empty());
    }

    #[test]
    fn segmented_hit_includes_the_padding() {
        let r = Rect::new(0, 0, 200, 28);
        assert_eq!(segmented_hit(r, 4, 1, 14), Some(0));
        assert_eq!(segmented_hit(r, 4, 199, 14), Some(3));
        assert_eq!(segmented_hit(r, 4, 100, 14), Some(2));
        assert_eq!(segmented_hit(r, 4, 100, 40), None);
        assert_eq!(segmented_hit(r, 4, -1, 14), None);
    }

    #[test]
    fn switch_knob_travels_between_the_ends() {
        let r = switch_rect(100, 50);
        let off = switch_knob(r, 0);
        let on = switch_knob(r, 256);
        assert_eq!(off.x, r.x + 2);
        assert_eq!(on.right(), r.right() - 2);
        assert_eq!((off.w, off.h), (SWITCH_H - 4, SWITCH_H - 4));
        let mid = switch_knob(r, 128);
        assert!(mid.x > off.x && mid.x < on.x);
        // Out-of-range t is clamped.
        assert_eq!(switch_knob(r, -5), off);
        assert_eq!(switch_knob(r, 999), on);
    }

    #[test]
    fn slider_value_and_knob_are_inverse() {
        let r = Rect::new(20, 100, 220, 24);
        for v in [0, 1, 25, 50, 99, 100] {
            let x = slider_knob_x(r, v, 0, 100);
            let back = slider_value(r, x, 0, 100);
            assert!((back - v).abs() <= 1, "{v} -> {x} -> {back}");
        }
        // Dragging beyond the ends clamps.
        assert_eq!(slider_value(r, -500, 0, 100), 0);
        assert_eq!(slider_value(r, 5000, 0, 100), 100);
        assert!(slider_value(r, 100, -50, 50) < 0);
    }

    #[test]
    fn scrollbar_fades_after_the_hold() {
        let mut s = ScrollbarFade::new();
        assert_eq!(s.alpha(10_000), 0);
        assert!(!s.active(10_000));
        s.touch(1000);
        assert_eq!(s.alpha(1000), 256);
        assert_eq!(s.alpha(1000 + SCROLLBAR_HOLD_MS), 256);
        let mid = s.alpha(1000 + SCROLLBAR_HOLD_MS + SCROLLBAR_FADE_MS / 2);
        assert!(mid > 100 && mid < 156, "{mid}");
        assert_eq!(s.alpha(1000 + SCROLLBAR_HOLD_MS + SCROLLBAR_FADE_MS), 0);
        assert!(!s.active(5000));
        s.touch(5000);
        assert!(s.active(5100));
        // The tick counter wrapping does not hide the bar.
        let mut w = ScrollbarFade::new();
        w.touch(u32::MAX - 10);
        assert_eq!(w.alpha(5), 256);
    }

    #[test]
    fn thumb_follows_the_scroll_position() {
        assert_eq!(scroll_thumb(100, 10, 10, 0, 16), (0, 100)); // everything fits
        let (o0, l0) = scroll_thumb(100, 100, 10, 0, 16);
        assert_eq!((o0, l0), (0, 16)); // min length
        let (o1, l1) = scroll_thumb(100, 100, 10, 90, 16);
        assert_eq!(o1 + l1, 100);
        let (om, _) = scroll_thumb(100, 100, 10, 45, 16);
        assert!(om > 30 && om < 60);
        // Out-of-range top is clamped.
        assert_eq!(scroll_thumb(100, 100, 10, 500, 16), (o1, l1));
        assert_eq!(scroll_thumb(0, 100, 10, 5, 16), (0, 0));
    }
}
