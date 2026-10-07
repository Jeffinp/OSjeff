//! Operations on the text of a laid-out [`Page`]: find in page (Ctrl+F) and
//! mouse selection with copy.
//!
//! Both work on the display list's words ([`Cmd::Text`]) in document order.
//! Reading order is the order of the commands; two words belong to the same
//! line when they share the same `y`.

use super::layout::{Cmd, Page};
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

struct Tw<'a> {
    x: i32,
    y: i32,
    cw: i32,
    h: i32,
    text: &'a str,
}

impl Tw<'_> {
    fn w(&self) -> i32 {
        self.cw * self.text.chars().count() as i32
    }
}

impl Page {
    fn text_words(&self) -> Vec<Tw<'_>> {
        self.cmds
            .iter()
            .filter_map(|c| match c {
                Cmd::Text {
                    x, y, text, scale, ..
                } if !text.is_empty() => Some(Tw {
                    x: *x,
                    y: *y,
                    cw: 6 * i32::from(*scale),
                    h: 9 * i32::from(*scale),
                    text: text.as_str(),
                }),
                _ => None,
            })
            .collect()
    }

    /// Number of words of text on the page.
    pub fn word_count(&self) -> usize {
        self.text_words().len()
    }

    /// Every case-insensitive (ASCII) occurrence of `needle` in the page's
    /// text, in reading order. A match may span several words (and lines): it
    /// is returned as one [`Span`] per word touched. Words are separated by one
    /// space for matching, so `"quick brown"` finds the two words even across
    /// a line wrap. Empty needle: no matches. At most [`MAX_MATCHES`].
    pub fn find(&self, needle: &str) -> Vec<Vec<Span>> {
        let needle: Vec<char> = needle
            .chars()
            .map(|c| c.to_ascii_lowercase())
            .collect::<Vec<_>>();
        if needle.is_empty() || needle.len() > 200 {
            return Vec::new();
        }
        let words = self.text_words();
        // The haystack: (lower-case char, word, char offset in the word); a
        // separator has word == usize::MAX.
        let mut hay: Vec<(char, usize, usize)> = Vec::new();
        for (wi, w) in words.iter().enumerate() {
            if hay.len() > MAX_SEARCH_CHARS {
                break;
            }
            if !hay.is_empty() {
                hay.push((' ', usize::MAX, 0));
            }
            for (ci, ch) in w.text.chars().enumerate() {
                hay.push((ch.to_ascii_lowercase(), wi, ci));
            }
        }
        let mut out: Vec<Vec<Span>> = Vec::new();
        let n = needle.len();
        let mut i = 0;
        while i + n <= hay.len() && out.len() < MAX_MATCHES {
            if hay[i..i + n].iter().zip(&needle).all(|(h, q)| h.0 == *q) {
                let mut spans: Vec<Span> = Vec::new();
                let mut cur: Option<(usize, usize, usize)> = None; // word, first, last char
                for &(_, wi, ci) in &hay[i..i + n] {
                    if wi == usize::MAX {
                        continue;
                    }
                    match &mut cur {
                        Some((w, _, last)) if *w == wi => *last = ci,
                        _ => {
                            if let Some((w, a, b)) = cur.take() {
                                spans.push(span_of(&words[w], a, b));
                            }
                            cur = Some((wi, ci, ci));
                        }
                    }
                }
                if let Some((w, a, b)) = cur {
                    spans.push(span_of(&words[w], a, b));
                }
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

    /// The word at or just before page point `(x, y)` in reading order: the
    /// word under the point if any, else the last word that ends above it or
    /// on its line to its left. `None` on a page without text.
    fn word_index_at(words: &[Tw<'_>], x: i32, y: i32) -> Option<usize> {
        if words.is_empty() {
            return None;
        }
        let mut best = 0;
        for (i, w) in words.iter().enumerate() {
            let before = w.y + w.h <= y || (y >= w.y && y < w.y + w.h && w.x <= x);
            if before {
                best = i;
            }
        }
        Some(best)
    }

    /// The inclusive range of word indices selected by dragging from page
    /// point `a` to page point `b`. `None` when the points coincide or there
    /// is no text.
    pub fn select(&self, a: (i32, i32), b: (i32, i32)) -> Option<(usize, usize)> {
        if a == b {
            return None;
        }
        let words = self.text_words();
        let ia = Self::word_index_at(&words, a.0, a.1)?;
        let ib = Self::word_index_at(&words, b.0, b.1)?;
        Some((ia.min(ib), ia.max(ib)))
    }

    /// The boxes of the words in `range` (inclusive word indices).
    pub fn selection_spans(&self, range: (usize, usize)) -> Vec<Span> {
        self.text_words()
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= range.0 && *i <= range.1)
            .map(|(_, w)| Span {
                x: w.x,
                y: w.y,
                w: w.w(),
                h: w.h,
            })
            .collect()
    }

    /// The text of `range`: words separated by a space, a newline where the
    /// line changes.
    pub fn selection_text(&self, range: (usize, usize)) -> String {
        let words = self.text_words();
        let mut out = String::new();
        let mut last_y: Option<i32> = None;
        for (i, w) in words.iter().enumerate() {
            if i < range.0 || i > range.1 {
                continue;
            }
            match last_y {
                Some(y) if y == w.y => out.push(' '),
                Some(_) => out.push('\n'),
                None => {}
            }
            out.push_str(w.text);
            last_y = Some(w.y);
        }
        out
    }
}

fn span_of(w: &Tw<'_>, first: usize, last: usize) -> Span {
    Span {
        x: w.x + first as i32 * w.cw,
        y: w.y,
        w: (last - first + 1) as i32 * w.cw,
        h: w.h,
    }
}
