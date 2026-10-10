//! GIF decoder (the first frame).
//!
//! * `GIF87a` and `GIF89a`; global and local palettes of 2 to 256 colours; interlaced and
//!   sequential frames; the transparent colour of a graphic control extension.
//! * Only the **first image** of the file is decoded, composed onto the logical screen (which is
//!   the size of the result): anything outside the frame, and the transparent colour, comes out
//!   fully transparent. Later frames, delays, disposal methods and the loop count of an animated
//!   GIF are ignored, so an animation shows its first picture.
//! * The LZW stream is decoded with a 4096-entry table and a bounded output: a malicious stream
//!   cannot produce more than `frame width * frame height` indices. A stream that ends early gives
//!   an image with the missing pixels transparent (what browsers draw for a cut-off download); a
//!   file with no pixel data at all is [`GifError::Truncated`].
//! * The size is checked against [`image::MAX_PIXELS`] before anything is allocated.
//!
//! The decoder is pure and `forbid(unsafe)`; the fuzz target `image_decode` drives it through
//! [`image::decode`].

use crate::format::image::{self, Image, ImageError, rgba};
use alloc::vec::Vec;
use core::fmt;

/// Why a GIF was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GifError {
    /// Not `GIF87a`/`GIF89a`.
    BadSignature,
    /// The file ends before the header, a palette or the first image descriptor.
    Truncated,
    /// A zero-sized logical screen or frame, or more pixels than [`image::MAX_PIXELS`].
    BadSize,
    /// An unknown block introducer.
    BadBlock(u8),
    /// The first image has no palette (neither local nor global).
    NoPalette,
    /// The LZW minimum code size is outside 2..=8.
    BadLzw,
    /// The file has no image at all.
    NoImage,
    /// The allocator or the pixel limit refused the image.
    Image(ImageError),
}

impl fmt::Display for GifError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GifError::BadSignature => f.write_str("not a GIF file"),
            GifError::Truncated => f.write_str("the GIF is cut short"),
            GifError::BadSize => f.write_str("the GIF has an invalid or too large size"),
            GifError::BadBlock(b) => write!(f, "unknown GIF block {b:#04x}"),
            GifError::NoPalette => f.write_str("the GIF has no colour table"),
            GifError::BadLzw => f.write_str("invalid LZW code size"),
            GifError::NoImage => f.write_str("the GIF holds no image"),
            GifError::Image(e) => write!(f, "{e}"),
        }
    }
}

impl From<ImageError> for GifError {
    fn from(e: ImageError) -> Self {
        GifError::Image(e)
    }
}

/// Does `bytes` start like a GIF?
pub fn is_gif(bytes: &[u8]) -> bool {
    bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")
}

/// Width and height of the logical screen (= the decoded image), read from the header without
/// decoding anything. `None` for a file too short to have one.
pub fn peek_dims(bytes: &[u8]) -> Option<(usize, usize)> {
    if !is_gif(bytes) {
        return None;
    }
    let w = u16::from_le_bytes(bytes.get(6..8)?.try_into().ok()?);
    let h = u16::from_le_bytes(bytes.get(8..10)?.try_into().ok()?);
    Some((usize::from(w), usize::from(h)))
}

struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], GifError> {
        let s = self
            .b
            .get(self.p..self.p.checked_add(n).ok_or(GifError::Truncated)?)
            .ok_or(GifError::Truncated)?;
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, GifError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, GifError> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    /// Concatenate the data sub-blocks up to (and including) the zero terminator. A file that
    /// ends inside the chain gives what is there so far.
    fn sub_blocks(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        while let Ok(n) = self.u8() {
            if n == 0 {
                break;
            }
            let avail = self.b.len().saturating_sub(self.p).min(n as usize);
            out.extend_from_slice(&self.b[self.p..self.p + avail]);
            self.p += avail;
            if avail < n as usize {
                break;
            }
        }
        out
    }
    fn skip_sub_blocks(&mut self) {
        while let Ok(n) = self.u8() {
            if n == 0 {
                break;
            }
            self.p = (self.p + n as usize).min(self.b.len());
        }
    }
}

fn palette(c: &mut Cur<'_>, flags: u8) -> Result<Vec<u32>, GifError> {
    let n = 1usize << ((flags & 7) + 1);
    let raw = c.take(n * 3)?;
    Ok((0..n)
        .map(|i| rgba(raw[3 * i], raw[3 * i + 1], raw[3 * i + 2], 255))
        .collect())
}

/// Decode the first image of a GIF.
pub fn decode(bytes: &[u8]) -> Result<Image, GifError> {
    if !is_gif(bytes) {
        return Err(GifError::BadSignature);
    }
    let mut c = Cur { b: bytes, p: 6 };
    let sw = c.u16()? as usize;
    let sh = c.u16()? as usize;
    let flags = c.u8()?;
    let _bg = c.u8()?;
    let _aspect = c.u8()?;
    if sw == 0 || sh == 0 {
        return Err(GifError::BadSize);
    }
    image::pixel_count(sw, sh).map_err(|_| GifError::BadSize)?;
    let global = if flags & 0x80 != 0 {
        Some(palette(&mut c, flags)?)
    } else {
        None
    };

    let mut transparent: Option<u8> = None;
    loop {
        match c.u8().map_err(|_| GifError::NoImage)? {
            0x21 => {
                let label = c.u8()?;
                if label == 0xF9 {
                    let blk = c.sub_blocks();
                    if blk.len() >= 4 && blk[0] & 1 != 0 {
                        transparent = Some(blk[3]);
                    } else {
                        transparent = None;
                    }
                } else {
                    c.skip_sub_blocks();
                }
            }
            0x2C => break,
            0x3B => return Err(GifError::NoImage),
            0x00 => {} // stray padding between blocks
            b => return Err(GifError::BadBlock(b)),
        }
    }

    let left = c.u16()? as usize;
    let top = c.u16()? as usize;
    let fw = c.u16()? as usize;
    let fh = c.u16()? as usize;
    let iflags = c.u8()?;
    if fw == 0 || fh == 0 {
        return Err(GifError::BadSize);
    }
    // The frame is decoded in full, so it is bounded like the screen.
    image::pixel_count(fw, fh).map_err(|_| GifError::BadSize)?;
    let pal = if iflags & 0x80 != 0 {
        palette(&mut c, iflags)?
    } else {
        global.ok_or(GifError::NoPalette)?
    };
    let min = c.u8()?;
    if !(2..=8).contains(&min) {
        return Err(GifError::BadLzw);
    }
    let data = c.sub_blocks();
    if data.is_empty() {
        return Err(GifError::Truncated);
    }
    let idx = lzw(&data, min, fw * fh);

    let mut img = Image::new(sw, sh, 0)?;
    let interlaced = iflags & 0x40 != 0;
    let rows: Vec<usize> = if interlaced {
        let mut v = Vec::with_capacity(fh);
        for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
            v.extend((start..fh).step_by(step));
        }
        v
    } else {
        (0..fh).collect()
    };
    let px = img.pixels_mut();
    for (n, &y) in rows.iter().enumerate() {
        let dy = top + y;
        if dy >= sh {
            continue;
        }
        let src = idx.get(n * fw..(n + 1) * fw).unwrap_or(&[]);
        for (x, &i) in src.iter().enumerate() {
            let dx = left + x;
            if dx >= sw || Some(i) == transparent {
                continue;
            }
            // An index past the palette is opaque black.
            px[dy * sw + dx] = pal.get(i as usize).copied().unwrap_or(rgba(0, 0, 0, 255));
        }
    }
    Ok(img)
}

/// Variable-width LZW (GIF flavour): LSB-first codes, clear and end codes, 12-bit table.
/// Returns at most `limit` indices.
fn lzw(data: &[u8], min: u8, limit: usize) -> Vec<u8> {
    const MAX_CODES: usize = 4096;
    let clear = 1usize << min;
    let eoi = clear + 1;
    let mut prefix = [0u16; MAX_CODES];
    let mut suffix = [0u8; MAX_CODES];
    for (i, s) in suffix.iter_mut().enumerate().take(clear) {
        *s = i as u8;
    }
    let mut out: Vec<u8> = Vec::with_capacity(limit.min(1 << 20));
    let mut stack: Vec<u8> = Vec::new();
    let mut size = min as usize + 1;
    let mut next = eoi + 1;
    let mut prev: Option<usize> = None;
    let (mut acc, mut nbits, mut pos) = (0u32, 0usize, 0usize);
    while out.len() < limit {
        while nbits < size {
            let Some(&b) = data.get(pos) else {
                return out;
            };
            acc |= (b as u32) << nbits;
            nbits += 8;
            pos += 1;
        }
        let code = (acc & ((1 << size) - 1)) as usize;
        acc >>= size;
        nbits -= size;
        if code == clear {
            size = min as usize + 1;
            next = eoi + 1;
            prev = None;
            continue;
        }
        if code == eoi {
            break;
        }
        let first_byte;
        match prev {
            None => {
                // Right after a clear only a literal is valid.
                if code >= clear {
                    break;
                }
                out.push(code as u8);
                prev = Some(code);
                continue;
            }
            Some(p) => {
                // Expand `code` (or, for the one not-yet-defined code, `prev + first(prev)`).
                stack.clear();
                let mut cur;
                if code < next {
                    cur = code;
                } else if code == next && next < MAX_CODES {
                    cur = p;
                } else {
                    break; // a code from the future: corrupt stream
                }
                while cur >= clear {
                    stack.push(suffix[cur]);
                    cur = prefix[cur] as usize;
                    if stack.len() > MAX_CODES {
                        return out;
                    }
                }
                stack.push(suffix[cur]);
                first_byte = suffix[cur];
                if code == next {
                    // KwKwK case: the string is prev + its own first byte.
                    stack.insert(0, first_byte);
                }
                for &b in stack.iter().rev() {
                    if out.len() >= limit {
                        break;
                    }
                    out.push(b);
                }
                if next < MAX_CODES {
                    prefix[next] = p as u16;
                    suffix[next] = first_byte;
                    next += 1;
                    if next == (1 << size) && size < 12 {
                        size += 1;
                    }
                }
                prev = Some(code);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
