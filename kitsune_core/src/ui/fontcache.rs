//! The UI text engine: three weights of an embedded font, a lazily filled glyph
//! cache per (weight, size) and the vertical metrics layout needs.
//!
//! Glyph coverage bitmaps are produced by [`crate::ui::glyph::rasterize`] the first
//! time a (weight, size, character) is drawn and kept in one arena. The kernel
//! pre-warms the common set at boot and logs the time and bytes. Only the
//! compositor thread uses an engine.

use crate::ui::glyph::{self, Bitmap};
use crate::ui::textlayout::Metrics;
use crate::ui::ttf::Font;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Faces available: three weights of the proportional UI font and the monospace
/// face of the terminal and the editor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weight {
    Regular = 0,
    Medium = 1,
    Semibold = 2,
    /// The monospace face (fixed pitch).
    Mono = 3,
}

/// Where a glyph's coverage lives in the arena and how to place it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Glyph {
    off: u32,
    pub w: u16,
    pub h: u16,
    pub left: i16,
    pub top: i16,
    pub adv_q8: i32,
}

/// Vertical metrics of a face at a size, in whole pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VMetrics {
    pub ascent: i32,
    pub descent: i32,
    pub line_height: i32,
    pub cap_height: i32,
    pub x_height: i32,
}

struct Strip {
    weight: u8,
    px: u16,
    /// Glyphs for U+0000..U+00FF, filled on demand.
    latin: Vec<Option<Glyph>>,
    other: BTreeMap<u32, Glyph>,
}

/// Usage counters for the boot trace and the monitor.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub glyphs: u32,
    pub arena_bytes: usize,
    pub strips: usize,
}

pub struct TextEngine {
    fonts: [Font<'static>; 4],
    strips: Vec<Strip>,
    arena: Vec<u8>,
    glyphs: u32,
}

impl TextEngine {
    /// Build an engine from the regular, medium, semibold and monospace font files.
    pub fn new(files: [&'static [u8]; 4]) -> Option<TextEngine> {
        Some(TextEngine {
            fonts: [
                Font::parse(files[0])?,
                Font::parse(files[1])?,
                Font::parse(files[2])?,
                Font::parse(files[3])?,
            ],
            strips: Vec::new(),
            arena: Vec::new(),
            glyphs: 0,
        })
    }

    pub fn stats(&self) -> Stats {
        Stats {
            glyphs: self.glyphs,
            arena_bytes: self.arena.len(),
            strips: self.strips.len(),
        }
    }

    fn font(&self, w: Weight) -> &Font<'static> {
        &self.fonts[w as usize]
    }

    fn strip_index(&mut self, w: Weight, px: u16) -> usize {
        if let Some(i) = self
            .strips
            .iter()
            .position(|s| s.weight == w as u8 && s.px == px)
        {
            return i;
        }
        self.strips.push(Strip {
            weight: w as u8,
            px,
            latin: alloc::vec![None; 256],
            other: BTreeMap::new(),
        });
        self.strips.len() - 1
    }

    /// Advance of `c` in q8 pixels (no rasterising).
    pub fn advance_q8(&self, w: Weight, px: u16, c: char) -> i32 {
        let f = self.font(w);
        let adv = f.advance(f.glyph_index(c)) as i64;
        ((adv * px as i64 * 256 + f.units_per_em as i64 / 2) / f.units_per_em as i64) as i32
    }

    /// Does face `w` have a glyph of its own for `c` (not the `?` stand-in)?
    pub fn has_glyph(&self, w: Weight, c: char) -> bool {
        self.font(w).glyph_index(c) != 0
    }

    /// Character cell of the monospace face at `px`: `(pitch, line height)` in whole
    /// pixels. Terminal and editor grids are built from this.
    pub fn mono_cell(&self, px: u16) -> (i32, i32) {
        let pitch = (self.advance_q8(Weight::Mono, px, '0') + 128) >> 8;
        (pitch.max(1), self.vmetrics(Weight::Mono, px).line_height)
    }

    pub fn kern_q8(&self, w: Weight, px: u16, l: char, r: char) -> i32 {
        let f = self.font(w);
        let k = f.kern(f.glyph_index(l), f.glyph_index(r)) as i64;
        (k * px as i64 * 256).div_euclid(f.units_per_em as i64) as i32
    }

    pub fn vmetrics(&self, w: Weight, px: u16) -> VMetrics {
        let f = self.font(w);
        let up = f.units_per_em as i32;
        let px = px as i32;
        let ceil = |v: i32| (v * px + up - 1).div_euclid(up);
        let round = |v: i32| (v * px + up / 2).div_euclid(up);
        let ascent = ceil(f.ascent as i32);
        let descent = ceil(-(f.descent as i32));
        VMetrics {
            ascent,
            descent,
            line_height: ascent + descent + round(f.line_gap as i32),
            cap_height: round(f.cap_height as i32),
            x_height: round(f.x_height as i32),
        }
    }

    /// The glyph for `c`, rasterising it on first use. Characters the font does
    /// not have fall back to `?`.
    pub fn glyph(&mut self, w: Weight, px: u16, c: char) -> Glyph {
        let si = self.strip_index(w, px);
        let cp = c as u32;
        let cached = if cp < 256 {
            self.strips[si].latin[cp as usize]
        } else {
            self.strips[si].other.get(&cp).copied()
        };
        if let Some(g) = cached {
            return g;
        }
        let font = &self.fonts[w as usize];
        let mut gid = font.glyph_index(c);
        if gid == 0 && c != '?' {
            // Unknown: use the question mark's outline but keep its own advance 0.
            gid = font.glyph_index('?');
        }
        let bmp: Bitmap = glyph::rasterize(font, gid, px);
        let adv = self.advance_q8(w, px, if font.glyph_index(c) == 0 { '?' } else { c });
        let off = self.arena.len() as u32;
        self.arena.extend_from_slice(&bmp.data);
        self.glyphs += 1;
        let g = Glyph {
            off,
            w: bmp.w,
            h: bmp.h,
            left: bmp.left,
            top: bmp.top,
            adv_q8: adv,
        };
        let strip = &mut self.strips[si];
        if cp < 256 {
            strip.latin[cp as usize] = Some(g);
        } else {
            strip.other.insert(cp, g);
        }
        g
    }

    /// Coverage bytes of a glyph returned by [`glyph`](Self::glyph).
    pub fn coverage(&self, g: &Glyph) -> &[u8] {
        let n = g.w as usize * g.h as usize;
        self.arena
            .get(g.off as usize..g.off as usize + n)
            .unwrap_or(&[])
    }

    /// Rasterise printable ASCII and the Latin-1 letters used by Portuguese for
    /// one face, so the first frame that shows text does not pay for it.
    pub fn prewarm(&mut self, w: Weight, px: u16) {
        for c in ' '..='~' {
            self.glyph(w, px, c);
        }
        for c in "áàâãçéêíóôõúüÁÀÂÃÇÉÊÍÓÔÕÚ·•…–—“”‘’°×".chars()
        {
            self.glyph(w, px, c);
        }
    }

    /// A [`Metrics`] view of one face, for `textlayout`.
    pub fn face(&self, w: Weight, px: u16) -> Face<'_> {
        Face { eng: self, w, px }
    }
}

/// Immutable metrics of one weight/size, usable with `textlayout`.
pub struct Face<'a> {
    eng: &'a TextEngine,
    w: Weight,
    px: u16,
}

impl Metrics for Face<'_> {
    fn advance_q8(&self, c: char) -> i32 {
        self.eng.advance_q8(self.w, self.px, c)
    }
    fn kern_q8(&self, l: char, r: char) -> i32 {
        self.eng.kern_q8(self.w, self.px, l, r)
    }
}

#[cfg(test)]
mod tests;
