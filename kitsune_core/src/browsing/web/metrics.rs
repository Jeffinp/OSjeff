//! Text measuring for the layout engine.
//!
//! The engine never assumes a character cell: every width comes from a
//! [`TextMetrics`] supplied by the caller. The kernel implements it over the
//! same glyph engine that paints the page (Inter for text, JetBrains Mono for
//! `<pre>` and `<code>`); the host tests use [`FixedAdvance`], a deterministic
//! stand-in with exact integer advances.

/// One face of the page text: pixel size and the style bits layout cares about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Font {
    /// Size in pixels (the page zoom is already applied).
    pub size: u16,
    /// Bold weight (the kernel maps it to Inter Semibold).
    pub bold: bool,
    /// Italic. The kernel has no italic face: it slants the regular one.
    pub italic: bool,
    /// Monospace face (`<pre>`, `<code>`, `font-family: monospace`).
    pub mono: bool,
}

impl Font {
    pub const fn new(size: u16) -> Font {
        Font {
            size,
            bold: false,
            italic: false,
            mono: false,
        }
    }
}

/// Source of text metrics for one page layout.
pub trait TextMetrics {
    /// Width in whole pixels (rounded up) of `text` set in `f`. Characters that
    /// have no glyph still need a width (the painter draws a box for them).
    fn width(&self, text: &str, f: Font) -> i32;
    /// [`width`](Self::width) in 1/256 pixel, for sums that must not drift. Defaults to
    /// the whole-pixel width.
    fn width_q8(&self, text: &str, f: Font) -> i32 {
        self.width(text, f).saturating_mul(256)
    }
    /// Height of the natural line box of `f`: ascent plus descent plus line gap.
    fn line_height(&self, f: Font) -> i32;
    /// Distance from the top of the natural line box to the baseline.
    fn ascent(&self, f: Font) -> i32;
    /// Does the face have a glyph for `c`? Layout does not depend on it (widths
    /// already account for the fallback box); the painter and the tests do.
    fn has_glyph(&self, _c: char, _f: Font) -> bool {
        true
    }
}

/// Characters that take no room and draw nothing.
pub fn is_zero_width(c: char) -> bool {
    matches!(
        c,
        '\u{200b}'..='\u{200f}'
            | '\u{2060}'
            | '\u{feff}'
            | '\u{fe00}'..='\u{fe0f}'
            | '\u{0300}'..='\u{036f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
    )
}

/// A deterministic metrics source for tests and fuzzing: every character of a
/// proportional face advances `size / 2` pixels (`size * 11 / 20` when bold),
/// every character of the monospace face `size * 3 / 5`, and a line is
/// `size * 6 / 5` tall with the baseline `size` below its top.
#[derive(Clone, Copy, Debug, Default)]
pub struct FixedAdvance;

impl FixedAdvance {
    /// Advance of one character of `f`.
    pub fn advance(f: Font) -> i32 {
        let s = i32::from(f.size);
        if f.mono {
            (s * 3 + 2) / 5
        } else if f.bold {
            (s * 11 + 10) / 20
        } else {
            (s + 1) / 2
        }
    }
}

impl TextMetrics for FixedAdvance {
    fn width(&self, text: &str, f: Font) -> i32 {
        let n = text.chars().filter(|&c| !is_zero_width(c)).count() as i64;
        (n * i64::from(Self::advance(f))).min(1_000_000) as i32
    }

    fn line_height(&self, f: Font) -> i32 {
        (i32::from(f.size) * 6 + 2) / 5
    }

    fn ascent(&self, f: Font) -> i32 {
        i32::from(f.size)
    }
}

#[cfg(test)]
mod tests;
