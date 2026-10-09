//! Text measuring, truncation and wrapping over any advance-width source.
//!
//! Pure and allocation-light: the kernel's glyph cache implements [`Metrics`]
//! (see [`crate::ui::fontcache`]); tests use fake metrics. Widths are in 24.8 fixed
//! point ("q8": 256 = one pixel) so fractional advances do not drift; the
//! `*_px` helpers round up to whole pixels.

use alloc::string::String;
use alloc::vec::Vec;

/// One pixel in q8 units.
pub const Q: i32 = 256;

/// Source of horizontal metrics for one face at one size.
pub trait Metrics {
    /// Advance of `c` in q8 pixels.
    fn advance_q8(&self, c: char) -> i32;
    /// Kerning between two adjacent characters in q8 pixels (usually <= 0).
    fn kern_q8(&self, left: char, right: char) -> i32;
}

/// The character drawn when text is cut.
pub const ELLIPSIS: char = '\u{2026}';

/// Width of `text` in q8 pixels, kerning included.
pub fn measure_q8<M: Metrics + ?Sized>(m: &M, text: &str) -> i32 {
    let mut w = 0i32;
    let mut prev: Option<char> = None;
    for c in text.chars() {
        if let Some(p) = prev {
            w += m.kern_q8(p, c);
        }
        w += m.advance_q8(c);
        prev = Some(c);
    }
    w
}

/// Width of `text` in whole pixels (rounded up).
pub fn measure<M: Metrics + ?Sized>(m: &M, text: &str) -> i32 {
    (measure_q8(m, text) + Q - 1) / Q
}

/// Byte length of the longest prefix of `text` that fits in `max_px`.
pub fn fit_prefix<M: Metrics + ?Sized>(m: &M, text: &str, max_px: i32) -> usize {
    let limit = max_px.max(0) * Q;
    let mut w = 0i32;
    let mut prev: Option<char> = None;
    for (i, c) in text.char_indices() {
        let k = prev.map_or(0, |p| m.kern_q8(p, c));
        let next = w + k + m.advance_q8(c);
        if next > limit {
            return i;
        }
        w = next;
        prev = Some(c);
    }
    text.len()
}

/// `text` cut to fit `max_px`, ending in an ellipsis when it had to be cut.
/// Returns `(string, was_truncated)`. A width too small for even the ellipsis
/// yields an empty string.
pub fn ellipsize<M: Metrics + ?Sized>(m: &M, text: &str, max_px: i32) -> (String, bool) {
    if measure(m, text) <= max_px {
        return (String::from(text), false);
    }
    let room = max_px - measure(m, "\u{2026}");
    if room < 0 {
        return (String::new(), true);
    }
    let cut = fit_prefix(m, text, room);
    let mut s = String::from(text[..cut].trim_end());
    s.push(ELLIPSIS);
    (s, true)
}

/// Like [`ellipsize`] but keeps the tail and the head and cuts the middle:
/// file names keep their extension (`report-fina...v2.pdf`).
pub fn ellipsize_middle<M: Metrics + ?Sized>(m: &M, text: &str, max_px: i32) -> String {
    if measure(m, text) <= max_px {
        return String::from(text);
    }
    let room = max_px - measure(m, "\u{2026}");
    if room < 0 {
        return String::new();
    }
    let head_room = room * 2 / 3;
    let head = fit_prefix(m, text, head_room);
    // Grow the tail from the end while it fits in what is left.
    let tail_room = room - measure(m, &text[..head]);
    let mut tail = text.len();
    for (i, _) in text.char_indices().rev() {
        if i < head {
            break;
        }
        if measure(m, &text[i..]) > tail_room {
            break;
        }
        tail = i;
    }
    let mut s = String::from(&text[..head]);
    s.push(ELLIPSIS);
    s.push_str(&text[tail..]);
    s
}

/// Word-wrap `text` into lines of at most `max_px` pixels. Returns byte ranges
/// into `text`. Hard newlines break lines; a word longer than the line is split
/// by characters. At most `max_lines` lines are returned (the rest is dropped).
pub fn wrap<M: Metrics + ?Sized>(
    m: &M,
    text: &str,
    max_px: i32,
    max_lines: usize,
) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0usize;
    while start <= text.len() && out.len() < max_lines {
        let rest = &text[start..];
        let nl = rest.find('\n').unwrap_or(rest.len());
        let para = &rest[..nl];
        if measure(m, para) <= max_px {
            out.push((start, start + nl));
        } else {
            let fit =
                fit_prefix(m, para, max_px).max(para.chars().next().map_or(0, char::len_utf8));
            // Prefer the last space inside the fitting prefix.
            let cut = para[..fit].rfind(' ').filter(|&i| i > 0).unwrap_or(fit);
            out.push((start, start + cut));
            start += cut;
            // Skip the space we broke at.
            if text[start..].starts_with(' ') {
                start += 1;
            }
            continue;
        }
        start += nl + 1;
        if nl == rest.len() {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests;
