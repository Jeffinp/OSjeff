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

    fn simple(
        &self,
        a: usize,
        ncont: usize,
        end: usize,
        m: [i32; 4],
        off: (i32, i32),
        sink: &mut impl FnMut(Seg),
    ) {
        let d = self.d;
        if ncont == 0 {
            return;
        }
        let ends_at = a + 10;
        let Some(last) = d.u16(ends_at + (ncont - 1) * 2) else {
            return;
        };
        let npts = last as usize + 1;
        if npts > MAX_POINTS {
            return;
        }
        let Some(ilen) = d.u16(ends_at + ncont * 2) else {
            return;
        };
        let mut p = ends_at + ncont * 2 + 2 + ilen as usize;
        // Flags.
        let mut flags: Vec<u8> = Vec::with_capacity(npts);
        while flags.len() < npts {
            let Some(f) = d.u8(p) else { return };
            p += 1;
            flags.push(f);
            if f & 8 != 0 {
                let Some(rep) = d.u8(p) else { return };
                p += 1;
                for _ in 0..rep {
                    if flags.len() < npts {
                        flags.push(f);
                    }
                }
            }
        }
        let mut xs: Vec<i32> = Vec::with_capacity(npts);
        let mut v = 0i32;
        for &f in &flags {
            if f & 2 != 0 {
                let Some(dx) = d.u8(p) else { return };
                p += 1;
                v += if f & 16 != 0 { dx as i32 } else { -(dx as i32) };
            } else if f & 16 == 0 {
                let Some(dx) = d.i16(p) else { return };
                p += 2;
                v += dx as i32;
            }
            xs.push(v);
        }
        let mut ys: Vec<i32> = Vec::with_capacity(npts);
        v = 0;
        for &f in &flags {
            if f & 4 != 0 {
                let Some(dy) = d.u8(p) else { return };
                p += 1;
                v += if f & 32 != 0 { dy as i32 } else { -(dy as i32) };
            } else if f & 32 == 0 {
                let Some(dy) = d.i16(p) else { return };
                p += 2;
                v += dy as i32;
            }
            ys.push(v);
        }
        if p > end {
            return;
        }
        let tx = |x: i32, y: i32| -> (i32, i32) {
            (
                mul14(x, m[0], y, m[2]).saturating_add(off.0),
                mul14(x, m[1], y, m[3]).saturating_add(off.1),
            )
        };
        let mut start = 0usize;
        for c in 0..ncont {
            let Some(e) = d.u16(ends_at + c * 2) else {
                return;
            };
            let e = e as usize;
            if e < start || e >= npts {
                return;
            }
            contour(&flags[start..=e], &xs[start..=e], &ys[start..=e], &tx, sink);
            start = e + 1;
        }
    }

    fn composite(
        &self,
        mut p: usize,
        end: usize,
        m_parent: [i32; 4],
        off_parent: (i32, i32),
        depth: u8,
        sink: &mut impl FnMut(Seg),
    ) {
        let d = self.d;
        for _ in 0..32 {
            let (Some(flags), Some(gid)) = (d.u16(p), d.u16(p + 2)) else {
                return;
            };
            p += 4;
            let (dx, dy);
            if flags & 1 != 0 {
                let (Some(a), Some(b)) = (d.i16(p), d.i16(p + 2)) else {
                    return;
                };
                p += 4;
                (dx, dy) = (a as i32, b as i32);
            } else {
                let (Some(a), Some(b)) = (d.u8(p), d.u8(p + 1)) else {
                    return;
                };
                p += 2;
                (dx, dy) = (a as i8 as i32, b as i8 as i32);
            }
            // Point-matching arguments (flag 2 clear) are not supported: no offset.
            let (dx, dy) = if flags & 2 != 0 { (dx, dy) } else { (0, 0) };
            let mut m = [1 << 14, 0, 0, 1 << 14];
            let f2 = |o: usize| d.i16(o).map(|v| v as i32);
            if flags & 8 != 0 {
                let Some(s) = f2(p) else { return };
                p += 2;
                m = [s, 0, 0, s];
            } else if flags & 0x40 != 0 {
                let (Some(sx), Some(sy)) = (f2(p), f2(p + 2)) else {
                    return;
                };
                p += 4;
                m = [sx, 0, 0, sy];
            } else if flags & 0x80 != 0 {
                let (Some(a), Some(b), Some(c), Some(e)) = (f2(p), f2(p + 2), f2(p + 4), f2(p + 6))
                else {
                    return;
                };
                p += 8;
                m = [a, b, c, e];
            }
            // Compose with the parent transform.
            let cm = [
                mul14(m[0], m_parent[0], m[1], m_parent[2]),
                mul14(m[0], m_parent[1], m[1], m_parent[3]),
                mul14(m[2], m_parent[0], m[3], m_parent[2]),
                mul14(m[2], m_parent[1], m[3], m_parent[3]),
            ];
            let o = (
                mul14(dx, m_parent[0], dy, m_parent[2]).saturating_add(off_parent.0),
                mul14(dx, m_parent[1], dy, m_parent[3]).saturating_add(off_parent.1),
            );
            self.outline_at(gid, cm, o, depth + 1, sink);
            if flags & 0x20 == 0 || p >= end {
                return;
            }
        }
    }

    // ---------------------------------------------------------------- kerning

    fn load_kerning(&mut self) {
        let d = self.d;
        let Some((gpos, _)) = find_table(d, b"GPOS") else {
            return;
        };
        let Some(ll) = d.u16(gpos + 8) else { return };
        let ll = gpos + ll as usize;
        let Some(n) = d.u16(ll) else { return };
        for i in 0..(n as usize).min(64) {
            let Some(lo) = d.u16(ll + 2 + i * 2) else {
                return;
            };
            let l = ll + lo as usize;
            let (Some(ty), Some(sn)) = (d.u16(l), d.u16(l + 4)) else {
                return;
            };
            for s in 0..(sn as usize).min(64) {
                let Some(so) = d.u16(l + 6 + s * 2) else {
                    return;
                };
                let mut sub = l + so as usize;
                let mut kind = ty;
                if ty == 9 {
                    let (Some(et), Some(eo)) = (d.u16(sub + 2), d.u32(sub + 4)) else {
                        continue;
                    };
                    kind = et;
                    sub += eo as usize;
                }
                if kind == 2 && sub < d.0.len() {
                    self.pairs.push(sub);
                }
            }
        }
    }

    /// Horizontal kerning between two glyphs in font units (usually negative).
    pub fn kern(&self, left: u16, right: u16) -> i16 {
        if self.pairs.is_empty() || left == 0 || right == 0 {
            return 0;
        }
        let key = 1 << 31 | u32::from(left) << 16 | u32::from(right);
        let slot = &self.kern_cache[(usize::from(left) * 31 + usize::from(right)) % KERN_CACHE];
        let (k, v) = slot.get();
        if k == key {
            return v;
        }
        let mut found = 0;
        for &sub in &self.pairs {
            if let Some(Some(v)) = self.pair_value(sub, left, right) {
                found = v;
                break;
            }
        }
        slot.set((key, found));
        found
    }

    /// `Some(None)`: subtable does not apply (glyph not covered / pair absent),
    /// `Some(Some(v))`: the value.
    fn pair_value(&self, sub: usize, left: u16, right: u16) -> Option<Option<i16>> {
        let d = self.d;
        let fmt = d.u16(sub)?;
        let cov = sub + d.u16(sub + 2)? as usize;
        let vf1 = d.u16(sub + 4)?;
        let vf2 = d.u16(sub + 6)?;
        let ci = coverage_index(d, cov, left);
        let Some(ci) = ci else {
            return Some(None);
        };
        let rec1 = (vf1.count_ones() * 2) as usize;
        let rec2 = (vf2.count_ones() * 2) as usize;
        // Offset of XAdvance inside value record 1 (bit 0x4).
        if vf1 & 4 == 0 {
            return Some(None);
        }
        let xadv_at = ((vf1 & 3).count_ones() * 2) as usize;
        match fmt {
            1 => {
                let n = d.u16(sub + 8)? as usize;
                if ci as usize >= n {
                    return Some(None);
                }
                let ps = sub + d.u16(sub + 10 + ci as usize * 2)? as usize;
                let cnt = d.u16(ps)? as usize;
                let step = 2 + rec1 + rec2;
                // Records are sorted by second glyph: binary search.
                let (mut lo, mut hi) = (0usize, cnt);
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    let g = d.u16(ps + 2 + mid * step)?;
                    if g < right {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                if lo < cnt && d.u16(ps + 2 + lo * step)? == right {
                    return Some(Some(d.i16(ps + 2 + lo * step + 2 + xadv_at)?));
                }
                Some(None)
            }
            2 => {
                let cd1 = sub + d.u16(sub + 8)? as usize;
                let cd2 = sub + d.u16(sub + 10)? as usize;
                let n1 = d.u16(sub + 12)? as usize;
                let n2 = d.u16(sub + 14)? as usize;
                let c1 = class_of(d, cd1, left)? as usize;
                let c2 = class_of(d, cd2, right)? as usize;
                if c1 >= n1 || c2 >= n2 {
                    return Some(None);
                }
                let at = sub + 16 + (c1 * n2 + c2) * (rec1 + rec2);
                Some(Some(d.i16(at + xadv_at)?))
            }
            _ => Some(None),
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
