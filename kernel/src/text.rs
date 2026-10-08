//! Proportional, anti-aliased UI text (Inter, SIL OFL) over the framebuffer.
//!
//! The engine itself (TrueType reader, rasteriser, glyph cache, measuring) is
//! pure and lives in `osjeff_core` (`ttf`, `glyph`, `fontcache`, `textlayout`).
//! This module owns the one global engine, the text gamma tables and the
//! `Canvas` drawing helpers. Only the compositor thread draws UI text, so the
//! lazily filled cache needs no lock (see the SAFETY note on [`engine`]).
//!
//! Sizes used by the UI: 11, 12, 13, 15, 17, 22, 28 (see `docs/design/ui-macos.md`);
//! any size from 8 to 64 works and is cached on first use. The 8x8 bitmap font in
//! `font.rs` remains only for the terminal and editor grids.

// Toolkit module: the whole API is public for the apps (wave 2) even where the chrome
// does not call it yet.
#![allow(dead_code)]

use crate::fb::{Canvas, Color};
use crate::sync::RacyCell;
use osjeff_core::Rect;
pub use osjeff_core::fontcache::Weight;
use osjeff_core::fontcache::{Stats, TextEngine, VMetrics};
use osjeff_core::gfx::luma;
use osjeff_core::textlayout;

static REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.subset.ttf");
static MEDIUM: &[u8] = include_bytes!("../../assets/fonts/Inter-Medium.subset.ttf");
static SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.subset.ttf");

/// UI type scale (pixels).
pub const CAPTION: u16 = 11;
pub const FOOTNOTE: u16 = 12;
pub const BODY: u16 = 13;
pub const CALLOUT: u16 = 15;
pub const TITLE3: u16 = 17;
pub const TITLE2: u16 = 22;
pub const TITLE1: u16 = 28;

static ENGINE: RacyCell<Option<TextEngine>> = RacyCell::new(None);

/// Coverage -> opacity tables. Dark text on a light surface is boosted (it looks
/// thin when blended in sRGB space), light text on a dark surface is left alone.
const fn gamma_lut(boost_percent: i32) -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut c = 0i32;
    while c < 256 {
        let d = c * (255 - c) * boost_percent / (100 * 255);
        let v = c + d;
        t[c as usize] = if v < 0 {
            0
        } else if v > 255 {
            255
        } else {
            v as u8
        };
        c += 1;
    }
    t
}
static LUT_DARK_TEXT: [u8; 256] = gamma_lut(42);
static LUT_LIGHT_TEXT: [u8; 256] = gamma_lut(-6);

/// Build the engine and rasterise the common faces. Returns the elapsed
/// microseconds so boot can log them. Call once, before the first frame.
pub fn init(tsc_khz: u64) -> (u64, Stats) {
    let t0 = crate::io::rdtsc();
    let mut eng = TextEngine::new([REGULAR, MEDIUM, SEMIBOLD]);
    if let Some(e) = eng.as_mut() {
        // The faces the chrome shows on every frame; the rest fill on first use.
        for (w, px) in [
            (Weight::Regular, BODY),
            (Weight::Medium, BODY),
            (Weight::Semibold, BODY),
            (Weight::Regular, FOOTNOTE),
            (Weight::Regular, CAPTION),
            (Weight::Regular, CALLOUT),
        ] {
            e.prewarm(w, px);
        }
    }
    let stats = eng.as_ref().map(TextEngine::stats).unwrap_or_default();
    // SAFETY: boot thread, before the compositor (the only other user) runs.
    unsafe { *ENGINE.get() = eng };
    let us = crate::io::rdtsc().wrapping_sub(t0) * 1000 / tsc_khz.max(1);
    (us, stats)
}

/// Usage counters (glyphs rasterised, bytes in the glyph arena).
pub fn stats() -> Stats {
    engine().map(|e| e.stats()).unwrap_or_default()
}

fn engine() -> Option<&'static mut TextEngine> {
    // SAFETY: only the compositor thread calls this (UI text is never drawn from
    // another thread) and no reference from a previous call is alive across calls
    // (every user below finishes with the engine before returning).
    // NOTE: not guaranteed by the type: a safe fn returning `&'static mut`.
    unsafe { (*ENGINE.get()).as_mut() }
}

/// Vertical metrics of a face.
pub fn vmetrics(px: u16, w: Weight) -> VMetrics {
    engine().map_or(
        VMetrics {
            ascent: px as i32,
            descent: px as i32 / 4,
            line_height: px as i32 * 5 / 4,
            cap_height: px as i32 * 7 / 10,
            x_height: px as i32 / 2,
        },
        |e| e.vmetrics(w, px),
    )
}

/// Height of a text line at `px` (ascent + descent).
pub fn line_height(px: u16) -> i32 {
    vmetrics(px, Weight::Regular).line_height
}

/// Width in pixels of `text`.
pub fn measure(text: &str, px: u16, w: Weight) -> i32 {
    match engine() {
        Some(e) => textlayout::measure(&e.face(w, px), text),
        None => text.chars().count() as i32 * px as i32 / 2,
    }
}

/// Top of the line box that vertically centres the cap height of `px` text in a
/// band of height `h` starting at `y`.
pub fn center_y(y: i32, h: i32, px: u16, w: Weight) -> i32 {
    let v = vmetrics(px, w);
    y + (h - v.cap_height + 1) / 2 - (v.ascent - v.cap_height)
}

/// `text` shortened with an ellipsis to fit `max_w` pixels.
pub fn ellipsize(text: &str, px: u16, w: Weight, max_w: i32) -> alloc::string::String {
    match engine() {
        Some(e) => textlayout::ellipsize(&e.face(w, px), text, max_w).0,
        None => alloc::string::String::from(text),
    }
}

/// Middle-ellipsis version (file names keep their extension).
pub fn ellipsize_middle(text: &str, px: u16, w: Weight, max_w: i32) -> alloc::string::String {
    match engine() {
        Some(e) => textlayout::ellipsize_middle(&e.face(w, px), text, max_w),
        None => alloc::string::String::from(text),
    }
}

/// Byte ranges of `text` word-wrapped into `max_w` pixels (at most `max_lines`).
pub fn wrap(
    text: &str,
    px: u16,
    w: Weight,
    max_w: i32,
    max_lines: usize,
) -> alloc::vec::Vec<(usize, usize)> {
    match engine() {
        Some(e) => textlayout::wrap(&e.face(w, px), text, max_w, max_lines),
        None => alloc::vec![(0, text.len())],
    }
}

/// Draw `text` with the top of its line box at `(x, y)`. `alpha` is 0..=256.
/// Returns the pen advance in pixels. Clipped by the canvas clip.
#[allow(clippy::too_many_arguments)]
pub fn draw_a(
    c: &mut Canvas,
    x: i32,
    y: i32,
    text: &str,
    px: u16,
    w: Weight,
    color: Color,
    alpha: u16,
) -> i32 {
    let t0 = crate::trace::t();
    let r = draw_inner(c, x, y, text, px, w, color, alpha);
    crate::trace::prim(crate::trace::Prim::Glyph, t0);
    r
}

#[allow(clippy::too_many_arguments)]
fn draw_inner(
    c: &mut Canvas,
    x: i32,
    y: i32,
    text: &str,
    px: u16,
    w: Weight,
    color: Color,
    alpha: u16,
) -> i32 {
    let Some(e) = engine() else {
        return 0;
    };
    let lut = if luma(color) > 140 {
        &LUT_LIGHT_TEXT
    } else {
        &LUT_DARK_TEXT
    };
    let base = y + e.vmetrics(w, px).ascent;
    let clip = c.clip_rect();
    let mut pen = x * 256;
    let mut prev: Option<char> = None;
    for ch in text.chars() {
        if let Some(p) = prev {
            pen += e.kern_q8(w, px, p, ch);
        }
        let g = e.glyph(w, px, ch);
        if g.w > 0 {
            let gx = ((pen + 128) >> 8) + g.left as i32;
            // Skip glyphs wholly outside the clip without touching the cache bytes.
            if gx < clip.right() && gx + g.w as i32 > clip.x {
                let gy = base - g.top as i32;
                if gy < clip.bottom() && gy + g.h as i32 > clip.y {
                    c.blend_coverage(
                        gx,
                        gy,
                        e.coverage(&g),
                        g.w as usize,
                        g.h as usize,
                        color,
                        alpha,
                        lut,
                    );
                }
            }
        }
        pen += g.adv_q8;
        prev = Some(ch);
    }
    (pen - x * 256 + 255) >> 8
}

/// [`draw_a`] fully opaque.
pub fn draw(c: &mut Canvas, x: i32, y: i32, text: &str, px: u16, w: Weight, color: Color) -> i32 {
    draw_a(c, x, y, text, px, w, color, 256)
}

/// Draw `text` cut with an ellipsis to `max_w` pixels. Returns the width drawn.
#[allow(clippy::too_many_arguments)]
pub fn draw_ellipsis(
    c: &mut Canvas,
    x: i32,
    y: i32,
    max_w: i32,
    text: &str,
    px: u16,
    w: Weight,
    color: Color,
) -> i32 {
    if measure(text, px, w) <= max_w {
        return draw(c, x, y, text, px, w, color);
    }
    let s = ellipsize(text, px, w, max_w);
    draw(c, x, y, &s, px, w, color)
}

/// Draw `text` horizontally and vertically centred (by cap height) in `r`.
pub fn draw_centered(c: &mut Canvas, r: Rect, text: &str, px: u16, w: Weight, color: Color) {
    draw_centered_a(c, r, text, px, w, color, 256)
}

pub fn draw_centered_a(
    c: &mut Canvas,
    r: Rect,
    text: &str,
    px: u16,
    w: Weight,
    color: Color,
    alpha: u16,
) {
    let t = ellipsize(text, px, w, r.w);
    let tw = measure(&t, px, w);
    let x = r.x + (r.w - tw) / 2;
    let y = center_y(r.y, r.h, px, w);
    draw_a(c, x, y, &t, px, w, color, alpha);
}

/// Draw `text` right-aligned to `r`'s right edge, vertically centred.
pub fn draw_right(c: &mut Canvas, r: Rect, text: &str, px: u16, w: Weight, color: Color) {
    let t = ellipsize(text, px, w, r.w);
    let tw = measure(&t, px, w);
    draw(
        c,
        r.right() - tw,
        center_y(r.y, r.h, px, w),
        &t,
        px,
        w,
        color,
    );
}

/// Draw `text` left-aligned in `r`, vertically centred, cut with an ellipsis.
pub fn draw_left(c: &mut Canvas, r: Rect, text: &str, px: u16, w: Weight, color: Color) {
    draw_ellipsis(c, r.x, center_y(r.y, r.h, px, w), r.w, text, px, w, color);
}

/// Text from bytes that are UTF-8 or (when not valid UTF-8) Latin-1: the apps'
/// legacy `&[u8]` strings.
pub fn from_bytes(t: &[u8]) -> alloc::borrow::Cow<'_, str> {
    match core::str::from_utf8(t) {
        Ok(s) => alloc::borrow::Cow::Borrowed(s),
        Err(_) => alloc::borrow::Cow::Owned(t.iter().map(|&b| b as char).collect()),
    }
}
