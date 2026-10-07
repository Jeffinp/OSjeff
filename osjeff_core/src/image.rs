//! In-memory RGBA8 raster images and the integer-only operations a viewer,
//! icon loader or wallpaper pipeline needs.
//!
//! # Pixel format
//!
//! A pixel is a `u32` laid out `0xAARRGGBB` (the same order the framebuffer
//! uses, so opaque pixels can be copied straight to the screen). Alpha is
//! *straight* (not premultiplied). [`rgba`] and [`channels`] convert to and
//! from `[r, g, b, a]`.
//!
//! # Limits
//!
//! [`MAX_PIXELS`] (16 Mpx, 64 MiB of pixels) bounds every image. The check
//! runs on the *dimensions*, with overflow-safe arithmetic, before any
//! allocation, so a hostile header cannot make a decoder reserve gigabytes.
//! Zero-sized images do not exist. Allocation goes through
//! `try_reserve`, so failure is an [`ImageError::OutOfMemory`], not an abort.
//!
//! # Arithmetic
//!
//! There is no floating point anywhere (the kernel is soft-float). Resampling
//! uses fixed point and documents its precision:
//!
//! * [`Image::resize_bilinear`]: sample positions have 8 fractional bits
//!   (1/256 of a source pixel); the 2-D weights sum to exactly 65536, so a
//!   constant image stays exactly constant.
//! * [`Image::resize_box`]: exact area weights in integers; the horizontal
//!   pass keeps 8 extra bits of precision before the vertical pass.
//! * Both interpolate in premultiplied alpha, so a transparent pixel of any
//!   colour does not tint its neighbours.

use alloc::vec::Vec;
use core::fmt;

/// Largest accepted image, in pixels (`width * height`).
pub const MAX_PIXELS: usize = 16 * 1024 * 1024;

/// Why an image operation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageError {
    /// A zero width or height.
    ZeroSize,
    /// `width * height` exceeds [`MAX_PIXELS`] (or overflows).
    TooLarge,
    /// The pixel/byte buffer does not match `width * height`.
    BadBuffer,
    /// A rectangle that does not lie inside the image.
    OutOfBounds,
    /// The allocator refused the request.
    OutOfMemory,
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ImageError::ZeroSize => "image has a zero dimension",
            ImageError::TooLarge => "image is larger than the pixel limit",
            ImageError::BadBuffer => "buffer size does not match the dimensions",
            ImageError::OutOfBounds => "rectangle is outside the image",
            ImageError::OutOfMemory => "out of memory",
        })
    }
}

/// Packs `r, g, b, a` into `0xAARRGGBB`.
#[inline]
pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

/// Unpacks a pixel into `[r, g, b, a]`.
#[inline]
pub const fn channels(p: u32) -> [u8; 4] {
    [(p >> 16) as u8, (p >> 8) as u8, p as u8, (p >> 24) as u8]
}

/// The alpha byte of a pixel.
#[inline]
pub const fn alpha(p: u32) -> u8 {
    (p >> 24) as u8
}

/// `round(x / 255)` for `x <= 65025`, without a division.
#[inline]
const fn div255(x: u32) -> u32 {
    let t = x + 128;
    (t + (t >> 8)) >> 8
}

/// Straight-alpha "source over destination".
///
/// Opaque destinations (the common case: a wallpaper) take a cheap path; the
/// general case divides by the exact combined weight so channels never exceed
/// 255.
#[inline]
pub fn over(src: u32, dst: u32) -> u32 {
    let sa = src >> 24;
    if sa == 255 {
        return src;
    }
    if sa == 0 {
        return dst;
    }
    let da = dst >> 24;
    let [sr, sg, sb, _] = channels(src);
    let [dr, dg, db, _] = channels(dst);
    if da == 255 {
        let ia = 255 - sa;
        let mix = |s: u8, d: u8| div255(s as u32 * sa + d as u32 * ia) as u8;
        return rgba(mix(sr, dr), mix(sg, dg), mix(sb, db), 255);
    }
    let wd = da * (255 - sa);
    let den = sa * 255 + wd;
    if den == 0 {
        return 0;
    }
    let mix = |s: u8, d: u8| ((s as u32 * sa * 255 + d as u32 * wd + den / 2) / den) as u8;
    rgba(
        mix(sr, dr),
        mix(sg, dg),
        mix(sb, db),
        ((den + 127) / 255) as u8,
    )
}

/// `width * height` if both are non-zero and the product is within
/// [`MAX_PIXELS`]. Overflow-safe; allocates nothing.
pub fn pixel_count(width: usize, height: usize) -> Result<usize, ImageError> {
    if width == 0 || height == 0 {
        return Err(ImageError::ZeroSize);
    }
    match width.checked_mul(height) {
        Some(n) if n <= MAX_PIXELS => Ok(n),
        _ => Err(ImageError::TooLarge),
    }
}

/// A `Vec` of `n` copies of `v`, or `OutOfMemory`.
pub(crate) fn try_vec<T: Clone>(n: usize, v: T) -> Result<Vec<T>, ImageError> {
    let mut out = Vec::new();
    out.try_reserve_exact(n)
        .map_err(|_| ImageError::OutOfMemory)?;
    out.resize(n, v);
    Ok(out)
}

/// How [`Image::resize`] samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// Pick the nearest source pixel (sharp, blocky; right for pixel art).
    Nearest,
    /// Interpolate the four nearest pixels (smooth enlargement; aliases when
    /// shrinking by more than 2x).
    Bilinear,
    /// Average the exact source area (right for thumbnails).
    Box,
    /// [`Filter::Box`] when either dimension shrinks, else
    /// [`Filter::Bilinear`].
    Auto,
}

/// An RGBA8 image (`0xAARRGGBB` pixels, straight alpha, row-major, top-down).
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    width: usize,
    height: usize,
    pixels: Vec<u32>,
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Image({}x{})", self.width, self.height)
    }
}

/// One bilinear sample: the two source indices and the weight (0..=255) of
/// the second.
#[inline]
fn bilinear_tap(d: usize, src: usize, dst: usize) -> (usize, usize, u32) {
    // Source coordinate of the destination pixel centre, minus the source
    // pixel centre, with 8 fractional bits:
    //   ((2d+1) * src / (2 dst) - 1/2) * 256
    let num = ((2 * d as i64 + 1) * src as i64 - dst as i64) * 256;
    let p = num.div_euclid(2 * dst as i64);
    let last = src - 1;
    if p < 0 {
        return (0, 0, 0);
    }
    let i = (p >> 8) as usize;
    if i >= last {
        return (last, last, 0);
    }
    (i, i + 1, (p & 255) as u32)
}

impl Image {
    /// A `width x height` image filled with `fill`.
    pub fn new(width: usize, height: usize, fill: u32) -> Result<Image, ImageError> {
        let n = pixel_count(width, height)?;
        Ok(Image {
            width,
            height,
            pixels: try_vec(n, fill)?,
        })
    }

    /// Wraps an existing pixel buffer (`width * height` entries).
    pub fn from_pixels(width: usize, height: usize, pixels: Vec<u32>) -> Result<Image, ImageError> {
        let n = pixel_count(width, height)?;
        if pixels.len() != n {
            return Err(ImageError::BadBuffer);
        }
        Ok(Image {
            width,
            height,
            pixels,
        })
    }

    /// Builds an image from `R, G, B, A` bytes (`4 * width * height` of them).
    pub fn from_rgba(width: usize, height: usize, bytes: &[u8]) -> Result<Image, ImageError> {
        let n = pixel_count(width, height)?;
        if bytes.len() != n * 4 {
            return Err(ImageError::BadBuffer);
        }
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(n)
            .map_err(|_| ImageError::OutOfMemory)?;
        pixels.extend(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| rgba(c[0], c[1], c[2], c[3])),
        );
        Ok(Image {
            width,
            height,
            pixels,
        })
    }

    /// The pixels as `R, G, B, A` bytes.
    pub fn to_rgba(&self) -> Result<Vec<u8>, ImageError> {
        let mut out = Vec::new();
        out.try_reserve_exact(self.pixels.len() * 4)
            .map_err(|_| ImageError::OutOfMemory)?;
        for &p in &self.pixels {
            out.extend_from_slice(&channels(p));
        }
        Ok(out)
    }

    #[inline]
    pub fn width(&self) -> usize {
        self.width
    }

    #[inline]
    pub fn height(&self) -> usize {
        self.height
    }

    /// All pixels, row-major.
    #[inline]
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    #[inline]
    pub fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.pixels
    }

    /// Gives the pixel buffer back.
    pub fn into_pixels(self) -> Vec<u32> {
        self.pixels
    }

    /// Row `y`, or an empty slice when out of range.
    #[inline]
    pub fn row(&self, y: usize) -> &[u32] {
        y.checked_mul(self.width)
            .and_then(|s| self.pixels.get(s..s + self.width))
            .unwrap_or(&[])
    }

    /// Mutable row `y`, or an empty slice when out of range.
    #[inline]
    pub fn row_mut(&mut self, y: usize) -> &mut [u32] {
        let w = self.width;
        match y.checked_mul(w) {
            Some(s) => self.pixels.get_mut(s..s + w).unwrap_or(&mut []),
            None => &mut [],
        }
    }

    /// The pixel at `(x, y)`.
    #[inline]
    pub fn get(&self, x: usize, y: usize) -> Option<u32> {
        if x < self.width {
            self.row(y).get(x).copied()
        } else {
            None
        }
    }

    /// Sets a pixel; `false` when out of range.
    #[inline]
    pub fn set(&mut self, x: usize, y: usize, p: u32) -> bool {
        if x >= self.width {
            return false;
        }
        match self.row_mut(y).get_mut(x) {
            Some(slot) => {
                *slot = p;
                true
            }
            None => false,
        }
    }

    /// True when every pixel has alpha 255.
    pub fn is_opaque(&self) -> bool {
        self.pixels.iter().all(|&p| p >> 24 == 255)
    }

    // ---------------------------------------------------------------- crop

    /// Copies the `w x h` rectangle at `(x, y)`.
    pub fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> Result<Image, ImageError> {
        let n = pixel_count(w, h)?;
        let x1 = x.checked_add(w).ok_or(ImageError::OutOfBounds)?;
        let y1 = y.checked_add(h).ok_or(ImageError::OutOfBounds)?;
        if x1 > self.width || y1 > self.height {
            return Err(ImageError::OutOfBounds);
        }
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(n)
            .map_err(|_| ImageError::OutOfMemory)?;
        for row in self.pixels.chunks_exact(self.width).skip(y).take(h) {
            pixels.extend_from_slice(&row[x..x1]);
        }
        Ok(Image {
            width: w,
            height: h,
            pixels,
        })
    }

    // ---------------------------------------------------------------- resize

    /// Resizes with `filter`.
    pub fn resize(&self, w: usize, h: usize, filter: Filter) -> Result<Image, ImageError> {
        match filter {
            Filter::Nearest => self.resize_nearest(w, h),
            Filter::Bilinear => self.resize_bilinear(w, h),
            Filter::Box => self.resize_box(w, h),
            Filter::Auto => {
                if w < self.width || h < self.height {
                    self.resize_box(w, h)
                } else {
                    self.resize_bilinear(w, h)
                }
            }
        }
    }

    /// Nearest-neighbour resize (samples the source pixel under each
    /// destination pixel's centre).
    pub fn resize_nearest(&self, w: usize, h: usize) -> Result<Image, ImageError> {
        let n = pixel_count(w, h)?;
        if w == self.width && h == self.height {
            return Ok(self.clone());
        }
        let xmap = {
            let mut m = try_vec(w, 0u32)?;
            for (dx, slot) in m.iter_mut().enumerate() {
                *slot = (((2 * dx as u64 + 1) * self.width as u64) / (2 * w as u64)) as u32;
            }
            m
        };
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(n)
            .map_err(|_| ImageError::OutOfMemory)?;
        for dy in 0..h {
            let sy = ((2 * dy as u64 + 1) * self.height as u64) / (2 * h as u64);
            let row = self.row(sy as usize);
            pixels.extend(
                xmap.iter()
                    .map(|&sx| row.get(sx as usize).copied().unwrap_or(0)),
            );
        }
        Ok(Image {
            width: w,
            height: h,
            pixels,
        })
    }

    /// Bilinear resize with 1/256-pixel sample positions (see the module docs
    /// for the precision). Shrinking by more than 2x aliases; use
    /// [`resize_box`](Self::resize_box) for thumbnails.
    pub fn resize_bilinear(&self, w: usize, h: usize) -> Result<Image, ImageError> {
        let n = pixel_count(w, h)?;
        if w == self.width && h == self.height {
            return Ok(self.clone());
        }
        let mut xtaps: Vec<(u32, u32, u32)> = Vec::new();
        xtaps
            .try_reserve_exact(w)
            .map_err(|_| ImageError::OutOfMemory)?;
        xtaps.extend((0..w).map(|dx| {
            let (a, b, f) = bilinear_tap(dx, self.width, w);
            (a as u32, b as u32, f)
        }));
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(n)
            .map_err(|_| ImageError::OutOfMemory)?;
        let opaque = self.is_opaque();
        for dy in 0..h {
            let (y0, y1, fy) = bilinear_tap(dy, self.height, h);
            let (r0, r1) = (self.row(y0), self.row(y1));
            let wy1 = fy;
            let wy0 = 256 - fy;
            pixels.extend(xtaps.iter().map(|&(x0, x1, fx)| {
                let (x0, x1) = (x0 as usize, x1 as usize);
                let p = [
                    r0.get(x0).copied().unwrap_or(0),
                    r0.get(x1).copied().unwrap_or(0),
                    r1.get(x0).copied().unwrap_or(0),
                    r1.get(x1).copied().unwrap_or(0),
                ];
                let wx1 = fx;
                let wx0 = 256 - fx;
                let wt = [wx0 * wy0, wx1 * wy0, wx0 * wy1, wx1 * wy1];
                if opaque {
                    blend4_opaque(p, wt)
                } else {
                    blend4_alpha(p, wt)
                }
            }));
        }
        Ok(Image {
            width: w,
            height: h,
            pixels,
        })
    }

    /// Area-average resize: every destination pixel is the exact weighted
    /// mean of the source area it covers (premultiplied alpha). Best for
    /// shrinking; when enlarging it degrades to a smoothed nearest.
    ///
    /// Memory is `O(w)` beyond the output: source rows are streamed.
    pub fn resize_box(&self, w: usize, h: usize) -> Result<Image, ImageError> {
        let n = pixel_count(w, h)?;
        if w == self.width && h == self.height {
            return Ok(self.clone());
        }
        let sh = self.height as u64;
        let dh = h as u64;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(n)
            .map_err(|_| ImageError::OutOfMemory)?;
        // Horizontally reduced source row: [A*256, Pr, Pg, Pb] per column,
        // where P = alpha * channel (premultiplied), all averages.
        let mut hrow = try_vec(w * 4, 0u32)?;
        let mut acc = try_vec(w * 4, 0u64)?;
        let mut cached = usize::MAX;
        for dy in 0..dh {
            acc.fill(0);
            let (y_lo, y_hi) = (dy * sh, (dy + 1) * sh);
            let k0 = y_lo / dh;
            let k1 = y_hi.div_ceil(dh).min(sh);
            for k in k0..k1 {
                let wv = y_hi.min((k + 1) * dh) - y_lo.max(k * dh);
                if cached != k as usize {
                    self.reduce_row_box(k as usize, w, &mut hrow);
                    cached = k as usize;
                }
                for (a, &hv) in acc.iter_mut().zip(hrow.iter()) {
                    *a += wv * hv as u64;
                }
            }
            for q in acc.as_chunks::<4>().0 {
                let a_v = (q[0] + sh / 2) / sh; // alpha * 256
                if a_v == 0 {
                    pixels.push(0);
                    continue;
                }
                let ch = |p: u64| -> u8 {
                    let pc = (p + sh / 2) / sh; // average of alpha*channel
                    ((pc * 256 + a_v / 2) / a_v).min(255) as u8
                };
                let a = ((a_v + 128) >> 8).min(255) as u8;
                pixels.push(rgba(ch(q[1]), ch(q[2]), ch(q[3]), a));
            }
        }
        Ok(Image {
            width: w,
            height: h,
            pixels,
        })
    }

    /// Horizontal area reduction of source row `k` into `out` (4 values per
    /// destination column).
    fn reduce_row_box(&self, k: usize, dst_w: usize, out: &mut [u32]) {
        let row = self.row(k);
        let sw = self.width as u64;
        let dw = dst_w as u64;
        for (dx, o) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (x_lo, x_hi) = (dx as u64 * sw, (dx as u64 + 1) * sw);
            let c0 = x_lo / dw;
            let c1 = x_hi.div_ceil(dw).min(sw);
            let (mut sa, mut sr, mut sg, mut sb) = (0u64, 0u64, 0u64, 0u64);
            let span = row.get(c0 as usize..c1 as usize).unwrap_or(&[]);
            for (i, &p) in span.iter().enumerate() {
                let c = c0 + i as u64;
                let wgt = x_hi.min((c + 1) * dw) - x_lo.max(c * dw);
                let [r, g, b, a] = channels(p);
                let a = a as u64;
                sa += wgt * a;
                sr += wgt * (r as u64 * a);
                sg += wgt * (g as u64 * a);
                sb += wgt * (b as u64 * a);
            }
            o[0] = ((sa * 256 + sw / 2) / sw) as u32;
            o[1] = ((sr + sw / 2) / sw) as u32;
            o[2] = ((sg + sw / 2) / sw) as u32;
            o[3] = ((sb + sw / 2) / sw) as u32;
        }
    }

    // ---------------------------------------------------------------- fit

    /// The size of the largest image with this one's aspect ratio that fits in
    /// `box_w x box_h` (each dimension at least 1). Without `upscale`, an
    /// image already inside the box keeps its size.
    pub fn fit_dims(
        &self,
        box_w: usize,
        box_h: usize,
        upscale: bool,
    ) -> Result<(usize, usize), ImageError> {
        if box_w == 0 || box_h == 0 {
            return Err(ImageError::ZeroSize);
        }
        let (sw, sh) = (self.width as u128, self.height as u128);
        let (bw, bh) = (box_w as u128, box_h as u128);
        if !upscale && sw <= bw && sh <= bh {
            return Ok((self.width, self.height));
        }
        let (w, h) = if sw * bh >= sh * bw {
            (bw, ((sh * bw + sw / 2) / sw).max(1))
        } else {
            (((sw * bh + sh / 2) / sh).max(1), bh)
        };
        Ok((w as usize, h as usize))
    }

    /// Scales to fit `box_w x box_h` keeping the aspect ratio
    /// ([`fit_dims`](Self::fit_dims)), resampling with `filter`.
    pub fn fit(
        &self,
        box_w: usize,
        box_h: usize,
        upscale: bool,
        filter: Filter,
    ) -> Result<Image, ImageError> {
        let (w, h) = self.fit_dims(box_w, box_h, upscale)?;
        self.resize(w, h, filter)
    }

    // ---------------------------------------------------------------- orientation

    /// Rotates 90 degrees clockwise (`w x h` becomes `h x w`).
    pub fn rotate90(&self) -> Result<Image, ImageError> {
        let (sw, sh) = (self.width, self.height);
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(self.pixels.len())
            .map_err(|_| ImageError::OutOfMemory)?;
        for dy in 0..sw {
            // Destination row `dy` is source column `dy`, read bottom-up.
            pixels.extend(
                (0..sh)
                    .rev()
                    .map(|sy| self.pixels.get(sy * sw + dy).copied().unwrap_or(0)),
            );
        }
        Ok(Image {
            width: sh,
            height: sw,
            pixels,
        })
    }

    /// Rotates 90 degrees counter-clockwise (`w x h` becomes `h x w`).
    pub fn rotate270(&self) -> Result<Image, ImageError> {
        let (sw, sh) = (self.width, self.height);
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(self.pixels.len())
            .map_err(|_| ImageError::OutOfMemory)?;
        for dy in 0..sw {
            // Destination row `dy` is source column `sw - 1 - dy`, top-down.
            let sx = sw - 1 - dy;
            pixels.extend((0..sh).map(|sy| self.pixels.get(sy * sw + sx).copied().unwrap_or(0)));
        }
        Ok(Image {
            width: sh,
            height: sw,
            pixels,
        })
    }

    /// Rotates 180 degrees in place.
    pub fn rotate180(&mut self) {
        self.pixels.reverse();
    }

    /// Mirrors left-right in place.
    pub fn flip_horizontal(&mut self) {
        let w = self.width;
        for row in self.pixels.chunks_exact_mut(w) {
            row.reverse();
        }
    }

    /// Mirrors top-bottom in place.
    pub fn flip_vertical(&mut self) {
        let w = self.width;
        let h = self.height;
        for y in 0..h / 2 {
            let (top, bottom) = self.pixels.split_at_mut((h - 1 - y) * w);
            top[y * w..(y + 1) * w].swap_with_slice(&mut bottom[..w]);
        }
    }

    // ---------------------------------------------------------------- compositing

    /// Composites every pixel over the opaque colour `bg` (alpha is ignored
    /// in `bg`), leaving a fully opaque image.
    pub fn flatten(&mut self, bg: u32) {
        let bg = bg | 0xFF00_0000;
        for p in &mut self.pixels {
            *p = over(*p, bg);
        }
    }

    /// Composites `src` over this image with its top-left corner at
    /// `(x, y)` (which may be negative). Parts outside are clipped.
    pub fn blit_over(&mut self, src: &Image, x: i32, y: i32) {
        let (dw, dh) = (self.width as i64, self.height as i64);
        let (sw, sh) = (src.width as i64, src.height as i64);
        let (x, y) = (x as i64, y as i64);
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + sw).min(dw);
        let y1 = (y + sh).min(dh);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let n = (x1 - x0) as usize;
        for dy in y0..y1 {
            let sy = (dy - y) as usize;
            let sx = (x0 - x) as usize;
            let s = src.row(sy).get(sx..sx + n).unwrap_or(&[]);
            if let Some(d) = self
                .row_mut(dy as usize)
                .get_mut(x0 as usize..x0 as usize + n)
            {
                for (dp, &sp) in d.iter_mut().zip(s) {
                    *dp = over(sp, *dp);
                }
            }
        }
    }
}

/// Weighted mean of four opaque pixels (weights sum to 65536).
#[inline]
fn blend4_opaque(p: [u32; 4], w: [u32; 4]) -> u32 {
    let ch = |shift: u32| -> u32 {
        let s = ((p[0] >> shift) & 255) * w[0]
            + ((p[1] >> shift) & 255) * w[1]
            + ((p[2] >> shift) & 255) * w[2]
            + ((p[3] >> shift) & 255) * w[3];
        (s + 32768) >> 16
    };
    0xFF00_0000 | (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// Weighted mean of four pixels in premultiplied alpha (weights sum to 65536).
#[inline]
fn blend4_alpha(p: [u32; 4], w: [u32; 4]) -> u32 {
    let ta = [
        w[0] * (p[0] >> 24),
        w[1] * (p[1] >> 24),
        w[2] * (p[2] >> 24),
        w[3] * (p[3] >> 24),
    ];
    let a_sum = ta[0] + ta[1] + ta[2] + ta[3];
    if a_sum == 0 {
        return 0;
    }
    let ch = |shift: u32| -> u32 {
        let s = ((p[0] >> shift) & 255) * ta[0]
            + ((p[1] >> shift) & 255) * ta[1]
            + ((p[2] >> shift) & 255) * ta[2]
            + ((p[3] >> shift) & 255) * ta[3];
        (s + a_sum / 2) / a_sum
    };
    let a = ((a_sum + 32768) >> 16).min(255);
    (a << 24) | (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

// ---------------------------------------------------------------------------
// Format detection and one-call decode/encode
// ---------------------------------------------------------------------------

/// A file format this crate can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Bmp,
    /// Netpbm `P3`/`P6`.
    Ppm,
}

impl Format {
    /// A short lowercase name ("png", "bmp", "ppm").
    pub fn name(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Bmp => "bmp",
            Format::Ppm => "ppm",
        }
    }
}

/// Identifies the format from the first bytes, without decoding. A file that
/// starts like a PNG/BMP/PPM but is damaged is still reported as that format,
/// so [`decode`] can say *why* it failed.
pub fn detect(bytes: &[u8]) -> Option<Format> {
    if bytes.starts_with(b"\x89PNG") {
        Some(Format::Png)
    } else if bytes.starts_with(b"BM") {
        Some(Format::Bmp)
    } else if bytes.len() >= 3
        && bytes[0] == b'P'
        && matches!(bytes[1], b'3' | b'6')
        && matches!(bytes[2], b' ' | b'\t' | b'\n' | b'\r' | b'#')
    {
        Some(Format::Ppm)
    } else {
        None
    }
}

/// Why [`decode`] failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The signature matches no supported format.
    UnknownFormat,
    Png(crate::png::PngError),
    Bmp(crate::bmp::BmpError),
    Ppm(crate::ppm::PpmError),
}

impl From<crate::png::PngError> for DecodeError {
    fn from(e: crate::png::PngError) -> Self {
        DecodeError::Png(e)
    }
}

impl From<crate::bmp::BmpError> for DecodeError {
    fn from(e: crate::bmp::BmpError) -> Self {
        DecodeError::Bmp(e)
    }
}

impl From<crate::ppm::PpmError> for DecodeError {
    fn from(e: crate::ppm::PpmError) -> Self {
        DecodeError::Ppm(e)
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::UnknownFormat => f.write_str("unknown image format"),
            DecodeError::Png(e) => write!(f, "png: {e}"),
            DecodeError::Bmp(e) => write!(f, "bmp: {e}"),
            DecodeError::Ppm(e) => write!(f, "ppm: {e}"),
        }
    }
}

/// Decodes a PNG, BMP or PPM file, picked by its signature.
pub fn decode(bytes: &[u8]) -> Result<Image, DecodeError> {
    match detect(bytes) {
        Some(Format::Png) => Ok(crate::png::decode(bytes)?),
        Some(Format::Bmp) => Ok(crate::bmp::decode(bytes)?),
        Some(Format::Ppm) => Ok(crate::ppm::decode(bytes)?),
        None => Err(DecodeError::UnknownFormat),
    }
}

/// Encodes `img` ("save screenshot"): PNG (RGB8 or RGBA8), BMP (24-bit when
/// opaque, else 32-bit with alpha) or P6 PPM (alpha flattened over black).
pub fn encode(img: &Image, format: Format) -> Result<Vec<u8>, ImageError> {
    match format {
        Format::Png => crate::png::encode(img),
        Format::Bmp if img.is_opaque() => crate::bmp::encode_24(img, 0),
        Format::Bmp => crate::bmp::encode_32(img),
        Format::Ppm => crate::ppm::encode_p6(img, 0),
    }
}

#[cfg(test)]
mod tests;
