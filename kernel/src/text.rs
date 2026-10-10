//! Proportional, anti-aliased UI text (Inter, SIL OFL) over the framebuffer.
//!
//! The engine itself (TrueType reader, rasteriser, glyph cache, measuring) is
//! pure and lives in `kitsune_core` (`ttf`, `glyph`, `fontcache`, `textlayout`).
//! This module owns the one global engine, the text gamma tables and the
//! `Canvas` drawing helpers. Only the compositor thread draws UI text, so the
//! lazily filled cache needs no lock (see the SAFETY note on [`engine`]).
//!
//! Sizes used by the UI: 11, 12, 13, 15, 17, 22, 28 (see `docs/design/ui-design.md`);
//! any size from 8 to 64 works and is cached on first use. The 8x8 bitmap font in
//! `font.rs` remains only for the terminal and editor grids.

// Toolkit module: the whole API is public for the apps (wave 2) even where the chrome
// does not call it yet.
#![allow(dead_code)]

use crate::fb::{Canvas, Color};
use crate::sync::RacyCell;
use kitsune_core::Rect;
pub use kitsune_core::fontcache::Weight;
use kitsune_core::fontcache::{Stats, TextEngine, VMetrics};
use kitsune_core::gfx::luma;
use kitsune_core::textlayout;

static REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.subset.ttf");
static MEDIUM: &[u8] = include_bytes!("../../assets/fonts/Inter-Medium.subset.ttf");
static SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.subset.ttf");
static MONO: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.subset.ttf");

/// Size of the terminal and editor text (a 9 px pitch: 600/1000 em).
pub const MONO_PX: u16 = 15;

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
    let mut eng = TextEngine::new([REGULAR, MEDIUM, SEMIBOLD, MONO]);
    if let Some(e) = eng.as_mut() {
        // The faces the chrome shows on every frame; the rest fill on first use.
        for (w, px) in [
            (Weight::Regular, BODY),
            (Weight::Medium, BODY),
            (Weight::Semibold, BODY),
            (Weight::Regular, FOOTNOTE),
            (Weight::Regular, CAPTION),
            (Weight::Regular, CALLOUT),
            (Weight::Mono, MONO_PX),
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

/// Width in 1/256 pixel of `text` (kerning included, not rounded).
pub fn measure_q8(text: &str, px: u16, w: Weight) -> i32 {
    match engine() {
        Some(e) => textlayout::measure_q8(&e.face(w, px), text),
        None => text.chars().count() as i32 * px as i32 * 128,
    }
}

/// Does face `w` have a glyph for `c` of its own (rather than the `?` stand-in)?
pub fn has_glyph(c: char, w: Weight) -> bool {
    engine().is_none_or(|e| e.has_glyph(w, c))
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

/// Like [`draw_a`] but slanted to the right, a synthetic italic: every row of a glyph is
/// shifted by its height above the baseline times about 0.21 (12 degrees). Returns the
/// pen advance.
#[allow(clippy::too_many_arguments)]
pub fn draw_slanted(
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
            let gy = base - g.top as i32;
            // Skip glyphs wholly outside the clip (with room for the slant).
            if gx < clip.right() && gx + g.w as i32 + px as i32 / 3 > clip.x {
                let cov = e.coverage(&g);
                for row in 0..g.h as usize {
                    let ry = gy + row as i32;
                    if ry < clip.y || ry >= clip.bottom() {
                        continue;
                    }
                    let shift = (base - ry) * 21 / 100;
                    c.blend_coverage(
                        gx + shift,
                        ry,
                        &cov[row * g.w as usize..(row + 1) * g.w as usize],
                        g.w as usize,
                        1,
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

// ------------------------------------------------------------------ monospace

/// The character cell of the terminal and the editor: `(pitch, line height)`.
pub fn mono_cell() -> (i32, i32) {
    engine().map_or((9, 20), |e| e.mono_cell(MONO_PX))
}

/// The character cell of the monospace face at `px` pixels: `(pitch, line height)`.
pub fn mono_cell_px(px: u16) -> (i32, i32) {
    engine().map_or((px as i32 * 3 / 5, px as i32 * 4 / 3), |e| e.mono_cell(px))
}

/// Draw `text` on the monospace grid: character `i` at `x + i * pitch`, with the top of
/// its line box at `y`. Returns the width drawn.
pub fn draw_mono(c: &mut Canvas, x: i32, y: i32, text: &str, px: u16, color: Color) -> i32 {
    match engine() {
        Some(e) => draw_mono_with(e, c, x, y, text, px, color),
        None => 0,
    }
}

fn draw_mono_with(
    e: &mut TextEngine,
    c: &mut Canvas,
    x: i32,
    y: i32,
    text: &str,
    px: u16,
    color: Color,
) -> i32 {
    let pitch = e.mono_cell(px).0;
    let base = y + e.vmetrics(Weight::Mono, px).ascent;
    let lut = if luma(color) > 140 {
        &LUT_LIGHT_TEXT
    } else {
        &LUT_DARK_TEXT
    };
    let clip = c.clip_rect();
    let mut n = 0;
    for ch in text.chars() {
        let cx = x + n * pitch;
        n += 1;
        if cx >= clip.right() || cx + pitch <= clip.x || ch == ' ' {
            continue;
        }
        let g = e.glyph(Weight::Mono, px, ch);
        if g.w > 0 {
            c.blend_coverage(
                cx + g.left as i32,
                base - g.top as i32,
                e.coverage(&g),
                g.w as usize,
                g.h as usize,
                color,
                256,
                lut,
            );
        }
    }
    n * pitch
}

/// The engine of the app thread (`appd`), separate from the compositor's so the two
/// never mutate one glyph cache. Built on first use.
static GUEST_ENGINE: RacyCell<Option<TextEngine>> = RacyCell::new(None);

fn guest_engine() -> Option<&'static mut TextEngine> {
    // SAFETY: only the `appd` thread draws guest text (host `draw_text` runs there), so
    // this cell has one user; the reference is not kept across calls.
    // NOTE: not guaranteed by the type: safe fn returning `&'static mut`.
    let slot = unsafe { &mut *GUEST_ENGINE.get() };
    if slot.is_none() {
        *slot = TextEngine::new([MONO, MONO, MONO, MONO]);
    }
    slot.as_mut()
}

// ----------------------------------------------------------- compatibility shims

/// The old bitmap-font API (`draw_text(c, x, y, text, colour, scale)`), for app
/// interiors not yet re-skinned: text is drawn with the proportional UI font at a
/// size picked from the old scale, `y` being the top of the old glyph box.
pub mod legacy {
    use super::*;

    /// UI size and weight standing in for bitmap scale `s`.
    fn face(scale: usize) -> (u16, Weight) {
        match scale {
            0 | 1 => (CAPTION, Weight::Regular),
            2 => (BODY, Weight::Regular),
            3 => (TITLE3 + 3, Weight::Medium),
            4 => (TITLE2 + 4, Weight::Semibold),
            5 => (TITLE1 + 6, Weight::Semibold),
            _ => (TITLE1 + 12, Weight::Semibold),
        }
    }

    pub fn draw_text(c: &mut Canvas, x: usize, y: usize, text: &str, color: Color, scale: usize) {
        let (px, w) = face(scale);
        let ty = center_y(y as i32, scale.max(1) as i32 * 8, px, w);
        draw(c, x as i32, ty, text, px, w, color);
    }

    pub fn draw_bytes(c: &mut Canvas, x: usize, y: usize, t: &[u8], color: Color, scale: usize) {
        draw_text(c, x, y, &from_bytes(t), color, scale);
    }

    /// Width of `text` at the stand-in size.
    pub fn text_width(text: &str, scale: usize) -> usize {
        let (px, w) = face(scale);
        measure(text, px, w).max(0) as usize
    }

    /// Average character cell (the width of a digit) at the stand-in size.
    pub fn cell_w(scale: usize) -> usize {
        let (px, w) = face(scale);
        measure("0", px, w).max(1) as usize
    }
}

/// The host drawing ABI of the WebAssembly apps: guests lay their text out on a
/// fixed `6 * scale` pixel pitch, so the host draws it with the monospace face at
/// `10 * scale` px (a pitch of exactly `6 * scale`).
pub mod guest {
    use super::*;

    fn px(scale: usize) -> u16 {
        (10 * scale.clamp(1, 6)) as u16
    }

    /// Horizontal advance of one character.
    pub const fn cell_w(scale: usize) -> usize {
        6 * scale
    }

    pub fn draw_char(c: &mut Canvas, x: usize, y: usize, ch: char, color: Color, scale: usize) {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        draw_text(c, x, y, s, color, scale);
    }

    pub fn draw_text(c: &mut Canvas, x: usize, y: usize, text: &str, color: Color, scale: usize) {
        if let Some(e) = guest_engine() {
            // The old glyphs sat on a baseline 7 rows below the top of their cell.
            let asc = e.vmetrics(Weight::Mono, px(scale)).ascent;
            let top = y as i32 + 7 * scale as i32 - asc;
            draw_mono_with(e, c, x as i32, top, text, px(scale), color);
        }
    }

    pub fn draw_bytes(c: &mut Canvas, x: usize, y: usize, t: &[u8], color: Color, scale: usize) {
        draw_text(c, x, y, &from_bytes(t), color, scale);
    }

    pub fn text_width(text: &str, scale: usize) -> usize {
        text.chars().count() * 6 * scale
    }
}
