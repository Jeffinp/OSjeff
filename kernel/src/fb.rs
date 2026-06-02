//! Minimal framebuffer drawing primitives. No allocation, writes pixels directly.

use bootloader_api::info::{FrameBufferInfo, PixelFormat};

/// Integer square root (floor). Used to inset rounded-rectangle corner rows.
fn isqrt(n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

#[derive(Clone, Copy)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Linear blend between `self` and `other`. `t` in 0..=255 (0 = self).
    pub fn lerp(self, other: Color, t: u16) -> Color {
        let mix = |a: u8, b: u8| -> u8 {
            let a = a as u16;
            let b = b as u16;
            ((a * (255 - t) + b * t) / 255) as u8
        };
        Color::rgb(
            mix(self.r, other.r),
            mix(self.g, other.g),
            mix(self.b, other.b),
        )
    }
}

pub struct Canvas<'a> {
    buf: &'a mut [u8],
    info: FrameBufferInfo,
}

impl<'a> Canvas<'a> {
    pub fn new(buf: &'a mut [u8], info: FrameBufferInfo) -> Self {
        Self { buf, info }
    }

    #[inline]
    pub fn width(&self) -> usize {
        self.info.width
    }

    #[inline]
    pub fn height(&self) -> usize {
        self.info.height
    }

    #[inline]
    pub fn bpp(&self) -> usize {
        self.info.bytes_per_pixel
    }

    /// Pack a color into a native-endian 32-bit pixel for the 4-byte-per-pixel
    /// formats. `None` for layouts we can't pack (grayscale / unknown), which
    /// fall back to the byte path. The padding byte is left 0.
    #[inline]
    fn pack32(&self, c: Color) -> Option<u32> {
        let (b0, b1, b2) = match self.info.pixel_format {
            PixelFormat::Rgb => (c.r, c.g, c.b),
            PixelFormat::Bgr => (c.b, c.g, c.r),
            _ => return None,
        };
        Some(u32::from_le_bytes([b0, b1, b2, 0]))
    }

    #[inline]
    pub fn put(&mut self, x: usize, y: usize, c: Color) {
        if x >= self.info.width || y >= self.info.height {
            return;
        }
        let bpp = self.info.bytes_per_pixel;
        let offset = (y * self.info.stride + x) * bpp;
        let px = &mut self.buf[offset..offset + bpp];
        match self.info.pixel_format {
            PixelFormat::Rgb => {
                px[0] = c.r;
                px[1] = c.g;
                px[2] = c.b;
            }
            PixelFormat::Bgr => {
                px[0] = c.b;
                px[1] = c.g;
                px[2] = c.r;
            }
            PixelFormat::U8 => {
                // Grayscale: luminance approximation.
                px[0] = ((c.r as u16 * 54 + c.g as u16 * 183 + c.b as u16 * 19) >> 8) as u8;
            }
            _ => {
                // Unknown layout: best-effort RGB.
                if bpp >= 3 {
                    px[0] = c.r;
                    px[1] = c.g;
                    px[2] = c.b;
                }
            }
        }
    }

    /// Alpha-blit a tightly-packed RGBA image (`iw*ih*4` bytes) at `(x0, y0)`.
    pub fn draw_rgba(&mut self, data: &[u8], iw: usize, ih: usize, x0: usize, y0: usize) {
        for y in 0..ih {
            for x in 0..iw {
                let o = (y * iw + x) * 4;
                let a = data[o + 3] as u16;
                if a == 0 {
                    continue;
                }
                let col = Color::rgb(data[o], data[o + 1], data[o + 2]);
                // Scale 0..255 alpha to the 0..256 range blend_pixel expects.
                self.blend_pixel(x0 + x, y0 + y, col, a + (a >> 7));
            }
        }
    }

    pub fn fill_rect(&mut self, x0: usize, y0: usize, w: usize, h: usize, c: Color) {
        if x0 >= self.info.width || y0 >= self.info.height {
            return;
        }
        let bpp = self.info.bytes_per_pixel;
        let stride = self.info.stride;
        let x_end = (x0 + w).min(self.info.width);
        let y_end = (y0 + h).min(self.info.height);
        if x_end <= x0 || y_end <= y0 {
            return;
        }
        let count = x_end - x0;

        // Fastest path: 4-byte pixels written 32 bits at a time. One store per
        // pixel instead of three, which the backend lowers to `rep stosd` / SSE.
        // The render buffers are 64-byte aligned and rows start on a 4-byte
        // boundary, so `align_to_mut::<u32>` yields no scalar prefix/suffix.
        if bpp == 4 {
            if let Some(packed) = self.pack32(c) {
                for y in y0..y_end {
                    let o = (y * stride + x0) * 4;
                    let row = &mut self.buf[o..o + count * 4];
                    let (pre, mid, suf) = unsafe { row.align_to_mut::<u32>() };
                    if pre.is_empty() && suf.is_empty() {
                        mid.fill(packed);
                    } else {
                        let b = packed.to_ne_bytes();
                        for (i, px) in row.iter_mut().enumerate() {
                            *px = b[i & 3];
                        }
                    }
                }
                return;
            }
        }

        // 3-byte RGB/BGR ordering written directly per row, skipping the
        // per-pixel bounds check + format match that `put` performs.
        let order = match self.info.pixel_format {
            PixelFormat::Rgb => Some((c.r, c.g, c.b)),
            PixelFormat::Bgr => Some((c.b, c.g, c.r)),
            _ => None,
        };
        let Some((p0, p1, p2)) = order else {
            for y in y0..y_end {
                for x in x0..x_end {
                    self.put(x, y, c);
                }
            }
            return;
        };
        for y in y0..y_end {
            let mut o = (y * stride + x0) * bpp;
            for _ in x0..x_end {
                self.buf[o] = p0;
                self.buf[o + 1] = p1;
                self.buf[o + 2] = p2;
                o += bpp;
            }
        }
    }

    /// Blend a solid color into one pixel. `alpha` in `0..=256`
    /// (0 = unchanged, 256 = fully `c`). Format-aware.
    #[inline]
    pub fn blend_pixel(&mut self, x: usize, y: usize, c: Color, alpha: u16) {
        if x >= self.info.width || y >= self.info.height {
            return;
        }
        let a = alpha.min(256);
        let ia = 256 - a;
        let bpp = self.info.bytes_per_pixel;
        let o = (y * self.info.stride + x) * bpp;
        let (c0, c1, c2) = match self.info.pixel_format {
            PixelFormat::Rgb => (c.r, c.g, c.b),
            PixelFormat::Bgr => (c.b, c.g, c.r),
            _ => return,
        };
        let mix = |dst: u8, src: u8| ((dst as u16 * ia + src as u16 * a) / 256) as u8;
        self.buf[o] = mix(self.buf[o], c0);
        self.buf[o + 1] = mix(self.buf[o + 1], c1);
        self.buf[o + 2] = mix(self.buf[o + 2], c2);
    }

    /// Rounded rectangle blended over existing pixels at `alpha` (0..=256).
    /// Used for soft drop shadows and translucent surfaces.
    #[allow(clippy::too_many_arguments)]
    pub fn fill_round_rect_alpha(
        &mut self,
        x0: usize,
        y0: usize,
        w: usize,
        h: usize,
        r: usize,
        c: Color,
        alpha: u16,
    ) {
        let r = r.min(w / 2).min(h / 2);
        for y in 0..h {
            for x in 0..w {
                if self.inside_round(x, y, w, h, r) {
                    self.blend_pixel(x0 + x, y0 + y, c, alpha);
                }
            }
        }
    }

    /// Rounded rectangle (filled). `r` = corner radius in pixels.
    pub fn fill_round_rect(
        &mut self,
        x0: usize,
        y0: usize,
        w: usize,
        h: usize,
        r: usize,
        c: Color,
    ) {
        let r = r.min(w / 2).min(h / 2);
        // Per-row spans: corner rows inset by the circle, middle rows full
        // width. Each span is one fast `fill_rect` instead of per-pixel `put`.
        for y in 0..h {
            let inset = if r == 0 {
                0
            } else if y < r {
                let dy = r - y; // 1..=r
                r - isqrt(r * r - dy * dy)
            } else if y >= h - r {
                let dy = y - (h - 1 - r); // 0..=r
                r - isqrt(r.saturating_mul(r).saturating_sub(dy * dy))
            } else {
                0
            };
            if w > 2 * inset {
                self.fill_rect(x0 + inset, y0 + y, w - 2 * inset, 1, c);
            }
        }
    }

    /// Copy a `w x h` region of the canvas into a tightly-packed buffer
    /// (`w*bpp` stride). Used to snapshot what is behind a window before it is
    /// composited, so a fade blends toward the real backdrop, not the wallpaper.
    pub fn snapshot_region(&self, dst: &mut [u8], x0: usize, y0: usize, w: usize, h: usize) {
        let bpp = self.info.bytes_per_pixel;
        let stride = self.info.stride;
        let x_end = (x0 + w).min(self.info.width);
        let y_end = (y0 + h).min(self.info.height);
        for y in y0..y_end {
            for x in x0..x_end {
                let o = (y * stride + x) * bpp;
                let d = ((y - y0) * w + (x - x0)) * bpp;
                dst[d..d + bpp].copy_from_slice(&self.buf[o..o + bpp]);
            }
        }
    }

    /// Blend the canvas toward a packed region buffer (`w*bpp` stride) at
    /// `alpha` (0..=256). `alpha=0` shows `src` (the backdrop), `256` keeps the
    /// canvas (the window). Inverse of [`snapshot_region`].
    pub fn blend_from_local(
        &mut self,
        src: &[u8],
        x0: usize,
        y0: usize,
        w: usize,
        h: usize,
        alpha: u16,
    ) {
        let bpp = self.info.bytes_per_pixel;
        let stride = self.info.stride;
        let a = alpha.min(256);
        let ia = 256 - a;
        let x_end = (x0 + w).min(self.info.width);
        let y_end = (y0 + h).min(self.info.height);
        for y in y0..y_end {
            for x in x0..x_end {
                let o = (y * stride + x) * bpp;
                let s = ((y - y0) * w + (x - x0)) * bpp;
                for k in 0..bpp {
                    self.buf[o + k] =
                        ((src[s + k] as u16 * ia + self.buf[o + k] as u16 * a) / 256) as u8;
                }
            }
        }
    }

    #[inline]
    fn inside_round(&self, x: usize, y: usize, w: usize, h: usize, r: usize) -> bool {
        if r == 0 {
            return true;
        }
        // Corner centers.
        let corners: [(usize, usize); 4] = [
            (r, r),
            (w - 1 - r, r),
            (r, h - 1 - r),
            (w - 1 - r, h - 1 - r),
        ];
        let in_left = x < r;
        let in_right = x >= w - r;
        let in_top = y < r;
        let in_bottom = y >= h - r;

        let (cx, cy) = if in_left && in_top {
            corners[0]
        } else if in_right && in_top {
            corners[1]
        } else if in_left && in_bottom {
            corners[2]
        } else if in_right && in_bottom {
            corners[3]
        } else {
            return true; // not in a corner zone
        };
        let dx = x as isize - cx as isize;
        let dy = y as isize - cy as isize;
        (dx * dx + dy * dy) <= (r * r) as isize
    }
}
