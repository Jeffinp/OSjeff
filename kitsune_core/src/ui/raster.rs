//! Pure 2-D drawing toolkit on premultiplied `0xAARRGGBB` surfaces: anti-aliased
//! rounded rectangles (circular and "continuous" corners), exact-area vector
//! paths, gradients, separable box blur, area/bilinear resampling, drop shadows
//! and the 1-D shadow profile the compositor uses for window shadows.
//!
//! Everything is integer / fixed point: the kernel has no hardware floating point.
//! The kernel's `Canvas` blits these surfaces (icons, cached backdrops) and uses
//! the same corner masks and profiles for its direct drawing, so the shapes drawn
//! into a cache and the ones drawn live agree pixel for pixel.

use crate::ui::gfx::isqrt;
use crate::ui::glyph::Path;
use alloc::vec;
use alloc::vec::Vec;

// ------------------------------------------------------------------ colour maths

/// Straight (non-premultiplied) colour from components.
#[inline]
pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

/// Opaque colour from `0xRRGGBB`.
#[inline]
pub const fn rgb(c: u32) -> u32 {
    0xFF00_0000 | (c & 0x00FF_FFFF)
}

/// Straight colour `0xRRGGBB` with alpha `a` (0..=255).
#[inline]
pub const fn rgba(c: u32, a: u8) -> u32 {
    ((a as u32) << 24) | (c & 0x00FF_FFFF)
}

#[inline]
pub fn alpha_of(c: u32) -> u32 {
    c >> 24
}

/// Straight -> premultiplied.
#[inline]
pub fn premul(c: u32) -> u32 {
    let a = c >> 24;
    if a == 255 {
        return c;
    }
    if a == 0 {
        return 0;
    }
    let m = |sh: u32| (((c >> sh) & 0xFF) * a + 127) / 255;
    (a << 24) | (m(16) << 16) | (m(8) << 8) | m(0)
}

/// Premultiplied -> straight (for tests and the odd conversion).
#[inline]
pub fn unpremul(c: u32) -> u32 {
    let a = c >> 24;
    if a == 0 || a == 255 {
        return c;
    }
    let u = |sh: u32| ((((c >> sh) & 0xFF) * 255 + a / 2) / a).min(255);
    (a << 24) | (u(16) << 16) | (u(8) << 8) | u(0)
}

/// Premultiplied source-over: `src` over `dst`.
#[inline]
pub fn over(dst: u32, src: u32) -> u32 {
    let sa = src >> 24;
    if sa == 255 {
        return src;
    }
    if sa == 0 {
        return dst;
    }
    let inv = 256 - (sa + (sa >> 7)); // 1..=255 (256 - 257 would underflow only for sa=255)
    let rb = (((dst & 0x00FF_00FF) * inv + 0x0080_0080) >> 8) & 0x00FF_00FF;
    let ag = ((((dst >> 8) & 0x00FF_00FF) * inv + 0x0080_0080) >> 8) & 0x00FF_00FF;
    src.wrapping_add(rb | (ag << 8))
}

/// Linear blend of two straight colours, `t` in `0..=256` (0 = `a`).
#[inline]
pub fn lerp(a: u32, b: u32, t: u32) -> u32 {
    let t = t.min(256);
    let m = |sh: u32| {
        let (x, y) = ((a >> sh) & 0xFF, (b >> sh) & 0xFF);
        (x * (256 - t) + y * t) >> 8
    };
    (m(24) << 24) | (m(16) << 16) | (m(8) << 8) | m(0)
}

/// Scale a straight colour's alpha by `a256` (0..=256).
#[inline]
pub fn mul_alpha(c: u32, a256: u32) -> u32 {
    let a = ((c >> 24) * a256.min(256)) >> 8;
    (a << 24) | (c & 0x00FF_FFFF)
}

/// How a shape is coloured. Colours are straight ARGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    Solid(u32),
    /// Top to bottom.
    Vertical(u32, u32),
    /// Top-left to bottom-right.
    Diagonal(u32, u32),
}

impl Paint {
    /// Colour at `(lx, ly)` of a `w x h` shape.
    #[inline]
    pub fn at(self, lx: usize, ly: usize, w: usize, h: usize) -> u32 {
        match self {
            Paint::Solid(c) => c,
            Paint::Vertical(a, b) => lerp(a, b, ((ly * 256 + 128) / h.max(1)) as u32),
            Paint::Diagonal(a, b) => {
                let t = (lx * 256 + 128) / w.max(1) + (ly * 256 + 128) / h.max(1);
                lerp(a, b, (t / 2) as u32)
            }
        }
    }
}

// ----------------------------------------------------------------- corner masks

/// Shape of a rounded corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    /// Quarter circle.
    Circle,
    /// "Continuous" corner: a superellipse (n = 4), what app icons and the dock use.
    Squircle,
}

/// Largest radius with a precomputed mask.
pub const MAX_RADIUS: usize = 64;

/// Coverage masks for the top-left corner of every radius `0..=MAX_RADIUS`.
pub struct CornerMasks {
    circle: Vec<Vec<u8>>,
    squircle: Vec<Vec<u8>>,
}

impl CornerMasks {
    pub fn new() -> CornerMasks {
        Self::with_max_radius(MAX_RADIUS)
    }

    /// Masks for radii `0..=max` (at most [`MAX_RADIUS`]); larger requests use the largest.
    pub fn with_max_radius(max: usize) -> CornerMasks {
        let max = max.min(MAX_RADIUS);
        let mut circle = Vec::with_capacity(max + 1);
        let mut squircle = Vec::with_capacity(max + 1);
        for r in 0..=max {
            circle.push(build_mask(r, Corner::Circle));
            squircle.push(build_mask(r, Corner::Squircle));
        }
        CornerMasks { circle, squircle }
    }

    /// `r * r` coverage bytes, row-major; `(0, 0)` is the outermost corner pixel.
    pub fn get(&self, style: Corner, r: usize) -> &[u8] {
        let r = r.min(self.circle.len() - 1);
        match style {
            Corner::Circle => &self.circle[r],
            Corner::Squircle => &self.squircle[r],
        }
    }
}

impl Default for CornerMasks {
    fn default() -> Self {
        Self::new()
    }
}

/// Coverage of pixel `(x, y)` of the corner square of radius `r`.
fn build_mask(r: usize, style: Corner) -> Vec<u8> {
    let mut m = vec![0u8; r * r];
    if r == 0 {
        return m;
    }
    for y in 0..r {
        for x in 0..r {
            // Pixel centre offset from the arc centre (at (r, r)), in 1/256 px.
            let dx = (r * 256) as i64 - (x * 256) as i64 - 128;
            let dy = (r * 256) as i64 - (y * 256) as i64 - 128;
            let cov256: i64 = match style {
                Corner::Circle => {
                    let d = isqrt((dx * dx + dy * dy) as usize) as i64;
                    ((r * 256) as i64 + 128 - d).clamp(0, 256)
                }
                Corner::Squircle => {
                    // u, v in Q16 (0..1), f = u^4 + v^4, signed distance ~ (1-f) r / |grad f|.
                    let uq = dx * 256 / r as i64;
                    let vq = dy * 256 / r as i64;
                    let (u2, v2) = ((uq * uq) >> 16, (vq * vq) >> 16);
                    let (u3, v3) = ((u2 * uq) >> 16, (v2 * vq) >> 16);
                    let (u4, v4) = ((u3 * uq) >> 16, (v3 * vq) >> 16);
                    let f = u4 + v4;
                    let g2 = 16 * ((u3 * u3 + v3 * v3) >> 16); // |grad f|^2, Q16
                    let g = isqrt((g2.max(1) as usize) << 16) as i64; // Q16
                    if g == 0 {
                        256
                    } else {
                        let sd = (65536 - f) * r as i64 * 256 / g; // pixels in 1/256
                        (sd + 128).clamp(0, 256)
                    }
                }
            };
            m[y * r + x] = ((cov256 * 255 + 128) >> 8) as u8;
        }
    }
    m
}

/// Coverage (0..=255) of pixel `(lx, ly)` of a `w x h` rounded rectangle with
/// corner radius `r` (already clamped to `min(w, h) / 2`) using `mask`.
#[inline]
pub fn rrect_cov(mask: &[u8], r: usize, w: usize, h: usize, lx: usize, ly: usize) -> u8 {
    if r == 0 {
        return 255;
    }
    let cx = if lx < r {
        lx
    } else if lx + r >= w {
        w - 1 - lx
    } else {
        return 255;
    };
    let cy = if ly < r {
        ly
    } else if ly + r >= h {
        h - 1 - ly
    } else {
        return 255;
    };
    mask[cy * r + cx]
}

// ------------------------------------------------------------------------ surface

/// A premultiplied-ARGB image.
#[derive(Clone)]
pub struct Surface {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Surface {
    /// A transparent surface.
    pub fn new(w: usize, h: usize) -> Surface {
        Surface {
            w,
            h,
            px: vec![0; w * h],
        }
    }

    pub fn fill(&mut self, straight: u32) {
        let p = premul(straight);
        self.px.fill(p);
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> u32 {
        self.px.get(y * self.w + x).copied().unwrap_or(0)
    }

    #[inline]
    fn put_over(&mut self, x: usize, y: usize, premultiplied: u32) {
        if x < self.w && y < self.h {
            let d = &mut self.px[y * self.w + x];
            *d = over(*d, premultiplied);
        }
    }

    /// Composite `paint` through coverage `a` (0..=255) at a pixel.
    #[inline]
    fn shade(&mut self, x: usize, y: usize, c: u32, a: u32) {
        let ca = (c >> 24) * a;
        let a8 = (ca + 127) / 255;
        if a8 == 0 {
            return;
        }
        self.put_over(x, y, premul((a8 << 24) | (c & 0x00FF_FFFF)));
    }

    /// Anti-aliased rounded rectangle. `r` is clamped to half the shorter side
    /// (and to [`MAX_RADIUS`]).
    #[allow(clippy::too_many_arguments)]
    pub fn fill_rrect(
        &mut self,
        x: i32,
        y: i32,
        w: usize,
        h: usize,
        r: usize,
        style: Corner,
        paint: Paint,
        masks: &CornerMasks,
    ) {
        let r = r.min(w / 2).min(h / 2).min(MAX_RADIUS);
        let mask = masks.get(style, r);
        for ly in 0..h {
            let py = y + ly as i32;
            if py < 0 || py as usize >= self.h {
                continue;
            }
            for lx in 0..w {
                let px = x + lx as i32;
                if px < 0 || px as usize >= self.w {
                    continue;
                }
                let cov = rrect_cov(mask, r, w, h, lx, ly) as u32;
                if cov != 0 {
                    self.shade(px as usize, py as usize, paint.at(lx, ly, w, h), cov);
                }
            }
        }
    }

    /// A `t`-pixel border drawn inside the rounded rectangle.
    #[allow(clippy::too_many_arguments)]
    pub fn stroke_rrect(
        &mut self,
        x: i32,
        y: i32,
        w: usize,
        h: usize,
        r: usize,
        t: usize,
        style: Corner,
        paint: Paint,
        masks: &CornerMasks,
    ) {
        let r = r.min(w / 2).min(h / 2).min(MAX_RADIUS);
        let ri = r.saturating_sub(t);
        let (wi, hi) = (w.saturating_sub(2 * t), h.saturating_sub(2 * t));
        let outer = masks.get(style, r);
        let inner = masks.get(style, ri.min(wi / 2).min(hi / 2));
        let ri = ri.min(wi / 2).min(hi / 2);
        for ly in 0..h {
            let py = y + ly as i32;
            if py < 0 || py as usize >= self.h {
                continue;
            }
            for lx in 0..w {
                let px = x + lx as i32;
                if px < 0 || px as usize >= self.w {
                    continue;
                }
                let co = rrect_cov(outer, r, w, h, lx, ly) as i32;
                let ci = if lx >= t && ly >= t && lx - t < wi && ly - t < hi {
                    rrect_cov(inner, ri, wi, hi, lx - t, ly - t) as i32
                } else {
                    0
                };
                let cov = (co - ci).max(0) as u32;
                if cov != 0 {
                    self.shade(px as usize, py as usize, paint.at(lx, ly, w, h), cov);
                }
            }
        }
    }

    /// Fill an arbitrary path (24.8 coordinates in surface pixels) with `paint`.
    pub fn fill_path(&mut self, path: &Path, paint: Paint) {
        let Some((x0, y0, x1, y1)) = path.bounds() else {
            return;
        };
        let bx = x0.div_euclid(256).max(0);
        let by = y0.div_euclid(256).max(0);
        let ex = ((x1 + 255).div_euclid(256)).min(self.w as i32);
        let ey = ((y1 + 255).div_euclid(256)).min(self.h as i32);
        if ex <= bx || ey <= by {
            return;
        }
        let (w, h) = ((ex - bx) as usize, (ey - by) as usize);
        let cov = path.coverage(w, h, bx * 256, by * 256);
        for ly in 0..h {
            for lx in 0..w {
                let c = cov[ly * w + lx] as u32;
                if c != 0 {
                    self.shade(
                        bx as usize + lx,
                        by as usize + ly,
                        paint.at(lx, ly, w, h),
                        c,
                    );
                }
            }
        }
    }

    /// Source-over `src` at `(dx, dy)` with opacity `0..=256`.
    pub fn blit(&mut self, src: &Surface, dx: i32, dy: i32, opacity: u32) {
        let op = opacity.min(256);
        for sy in 0..src.h {
            let y = dy + sy as i32;
            if y < 0 || y as usize >= self.h {
                continue;
            }
            for sx in 0..src.w {
                let x = dx + sx as i32;
                if x < 0 || x as usize >= self.w {
                    continue;
                }
                let mut p = src.px[sy * src.w + sx];
                if p >> 24 == 0 {
                    continue;
                }
                if op != 256 {
                    p = scale_premul(p, op);
                }
                self.put_over(x as usize, y as usize, p);
            }
        }
    }

    /// Multiply every pixel (all four channels) by `a256 / 256`.
    pub fn fade(&mut self, a256: u32) {
        for p in &mut self.px {
            *p = scale_premul(*p, a256);
        }
    }

    /// Keep only the part of the surface inside a rounded rectangle covering all of it.
    pub fn mask_rrect(&mut self, r: usize, style: Corner, masks: &CornerMasks) {
        let (w, h) = (self.w, self.h);
        let r = r.min(w / 2).min(h / 2).min(MAX_RADIUS);
        let mask = masks.get(style, r);
        for ly in 0..h {
            for lx in 0..w {
                let c = rrect_cov(mask, r, w, h, lx, ly) as u32;
                if c != 255 {
                    let p = &mut self.px[ly * w + lx];
                    *p = scale_premul(*p, c + (c >> 7));
                }
            }
        }
    }

    /// Separable box blur of all four channels, `passes` times (3 approximates a
    /// Gaussian). Edge pixels are repeated.
    pub fn blur(&mut self, radius: usize, passes: usize) {
        blur_u32(&mut self.px, self.w, self.h, radius, passes);
    }

    /// Resample to `nw x nh`: area average when shrinking, bilinear when growing.
    pub fn resized(&self, nw: usize, nh: usize) -> Surface {
        if nw == 0 || nh == 0 || self.w == 0 || self.h == 0 {
            return Surface::new(nw, nh);
        }
        if nw == self.w && nh == self.h {
            return self.clone();
        }
        // Horizontal pass into (nw x h), then vertical into (nw x nh).
        let taps_x = taps(self.w, nw);
        let taps_y = taps(self.h, nh);
        let mut mid = vec![0u32; nw * self.h];
        for y in 0..self.h {
            let row = &self.px[y * self.w..(y + 1) * self.w];
            for (x, t) in taps_x.iter().enumerate() {
                mid[y * nw + x] = weigh(t, |i| row[i]);
            }
        }
        let mut out = Surface::new(nw, nh);
        for (y, t) in taps_y.iter().enumerate() {
            for x in 0..nw {
                out.px[y * nw + x] = weigh(t, |i| mid[i * nw + x]);
            }
        }
        out
    }

    /// A copy with a soft drop shadow (`dy` down, blur radius `blur`, `alpha`
    /// 0..=255 of black) around the shape, on a surface `margin` pixels larger on
    /// each side.
    pub fn with_shadow(&self, margin: usize, dy: i32, blur: usize, alpha: u32) -> Surface {
        let (w, h) = (self.w + 2 * margin, self.h + 2 * margin);
        let mut sh = Surface::new(w, h);
        for y in 0..self.h {
            for x in 0..self.w {
                let a = self.px[y * self.w + x] >> 24;
                let ty = y as i32 + margin as i32 + dy;
                if a != 0 && ty >= 0 && (ty as usize) < h {
                    sh.px[ty as usize * w + x + margin] = (a * alpha / 255) << 24;
                }
            }
        }
        sh.blur(blur, 3);
        sh.blit(self, margin as i32, margin as i32, 256);
        sh
    }
}

/// Scale all four premultiplied channels by `a256 / 256`.
#[inline]
pub fn scale_premul(p: u32, a256: u32) -> u32 {
    let rb = (((p & 0x00FF_00FF) * a256 + 0x0080_0080) >> 8) & 0x00FF_00FF;
    let ag = ((((p >> 8) & 0x00FF_00FF) * a256 + 0x0080_0080) >> 8) & 0x00FF_00FF;
    rb | (ag << 8)
}

// Resampling taps: for every destination index the first source index and the
// weights (Q16, summing to exactly 65536).
struct Tap {
    start: usize,
    w: Vec<u32>,
}

fn taps(src: usize, dst: usize) -> Vec<Tap> {
    let mut out = Vec::with_capacity(dst);
    for d in 0..dst {
        if dst < src {
            // Box filter over [d*src/dst, (d+1)*src/dst) in Q8.
            let a = d * src * 256 / dst;
            let b = ((d + 1) * src * 256 / dst).max(a + 1);
            let first = a / 256;
            let last = b.div_ceil(256).min(src);
            let mut w = Vec::with_capacity(last - first);
            for i in first..last {
                let lo = a.max(i * 256);
                let hi = b.min((i + 1) * 256);
                w.push(((hi - lo) * 65536 / (b - a)) as u32);
            }
            normalise(&mut w);
            out.push(Tap { start: first, w });
        } else {
            // Bilinear: sample position of the destination pixel centre in the source.
            let pos = ((2 * d + 1) * src * 128 / dst) as i64 - 128; // Q8 of (x+0.5)*s - 0.5
            let pos = pos.clamp(0, ((src - 1) * 256) as i64) as usize;
            let i = pos / 256;
            let f = (pos % 256) as u32;
            if i + 1 >= src || f == 0 {
                out.push(Tap {
                    start: i.min(src - 1),
                    w: vec![65536],
                });
            } else {
                out.push(Tap {
                    start: i,
                    w: vec![(256 - f) * 256, f * 256],
                });
            }
        }
    }
    out
}

fn normalise(w: &mut [u32]) {
    let sum: u32 = w.iter().sum();
    if let Some(last) = w.last_mut() {
        if sum < 65536 {
            *last += 65536 - sum;
        } else {
            *last -= (sum - 65536).min(*last);
        }
    }
}

fn weigh(t: &Tap, get: impl Fn(usize) -> u32) -> u32 {
    let (mut a, mut r, mut g, mut b) = (0u32, 0u32, 0u32, 0u32);
    for (k, &wt) in t.w.iter().enumerate() {
        let p = get(t.start + k);
        a += (p >> 24) * wt;
        r += ((p >> 16) & 0xFF) * wt;
        g += ((p >> 8) & 0xFF) * wt;
        b += (p & 0xFF) * wt;
    }
    let q = |v: u32| (v + 32768) >> 16;
    (q(a).min(255) << 24) | (q(r).min(255) << 16) | (q(g).min(255) << 8) | q(b).min(255)
}

/// Separable box blur on packed 4-channel pixels (alpha-premultiplied or opaque).
pub fn blur_u32(px: &mut [u32], w: usize, h: usize, radius: usize, passes: usize) {
    if radius == 0 || w == 0 || h == 0 || px.len() < w * h {
        return;
    }
    let radius = radius.min(127);
    let mut tmp = vec![0u32; w.max(h)];
    for _ in 0..passes.max(1) {
        for y in 0..h {
            blur_line(&mut px[y * w..(y + 1) * w], radius, &mut tmp);
        }
        // Columns: gather, blur, scatter.
        let mut col = vec![0u32; h];
        for x in 0..w {
            for y in 0..h {
                col[y] = px[y * w + x];
            }
            blur_line(&mut col, radius, &mut tmp);
            for y in 0..h {
                px[y * w + x] = col[y];
            }
        }
    }
}

fn blur_line(line: &mut [u32], radius: usize, tmp: &mut [u32]) {
    let n = line.len();
    let win = 2 * radius + 1;
    let inv = (1u32 << 16) / win as u32;
    let at = |i: isize| -> u32 { line[i.clamp(0, n as isize - 1) as usize] };
    let (mut a, mut r, mut g, mut b) = (0u32, 0u32, 0u32, 0u32);
    for k in -(radius as isize)..=(radius as isize) {
        let p = at(k);
        a += p >> 24;
        r += (p >> 16) & 0xFF;
        g += (p >> 8) & 0xFF;
        b += p & 0xFF;
    }
    for (i, slot) in tmp.iter_mut().enumerate().take(n) {
        let q = |v: u32| ((v * inv + 32768) >> 16).min(255);
        *slot = (q(a) << 24) | (q(r) << 16) | (q(g) << 8) | q(b);
        let out = at(i as isize - radius as isize);
        let inn = at(i as isize + radius as isize + 1);
        a = a + (inn >> 24) - (out >> 24);
        r = r + ((inn >> 16) & 0xFF) - ((out >> 16) & 0xFF);
        g = g + ((inn >> 8) & 0xFF) - ((out >> 8) & 0xFF);
        b = b + (inn & 0xFF) - (out & 0xFF);
    }
    line.copy_from_slice(&tmp[..n]);
}

// ------------------------------------------------------------------------ shadows

/// One axis of a blurred-rectangle shadow: coverage (0..=255) of `len + 2 * blur`
/// positions for a segment of `len` pixels starting at index `blur`, with a
/// smoothstep transition `blur` pixels either side of each edge. The 2-D shadow of
/// a rectangle is the product of two such profiles (the blur is separable).
pub fn shadow_profile(len: usize, blur: usize) -> Vec<u8> {
    let total = len + 2 * blur;
    let mut p = vec![0u8; total];
    if blur == 0 {
        for v in p.iter_mut().take(len) {
            *v = 255;
        }
        return p;
    }
    let cdf = |x2: i64| -> i64 {
        // x2 = 2 * (distance past the edge); transition covers -2b..2b.
        let s =
            (((x2 + 2 * blur as i64) * 256 + 2 * blur as i64) / (4 * blur as i64)).clamp(0, 256);
        s * s * (768 - 2 * s) / (256 * 256) // 0..=256
    };
    for (i, v) in p.iter_mut().enumerate() {
        let x2 = 2 * i as i64 + 1 - 2 * blur as i64;
        let cov = cdf(x2) - cdf(x2 - 2 * len as i64);
        *v = (cov.clamp(0, 256) * 255 / 256) as u8;
    }
    p
}

/// Opacity (0..=255) of a shadow pixel from the two axis profile values and the
/// shadow's overall opacity (0..=256).
#[inline]
pub fn shadow_alpha(px: u8, py: u8, opacity: u32) -> u32 {
    (px as u32 * py as u32 * opacity.min(256)) >> 16
}

// ------------------------------------------------------------------------ glow

/// Falloff table for a soft radial glow: entry `i` is the opacity (0..=255) at a
/// squared distance of `i / 255` of the squared radius (smoothstep fall-off).
pub fn glow_lut() -> [u8; 256] {
    let mut t = [0u8; 256];
    for (i, v) in t.iter_mut().enumerate() {
        // Distance fraction d = sqrt(i/255): approximate with i itself in d^2 space,
        // then smoothstep(1 - d^2) for a long soft tail.
        let s = 256 - i as i64 * 256 / 255;
        *v = (s * s * (768 - 2 * s) / (256 * 256) * 255 / 256) as u8;
    }
    t
}

#[cfg(test)]
mod tests;
