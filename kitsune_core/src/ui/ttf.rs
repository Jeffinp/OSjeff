//! A small, total TrueType reader: just what a UI text engine needs.
//!
//! Reads `head`, `hhea`, `maxp`, `hmtx`, `cmap` (format 4), `loca`, `glyf` (simple
//! and composite glyphs), `OS/2` (cap and x height) and pair kerning from `GPOS`
//! (lookup type 2, directly or through an extension lookup, formats 1 and 2).
//! Every access is bounds checked: a damaged or hostile font yields `None`/empty
//! outlines, never a panic. No hinting, no variations, no vertical metrics.
//!
//! The font bytes are borrowed (`&'static [u8]` in the kernel), nothing is copied
//! except the offsets of the kerning subtables.

use alloc::vec::Vec;

mod glyf;
mod kerning;

/// One element of a glyph outline, in font units (y grows upwards).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seg {
    Move(i32, i32),
    Line(i32, i32),
    /// Quadratic curve: control point, then end point.
    Quad(i32, i32, i32, i32),
    Close,
}

/// Most points a simple glyph may have (real glyphs have far fewer).
const MAX_POINTS: usize = 4096;
/// Deepest nesting of composite glyphs followed.
const MAX_DEPTH: u8 = 4;

#[derive(Clone, Copy)]
struct Rd<'a>(&'a [u8]);

impl Rd<'_> {
    fn u8(&self, o: usize) -> Option<u8> {
        self.0.get(o).copied()
    }
    fn u16(&self, o: usize) -> Option<u16> {
        let b = self.0.get(o..o.checked_add(2)?)?;
        Some(u16::from_be_bytes([b[0], b[1]]))
    }
    fn i16(&self, o: usize) -> Option<i16> {
        self.u16(o).map(|v| v as i16)
    }
    fn u32(&self, o: usize) -> Option<u32> {
        let b = self.0.get(o..o.checked_add(4)?)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// A parsed font. Cheap to build; all data stays in the borrowed bytes.
pub struct Font<'a> {
    d: Rd<'a>,
    pub units_per_em: u16,
    /// Distance from the baseline to the top of the line box (positive).
    pub ascent: i16,
    /// Distance from the baseline to the bottom of the line box (negative).
    pub descent: i16,
    pub line_gap: i16,
    pub cap_height: i16,
    pub x_height: i16,
    num_glyphs: u16,
    loca_long: bool,
    loca: usize,
    glyf: usize,
    glyf_len: usize,
    hmtx: usize,
    num_hmetrics: u16,
    cmap4: usize,
    /// Offsets (from the start of the file) of the `PairPos` subtables.
    pairs: Vec<usize>,
    /// Glyphs of U+0000..U+00FF, so common text skips the `cmap` search.
    latin: [u16; 256],
    /// Recently looked up kerning pairs, direct mapped: `(1 << 31 | left << 16 | right, value)`.
    kern_cache: [core::cell::Cell<(u32, i16)>; KERN_CACHE],
}

const KERN_CACHE: usize = 512;

fn find_table(d: Rd<'_>, tag: &[u8; 4]) -> Option<(usize, usize)> {
    let n = d.u16(4)? as usize;
    for i in 0..n {
        let rec = 12 + i * 16;
        if d.0.get(rec..rec + 4)? == tag {
            let off = d.u32(rec + 8)? as usize;
            let len = d.u32(rec + 12)? as usize;
            if off.checked_add(len)? <= d.0.len() {
                return Some((off, len));
            }
            return None;
        }
    }
    None
}

impl<'a> Font<'a> {
    /// Parse `data` (a TrueType file with `glyf` outlines). `None` when a table
    /// the engine needs is missing or inconsistent.
    pub fn parse(data: &'a [u8]) -> Option<Font<'a>> {
        let d = Rd(data);
        // 0x00010000 or 'true'.
        let ver = d.u32(0)?;
        if ver != 0x0001_0000 && ver != 0x7472_7565 {
            return None;
        }
        let (head, head_len) = find_table(d, b"head")?;
        let (hhea, _) = find_table(d, b"hhea")?;
        let (maxp, _) = find_table(d, b"maxp")?;
        let (hmtx, _) = find_table(d, b"hmtx")?;
        let (cmap, cmap_len) = find_table(d, b"cmap")?;
        let (loca, loca_len) = find_table(d, b"loca")?;
        let (glyf, glyf_len) = find_table(d, b"glyf")?;
        if head_len < 54 {
            return None;
        }
        let units_per_em = d.u16(head + 18)?;
        if !(16..=16384).contains(&units_per_em) {
            return None;
        }
        let loca_long = d.i16(head + 50)? != 0;
        let num_glyphs = d.u16(maxp + 4)?;
        let ascent = d.i16(hhea + 4)?;
        let descent = d.i16(hhea + 6)?;
        let line_gap = d.i16(hhea + 8)?;
        let num_hmetrics = d.u16(hhea + 34)?;
        if num_hmetrics == 0 {
            return None;
        }
        let entry = if loca_long { 4 } else { 2 };
        if loca_len < (num_glyphs as usize + 1) * entry {
            return None;
        }
        // cmap: first Windows/Unicode (or Unicode) format 4 subtable.
        let nsub = d.u16(cmap + 2)? as usize;
        let mut cmap4 = 0usize;
        for i in 0..nsub.min(16) {
            let rec = cmap + 4 + i * 8;
            let plat = d.u16(rec)?;
            let enc = d.u16(rec + 2)?;
            let off = cmap + d.u32(rec + 4)? as usize;
            if off + 4 > cmap + cmap_len {
                continue;
            }
            let unicode = plat == 0 || (plat == 3 && (enc == 1 || enc == 10));
            if unicode && d.u16(off) == Some(4) {
                cmap4 = off;
                break;
            }
        }
        if cmap4 == 0 {
            return None;
        }
        let (mut cap_height, mut x_height) = (0, 0);
        if let Some((os2, len)) = find_table(d, b"OS/2")
            && len >= 90
            && d.u16(os2)? >= 2
        {
            x_height = d.i16(os2 + 86)?;
            cap_height = d.i16(os2 + 88)?;
        }
        if cap_height <= 0 {
            cap_height = (ascent as i32 * 7 / 10) as i16;
        }
        if x_height <= 0 {
            x_height = (cap_height as i32 * 3 / 4) as i16;
        }
        let mut font = Font {
            d,
            units_per_em,
            ascent,
            descent,
            line_gap,
            cap_height,
            x_height,
            num_glyphs,
            loca_long,
            loca,
            glyf,
            glyf_len,
            hmtx,
            num_hmetrics,
            cmap4,
            pairs: Vec::new(),
            latin: [0; 256],
            kern_cache: [const { core::cell::Cell::new((0, 0)) }; KERN_CACHE],
        };
        font.load_kerning();
        for cp in 0..256u16 {
            font.latin[usize::from(cp)] = font.cmap4_lookup(cp).unwrap_or(0);
        }
        Some(font)
    }

    pub fn glyph_count(&self) -> u16 {
        self.num_glyphs
    }

    /// Glyph for `c`, or `0` (`.notdef`) when the font has none.
    pub fn glyph_index(&self, c: char) -> u16 {
        let cp = c as u32;
        if cp < 256 {
            return self.latin[cp as usize];
        }
        if cp > 0xFFFF {
            return 0;
        }
        self.cmap4_lookup(cp as u16).unwrap_or(0)
    }

    fn cmap4_lookup(&self, cp: u16) -> Option<u16> {
        let d = self.d;
        let t = self.cmap4;
        let segx2 = d.u16(t + 6)? as usize;
        let seg = segx2 / 2;
        let end_codes = t + 14;
        let start_codes = end_codes + segx2 + 2;
        let deltas = start_codes + segx2;
        let range_offs = deltas + segx2;
        // Binary search the first segment whose endCode >= cp.
        let (mut lo, mut hi) = (0usize, seg);
        while lo < hi {
            let mid = (lo + hi) / 2;
            if d.u16(end_codes + mid * 2)? < cp {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo >= seg {
            return None;
        }
        let start = d.u16(start_codes + lo * 2)?;
        if cp < start {
            return None;
        }
        let delta = d.u16(deltas + lo * 2)?;
        let ro = d.u16(range_offs + lo * 2)?;
        let gid = if ro == 0 {
            cp.wrapping_add(delta)
        } else {
            let at = range_offs + lo * 2 + ro as usize + (cp - start) as usize * 2;
            let g = d.u16(at)?;
            if g == 0 { 0 } else { g.wrapping_add(delta) }
        };
        (gid != 0 && gid < self.num_glyphs).then_some(gid)
    }

    /// Horizontal advance of `gid` in font units.
    pub fn advance(&self, gid: u16) -> u16 {
        let i = gid.min(self.num_hmetrics - 1) as usize;
        self.d.u16(self.hmtx + i * 4).unwrap_or(0)
    }

    fn loca_at(&self, gid: u16) -> Option<(usize, usize)> {
        if gid >= self.num_glyphs {
            return None;
        }
        let g = gid as usize;
        let (a, b) = if self.loca_long {
            (
                self.d.u32(self.loca + g * 4)? as usize,
                self.d.u32(self.loca + g * 4 + 4)? as usize,
            )
        } else {
            (
                self.d.u16(self.loca + g * 2)? as usize * 2,
                self.d.u16(self.loca + g * 2 + 2)? as usize * 2,
            )
        };
        (a <= b && b <= self.glyf_len).then_some((self.glyf + a, self.glyf + b))
    }

    /// Emit the outline of `gid` (composites flattened) through `sink`.
    pub fn outline(&self, gid: u16, sink: &mut impl FnMut(Seg)) {
        self.outline_at(gid, [1 << 14, 0, 0, 1 << 14], (0, 0), 0, sink);
    }

    fn outline_at(
        &self,
        gid: u16,
        m: [i32; 4],
        off: (i32, i32),
        depth: u8,
        sink: &mut impl FnMut(Seg),
    ) {
        let Some((a, b)) = self.loca_at(gid) else {
            return;
        };
        if b <= a + 10 {
            return; // empty glyph (space)
        }
        let d = self.d;
        let Some(nc) = d.i16(a) else { return };
        if nc >= 0 {
            self.simple(a, nc as usize, b, m, off, sink);
        } else if depth < MAX_DEPTH {
            self.composite(a + 10, b, m, off, depth, sink);
        }
    }
}

/// `(a*b + c*d) >> 14` without overflow, clamped to a sane coordinate range.
fn mul14(a: i32, b: i32, c: i32, d: i32) -> i32 {
    let v = (a as i64 * b as i64 + c as i64 * d as i64) >> 14;
    v.clamp(-(1 << 24), 1 << 24) as i32
}

fn coverage_index(d: Rd<'_>, t: usize, gid: u16) -> Option<u16> {
    match d.u16(t)? {
        1 => {
            let n = d.u16(t + 2)? as usize;
            let (mut lo, mut hi) = (0usize, n);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let g = d.u16(t + 4 + mid * 2)?;
                if g < gid {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            (lo < n && d.u16(t + 4 + lo * 2)? == gid).then_some(lo as u16)
        }
        2 => {
            let n = d.u16(t + 2)? as usize;
            let (mut lo, mut hi) = (0usize, n);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let end = d.u16(t + 4 + mid * 6 + 2)?;
                if end < gid {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            if lo >= n {
                return None;
            }
            let r = t + 4 + lo * 6;
            let (s, e, sc) = (d.u16(r)?, d.u16(r + 2)?, d.u16(r + 4)?);
            (gid >= s && gid <= e).then(|| sc.wrapping_add(gid - s))
        }
        _ => None,
    }
}

fn class_of(d: Rd<'_>, t: usize, gid: u16) -> Option<u16> {
    match d.u16(t)? {
        1 => {
            let start = d.u16(t + 2)?;
            let n = d.u16(t + 4)?;
            if gid >= start && gid - start < n {
                d.u16(t + 6 + (gid - start) as usize * 2)
            } else {
                Some(0)
            }
        }
        2 => {
            let n = d.u16(t + 2)? as usize;
            let (mut lo, mut hi) = (0usize, n);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let end = d.u16(t + 4 + mid * 6 + 2)?;
                if end < gid {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            if lo >= n {
                return Some(0);
            }
            let r = t + 4 + lo * 6;
            let (s, e, c) = (d.u16(r)?, d.u16(r + 2)?, d.u16(r + 4)?);
            Some(if gid >= s && gid <= e { c } else { 0 })
        }
        _ => None,
    }
}

/// Convert one contour of on/off-curve points to segments.
fn contour(
    flags: &[u8],
    xs: &[i32],
    ys: &[i32],
    tx: &impl Fn(i32, i32) -> (i32, i32),
    sink: &mut impl FnMut(Seg),
) {
    let n = flags.len();
    if n == 0 {
        return;
    }
    let pt = |i: usize| -> (i32, i32, bool) {
        let (x, y) = tx(xs[i % n], ys[i % n]);
        (x, y, flags[i % n] & 1 != 0)
    };
    // Starting point: the first on-curve point, or the midpoint of the first two
    // (off-curve) points when the contour has none.
    let mut first_on = None;
    for i in 0..n {
        if pt(i).2 {
            first_on = Some(i);
            break;
        }
    }
    let (sx, sy, begin) = match first_on {
        Some(i) => {
            let (x, y, _) = pt(i);
            (x, y, i + 1)
        }
        None => {
            let (x0, y0, _) = pt(0);
            let (x1, y1, _) = pt(1);
            ((x0 + x1) / 2, (y0 + y1) / 2, 1)
        }
    };
    sink(Seg::Move(sx, sy));
    let total = if first_on.is_some() { n - 1 } else { n };
    let mut ctrl: Option<(i32, i32)> = None;
    for k in 0..total {
        let (x, y, on) = pt(begin + k);
        if on {
            match ctrl.take() {
                Some((cx, cy)) => sink(Seg::Quad(cx, cy, x, y)),
                None => sink(Seg::Line(x, y)),
            }
        } else {
            if let Some((cx, cy)) = ctrl {
                // Two consecutive off-curve points: implied on-curve midpoint.
                sink(Seg::Quad(cx, cy, (cx + x) / 2, (cy + y) / 2));
            }
            ctrl = Some((x, y));
        }
    }
    if let Some((cx, cy)) = ctrl {
        sink(Seg::Quad(cx, cy, sx, sy));
    }
    sink(Seg::Close);
}

#[cfg(test)]
mod tests;
