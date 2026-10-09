//! A damage region: a small set of **disjoint** rectangles.
//!
//! One rectangle is too coarse (moving a window to the other corner of the screen would repaint
//! everything in between) and an unbounded list is too slow, so a [`Region`] keeps at most
//! [`MAX_RECTS`] rectangles and merges the ones whose bounding box wastes little. Merging only ever
//! *grows* the region, never shrinks it, so a region is always a superset of what was added:
//! repainting too much is harmless, repainting too little is the bug the compositor must not have.
//!
//! Invariants (checked by the tests after every operation):
//! - every rectangle is non-empty;
//! - rectangles are pairwise disjoint (the engine paints layer by layer over each rectangle, and a
//!   pixel painted twice would get its shadow or its translucent fill twice);
//! - at most [`MAX_RECTS`] rectangles.

use crate::window::Rect;
use alloc::vec::Vec;

/// Most rectangles a region keeps; past it the closest pair is merged into its bounding box.
pub const MAX_RECTS: usize = 12;

/// Two rectangles are merged into their bounding box when the box is at most this many
/// percent of the area of the two together (so 125 allows 25 % of waste).
const MERGE_WASTE_PERCENT: u64 = 125;

/// A set of disjoint, non-empty rectangles.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Region {
    rects: Vec<Rect>,
}

/// Area of `r` in pixels (0 for an empty rectangle).
pub fn area(r: &Rect) -> u64 {
    if r.is_empty() {
        0
    } else {
        r.w as u64 * r.h as u64
    }
}

/// `a` minus `b`: up to four disjoint rectangles covering the part of `a` outside `b`.
pub fn subtract(a: &Rect, b: &Rect, out: &mut Vec<Rect>) {
    let Some(i) = a.intersection(b) else {
        if !a.is_empty() {
            out.push(*a);
        }
        return;
    };
    // Top band, bottom band, then the left and right pieces of the middle rows.
    if i.y > a.y {
        out.push(Rect::new(a.x, a.y, a.w, i.y - a.y));
    }
    if i.bottom() < a.bottom() {
        out.push(Rect::new(a.x, i.bottom(), a.w, a.bottom() - i.bottom()));
    }
    if i.x > a.x {
        out.push(Rect::new(a.x, i.y, i.x - a.x, i.h));
    }
    if i.right() < a.right() {
        out.push(Rect::new(i.right(), i.y, a.right() - i.right(), i.h));
    }
}

/// Do the two rectangles overlap or share an edge segment (so a union of them is gap-free)?
fn touches(a: &Rect, b: &Rect) -> bool {
    a.x <= b.right() && b.x <= a.right() && a.y <= b.bottom() && b.y <= a.bottom()
}

impl Region {
    /// The empty region.
    pub const fn new() -> Self {
        Self { rects: Vec::new() }
    }

    /// A region of one rectangle (empty if `r` is).
    pub fn from_rect(r: Rect) -> Self {
        let mut s = Self::new();
        s.add(r);
        s
    }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rects.len()
    }

    /// The disjoint rectangles.
    pub fn rects(&self) -> &[Rect] {
        &self.rects
    }

    pub fn clear(&mut self) {
        self.rects.clear();
    }

    /// Pixels covered.
    pub fn area(&self) -> u64 {
        self.rects.iter().map(area).sum()
    }

    /// Smallest rectangle covering everything (`None` when empty).
    pub fn bounds(&self) -> Option<Rect> {
        self.rects.iter().copied().reduce(|a, b| a.union(&b))
    }

    /// Does any rectangle contain the pixel `(x, y)`?
    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        self.rects.iter().any(|r| r.contains(x, y))
    }

    /// Does the region overlap `r`?
    pub fn intersects(&self, r: &Rect) -> bool {
        self.rects.iter().any(|q| q.intersection(r).is_some())
    }

    /// Is every pixel of `r` in the region?
    pub fn covers(&self, r: &Rect) -> bool {
        if r.is_empty() {
            return true;
        }
        let mut rest = alloc::vec![*r];
        let mut next = Vec::new();
        for q in &self.rects {
            next.clear();
            for p in &rest {
                subtract(p, q, &mut next);
            }
            core::mem::swap(&mut rest, &mut next);
            if rest.is_empty() {
                return true;
            }
        }
        rest.is_empty()
    }

    /// Add `r` to the region (a superset of the union may result, see the module docs).
    pub fn add(&mut self, r: Rect) {
        if r.is_empty() {
            return;
        }
        // Already inside one rectangle: nothing to do.
        if self.rects.iter().any(|q| contains_rect(q, &r)) {
            return;
        }
        // Rectangles swallowed by the new one go away; the rest are cut out of it.
        self.rects.retain(|q| !contains_rect(&r, q));
        let mut pieces = alloc::vec![r];
        let mut next = Vec::new();
        for q in &self.rects {
            if q.intersection(&r).is_none() {
                continue;
            }
            next.clear();
            for p in &pieces {
                subtract(p, q, &mut next);
            }
            core::mem::swap(&mut pieces, &mut next);
        }
        self.rects.extend(pieces);
        self.coalesce();
    }

    /// Add every rectangle of `other`.
    pub fn add_region(&mut self, other: &Region) {
        for r in &other.rects {
            self.add(*r);
        }
    }

    /// Remove the pixels of `r` from the region.
    pub fn subtract_rect(&mut self, r: &Rect) {
        if r.is_empty() || !self.intersects(r) {
            return;
        }
        let mut out = Vec::with_capacity(self.rects.len() + 3);
        for q in &self.rects {
            subtract(q, r, &mut out);
        }
        self.rects = out;
    }

    /// Remove every pixel of `other`.
    pub fn subtract_region(&mut self, other: &Region) {
        for r in &other.rects {
            self.subtract_rect(r);
        }
    }

    /// The part of the region inside `r`.
    pub fn clipped(&self, r: &Rect) -> Region {
        Region {
            rects: self
                .rects
                .iter()
                .filter_map(|q| q.intersection(r))
                .collect(),
        }
    }

    /// Keep only the part inside `r`.
    pub fn clip_to(&mut self, r: &Rect) {
        self.rects = self
            .rects
            .iter()
            .filter_map(|q| q.intersection(r))
            .collect();
    }

    /// Merge neighbours whose bounding box wastes little, then enforce [`MAX_RECTS`].
    fn coalesce(&mut self) {
        while let Some((i, j)) = self.cheap_pair() {
            self.merge(i, j);
        }
        while self.rects.len() > MAX_RECTS {
            let Some((i, j)) = self.closest_pair() else {
                break;
            };
            self.merge(i, j);
        }
    }

    /// A pair that merges for free (a bigger rectangle exactly) or nearly so.
    fn cheap_pair(&self) -> Option<(usize, usize)> {
        for i in 0..self.rects.len() {
            for j in i + 1..self.rects.len() {
                let (a, b) = (&self.rects[i], &self.rects[j]);
                if !touches(a, b) {
                    continue;
                }
                let sum = area(a) + area(b);
                if area(&a.union(b)) * 100 <= sum * MERGE_WASTE_PERCENT {
                    return Some((i, j));
                }
            }
        }
        None
    }

    /// The pair whose bounding box wastes the fewest pixels.
    fn closest_pair(&self) -> Option<(usize, usize)> {
        let mut best: Option<(u64, usize, usize)> = None;
        for i in 0..self.rects.len() {
            for j in i + 1..self.rects.len() {
                let (a, b) = (&self.rects[i], &self.rects[j]);
                let waste = area(&a.union(b)).saturating_sub(area(a) + area(b));
                if best.is_none_or(|(w, _, _)| waste < w) {
                    best = Some((waste, i, j));
                }
            }
        }
        best.map(|(_, i, j)| (i, j))
    }

    /// Replace rectangles `i` and `j` by their bounding box, growing it over everything it
    /// touches so the rectangles stay disjoint.
    fn merge(&mut self, i: usize, j: usize) {
        let (hi, lo) = if i > j { (i, j) } else { (j, i) };
        let b = self.rects.swap_remove(hi);
        let a = self.rects.swap_remove(lo);
        let mut bb = a.union(&b);
        loop {
            let before = self.rects.len();
            let mut keep = Vec::with_capacity(before);
            for q in self.rects.drain(..) {
                if q.intersection(&bb).is_some() {
                    bb = bb.union(&q);
                } else {
                    keep.push(q);
                }
            }
            let grew = keep.len() < before;
            self.rects = keep;
            if !grew {
                break;
            }
        }
        self.rects.push(bb);
    }
}

fn contains_rect(outer: &Rect, inner: &Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}
