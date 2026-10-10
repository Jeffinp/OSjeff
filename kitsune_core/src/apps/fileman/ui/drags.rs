//! drags (split out of `ui.rs`).

use super::*;

/// The normalised rectangle spanned by two corners (both inclusive).
pub fn band_rect(a: (i32, i32), b: (i32, i32)) -> Rect {
    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
    let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
    Rect::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1)
}

/// Whether the pointer has moved far enough from where it was pressed to start a drag.
pub fn drag_started(press: (i32, i32), now: (i32, i32)) -> bool {
    let (dx, dy) = ((now.0 - press.0).abs(), (now.1 - press.1).abs());
    dx.max(dy) >= DRAG_THRESHOLD
}

/// Pixels to scroll per frame while a drag holds the pointer at `y` over a viewport spanning
/// `top..bottom`: negative above, positive below, zero in the middle.
pub fn edge_scroll(y: i32, top: i32, bottom: i32) -> i32 {
    const ZONE: i32 = 28;
    const MAX: i32 = 18;
    if y < top + ZONE {
        -(((top + ZONE - y).min(ZONE + 24) * MAX) / ZONE).min(MAX)
    } else if y > bottom - ZONE {
        (((y - (bottom - ZONE)).min(ZONE + 24) * MAX) / ZONE).min(MAX)
    } else {
        0
    }
}

/// The selection a rubber band produces: the items it touches, added to `base` with Ctrl (and
/// toggled off when both), else alone. Ascending, no duplicates.
pub fn band_selection(base: &[usize], touched: &[usize], additive: bool) -> Vec<usize> {
    let mut out: Vec<usize> = if additive { base.to_vec() } else { Vec::new() };
    for &i in touched {
        if !out.contains(&i) {
            out.push(i);
        }
    }
    out.sort_unstable();
    out
}
