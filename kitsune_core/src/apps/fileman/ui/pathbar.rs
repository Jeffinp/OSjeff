//! pathbar (split out of `ui.rs`).

use super::*;

/// Gap between two crumbs (the chevron is drawn centred in it).
pub const CRUMB_GAP: i32 = 12;

/// Horizontal padding inside a crumb's hit area.
pub const CRUMB_PAD: i32 = 6;

/// Width of the folded-crumbs marker.
pub const CRUMB_FOLD_W: i32 = 28;

/// Padding of the path bar's ends.
pub const PATH_PAD: i32 = 8;

/// Where the crumbs of the path bar go.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CrumbLayout {
    /// Index of the first crumb shown.
    pub first: usize,
    /// `(crumb index, hit rectangle)`; the text starts [`CRUMB_PAD`] inside the rectangle.
    pub spans: Vec<(usize, Rect)>,
    /// The `...` marker standing for the folded crumbs.
    pub fold: Option<Rect>,
}

/// Lay the crumbs (whose measured text widths are `widths`) into the path bar `bar`. The last
/// crumb is always shown (cut to the room left when it alone is too wide); leading crumbs fold
/// into a marker when they do not fit.
pub fn crumb_layout(bar: Rect, widths: &[i32]) -> CrumbLayout {
    let n = widths.len();
    if n == 0 {
        return CrumbLayout {
            first: 0,
            spans: Vec::new(),
            fold: None,
        };
    }
    let avail = (bar.w - 2 * PATH_PAD).max(0);
    let item = |i: usize| widths[i].max(0) + 2 * CRUMB_PAD;
    let total = |from: usize| -> i32 {
        let body: i32 = (from..n).map(item).sum::<i32>() + CRUMB_GAP * (n - from - 1) as i32;
        if from > 0 {
            body + CRUMB_FOLD_W + CRUMB_GAP
        } else {
            body
        }
    };
    let mut first = 0;
    while first + 1 < n && total(first) > avail {
        first += 1;
    }
    let mut x = bar.x + PATH_PAD;
    let mut fold = None;
    let hit_h = bar.h - 8;
    let y = bar.y + 4;
    if first > 0 {
        fold = Some(Rect::new(x, y, CRUMB_FOLD_W, hit_h));
        x += CRUMB_FOLD_W + CRUMB_GAP;
    }
    let mut spans = Vec::new();
    for i in first..n {
        let room = (bar.right() - PATH_PAD - x).max(0);
        let w = item(i).min(room);
        spans.push((i, Rect::new(x, y, w, hit_h)));
        x += w + CRUMB_GAP;
    }
    CrumbLayout { first, spans, fold }
}

impl CrumbLayout {
    /// The crumb whose hit area contains `(px, py)`.
    pub fn crumb_at(&self, px: i32, py: i32) -> Option<usize> {
        self.spans
            .iter()
            .find(|(_, r)| r.contains(px, py))
            .map(|&(i, _)| i)
    }

    /// The folded marker was hit.
    pub fn fold_hit(&self, px: i32, py: i32) -> bool {
        self.fold.is_some_and(|r| r.contains(px, py))
    }
}
