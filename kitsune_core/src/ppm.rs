//! Netpbm PPM (`P3` ASCII and `P6` binary) decoder and a `P6` encoder.
//!
//! * Header: magic, width, height, maxval, separated by whitespace, with `#`
//!   comments running to the end of the line. Maxval is 1..=65535; above 255
//!   each sample is two bytes, big endian.
//! * Samples are rescaled to 8 bits as `round(v * 255 / maxval)`.
//! * `P6` has exactly one whitespace byte between maxval and the raster.
//! * The size is validated against [`image::MAX_PIXELS`] and against the bytes
//!   present (a `P3` sample needs at least one digit and one separator) before
//!   the image is allocated.
//!
//! Only colour maps are handled; `P1`/`P2`/`P4`/`P5`/`P7` are
//! [`PpmError::BadMagic`].

use crate::image::{self, Image, ImageError, rgba};
use alloc::vec::Vec;
use core::fmt;

/// Why a PPM was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PpmError {
    /// Not `P3` or `P6`.
    BadMagic,
    /// The data ends inside the header or the raster.
    Truncated,
    /// A missing/oversized header number, zero size, or maxval outside 1..=65535.
    BadHeader,
    /// An ASCII sample above maxval, or not a number.
    BadSample,
    /// Dimensions or allocation refused by [`Image`].
    Image(ImageError),
}

impl From<ImageError> for PpmError {
    fn from(e: ImageError) -> Self {
        PpmError::Image(e)
    }
}

impl fmt::Display for PpmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PpmError::BadMagic => f.write_str("not a P3/P6 ppm"),
            PpmError::Truncated => f.write_str("ppm data is truncated"),
            PpmError::BadHeader => f.write_str("invalid ppm header"),
            PpmError::BadSample => f.write_str("invalid ppm sample"),
            PpmError::Image(e) => write!(f, "ppm image: {e}"),
        }
    }
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

/// Cursor over the header / ASCII raster.
struct Reader<'a> {
    d: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    /// Skips whitespace and `#` comments.
    fn skip(&mut self) {
        while let Some(&b) = self.d.get(self.pos) {
            if is_ws(b) {
                self.pos += 1;
            } else if b == b'#' {
                while let Some(&c) = self.d.get(self.pos) {
                    self.pos += 1;
                    if c == b'\n' || c == b'\r' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    /// Reads an unsigned decimal of at most 10 digits.
    fn number(&mut self) -> Result<u32, PpmError> {
        self.skip();
        let start = self.pos;
        let mut v: u64 = 0;
        while let Some(&b) = self.d.get(self.pos) {
            if !b.is_ascii_digit() {
                break;
            }
            v = v * 10 + (b - b'0') as u64;
            self.pos += 1;
            if self.pos - start > 10 {
                return Err(PpmError::BadHeader);
            }
        }
        if self.pos == start {
            return Err(if self.pos >= self.d.len() {
                PpmError::Truncated
            } else {
                PpmError::BadHeader
            });
        }
        // A number must be followed by whitespace, a comment or the end.
        match self.d.get(self.pos) {
            Some(&b) if !is_ws(b) && b != b'#' => return Err(PpmError::BadHeader),
            _ => {}
        }
        u32::try_from(v).map_err(|_| PpmError::BadHeader)
    }
}

/// Decodes a `P3` or `P6` PPM.
pub fn decode(data: &[u8]) -> Result<Image, PpmError> {
    if data.len() < 2 || data[0] != b'P' {
        return Err(PpmError::BadMagic);
    }
    let binary = match data[1] {
        b'6' => true,
        b'3' => false,
        _ => return Err(PpmError::BadMagic),
    };
    let mut r = Reader { d: data, pos: 2 };
    let w = r.number()? as usize;
    let h = r.number()? as usize;
    let maxval = r.number()?;
    if w == 0 || h == 0 || maxval == 0 || maxval > 65535 {
        return Err(PpmError::BadHeader);
    }
    let n = image::pixel_count(w, h)?;
    let wide = maxval > 255;
    let mv = maxval as u64;
    let scale = |v: u32| -> u8 {
        if maxval == 255 {
            v as u8
        } else {
            ((v as u64 * 255 + mv / 2) / mv) as u8
        }
    };
    let mut px = Vec::new();
    if binary {
        // Exactly one whitespace byte follows maxval.
        let start = r.pos.checked_add(1).ok_or(PpmError::Truncated)?;
        match data.get(r.pos) {
            Some(&b) if is_ws(b) => {}
            Some(_) => return Err(PpmError::BadHeader),
            None => return Err(PpmError::Truncated),
        }
        let bps = if wide { 2 } else { 1 };
        let need = n * 3 * bps;
        let raster = data
            .get(start..)
            .and_then(|s| s.get(..need))
            .ok_or(PpmError::Truncated)?;
        px.try_reserve_exact(n)
            .map_err(|_| ImageError::OutOfMemory)?;
        if wide {
            for c in raster.as_chunks::<6>().0 {
                let s = |i: usize| scale(u16::from_be_bytes([c[i], c[i + 1]]) as u32);
                px.push(rgba(s(0), s(2), s(4), 255));
            }
        } else {
            for c in raster.as_chunks::<3>().0 {
                px.push(rgba(
                    scale(c[0] as u32),
                    scale(c[1] as u32),
                    scale(c[2] as u32),
                    255,
                ));
            }
        }
    } else {
        // Each sample is at least one digit plus a separator (the last one
        // may lack it): refuse before allocating if the data cannot hold n.
        let remaining = data.len() - r.pos;
        if remaining < n * 6 - 1 {
            return Err(PpmError::Truncated);
        }
        px.try_reserve_exact(n)
            .map_err(|_| ImageError::OutOfMemory)?;
        for _ in 0..n {
            let mut c = [0u8; 3];
            for slot in &mut c {
                let v = r.number().map_err(|e| match e {
                    PpmError::BadHeader => PpmError::BadSample,
                    other => other,
                })?;
                if v > maxval {
                    return Err(PpmError::BadSample);
                }
                *slot = scale(v);
            }
            px.push(rgba(c[0], c[1], c[2], 255));
        }
    }
    Ok(Image::from_pixels(w, h, px)?)
}

/// Encodes a binary `P6` PPM (maxval 255). Transparency is composited over
/// `bg` (its alpha is ignored).
pub fn encode_p6(img: &Image, bg: u32) -> Result<Vec<u8>, ImageError> {
    let head = alloc::format!("P6\n{} {}\n255\n", img.width(), img.height());
    let mut out = Vec::new();
    out.try_reserve_exact(head.len() + img.pixels().len() * 3)
        .map_err(|_| ImageError::OutOfMemory)?;
    out.extend_from_slice(head.as_bytes());
    let bg = bg | 0xFF00_0000;
    for &p in img.pixels() {
        let [r, g, b, _] = image::channels(image::over(p, bg));
        out.extend_from_slice(&[r, g, b]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod vectors;
