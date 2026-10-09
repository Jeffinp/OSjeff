//! PNG decoder and encoder (ISO/IEC 15948), built on [`crate::inflate`] and
//! [`crate::deflate`].
//!
//! # Decoding
//!
//! Every colour type (grey, RGB, palette, grey+alpha, RGBA) at every legal
//! bit depth (1, 2, 4, 8, 16), filters 0-4, Adam7 interlacing, `PLTE`, `tRNS`
//! (palette alpha and grey/RGB colour keys) and unknown ancillary chunks
//! (skipped; `gAMA`, `sRGB`, `iCCP` and friends are ignored, so there is no
//! gamma correction). Output is always [`Image`] RGBA8; 16-bit samples are
//! rounded to 8 bits as `(v + 128) / 257`.
//!
//! Safety properties (all covered by tests and the fuzz target):
//!
//! * Every chunk CRC is verified (including skipped chunks), the signature and
//!   chunk framing are checked, and chunk order is enforced (`IHDR` first,
//!   `PLTE`/`tRNS` before `IDAT`, `IDAT`s consecutive).
//! * The dimensions are checked against [`image::MAX_PIXELS`] and the exact
//!   decompressed size is computed from the header *before* inflating; it is
//!   passed to the inflater as `max_output`, so an `IDAT` that expands past
//!   what the header describes is [`PngError::ImageDataTooLong`] and one that
//!   ends early is [`PngError::ImageDataTooShort`]. A header that claims far
//!   more pixels than the compressed bytes could ever produce (deflate cannot
//!   expand more than ~1032:1) is rejected before the image is allocated.
//! * Scanlines are inflated and unfiltered one at a time (the decompressed
//!   stream is never held whole), writing straight into the output image.
//! * An unknown *critical* chunk is an error (the spec requires it); an
//!   out-of-range palette index is an error; trailing bytes after `IEND` and
//!   after the zlib stream inside `IDAT` are ignored.
//!
//! # Encoding
//!
//! [`encode`] writes RGB8 when the image is opaque and RGBA8 otherwise;
//! [`encode_rgba`] always writes RGBA8. Each scanline picks the filter with
//! the smallest sum of absolute residuals, and the result is compressed with
//! [`deflate::zlib_compress`] (LZ77 + fixed Huffman).

use crate::deflate;
use crate::image::{self, Image, ImageError, rgba, try_vec};
use crate::inflate::{Crc32, InflateError, Inflater};
use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::fmt;

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// PNG colour types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorType {
    Gray,
    Rgb,
    Palette,
    GrayAlpha,
    Rgba,
}

impl ColorType {
    fn channels(self) -> usize {
        match self {
            ColorType::Gray | ColorType::Palette => 1,
            ColorType::GrayAlpha => 2,
            ColorType::Rgb => 3,
            ColorType::Rgba => 4,
        }
    }
}

/// What `IHDR` says about an image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub color_type: ColorType,
    pub interlaced: bool,
}

impl Header {
    fn bits_per_pixel(&self) -> usize {
        self.color_type.channels() * self.bit_depth as usize
    }

    /// Bytes per complete pixel, at least 1 (the filter distance).
    fn filter_bpp(&self) -> usize {
        self.bits_per_pixel().div_ceil(8)
    }
}

/// Why a PNG was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PngError {
    /// Not a PNG signature.
    BadSignature,
    /// The file ends inside a chunk or before `IEND`.
    Truncated,
    /// Malformed chunk framing (huge length, non-letter type).
    BadChunk,
    /// A chunk whose CRC-32 does not match.
    CrcMismatch,
    /// The first chunk is not `IHDR`.
    MissingIhdr,
    /// An invalid `IHDR` (length, depth/colour combination, methods).
    BadIhdr,
    /// Zero or over-31-bit width or height.
    BadDimensions,
    /// `PLTE` / `tRNS` with an invalid size or colour type.
    BadPalette,
    /// A palette image without `PLTE`.
    MissingPalette,
    /// No `IDAT` chunk.
    MissingIdat,
    /// Chunks in an illegal order or repeated.
    ChunkOrder,
    /// An unrecognised critical chunk.
    UnknownCriticalChunk,
    /// The zlib/deflate data is invalid.
    Inflate(InflateError),
    /// The scanline data ends before the image is complete.
    ImageDataTooShort,
    /// The scanline data is longer than the header describes.
    ImageDataTooLong,
    /// A scanline filter byte above 4.
    BadFilter,
    /// A palette index past the end of `PLTE`.
    BadPaletteIndex,
    /// Dimensions or allocation refused by [`Image`].
    Image(ImageError),
}

impl From<ImageError> for PngError {
    fn from(e: ImageError) -> Self {
        PngError::Image(e)
    }
}

impl From<InflateError> for PngError {
    fn from(e: InflateError) -> Self {
        PngError::Inflate(e)
    }
}

impl fmt::Display for PngError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PngError::BadSignature => f.write_str("not a png (bad signature)"),
            PngError::Truncated => f.write_str("png is truncated"),
            PngError::BadChunk => f.write_str("malformed png chunk"),
            PngError::CrcMismatch => f.write_str("png chunk crc mismatch"),
            PngError::MissingIhdr => f.write_str("png has no IHDR first"),
            PngError::BadIhdr => f.write_str("invalid png IHDR"),
            PngError::BadDimensions => f.write_str("invalid png dimensions"),
            PngError::BadPalette => f.write_str("invalid png palette or transparency"),
            PngError::MissingPalette => f.write_str("palette png without PLTE"),
            PngError::MissingIdat => f.write_str("png has no IDAT"),
            PngError::ChunkOrder => f.write_str("png chunks out of order"),
            PngError::UnknownCriticalChunk => f.write_str("unknown critical png chunk"),
            PngError::Inflate(e) => write!(f, "png data: {e}"),
            PngError::ImageDataTooShort => f.write_str("png image data too short"),
            PngError::ImageDataTooLong => f.write_str("png image data too long"),
            PngError::BadFilter => f.write_str("invalid png filter type"),
            PngError::BadPaletteIndex => f.write_str("png palette index out of range"),
            PngError::Image(e) => write!(f, "png image: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Chunks
// ---------------------------------------------------------------------------

struct Chunk<'a> {
    ty: [u8; 4],
    data: &'a [u8],
}

/// Reads and CRC-checks the chunk at `*pos`, advancing past it.
fn next_chunk<'a>(data: &'a [u8], pos: &mut usize) -> Result<Chunk<'a>, PngError> {
    let head = data
        .get(*pos..)
        .and_then(|s| s.get(..8))
        .ok_or(PngError::Truncated)?;
    let len = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
    if len > 0x7FFF_FFFF {
        return Err(PngError::BadChunk);
    }
    let ty = [head[4], head[5], head[6], head[7]];
    if !ty.iter().all(u8::is_ascii_alphabetic) {
        return Err(PngError::BadChunk);
    }
    let start = *pos + 8;
    let end = start.checked_add(len as usize).ok_or(PngError::Truncated)?;
    let body = data.get(start..end).ok_or(PngError::Truncated)?;
    let crc = data.get(end..end + 4).ok_or(PngError::Truncated)?;
    let mut c = Crc32::new();
    c.update(&ty);
    c.update(body);
    if c.finish() != u32::from_be_bytes([crc[0], crc[1], crc[2], crc[3]]) {
        return Err(PngError::CrcMismatch);
    }
    *pos = end + 4;
    Ok(Chunk { ty, data: body })
}

fn parse_ihdr(d: &[u8]) -> Result<Header, PngError> {
    if d.len() != 13 {
        return Err(PngError::BadIhdr);
    }
    let width = u32::from_be_bytes([d[0], d[1], d[2], d[3]]);
    let height = u32::from_be_bytes([d[4], d[5], d[6], d[7]]);
    if width == 0 || height == 0 || width > 0x7FFF_FFFF || height > 0x7FFF_FFFF {
        return Err(PngError::BadDimensions);
    }
    let bit_depth = d[8];
    let color_type = match (d[9], bit_depth) {
        (0, 1 | 2 | 4 | 8 | 16) => ColorType::Gray,
        (2, 8 | 16) => ColorType::Rgb,
        (3, 1 | 2 | 4 | 8) => ColorType::Palette,
        (4, 8 | 16) => ColorType::GrayAlpha,
        (6, 8 | 16) => ColorType::Rgba,
        _ => return Err(PngError::BadIhdr),
    };
    if d[10] != 0 || d[11] != 0 || d[12] > 1 {
        return Err(PngError::BadIhdr);
    }
    image::pixel_count(width as usize, height as usize)?;
    Ok(Header {
        width,
        height,
        bit_depth,
        color_type,
        interlaced: d[12] == 1,
    })
}

/// Reads just the `IHDR` (signature, first chunk, CRC) without decoding.
pub fn read_header(data: &[u8]) -> Result<Header, PngError> {
    if data.get(..8) != Some(&SIGNATURE[..]) {
        return Err(PngError::BadSignature);
    }
    let mut pos = 8;
    let c = next_chunk(data, &mut pos)?;
    if &c.ty != b"IHDR" {
        return Err(PngError::MissingIhdr);
    }
    parse_ihdr(c.data)
}

// ---------------------------------------------------------------------------
// Adam7 geometry
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Pass {
    x0: usize,
    y0: usize,
    dx: usize,
    dy: usize,
    w: usize,
    h: usize,
}

const ADAM7: [(usize, usize, usize, usize); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

/// The (up to 7) passes of an image; empty passes have `w == 0 || h == 0`.
fn passes(w: usize, h: usize, interlaced: bool) -> ([Pass; 7], usize) {
    let mut out = [Pass {
        x0: 0,
        y0: 0,
        dx: 1,
        dy: 1,
        w,
        h,
    }; 7];
    if !interlaced {
        return (out, 1);
    }
    for (p, &(x0, y0, dx, dy)) in out.iter_mut().zip(ADAM7.iter()) {
        let pw = if w > x0 { (w - x0).div_ceil(dx) } else { 0 };
        let ph = if h > y0 { (h - y0).div_ceil(dy) } else { 0 };
        *p = Pass {
            x0,
            y0,
            dx,
            dy,
            w: pw,
            h: ph,
        };
    }
    (out, 7)
}

/// Bytes in one scanline of `pixels` pixels (without the filter byte).
fn row_bytes(pixels: usize, bits_per_pixel: usize) -> usize {
    (pixels * bits_per_pixel).div_ceil(8)
}

/// Exact size of the decompressed scanline stream (filter bytes included).
fn raw_size(h: &Header) -> u64 {
    let (ps, n) = passes(h.width as usize, h.height as usize, h.interlaced);
    let bpp = h.bits_per_pixel();
    let mut total = 0u64;
    for p in ps.iter().take(n) {
        if p.w > 0 && p.h > 0 {
            total += p.h as u64 * (1 + row_bytes(p.w, bpp) as u64);
        }
    }
    total
}

// ---------------------------------------------------------------------------
// Unfiltering
// ---------------------------------------------------------------------------

#[inline]
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (a as i16, b as i16, c as i16);
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Reverses filter `ft` in place. `prev` is the previous (already
/// unfiltered) scanline of the same length, or zeros for the first row.
fn unfilter(ft: u8, row: &mut [u8], prev: &[u8], bpp: usize) -> Result<(), PngError> {
    let n = row.len();
    let prev = &prev[..n];
    match ft {
        0 => {}
        1 => {
            for i in bpp..n {
                row[i] = row[i].wrapping_add(row[i - bpp]);
            }
        }
        2 => {
            for (r, &p) in row.iter_mut().zip(prev) {
                *r = r.wrapping_add(p);
            }
        }
        3 => {
            for i in 0..bpp.min(n) {
                row[i] = row[i].wrapping_add(prev[i] >> 1);
            }
            for i in bpp..n {
                let avg = ((row[i - bpp] as u16 + prev[i] as u16) >> 1) as u8;
                row[i] = row[i].wrapping_add(avg);
            }
        }
        4 => {
            for i in 0..bpp.min(n) {
                row[i] = row[i].wrapping_add(prev[i]); // paeth(0, b, 0) == b
            }
            for i in bpp..n {
                let p = paeth(row[i - bpp], prev[i], prev[i - bpp]);
                row[i] = row[i].wrapping_add(p);
            }
        }
        _ => return Err(PngError::BadFilter),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Pixel conversion
// ---------------------------------------------------------------------------

struct Ctx {
    color_type: ColorType,
    depth: u8,
    palette: [u32; 256],
    palette_len: usize,
    /// `tRNS` colour key for grey (`[g, 0, 0]`) and RGB images.
    key: Option<[u16; 3]>,
}

#[inline]
fn s16(v: u16) -> u8 {
    ((v as u32 + 128) / 257) as u8
}

/// The `i`th sample of a packed row of `depth` (1, 2 or 4) bit samples.
#[inline]
fn packed_sample(raw: &[u8], i: usize, depth: usize) -> u8 {
    let bit = i * depth;
    let b = raw.get(bit >> 3).copied().unwrap_or(0);
    (b >> (8 - depth - (bit & 7))) & ((1u8 << depth) - 1)
}

fn convert_row(c: &Ctx, raw: &[u8], out: &mut [u32]) -> Result<(), PngError> {
    let depth = c.depth as usize;
    match (c.color_type, depth) {
        (ColorType::Gray, 8) => {
            let k = c.key.map_or(-1, |k| k[0] as i32);
            for (o, &g) in out.iter_mut().zip(raw) {
                *o = rgba(g, g, g, if g as i32 == k { 0 } else { 255 });
            }
        }
        (ColorType::Gray, 16) => {
            let k = c.key.map_or(-1, |k| k[0] as i32);
            for (o, v) in out.iter_mut().zip(raw.as_chunks::<2>().0) {
                let v = u16::from_be_bytes(*v);
                let g = s16(v);
                *o = rgba(g, g, g, if v as i32 == k { 0 } else { 255 });
            }
        }
        (ColorType::Gray, d) => {
            let k = c.key.map_or(-1, |k| k[0] as i32);
            let mult = 255 / ((1u32 << d) - 1);
            for (i, o) in out.iter_mut().enumerate() {
                let v = packed_sample(raw, i, d);
                let g = (v as u32 * mult) as u8;
                *o = rgba(g, g, g, if v as i32 == k { 0 } else { 255 });
            }
        }
        (ColorType::Rgb, 8) => {
            for (o, p) in out.iter_mut().zip(raw.as_chunks::<3>().0) {
                let a = match c.key {
                    Some(k) if k == [p[0] as u16, p[1] as u16, p[2] as u16] => 0,
                    _ => 255,
                };
                *o = rgba(p[0], p[1], p[2], a);
            }
        }
        (ColorType::Rgb, _) => {
            for (o, p) in out.iter_mut().zip(raw.as_chunks::<6>().0) {
                let v = [
                    u16::from_be_bytes([p[0], p[1]]),
                    u16::from_be_bytes([p[2], p[3]]),
                    u16::from_be_bytes([p[4], p[5]]),
                ];
                let a = if c.key == Some(v) { 0 } else { 255 };
                *o = rgba(s16(v[0]), s16(v[1]), s16(v[2]), a);
            }
        }
        (ColorType::Palette, 8) => {
            for (o, &i) in out.iter_mut().zip(raw) {
                if i as usize >= c.palette_len {
                    return Err(PngError::BadPaletteIndex);
                }
                *o = c.palette[i as usize];
            }
        }
        (ColorType::Palette, d) => {
            for (i, o) in out.iter_mut().enumerate() {
                let idx = packed_sample(raw, i, d) as usize;
                if idx >= c.palette_len {
                    return Err(PngError::BadPaletteIndex);
                }
                *o = c.palette[idx];
            }
        }
        (ColorType::GrayAlpha, 8) => {
            for (o, p) in out.iter_mut().zip(raw.as_chunks::<2>().0) {
                *o = rgba(p[0], p[0], p[0], p[1]);
            }
        }
        (ColorType::GrayAlpha, _) => {
            for (o, p) in out.iter_mut().zip(raw.as_chunks::<4>().0) {
                let g = s16(u16::from_be_bytes([p[0], p[1]]));
                *o = rgba(g, g, g, s16(u16::from_be_bytes([p[2], p[3]])));
            }
        }
        (ColorType::Rgba, 8) => {
            for (o, p) in out.iter_mut().zip(raw.as_chunks::<4>().0) {
                *o = rgba(p[0], p[1], p[2], p[3]);
            }
        }
        (ColorType::Rgba, _) => {
            for (o, p) in out.iter_mut().zip(raw.as_chunks::<8>().0) {
                *o = rgba(
                    s16(u16::from_be_bytes([p[0], p[1]])),
                    s16(u16::from_be_bytes([p[2], p[3]])),
                    s16(u16::from_be_bytes([p[4], p[5]])),
                    s16(u16::from_be_bytes([p[6], p[7]])),
                );
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------

fn read_full(inf: &mut Inflater<'_>, buf: &mut [u8]) -> Result<(), PngError> {
    let mut n = 0;
    while n < buf.len() {
        match inf.read(&mut buf[n..])? {
            0 => return Err(PngError::ImageDataTooShort),
            k => n += k,
        }
    }
    Ok(())
}

/// Decodes a PNG into an RGBA8 [`Image`].
pub fn decode(data: &[u8]) -> Result<Image, PngError> {
    if data.get(..8) != Some(&SIGNATURE[..]) {
        return Err(PngError::BadSignature);
    }
    let mut pos = 8usize;
    let first = next_chunk(data, &mut pos)?;
    if &first.ty != b"IHDR" {
        return Err(PngError::MissingIhdr);
    }
    let hdr = parse_ihdr(first.data)?;

    let mut plte: Option<&[u8]> = None;
    let mut trns: Option<&[u8]> = None;
    let mut idat: Vec<&[u8]> = Vec::new();
    let mut idat_done = false; // a non-IDAT chunk followed the IDATs
    let mut seen_end = false;
    while pos < data.len() {
        let c = next_chunk(data, &mut pos)?;
        match &c.ty {
            b"IHDR" => return Err(PngError::ChunkOrder),
            b"PLTE" => {
                if plte.is_some() || !idat.is_empty() || trns.is_some() {
                    return Err(PngError::ChunkOrder);
                }
                plte = Some(c.data);
            }
            b"tRNS" => {
                if trns.is_some() || !idat.is_empty() {
                    return Err(PngError::ChunkOrder);
                }
                trns = Some(c.data);
            }
            b"IDAT" => {
                if idat_done {
                    return Err(PngError::ChunkOrder);
                }
                idat.push(c.data);
            }
            b"IEND" => {
                if !c.data.is_empty() {
                    return Err(PngError::BadChunk);
                }
                seen_end = true;
                break;
            }
            ty => {
                if ty[0] & 0x20 == 0 {
                    return Err(PngError::UnknownCriticalChunk);
                }
            }
        }
        if &c.ty != b"IDAT" && !idat.is_empty() {
            idat_done = true;
        }
    }
    if !seen_end {
        return Err(PngError::Truncated);
    }
    if idat.is_empty() {
        return Err(PngError::MissingIdat);
    }

    // Palette / transparency.
    let mut ctx = Ctx {
        color_type: hdr.color_type,
        depth: hdr.bit_depth,
        palette: [0xFF00_0000; 256],
        palette_len: 0,
        key: None,
    };
    match hdr.color_type {
        ColorType::Palette => {
            let p = plte.ok_or(PngError::MissingPalette)?;
            let n = p.len() / 3;
            if p.len() % 3 != 0 || n == 0 || n > 256 || n > (1usize << hdr.bit_depth) {
                return Err(PngError::BadPalette);
            }
            for (slot, e) in ctx.palette.iter_mut().zip(p.as_chunks::<3>().0) {
                *slot = rgba(e[0], e[1], e[2], 255);
            }
            ctx.palette_len = n;
            if let Some(t) = trns {
                if t.len() > n {
                    return Err(PngError::BadPalette);
                }
                for (slot, &a) in ctx.palette.iter_mut().zip(t) {
                    *slot = (*slot & 0x00FF_FFFF) | ((a as u32) << 24);
                }
            }
        }
        ColorType::Gray | ColorType::Rgb => {
            if plte.is_some() && hdr.color_type == ColorType::Gray {
                return Err(PngError::BadPalette); // PLTE is illegal for greyscale
            }
            if let Some(t) = trns {
                let want = if hdr.color_type == ColorType::Gray {
                    2
                } else {
                    6
                };
                if t.len() != want {
                    return Err(PngError::BadPalette);
                }
                let mask = if hdr.bit_depth == 16 {
                    0xFFFF
                } else {
                    (1u16 << hdr.bit_depth) - 1
                };
                let rd = |i: usize| u16::from_be_bytes([t[2 * i], t[2 * i + 1]]) & mask;
                ctx.key = Some(if want == 2 {
                    [rd(0), 0, 0]
                } else {
                    [rd(0), rd(1), rd(2)]
                });
            }
        }
        ColorType::GrayAlpha => {
            if plte.is_some() {
                return Err(PngError::BadPalette);
            }
        }
        ColorType::Rgba => {}
    }

    // Size checks before any large allocation.
    let (w, h) = (hdr.width as usize, hdr.height as usize);
    let expected = usize::try_from(raw_size(&hdr)).map_err(|_| ImageError::TooLarge)?;
    let compressed: usize = idat.iter().map(|c| c.len()).sum();
    // Deflate expands at most ~1032:1; a header promising more than that was
    // lying (and would make us allocate the image for nothing).
    if expected as u64 > (compressed as u64 + 16) * 1100 {
        return Err(PngError::ImageDataTooShort);
    }
    let stream: Cow<[u8]> = if idat.len() == 1 {
        Cow::Borrowed(idat[0])
    } else {
        let mut v = Vec::new();
        v.try_reserve_exact(compressed)
            .map_err(|_| ImageError::OutOfMemory)?;
        for c in &idat {
            v.extend_from_slice(c);
        }
        Cow::Owned(v)
    };

    let mut img = Image::new(w, h, 0)?;
    let mut inf = Inflater::new_zlib(&stream, expected)?;
    let bpp_bits = hdr.bits_per_pixel();
    let fbpp = hdr.filter_bpp();
    let max_rb = row_bytes(w, bpp_bits);
    let mut prev = try_vec(max_rb, 0u8)?;
    let mut cur = try_vec(max_rb + 1, 0u8)?;
    let mut tmp = if hdr.interlaced {
        try_vec(w, 0u32)?
    } else {
        Vec::new()
    };
    let (ps, np) = passes(w, h, hdr.interlaced);
    for p in ps.iter().take(np) {
        if p.w == 0 || p.h == 0 {
            continue;
        }
        let rb = row_bytes(p.w, bpp_bits);
        prev[..rb].fill(0);
        for j in 0..p.h {
            read_full(&mut inf, &mut cur[..rb + 1])?;
            let (ft, row) = (cur[0], &mut cur[1..rb + 1]);
            unfilter(ft, row, &prev[..rb], fbpp)?;
            let y = p.y0 + j * p.dy;
            if hdr.interlaced {
                convert_row(&ctx, row, &mut tmp[..p.w])?;
                let dst = img.row_mut(y);
                for (i, &px) in tmp[..p.w].iter().enumerate() {
                    if let Some(slot) = dst.get_mut(p.x0 + i * p.dx) {
                        *slot = px;
                    }
                }
            } else {
                convert_row(&ctx, row, img.row_mut(y))?;
            }
            prev[..rb].copy_from_slice(row);
        }
    }
    // The stream must end exactly here (and its Adler-32 must check out).
    let mut probe = [0u8; 1];
    match inf.read(&mut probe) {
        Ok(0) => {}
        Ok(_) | Err(InflateError::OutputLimit) => return Err(PngError::ImageDataTooLong),
        Err(e) => return Err(PngError::Inflate(e)),
    }
    Ok(img)
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

fn write_chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(ty);
    out.extend_from_slice(data);
    let mut c = Crc32::new();
    c.update(ty);
    c.update(data);
    out.extend_from_slice(&c.finish().to_be_bytes());
}

/// Applies filter `ft` to `row` (previous row `prev`) into `out`.
fn filter_row(ft: u8, row: &[u8], prev: &[u8], bpp: usize, out: &mut [u8]) {
    for i in 0..row.len() {
        let a = if i >= bpp { row[i - bpp] } else { 0 };
        let b = prev[i];
        let c = if i >= bpp { prev[i - bpp] } else { 0 };
        let pred = match ft {
            0 => 0,
            1 => a,
            2 => b,
            3 => ((a as u16 + b as u16) >> 1) as u8,
            _ => paeth(a, b, c),
        };
        out[i] = row[i].wrapping_sub(pred);
    }
}

fn encode_impl(img: &Image, rgb: bool) -> Result<Vec<u8>, ImageError> {
    let (w, h) = (img.width(), img.height());
    let bpp = if rgb { 3 } else { 4 };
    let rb = w * bpp;
    let mut raw = Vec::new();
    raw.try_reserve_exact(h * (rb + 1))
        .map_err(|_| ImageError::OutOfMemory)?;
    let mut row = try_vec(rb, 0u8)?;
    let mut prev = try_vec(rb, 0u8)?;
    let mut cand: [Vec<u8>; 5] = [
        try_vec(rb, 0)?,
        try_vec(rb, 0)?,
        try_vec(rb, 0)?,
        try_vec(rb, 0)?,
        try_vec(rb, 0)?,
    ];
    for y in 0..h {
        for (dst, &p) in row.chunks_exact_mut(bpp).zip(img.row(y)) {
            let c = image::channels(p);
            dst.copy_from_slice(&c[..bpp]);
        }
        let mut best = (u64::MAX, 0usize);
        for (ft, out) in cand.iter_mut().enumerate() {
            filter_row(ft as u8, &row, &prev, bpp, out);
            let score: u64 = out.iter().map(|&b| (b as i8).unsigned_abs() as u64).sum();
            if score < best.0 {
                best = (score, ft);
            }
        }
        raw.push(best.1 as u8);
        raw.extend_from_slice(&cand[best.1]);
        core::mem::swap(&mut row, &mut prev);
    }
    let z = deflate::zlib_compress(&raw);
    drop(raw);
    let mut out = Vec::new();
    out.try_reserve_exact(z.len() + 80 + z.len() / 65536 * 12)
        .map_err(|_| ImageError::OutOfMemory)?;
    out.extend_from_slice(&SIGNATURE);
    let mut ihdr = [0u8; 13];
    ihdr[..4].copy_from_slice(&(w as u32).to_be_bytes());
    ihdr[4..8].copy_from_slice(&(h as u32).to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = if rgb { 2 } else { 6 };
    write_chunk(&mut out, b"IHDR", &ihdr);
    for c in z.chunks(65536) {
        write_chunk(&mut out, b"IDAT", c);
    }
    write_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// Encodes `img` as a PNG: RGB8 if it is fully opaque, RGBA8 otherwise.
pub fn encode(img: &Image) -> Result<Vec<u8>, ImageError> {
    encode_impl(img, img.is_opaque())
}

/// Encodes `img` as an RGBA8 PNG.
pub fn encode_rgba(img: &Image) -> Result<Vec<u8>, ImageError> {
    encode_impl(img, false)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod vector_tests;
#[cfg(test)]
mod vectors;
