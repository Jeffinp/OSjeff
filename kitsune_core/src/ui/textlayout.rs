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
mod tests {
    use super::*;

    /// 8 px per character, 2 px kern between 'A' and 'V'.
    struct Fake;
    impl Metrics for Fake {
        fn advance_q8(&self, c: char) -> i32 {
            if c == '\u{2026}' { 12 * Q } else { 8 * Q }
        }
        fn kern_q8(&self, l: char, r: char) -> i32 {
            if (l, r) == ('A', 'V') { -2 * Q } else { 0 }
        }
    }

    #[test]
    fn measure_adds_advances_and_kerning() {
        assert_eq!(measure(&Fake, ""), 0);
        assert_eq!(measure(&Fake, "abc"), 24);
        assert_eq!(measure(&Fake, "AV"), 14);
        assert_eq!(measure_q8(&Fake, "AV"), 14 * Q);
    }

    #[test]
    fn fit_prefix_is_exact() {
        assert_eq!(fit_prefix(&Fake, "abcdef", 24), 3);
        assert_eq!(fit_prefix(&Fake, "abcdef", 23), 2);
        assert_eq!(fit_prefix(&Fake, "abcdef", 1000), 6);
        assert_eq!(fit_prefix(&Fake, "abcdef", 0), 0);
        assert_eq!(fit_prefix(&Fake, "abcdef", -5), 0);
        // Multi-byte characters never split.
        assert_eq!(fit_prefix(&Fake, "çãõ", 16), "çã".len());
    }

    #[test]
    fn ellipsize_fits_and_marks() {
        let (s, cut) = ellipsize(&Fake, "hello world", 200);
        assert_eq!((s.as_str(), cut), ("hello world", false));
        let (s, cut) = ellipsize(&Fake, "hello world", 50);
        assert!(cut);
        assert!(s.ends_with('\u{2026}'));
        assert!(measure(&Fake, &s) <= 50, "{s:?} {}", measure(&Fake, &s));
        assert_eq!(s, "hell\u{2026}");
        // No room for the ellipsis itself.
        assert_eq!(ellipsize(&Fake, "hello", 8).0, "");
    }

    #[test]
    fn ellipsize_never_exceeds_the_width() {
        for w in 0..120 {
            let (s, _) = ellipsize(&Fake, "The quick brown fox", w);
            assert!(measure(&Fake, &s) <= w.max(0), "w={w} {s:?}");
        }
    }

    #[test]
    fn middle_ellipsis_keeps_the_extension() {
        let s = ellipsize_middle(&Fake, "relatorio-final-v2.pdf", 120);
        assert!(s.contains('\u{2026}') && s.ends_with(".pdf"), "{s:?}");
        assert!(measure(&Fake, &s) <= 120, "{}", measure(&Fake, &s));
        assert_eq!(ellipsize_middle(&Fake, "a.txt", 120), "a.txt");
    }

    #[test]
    fn wrap_breaks_on_spaces_and_newlines() {
        let t = "one two three\nfour";
        let lines: Vec<&str> = wrap(&Fake, t, 8 * 8, 10)
            .into_iter()
            .map(|(a, b)| &t[a..b])
            .collect();
        assert_eq!(lines, ["one two", "three", "four"]);
        // A long word is split by characters.
        let t = "abcdefghijkl";
        let lines: Vec<&str> = wrap(&Fake, t, 40, 10)
            .into_iter()
            .map(|(a, b)| &t[a..b])
            .collect();
        assert_eq!(lines, ["abcde", "fghij", "kl"]);
        assert_eq!(wrap(&Fake, "a b c d e f g h", 16, 2).len(), 2);
    }
}
