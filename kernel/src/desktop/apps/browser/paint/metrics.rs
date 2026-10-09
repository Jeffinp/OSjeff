//! Text metrics and glyph drawing for the web engine: the kernel half of its text measuring.

use crate::desktop::*;
use crate::text::{self, Weight};
use kitsune_core::web::{Font, Rgb, TextMetrics};

/// Text metrics backed by the kernel's glyph engine.
pub(crate) struct KernelMetrics;

/// The face and size the engine's [`Font`] stands for.
pub(crate) fn face_of(f: Font) -> (u16, Weight) {
    let w = if f.mono {
        Weight::Mono
    } else if f.bold {
        Weight::Semibold
    } else {
        Weight::Regular
    };
    (f.size.clamp(4, 400), w)
}

/// Width of a character without a glyph, as a box: 7/10 em.
fn tofu_q8(px: u16) -> i32 {
    i32::from(px) * 256 * 7 / 10
}

/// Is `c` drawn by the face (as opposed to a box or nothing)?
fn drawable(c: char, w: Weight) -> bool {
    text::has_glyph(c, w)
}

impl TextMetrics for KernelMetrics {
    fn width(&self, t: &str, f: Font) -> i32 {
        (self.width_q8(t, f) + 255) / 256
    }

    fn width_q8(&self, t: &str, f: Font) -> i32 {
        let (px, w) = face_of(f);
        // The usual case: text the face has entirely.
        if t.bytes().all(|b| (0x20..0x7f).contains(&b)) {
            return text::measure_q8(t, px, w);
        }
        let mut total = 0i32;
        let mut start = 0;
        let mut in_run = true;
        for (i, ch) in t.char_indices() {
            let zero = kitsune_core::web::metrics::is_zero_width(ch);
            let ok = zero || drawable(ch, w);
            if ok != in_run {
                if in_run {
                    total = total.saturating_add(text::measure_q8(&t[start..i], px, w));
                }
                start = i;
                in_run = ok;
            }
            if !ok {
                total = total.saturating_add(tofu_q8(px));
                start = i + ch.len_utf8();
                in_run = true;
            }
        }
        if in_run && start < t.len() {
            total = total.saturating_add(text::measure_q8(&t[start..], px, w));
        }
        total
    }

    fn line_height(&self, f: Font) -> i32 {
        let (px, w) = face_of(f);
        text::vmetrics(px, w).line_height
    }

    fn ascent(&self, f: Font) -> i32 {
        let (px, w) = face_of(f);
        text::vmetrics(px, w).ascent
    }

    fn has_glyph(&self, c: char, f: Font) -> bool {
        drawable(c, face_of(f).1)
    }
}

/// Convert a `web` engine color to a framebuffer color.
pub(super) fn rgb(c: Rgb) -> Color {
    Color::rgb(c.0, c.1, c.2)
}

/// `a` moved towards black by `t`/256 (a darker link on hover).
pub(super) fn darker(a: Color, t: u16) -> Color {
    a.lerp(Color::rgb(0, 0, 0), t)
}

/// Draw one run of page text with its top at `y`: slanted when italic, boxes for the
/// characters the face lacks. Returns the pen advance.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_run(c: &mut Canvas, x: i32, y: i32, t: &str, f: Font, color: Color) -> i32 {
    let (px, w) = face_of(f);
    let put = |c: &mut Canvas, x: i32, s: &str| -> i32 {
        if f.mono && !f.italic {
            text::draw(c, x, y, s, px, w, color)
        } else if f.italic {
            text::draw_slanted(c, x, y, s, px, w, color, 256)
        } else {
            text::draw(c, x, y, s, px, w, color)
        }
    };
    if t.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return put(c, x, t);
    }
    let mut pen = x;
    let mut seg_start = 0;
    let flush = |c: &mut Canvas, pen: &mut i32, from: usize, to: usize| {
        if to > from {
            *pen += put(c, *pen, &t[from..to]);
        }
    };
    for (i, ch) in t.char_indices() {
        if kitsune_core::web::metrics::is_zero_width(ch) {
            flush(c, &mut pen, seg_start, i);
            seg_start = i + ch.len_utf8();
        } else if !drawable(ch, w) {
            flush(c, &mut pen, seg_start, i);
            seg_start = i + ch.len_utf8();
            // A hollow box the height of a capital.
            let v = text::vmetrics(px, w);
            let bw = (tofu_q8(px) + 255) / 256;
            let bx = pen + 1.max(px as i32 / 12);
            let by = y + v.ascent - v.cap_height;
            let r = Rect::new(
                bx,
                by,
                (bw - 2 * 1.max(px as i32 / 12)).max(2),
                v.cap_height,
            );
            c.stroke_rrect(r, 1, Corner::Circle, color, 200);
            pen += bw;
        }
    }
    flush(c, &mut pen, seg_start, t.len());
    pen - x
}
