//! Minimal framebuffer drawing primitives. No allocation, writes pixels directly.

use bootloader_api::info::{FrameBufferInfo, PixelFormat};

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

    /// Blend a rectangular region toward another buffer of identical layout.
    /// `alpha` in `0..=256`: 0 = fully `src` (e.g. the wallpaper), 256 = keep
    /// what is already drawn. Used to fade windows in/out against the desktop.
    pub fn blend_region(&mut self, src: &[u8], x0: usize, y0: usize, w: usize, h: usize, alpha: u16) {
        let bpp = self.info.bytes_per_pixel;
        let stride = self.info.stride;
        let a = alpha.min(256);
        let ia = 256 - a;
        let x_end = (x0 + w).min(self.info.width);
        let y_end = (y0 + h).min(self.info.height);
        for y in y0..y_end {
            for x in x0..x_end {
                let o = (y * stride + x) * bpp;
                for k in 0..bpp {
                    let cur = self.buf[o + k] as u16;
                    let bg = src[o + k] as u16;
                    self.buf[o + k] = ((bg * ia + cur * a) / 256) as u8;
                }
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

        // Fast path: 3-byte RGB/BGR ordering written directly per row, skipping
        // the per-pixel bounds check + format match that `put` performs.
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
        return;
        #[allow(unreachable_code)]
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
        for y in 0..h {
            for x in 0..w {
                if self.inside_round(x, y, w, h, r) {
                    self.put(x0 + x, y0 + y, c);
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
