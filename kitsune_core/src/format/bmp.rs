//! BMP decoder and encoder.
//!
//! # Decoding
//!
//! * Headers: `BITMAPCOREHEADER` (12), `BITMAPINFOHEADER` (40), the 52/56-byte
//!   intermediates, `BITMAPV4HEADER` (108) and `BITMAPV5HEADER` (124).
//! * Depths: 1, 4 and 8 bpp with a palette, 16 bpp (default 5-5-5 or
//!   `BI_BITFIELDS` masks), 24 bpp and 32 bpp (BGRA, or masks).
//! * Compression: `BI_RGB`, `BI_RLE8`, `BI_RLE4`, `BI_BITFIELDS`,
//!   `BI_ALPHABITFIELDS`. JPEG/PNG-in-BMP and Huffman 1D are
//!   [`BmpError::UnsupportedCompression`].
//! * Rows are bottom-up unless the height is negative (top-down).
//!
//! Behaviours worth knowing:
//!
//! * Palette entries ignore their fourth byte; an index past the palette is
//!   opaque black (what GDI draws).
//! * 16/32 bpp channels use their bit masks, widened to 8 bits by bit
//!   replication. A 32 bpp image whose alpha bytes are *all* zero is treated
//!   as opaque (most writers leave that byte unused).
//! * RLE pixels skipped by an end-of-line/delta code stay fully transparent.
//!   Writes outside the image are clipped. A stream that ends without an
//!   end-of-bitmap code is accepted; one that ends inside a command is
//!   [`BmpError::Truncated`].
//!
//! The pixel count is validated against [`image::MAX_PIXELS`] and (for
//! uncompressed data) against the bytes actually present *before* the image is
//! allocated.
//!
//! # Encoding
//!
//! [`encode_24`] writes a classic 24-bit `BITMAPINFOHEADER` file (alpha is
//! flattened over a background colour); [`encode_32`] writes a `BITMAPV4HEADER`
//! 32-bit `BI_BITFIELDS` file that keeps alpha. Both are bottom-up.

use crate::format::image::{self, Image, ImageError, rgba};
use alloc::vec::Vec;
use core::fmt;

/// Why a BMP was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BmpError {
    /// The data ends before a required field or the pixel array.
    Truncated,
    /// Does not start with `BM`.
    BadSignature,
    /// A DIB header size that is not 12, 40, 52, 56, 108 or 124.
    UnsupportedHeader,
    /// Inconsistent header (planes, offsets, palette size, ...).
    BadHeader,
    /// Zero or negative width, zero height.
    BadDimensions,
    /// A bit depth this decoder does not handle for the compression type.
    UnsupportedBitDepth,
    /// A compression method this decoder does not handle.
    UnsupportedCompression,
    /// Colour masks that are empty, non-contiguous or wider than the pixel.
    BadBitfields,
    /// Dimensions or allocation refused by [`Image`].
    Image(ImageError),
}

impl From<ImageError> for BmpError {
    fn from(e: ImageError) -> Self {
        BmpError::Image(e)
    }
}

impl fmt::Display for BmpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BmpError::Truncated => f.write_str("bmp data is truncated"),
            BmpError::BadSignature => f.write_str("not a bmp (missing BM)"),
            BmpError::UnsupportedHeader => f.write_str("unsupported bmp header size"),
            BmpError::BadHeader => f.write_str("inconsistent bmp header"),
            BmpError::BadDimensions => f.write_str("invalid bmp dimensions"),
            BmpError::UnsupportedBitDepth => f.write_str("unsupported bmp bit depth"),
            BmpError::UnsupportedCompression => f.write_str("unsupported bmp compression"),
            BmpError::BadBitfields => f.write_str("invalid bmp colour masks"),
            BmpError::Image(e) => write!(f, "bmp image: {e}"),
        }
    }
}

const BI_RGB: u32 = 0;
const BI_RLE8: u32 = 1;
const BI_RLE4: u32 = 2;
const BI_BITFIELDS: u32 = 3;
const BI_ALPHABITFIELDS: u32 = 6;

fn le16(d: &[u8], o: usize) -> Result<u16, BmpError> {
    let b = d.get(o..o + 2).ok_or(BmpError::Truncated)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn le32(d: &[u8], o: usize) -> Result<u32, BmpError> {
    let b = d.get(o..o + 4).ok_or(BmpError::Truncated)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// One colour channel of a 16/32-bit pixel.
#[derive(Clone, Copy)]
struct Chan {
    mask: u32,
    shift: u32,
    bits: u32,
}

impl Chan {
    const NONE: Chan = Chan {
        mask: 0,
        shift: 0,
        bits: 0,
    };

    fn new(mask: u32, bpp: u16) -> Result<Chan, BmpError> {
        if mask == 0 {
            return Ok(Chan::NONE);
        }
        if bpp == 16 && mask > 0xFFFF {
            return Err(BmpError::BadBitfields);
        }
        let shift = mask.trailing_zeros();
        let run = mask >> shift;
        if run & run.wrapping_add(1) != 0 {
            return Err(BmpError::BadBitfields); // not one contiguous run of ones
        }
        Ok(Chan {
            mask,
            shift,
            bits: run.count_ones(),
        })
    }

    /// The channel of `px` widened to 8 bits.
    #[inline]
    fn get(&self, px: u32) -> u8 {
        if self.bits == 0 {
            return 0;
        }
        let v = (px & self.mask) >> self.shift;
        if self.bits >= 8 {
            (v >> (self.bits - 8)) as u8
        } else {
            let mut r = v << (8 - self.bits);
            let mut s = self.bits;
            while s < 8 {
                r |= r >> s;
                s *= 2;
            }
            r as u8
        }
    }
}

struct Header {
    width: usize,
    height: usize,
    topdown: bool,
    bpp: u16,
    comp: u32,
    /// Start of the pixel array.
    offset: usize,
    /// RGB(A) masks for 16/32 bpp.
    masks: [Chan; 4],
    palette: [u32; 256],
    palette_len: usize,
}

fn parse_header(d: &[u8]) -> Result<Header, BmpError> {
    if d.len() < 2 {
        return Err(BmpError::Truncated);
    }
    if &d[..2] != b"BM" {
        return Err(BmpError::BadSignature);
    }
    let offset = le32(d, 10)? as usize;
    let dib = le32(d, 14)? as usize;
    let (width, height, topdown, bpp, comp, clr_used);
    match dib {
        12 => {
            width = le16(d, 18)? as usize;
            height = le16(d, 20)? as usize;
            topdown = false;
            if le16(d, 22)? != 1 {
                return Err(BmpError::BadHeader);
            }
            bpp = le16(d, 24)?;
            comp = BI_RGB;
            clr_used = 0;
        }
        40 | 52 | 56 | 108 | 124 => {
            let w = le32(d, 18)? as i32;
            let h = le32(d, 22)? as i32;
            if w <= 0 {
                return Err(BmpError::BadDimensions);
            }
            width = w as usize;
            height = h.unsigned_abs() as usize;
            topdown = h < 0;
            if le16(d, 26)? != 1 {
                return Err(BmpError::BadHeader);
            }
            bpp = le16(d, 28)?;
            comp = le32(d, 30)?;
            clr_used = le32(d, 46)? as usize;
        }
        _ => return Err(BmpError::UnsupportedHeader),
    }
    if width == 0 || height == 0 {
        return Err(BmpError::BadDimensions);
    }
    match (comp, bpp) {
        (BI_RGB, 1 | 4 | 8 | 24) => {}
        (BI_RGB, 16 | 32) if dib != 12 => {}
        (BI_RLE8, 8) | (BI_RLE4, 4) if dib != 12 => {}
        (BI_BITFIELDS | BI_ALPHABITFIELDS, 16 | 32) if dib != 12 => {}
        (BI_RGB | BI_RLE8 | BI_RLE4 | BI_BITFIELDS | BI_ALPHABITFIELDS, _) => {
            return Err(BmpError::UnsupportedBitDepth);
        }
        _ => return Err(BmpError::UnsupportedCompression),
    }

    // Colour masks.
    let mut masks = [Chan::NONE; 4];
    let mut after_header = 14 + dib;
    if bpp == 16 || bpp == 32 {
        let raw: [u32; 4] = if comp == BI_BITFIELDS || comp == BI_ALPHABITFIELDS {
            let count = if comp == BI_ALPHABITFIELDS { 4 } else { 3 };
            let (base, has_alpha_in_hdr) = if dib == 40 {
                after_header += 4 * count;
                (14 + 40, false)
            } else {
                (14 + 40, dib >= 56)
            };
            let r = le32(d, base)?;
            let g = le32(d, base + 4)?;
            let b = le32(d, base + 8)?;
            let a = if dib == 40 {
                if count == 4 { le32(d, base + 12)? } else { 0 }
            } else if has_alpha_in_hdr {
                le32(d, base + 12)?
            } else {
                0
            };
            [r, g, b, a]
        } else if bpp == 16 {
            [0x7C00, 0x03E0, 0x001F, 0]
        } else {
            [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000]
        };
        if raw[0] == 0 || raw[1] == 0 || raw[2] == 0 {
            return Err(BmpError::BadBitfields);
        }
        for (slot, &m) in masks.iter_mut().zip(raw.iter()) {
            *slot = Chan::new(m, bpp)?;
        }
    }

    // Palette.
    let mut palette = [0u32; 256];
    let mut palette_len = 0;
    if bpp <= 8 {
        let full = 1usize << bpp;
        let n = if clr_used == 0 { full } else { clr_used };
        if n > full {
            return Err(BmpError::BadHeader);
        }
        let esz = if dib == 12 { 3 } else { 4 };
        let end = after_header + n * esz;
        if end > d.len() {
            return Err(BmpError::Truncated);
        }
        if offset < end {
            return Err(BmpError::BadHeader);
        }
        for (i, slot) in palette.iter_mut().take(n).enumerate() {
            let e = &d[after_header + i * esz..];
            *slot = rgba(e[2], e[1], e[0], 255);
        }
        palette_len = n;
    } else if offset < after_header {
        return Err(BmpError::BadHeader);
    }
    if offset > d.len() {
        return Err(BmpError::Truncated);
    }
    Ok(Header {
        width,
        height,
        topdown,
        bpp,
        comp,
        offset,
        masks,
        palette,
        palette_len,
    })
}

impl Header {
    #[inline]
    fn color(&self, index: usize) -> u32 {
        if index < self.palette_len {
            self.palette[index]
        } else {
            0xFF00_0000
        }
    }
}

/// Decodes a BMP file.
pub fn decode(data: &[u8]) -> Result<Image, BmpError> {
    let h = parse_header(data)?;
    image::pixel_count(h.width, h.height)?;
    let pixels = data.get(h.offset..).unwrap_or(&[]);
    match h.comp {
        BI_RLE8 | BI_RLE4 => {
            let mut img = Image::new(h.width, h.height, 0)?;
            decode_rle(&h, pixels, &mut img)?;
            Ok(img)
        }
        _ => {
            // Make sure the bytes are really there before allocating.
            let stride = (h.width as u64 * h.bpp as u64).div_ceil(32) * 4;
            let need = stride
                .checked_mul(h.height as u64)
                .ok_or(BmpError::Truncated)?;
            if need > pixels.len() as u64 {
                return Err(BmpError::Truncated);
            }
            let mut img = Image::new(h.width, h.height, 0)?;
            decode_raw(&h, pixels, stride as usize, &mut img);
            Ok(img)
        }
    }
}

fn decode_raw(h: &Header, data: &[u8], stride: usize, img: &mut Image) {
    let w = h.width;
    let mut any_alpha = false;
    let has_alpha_chan = h.masks[3].bits != 0;
    for r in 0..h.height {
        let src = &data[r * stride..r * stride + stride];
        let y = if h.topdown { r } else { h.height - 1 - r };
        let out = img.row_mut(y);
        match h.bpp {
            1 => {
                for (x, o) in out.iter_mut().enumerate() {
                    let bit = (src[x >> 3] >> (7 - (x & 7))) & 1;
                    *o = h.color(bit as usize);
                }
            }
            4 => {
                for (x, o) in out.iter_mut().enumerate() {
                    let b = src[x >> 1];
                    let v = if x & 1 == 0 { b >> 4 } else { b & 15 };
                    *o = h.color(v as usize);
                }
            }
            8 => {
                for (o, &v) in out.iter_mut().zip(src.iter()) {
                    *o = h.color(v as usize);
                }
            }
            16 => {
                for (o, c) in out.iter_mut().zip(src.as_chunks::<2>().0) {
                    let px = u16::from_le_bytes(*c) as u32;
                    let a = if has_alpha_chan {
                        h.masks[3].get(px)
                    } else {
                        255
                    };
                    any_alpha |= a != 0;
                    *o = rgba(
                        h.masks[0].get(px),
                        h.masks[1].get(px),
                        h.masks[2].get(px),
                        a,
                    );
                }
            }
            24 => {
                for (o, c) in out.iter_mut().zip(src.as_chunks::<3>().0) {
                    *o = rgba(c[2], c[1], c[0], 255);
                }
            }
            _ => {
                for (o, c) in out.iter_mut().zip(src.as_chunks::<4>().0) {
                    let px = u32::from_le_bytes(*c);
                    let a = if has_alpha_chan {
                        h.masks[3].get(px)
                    } else {
                        255
                    };
                    any_alpha |= a != 0;
                    *o = rgba(
                        h.masks[0].get(px),
                        h.masks[1].get(px),
                        h.masks[2].get(px),
                        a,
                    );
                }
            }
        }
        debug_assert_eq!(out.len(), w);
    }
    // An alpha channel that is zero everywhere was just unused padding.
    if has_alpha_chan && !any_alpha {
        for p in img.pixels_mut() {
            *p |= 0xFF00_0000;
        }
    }
}

fn decode_rle(h: &Header, data: &[u8], img: &mut Image) -> Result<(), BmpError> {
    let four = h.comp == BI_RLE4;
    let (w, ht) = (h.width as i64, h.height as i64);
    let (mut x, mut y) = (0i64, 0i64); // y counts rows from the first stored row
    let put = |x: i64, y: i64, idx: usize, img: &mut Image| {
        if x >= 0 && x < w && y >= 0 && y < ht {
            let row = if h.topdown { y } else { ht - 1 - y };
            img.set(x as usize, row as usize, h.color(idx));
        }
    };
    let mut i = 0usize;
    while i < data.len() {
        let count = data[i] as usize;
        let val = *data.get(i + 1).ok_or(BmpError::Truncated)?;
        i += 2;
        if count > 0 {
            for k in 0..count {
                let idx = if four {
                    if k & 1 == 0 { val >> 4 } else { val & 15 }
                } else {
                    val
                };
                put(x + k as i64, y, idx as usize, img);
            }
            x += count as i64;
            continue;
        }
        match val {
            0 => {
                x = 0;
                y += 1;
            }
            1 => return Ok(()),
            2 => {
                let dx = *data.get(i).ok_or(BmpError::Truncated)? as i64;
                let dy = *data.get(i + 1).ok_or(BmpError::Truncated)? as i64;
                i += 2;
                x += dx;
                y += dy;
            }
            n => {
                let n = n as usize;
                let bytes = if four { n.div_ceil(2) } else { n };
                let run = data.get(i..i + bytes).ok_or(BmpError::Truncated)?;
                for k in 0..n {
                    let idx = if four {
                        let b = run[k >> 1];
                        if k & 1 == 0 { b >> 4 } else { b & 15 }
                    } else {
                        run[k]
                    };
                    put(x + k as i64, y, idx as usize, img);
                }
                x += n as i64;
                i += bytes + (bytes & 1); // runs are padded to a 16-bit boundary
            }
        }
        if y >= ht {
            // Everything past the last row would be clipped anyway.
            return Ok(());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

fn put16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn put32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn start_file(total: usize, offset: usize) -> Result<Vec<u8>, ImageError> {
    let mut out = Vec::new();
    out.try_reserve_exact(total)
        .map_err(|_| ImageError::OutOfMemory)?;
    out.extend_from_slice(b"BM");
    put32(&mut out, total as u32);
    put32(&mut out, 0);
    put32(&mut out, offset as u32);
    Ok(out)
}

/// Encodes a 24-bit bottom-up BMP. Transparency is composited over `bg`
/// (its alpha is ignored).
pub fn encode_24(img: &Image, bg: u32) -> Result<Vec<u8>, ImageError> {
    let (w, h) = (img.width(), img.height());
    let stride = (w * 3).div_ceil(4) * 4;
    let total = 54 + stride * h;
    let mut out = start_file(total, 54)?;
    put32(&mut out, 40);
    put32(&mut out, w as u32);
    put32(&mut out, h as u32);
    put16(&mut out, 1);
    put16(&mut out, 24);
    put32(&mut out, BI_RGB);
    put32(&mut out, (stride * h) as u32);
    put32(&mut out, 2835);
    put32(&mut out, 2835);
    put32(&mut out, 0);
    put32(&mut out, 0);
    let bg = bg | 0xFF00_0000;
    for y in (0..h).rev() {
        for &p in img.row(y) {
            let [r, g, b, _] = image::channels(image::over(p, bg));
            out.extend_from_slice(&[b, g, r]);
        }
        out.resize(out.len() + (stride - w * 3), 0);
    }
    Ok(out)
}

/// Encodes a 32-bit bottom-up BMP with a `BITMAPV4HEADER` and explicit
/// BGRA masks, keeping the alpha channel.
pub fn encode_32(img: &Image) -> Result<Vec<u8>, ImageError> {
    let (w, h) = (img.width(), img.height());
    let total = 14 + 108 + w * h * 4;
    let mut out = start_file(total, 14 + 108)?;
    put32(&mut out, 108);
    put32(&mut out, w as u32);
    put32(&mut out, h as u32);
    put16(&mut out, 1);
    put16(&mut out, 32);
    put32(&mut out, BI_BITFIELDS);
    put32(&mut out, (w * h * 4) as u32);
    put32(&mut out, 2835);
    put32(&mut out, 2835);
    put32(&mut out, 0);
    put32(&mut out, 0);
    put32(&mut out, 0x00FF_0000);
    put32(&mut out, 0x0000_FF00);
    put32(&mut out, 0x0000_00FF);
    put32(&mut out, 0xFF00_0000);
    put32(&mut out, 0x7352_4742); // LCS_sRGB
    out.resize(out.len() + 36 + 12, 0); // endpoints, gamma
    for y in (0..h).rev() {
        for &p in img.row(y) {
            let [r, g, b, a] = image::channels(p);
            out.extend_from_slice(&[b, g, r, a]);
        }
    }
    debug_assert_eq!(out.len(), total);
    Ok(out)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod vectors;
