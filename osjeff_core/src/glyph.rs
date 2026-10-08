//! Integer outline rasteriser: TrueType outline -> 8-bit coverage bitmap.
//!
//! Exact-area anti-aliasing with a signed-area accumulation buffer (the scheme
//! used by `font-rs`/`stb_truetype` v2), in 24.8 fixed point. The kernel has no
//! hardware floating point (`f32` is emulated), so this never uses a float.
//! Overlapping contours union (`min(|winding|, 1)`), opposite ones cut holes.

use crate::ttf::{Font, Seg};
use alloc::vec;
use alloc::vec::Vec;

/// A rasterised glyph: coverage `data` of `w x h` pixels, positioned so that
/// pixel `(0, 0)` lies `left` pixels right of the pen and `top` pixels above the
/// baseline.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bitmap {
    pub w: u16,
    pub h: u16,
    pub left: i16,
    pub top: i16,
    pub data: Vec<u8>,
}

/// Largest bitmap side the rasteriser produces (guards the allocation).
const MAX_SIDE: i32 = 256;

#[derive(Clone, Copy)]
struct Line {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

/// A closed-path builder in 24.8 fixed point (256 = one pixel, y down) with exact
/// area coverage output. Used for glyph outlines and for vector icon shapes.
#[derive(Default)]
pub struct Path {
    lines: Vec<Line>,
    cur: (i32, i32),
    start: (i32, i32),
}

impl Path {
    pub fn new() -> Path {
        Path::default()
    }

    pub fn move_to(&mut self, x: i32, y: i32) {
        self.close();
        self.cur = (x, y);
        self.start = (x, y);
    }

    pub fn line_to(&mut self, x: i32, y: i32) {
        push(&mut self.lines, self.cur.0, self.cur.1, x, y);
        self.cur = (x, y);
    }

    pub fn quad_to(&mut self, qx: i32, qy: i32, x: i32, y: i32) {
        flatten_quad(&mut self.lines, self.cur, (qx, qy), (x, y));
        self.cur = (x, y);
    }

    /// Cubic Bezier, flattened as two quadratic approximations of each half.
    pub fn cubic_to(&mut self, c1: (i32, i32), c2: (i32, i32), p: (i32, i32)) {
        let p0 = self.cur;
        // De Casteljau split at t = 1/2, each half approximated by one quad.
        let mid = |a: (i64, i64), b: (i64, i64)| ((a.0 + b.0) / 2, (a.1 + b.1) / 2);
        let w = |t: (i32, i32)| (t.0 as i64, t.1 as i64);
        let (a, b, c, d) = (w(p0), w(c1), w(c2), w(p));
        let ab = mid(a, b);
        let bc = mid(b, c);
        let cd = mid(c, d);
        let abc = mid(ab, bc);
        let bcd = mid(bc, cd);
        let m = mid(abc, bcd);
        // Control point of the quad approximating each half: intersection-like average.
        let q1 = (
            (3 * ab.0 + 3 * abc.0 - a.0 - m.0) / 4,
            (3 * ab.1 + 3 * abc.1 - a.1 - m.1) / 4,
        );
        let q2 = (
            (3 * bcd.0 + 3 * cd.0 - m.0 - d.0) / 4,
            (3 * bcd.1 + 3 * cd.1 - m.1 - d.1) / 4,
        );
        let cl = |v: (i64, i64)| (v.0 as i32, v.1 as i32);
        self.quad_to(cl(q1).0, cl(q1).1, cl(m).0, cl(m).1);
        self.quad_to(cl(q2).0, cl(q2).1, p.0, p.1);
    }

    pub fn close(&mut self) {
        if self.cur != self.start {
            push(
                &mut self.lines,
                self.cur.0,
                self.cur.1,
                self.start.0,
                self.start.1,
            );
            self.cur = self.start;
        }
    }

    /// Closed rectangle.
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.move_to(x, y);
        self.line_to(x + w, y);
        self.line_to(x + w, y + h);
        self.line_to(x, y + h);
        self.close();
    }

    /// Closed regular polygon-ish circle approximation with `n` segments.
    pub fn ellipse(&mut self, cx: i32, cy: i32, rx: i32, ry: i32) {
        // Four cubic arcs with the usual 0.5523 handle length.
        let kx = (rx as i64 * 5523 / 10000) as i32;
        let ky = (ry as i64 * 5523 / 10000) as i32;
        self.move_to(cx + rx, cy);
        self.cubic_to((cx + rx, cy + ky), (cx + kx, cy + ry), (cx, cy + ry));
        self.cubic_to((cx - kx, cy + ry), (cx - rx, cy + ky), (cx - rx, cy));
        self.cubic_to((cx - rx, cy - ky), (cx - kx, cy - ry), (cx, cy - ry));
        self.cubic_to((cx + kx, cy - ry), (cx + rx, cy - ky), (cx + rx, cy));
        self.close();
    }

    /// Number of flattened segments (0 = nothing to draw).
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Bounding box `(min_x, min_y, max_x, max_y)` in 24.8, if any.
    pub fn bounds(&self) -> Option<(i32, i32, i32, i32)> {
        if self.lines.is_empty() {
            return None;
        }
        let (mut minx, mut miny, mut maxx, mut maxy) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for l in &self.lines {
            minx = minx.min(l.x0).min(l.x1);
            maxx = maxx.max(l.x0).max(l.x1);
            miny = miny.min(l.y0).min(l.y1);
            maxy = maxy.max(l.y0).max(l.y1);
        }
        Some((minx, miny, maxx, maxy))
    }

    /// Exact-area coverage of the path (nonzero winding, overlaps union) in a
    /// `w x h` buffer whose pixel `(0, 0)` has its top-left corner at the path's
    /// `(ox, oy)` (24.8 units). Row-major, `w * h` bytes.
    pub fn coverage(&self, w: usize, h: usize, ox: i32, oy: i32) -> Vec<u8> {
        let stride = w + 2;
        let mut acc = vec![0i32; stride * h];
        let wmax = (w as i32).saturating_mul(256);
        let hmax = (h as i32).saturating_mul(256);
        for l in &self.lines {
            let x0 = (l.x0 - ox).clamp(0, wmax);
            let x1 = (l.x1 - ox).clamp(0, wmax);
            let y0 = (l.y0 - oy).clamp(0, hmax);
            let y1 = (l.y1 - oy).clamp(0, hmax);
            add_line(&mut acc, stride, x0, y0, x1, y1);
        }
        let mut data = vec![0u8; w * h];
        for y in 0..h {
            let mut sum = 0i32;
            for x in 0..w {
                sum += acc[y * stride + x];
                let a = sum.unsigned_abs().min(65536);
                data[y * w + x] = ((a * 255 + 32768) >> 16) as u8;
            }
        }
        data
    }
}

/// Rasterise `gid` of `font` at `px` pixels per em. Empty outlines (space) give
/// an empty bitmap. Never panics.
pub fn rasterize(font: &Font, gid: u16, px: u16) -> Bitmap {
    let upem = font.units_per_em as i64;
    let size = px as i64 * 256;
    // Font units -> 24.8 pixels, y flipped (down).
    let sx = |v: i32| -> i32 { ((v as i64 * size + upem / 2).div_euclid(upem)) as i32 };
    let sy = |v: i32| -> i32 { -sx(v) };
    let mut path = Path::new();
    font.outline(gid, &mut |seg| match seg {
        Seg::Move(x, y) => path.move_to(sx(x), sy(y)),
        Seg::Line(x, y) => path.line_to(sx(x), sy(y)),
        Seg::Quad(qx, qy, x, y) => path.quad_to(sx(qx), sy(qy), sx(x), sy(y)),
        Seg::Close => path.close(),
    });
    let Some((minx, miny, maxx, maxy)) = path.bounds() else {
        return Bitmap::default();
    };
    let ox = minx.div_euclid(256); // floor, pixels
    let top = miny.div_euclid(256); // floor of the highest point (y down => smallest y)
    let w = ((maxx + 255).div_euclid(256) - ox).max(1);
    let h = ((maxy + 255).div_euclid(256) - top).max(1);
    if w > MAX_SIDE || h > MAX_SIDE {
        return Bitmap::default();
    }
    let data = path.coverage(w as usize, h as usize, ox * 256, top * 256);
    Bitmap {
        w: w as u16,
        h: h as u16,
        left: ox as i16,
        top: (-top) as i16,
        data,
    }
}

fn push(lines: &mut Vec<Line>, x0: i32, y0: i32, x1: i32, y1: i32) {
    if y0 != y1 {
        lines.push(Line { x0, y0, x1, y1 });
    } else if x0 != x1 {
        // Horizontal edges carry no area, but keep them for the bounding box.
        lines.push(Line { x0, y0, x1, y1 });
    }
}

/// Flatten a quadratic Bezier into line segments (tolerance about 1/8 pixel).
fn flatten_quad(lines: &mut Vec<Line>, p0: (i32, i32), p1: (i32, i32), p2: (i32, i32)) {
    let dx = (p0.0 - 2 * p1.0 + p2.0).unsigned_abs();
    let dy = (p0.1 - 2 * p1.1 + p2.1).unsigned_abs();
    let dev = dx.max(dy) + dx.min(dy) / 2; // |p0 - 2 p1 + p2|, within 12%
    // Chord deviation is dev/4 for one segment and falls with n^2: want <= 32 (1/8 px).
    let n = (crate::gfx::isqrt((dev / 128) as usize) + 1).min(24) as i64;
    let (mut px, mut py) = (p0.0, p0.1);
    for i in 1..=n {
        let (a, b, c) = ((n - i) * (n - i), 2 * i * (n - i), i * i);
        let nn = n * n;
        let x =
            ((a * p0.0 as i64 + b * p1.0 as i64 + c * p2.0 as i64 + nn / 2).div_euclid(nn)) as i32;
        let y =
            ((a * p0.1 as i64 + b * p1.1 as i64 + c * p2.1 as i64 + nn / 2).div_euclid(nn)) as i32;
        let (x, y) = if i == n { (p2.0, p2.1) } else { (x, y) };
        push(lines, px, py, x, y);
        (px, py) = (x, y);
    }
}

/// Accumulate the signed area of the segment `(x0,y0)-(x1,y1)` (bitmap space,
/// 24.8, `0 <= x <= w*256`, `0 <= y <= h*256`).
fn add_line(acc: &mut [i32], stride: usize, x0: i32, y0: i32, x1: i32, y1: i32) {
    if y0 == y1 {
        return;
    }
    let (dir, x0, y0, x1, y1) = if y0 < y1 {
        (1i64, x0, y0, x1, y1)
    } else {
        (-1i64, x1, y1, x0, y0)
    };
    let dy_total = (y1 - y0) as i64;
    let dx_total = (x1 - x0) as i64;
    let row0 = (y0 >> 8) as usize;
    let row1 = ((y1 - 1) >> 8) as usize;
    for row in row0..=row1 {
        let ya = y0.max((row as i32) << 8);
        let yb = y1.min(((row + 1) as i32) << 8);
        let dy = (yb - ya) as i64;
        if dy <= 0 {
            continue;
        }
        let xa = x0 as i64 + (ya - y0) as i64 * dx_total / dy_total;
        let xb = x0 as i64 + (yb - y0) as i64 * dx_total / dy_total;
        span(acc, row * stride, xa as i32, xb as i32, dy * dir);
    }
}

/// One scanline piece from `xa` to `xb` (24.8) with signed height `d` (1/256).
fn span(acc: &mut [i32], base: usize, xa: i32, xb: i32, d: i64) {
    let (xl, xr) = if xa < xb { (xa, xb) } else { (xb, xa) };
    let xli = (xl >> 8) as usize;
    let xri = ((xr + 255) >> 8) as usize;
    let add = |acc: &mut [i32], i: usize, v: i64| {
        if let Some(c) = acc.get_mut(base + i) {
            *c = c.wrapping_add(v as i32);
        }
    };
    if xri <= xli + 1 {
        let xmf = (((xl + xr) >> 1) - ((xli as i32) << 8)) as i64; // 0..=256
        add(acc, xli, d * (256 - xmf));
        add(acc, xli + 1, d * xmf);
        return;
    }
    let wdt = (xr - xl) as i64; // > 0 and spans more than one column boundary
    let x0f = (xl - ((xli as i32) << 8)) as i64; // 0..255
    let x1f = (xr - (((xri - 1) as i32) << 8)) as i64; // 1..256
    let f0 = (256 - x0f) * (256 - x0f) / (2 * wdt);
    let fm = x1f * x1f / (2 * wdt);
    add(acc, xli, d * f0);
    if xri == xli + 2 {
        add(acc, xli + 1, d * (256 - f0 - fm));
    } else {
        let f1 = 256 * (384 - x0f) / wdt;
        let s = 65536 / wdt;
        add(acc, xli + 1, d * (f1 - f0));
        for xi in xli + 2..xri - 1 {
            add(acc, xi, d * s);
        }
        let m = (xri - xli - 3) as i64;
        let f2 = f1 + m * s;
        add(acc, xri - 1, d * (256 - f2 - fm));
    }
    add(acc, xri, d * fm);
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.subset.ttf");

    fn total(b: &Bitmap) -> u32 {
        b.data.iter().map(|&v| v as u32).sum()
    }

    #[test]
    fn space_is_empty() {
        let f = Font::parse(REGULAR).unwrap();
        let b = rasterize(&f, f.glyph_index(' '), 13);
        assert_eq!((b.w, b.h), (0, 0));
        assert!(b.data.is_empty());
    }

    #[test]
    fn capital_i_is_a_vertical_bar_with_the_right_height() {
        let f = Font::parse(REGULAR).unwrap();
        let b = rasterize(&f, f.glyph_index('I'), 26);
        // Cap height of Inter is 0.7275 em: 18.9 px.
        assert!((18..=21).contains(&(b.h as i32)), "h={}", b.h);
        assert_eq!(b.top as i32, b.h as i32, "I sits on the baseline");
        // A solid stem: the middle row has one run of full coverage.
        let row = &b.data[(b.h as usize / 2) * b.w as usize..][..b.w as usize];
        // Inter's stem is about 0.09 em: 2.3 px at 26 px.
        let width = row.iter().map(|&v| v as u32).sum::<u32>() as f32 / 255.0;
        assert!((1.8..3.2).contains(&width), "stem {width}");
        assert!(row.iter().filter(|&&v| v > 0).count() <= 4);
    }

    #[test]
    fn ink_scales_with_the_square_of_the_size() {
        let f = Font::parse(REGULAR).unwrap();
        let g = f.glyph_index('o');
        let a = total(&rasterize(&f, g, 20)) as f32;
        let b = total(&rasterize(&f, g, 40)) as f32;
        let ratio = b / a;
        assert!((3.7..4.3).contains(&ratio), "ratio {ratio}");
    }

    #[test]
    fn counters_stay_open() {
        // The inside of an 'o' must be empty: opposite winding cuts the hole.
        let f = Font::parse(REGULAR).unwrap();
        let b = rasterize(&f, f.glyph_index('o'), 40);
        let (w, h) = (b.w as usize, b.h as usize);
        assert_eq!(b.data[(h / 2) * w + w / 2], 0);
        assert!(b.data[(h / 2) * w + 1] > 100 || b.data[(h / 2) * w + 2] > 100);
    }

    #[test]
    fn coverage_is_anti_aliased_not_binary() {
        let f = Font::parse(REGULAR).unwrap();
        let b = rasterize(&f, f.glyph_index('S'), 13);
        let mid = b.data.iter().filter(|&&v| v > 16 && v < 240).count();
        assert!(
            mid * 4 > b.data.iter().filter(|&&v| v > 0).count(),
            "mid {mid}"
        );
    }

    #[test]
    fn accents_sit_above_and_cedilla_below() {
        let f = Font::parse(REGULAR).unwrap();
        let a = rasterize(&f, f.glyph_index('a'), 20);
        let acute = rasterize(&f, f.glyph_index('á'), 20);
        assert!(acute.top > a.top + 3);
        let c = rasterize(&f, f.glyph_index('c'), 20);
        let cedilla = rasterize(&f, f.glyph_index('ç'), 20);
        assert!(cedilla.top - (cedilla.h as i16) < c.top - (c.h as i16) - 2);
    }

    #[test]
    fn synthetic_square_covers_exactly() {
        // Build a 1x1 px square by hand through the accumulator.
        let mut acc = vec![0i32; 3];
        // Square from (0,0) to (256,256): left edge down, right edge up.
        add_line(&mut acc, 3, 0, 0, 0, 256);
        add_line(&mut acc, 3, 256, 256, 256, 0);
        let c0 = acc[0].unsigned_abs().min(65536);
        assert_eq!(c0, 65536);
        // Half-covered column.
        let mut acc = vec![0i32; 3];
        add_line(&mut acc, 3, 128, 0, 128, 256);
        add_line(&mut acc, 3, 256, 256, 256, 0);
        let s0 = acc[0];
        assert_eq!(s0.unsigned_abs(), 32768);
    }

    #[test]
    fn slanted_edges_telescope_to_full_coverage() {
        // A wide parallelogram: the area right of the edges must sum to exactly 1.
        let mut acc = vec![0i32; 40 * 4];
        add_line(&mut acc, 40, 0, 0, 0, 4 * 256);
        add_line(&mut acc, 40, 37 * 256, 4 * 256, 3 * 256 + 77, 0); // slanted
        for row in 0..4 {
            let mut sum = 0i64;
            for x in 0..38 {
                sum += acc[row * 40 + x] as i64;
            }
            assert!(sum.abs() <= 8, "row {row} residue {sum}");
        }
    }

    #[test]
    fn all_glyphs_at_all_ui_sizes_rasterise() {
        let f = Font::parse(REGULAR).unwrap();
        for px in [11u16, 12, 13, 15, 17, 22, 28] {
            for g in 0..f.glyph_count() {
                let b = rasterize(&f, g, px);
                assert_eq!(b.data.len(), b.w as usize * b.h as usize);
                assert!(b.w <= 64 && b.h <= 64, "gid {g} {px}px {}x{}", b.w, b.h);
            }
        }
    }
}
