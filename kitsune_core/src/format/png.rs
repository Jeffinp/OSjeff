//! PNG decoder and encoder (ISO/IEC 15948), built on [`crate::format::inflate`] and
//! [`crate::format::deflate`].
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

use crate::format::deflate;
use crate::format::image::{self, Image, ImageError, rgba, try_vec};
use crate::format::inflate::{Crc32, InflateError, Inflater};
use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::fmt;

mod adam7;
mod chunks;
mod decoder;
mod encoder;
mod pixels;
mod unfilter;
use adam7::*;
pub use chunks::*;
pub use decoder::*;
pub use encoder::*;
use pixels::*;
use unfilter::*;

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

// ---------------------------------------------------------------------------
// Adam7 geometry
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Unfiltering
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Pixel conversion
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
#[cfg(test)]
mod vector_tests;
#[cfg(test)]
mod vectors;
