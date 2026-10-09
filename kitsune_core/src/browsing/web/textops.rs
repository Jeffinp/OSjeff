//! Operations on the text of a laid-out [`Page`]: find in page (Ctrl+F) and
//! mouse selection with copy.
//!
//! Both work on the display list's text runs ([`Cmd::Text`]) in document order.
//! Positions inside a run are byte offsets at character boundaries, converted to
//! pixels by measuring the prefix with the same [`TextMetrics`] the layout used,
//! so highlights sit exactly under the glyphs whatever the face. Two runs belong
//! to the same line when they share the same `y`.

use super::layout::{Cmd, Page};
use super::metrics::{Font, TextMetrics};
use alloc::string::String;
use alloc::vec::Vec;

/// A rectangle in page coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Most matches reported for one search.
pub const MAX_MATCHES: usize = 1000;
/// Most characters of page text searched (a hostile page cannot stall a search).
pub const MAX_SEARCH_CHARS: usize = 400_000;

/// A position in the page's text: the run (in display-list order) and a byte
/// offset into it, always on a character boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CharPos {
    pub run: usize,
    pub off: usize,
}

/// A selected stretch of text, `start <= end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub start: CharPos,
    pub end: CharPos,
}

struct Tr<'a> {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    font: Font,
    text: &'a str,
}

/// Pixel width of the first `off` bytes of a run, rounded to the nearest pixel.
fn prefix_px(m: &dyn TextMetrics, r: &Tr<'_>, off: usize) -> i32 {
    if off == 0 {
        return 0;
    }
    if off >= r.text.len() {
        return r.w;
    }
    ((m.width_q8(&r.text[..off], r.font) + 128) / 256).min(r.w)
}

impl Page {
    fn text_runs(&self) -> Vec<Tr<'_>> {
        self.cmds
            .iter()
            .filter_map(|c| match c {
                Cmd::Text {
                    x,
                    y,
                    w,
                    h,
                    text,
                    font,
                    ..
                } if !text.is_empty() => Some(Tr {
                    x: *x,
                    y: *y,
                    w: *w,
                    h: *h,
                    font: *font,
                    text: text.as_str(),
                }),
                _ => None,
            })
            .collect()
    }

    /// Number of text runs on the page.
    pub fn word_count(&self) -> usize {
        self.cmds
            .iter()
            .filter(|c| matches!(c, Cmd::Text { text, .. } if !text.is_empty()))
            .count()
    }

    /// Every case-insensitive occurrence of `needle` in the page's text, in
    /// reading order. A match may span several runs (and lines): it is returned as
    /// one [`Span`] per run touched. Runs are separated by one space for matching,
    /// so `"quick brown"` finds the two words even across a line wrap. Empty
    /// needle: no matches. At most [`MAX_MATCHES`].
    pub fn find(&self, needle: &str, m: &dyn TextMetrics) -> Vec<Vec<Span>> {
        let needle: Vec<char> = needle.chars().map(fold_one).collect();
        if needle.is_empty() || needle.len() > 200 {
            return Vec::new();
        }
        let runs = self.text_runs();
        // The haystack: (folded char, run, byte offset in the run, byte length); a
        // separator has run == usize::MAX.
        let mut hay: Vec<(char, usize, usize, usize)> = Vec::new();
        for (ri, r) in runs.iter().enumerate() {
            if hay.len() > MAX_SEARCH_CHARS {
                break;
            }
            if !hay.is_empty() {
                hay.push((' ', usize::MAX, 0, 0));
            }
            for (off, ch) in r.text.char_indices() {
                // One lower-case character per character keeps offsets simple.
                hay.push((fold_one(ch), ri, off, ch.len_utf8()));
            }
        }
        let mut out: Vec<Vec<Span>> = Vec::new();
        let n = needle.len();
        let mut i = 0;
        while i + n <= hay.len() && out.len() < MAX_MATCHES {
            if hay[i..i + n].iter().zip(&needle).all(|(h, q)| h.0 == *q) {
                // Group the matched characters by run: (run, first byte, end byte).
                let mut groups: Vec<(usize, usize, usize)> = Vec::new();
                for &(_, ri, off, len) in &hay[i..i + n] {
                    if ri == usize::MAX {
                        continue;
                    }
                    match groups.last_mut() {
                        Some((r, _, end)) if *r == ri => *end = off + len,
                        _ => groups.push((ri, off, off + len)),
                    }
                }
                let spans: Vec<Span> = groups
                    .into_iter()
                    .map(|(ri, a, b)| span_of(m, &runs[ri], a, b))
                    .collect();
                if !spans.is_empty() {
                    out.push(spans);
                }
                i += n; // non-overlapping, like browsers
            } else {
                i += 1;
            }
        }
        out
    }

    /// The text position nearest page point `(x, y)` in reading order: inside the
    /// run under the point, at the end of the last run above it, or at the start of
    /// the first run on its line when the point is left of everything there.
    fn pos_at(runs: &[Tr<'_>], m: &dyn TextMetrics, x: i32, y: i32) -> Option<CharPos> {
        if runs.is_empty() {
            return None;
        }
        let mut best: Option<(usize, bool)> = None; // run, on the pointer's line
        let mut first_on_line: Option<usize> = None;
        for (i, r) in runs.iter().enumerate() {
            let on_line = y >= r.y && y < r.y + r.h;
            if on_line {
                if x >= r.x {
                    best = Some((i, true));
                } else if first_on_line.is_none() {
                    first_on_line = Some(i);
                }
            } else if r.y + r.h <= y {
                best = Some((i, false));
            }
        }
        let (run, on_line) = match (best, first_on_line) {
            (Some((i, true)), _) => (i, true),
            (_, Some(f)) => {
                return Some(CharPos { run: f, off: 0 });
            }
            (Some((i, false)), None) => (i, false),
            (None, None) => return Some(CharPos { run: 0, off: 0 }),
        };
        let r = &runs[run];
        let off = if on_line && x < r.x + r.w {
            offset_at(m, r, x - r.x)
        } else {
            r.text.len()
        };
        Some(CharPos { run, off })
    }

    /// The selection made by dragging from page point `a` to page point `b`.
    /// `None` when the points coincide, there is no text, or nothing lies between.
    pub fn select(&self, a: (i32, i32), b: (i32, i32), m: &dyn TextMetrics) -> Option<Selection> {
        if a == b {
            return None;
        }
        let runs = self.text_runs();
        let pa = Self::pos_at(&runs, m, a.0, a.1)?;
        let pb = Self::pos_at(&runs, m, b.0, b.1)?;
        let (start, end) = if pa <= pb { (pa, pb) } else { (pb, pa) };
        (start != end).then_some(Selection { start, end })
    }

    /// The whole run containing page point `(x, y)` as a selection (a double
    /// click picks the word instead, see [`Page::select_word`]).
    pub fn select_word(&self, x: i32, y: i32, m: &dyn TextMetrics) -> Option<Selection> {
        let runs = self.text_runs();
        let p = Self::pos_at(&runs, m, x, y)?;
        let r = runs.get(p.run)?;
        if !(y >= r.y && y < r.y + r.h && x >= r.x && x < r.x + r.w) {
            return None;
        }
        let t = r.text;
        let off = p.off.min(t.len());
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let start = t[..off]
            .char_indices()
            .rev()
            .take_while(|&(_, c)| is_word(c))
            .last()
            .map_or(off, |(i, _)| i);
        let end = t[off..]
            .char_indices()
            .find(|&(_, c)| !is_word(c))
            .map_or(t.len(), |(i, _)| off + i);
        (start < end).then_some(Selection {
            start: CharPos {
                run: p.run,
                off: start,
            },
            end: CharPos {
                run: p.run,
                off: end,
            },
        })
    }

    /// The boxes of the selected text, one per run touched.
    pub fn selection_spans(&self, sel: &Selection, m: &dyn TextMetrics) -> Vec<Span> {
        let runs = self.text_runs();
        let mut out = Vec::new();
        for ri in sel.start.run..=sel.end.run.min(runs.len().saturating_sub(1)) {
            let Some(r) = runs.get(ri) else { break };
            let a = if ri == sel.start.run {
                sel.start.off
            } else {
                0
            };
            let b = if ri == sel.end.run {
                sel.end.off
            } else {
                r.text.len()
            };
            if a < b
                && b <= r.text.len()
                && r.text.is_char_boundary(a)
                && r.text.is_char_boundary(b)
            {
                out.push(span_of(m, r, a, b));
            }
        }
        out
    }

    /// The selected text: runs on one line joined by a space, a newline where the
    /// line changes.
    pub fn selection_text(&self, sel: &Selection) -> String {
        let runs = self.text_runs();
        let mut out = String::new();
        let mut last_y: Option<i32> = None;
        for ri in sel.start.run..=sel.end.run.min(runs.len().saturating_sub(1)) {
            let Some(r) = runs.get(ri) else { break };
            let a = if ri == sel.start.run {
                sel.start.off
            } else {
                0
            };
            let b = if ri == sel.end.run {
                sel.end.off
            } else {
                r.text.len()
            };
            let Some(piece) = r.text.get(a..b.min(r.text.len())) else {
                continue;
            };
            match last_y {
                Some(y) if y == r.y => out.push(' '),
                Some(_) => out.push('\n'),
                None => {}
            }
            out.push_str(piece);
            last_y = Some(r.y);
        }
        out
    }

    /// Does `sel` still point into this page (after a relayout)?
    pub fn selection_valid(&self, sel: &Selection) -> bool {
        sel.end.run < self.word_count()
    }
}

/// The byte offset of the character boundary of `r` nearest to `dx` pixels from
/// its left edge (binary search over prefix widths).
fn offset_at(m: &dyn TextMetrics, r: &Tr<'_>, dx: i32) -> usize {
    let bounds: Vec<usize> = r
        .text
        .char_indices()
        .map(|(i, _)| i)
        .skip(1)
        .chain([r.text.len()])
        .collect();
    let at = |k: usize| -> i32 {
        // Width before boundary k-1 (k = 0 is the start).
        if k == 0 {
            0
        } else {
            m.width_q8(&r.text[..bounds[k - 1]], r.font)
        }
    };
    let x = dx.max(0).saturating_mul(256);
    // Largest k (0..=bounds.len()) with at(k) <= x.
    let (mut lo, mut hi) = (0usize, bounds.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if at(mid) <= x {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    // Snap to the nearer of the two boundaries around x.
    if lo < bounds.len() {
        let (a, b) = (at(lo), at(lo + 1));
        if x - a > b - x {
            lo += 1;
        }
    }
    if lo == 0 { 0 } else { bounds[lo - 1] }
}

fn span_of(m: &dyn TextMetrics, r: &Tr<'_>, a: usize, b: usize) -> Span {
    let x0 = prefix_px(m, r, a);
    let x1 = prefix_px(m, r, b);
    Span {
        x: r.x + x0,
        y: r.y,
        w: (x1 - x0).max(1),
        h: r.h,
    }
}

/// Lower-case `c` to exactly one character (the haystack keeps byte offsets).
fn fold_one(c: char) -> char {
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}
