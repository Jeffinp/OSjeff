//! chunks (split out of `png.rs`).

use super::*;

pub(super) struct Chunk<'a> {
    pub(super) ty: [u8; 4],
    pub(super) data: &'a [u8],
}

/// Reads and CRC-checks the chunk at `*pos`, advancing past it.
pub(super) fn next_chunk<'a>(data: &'a [u8], pos: &mut usize) -> Result<Chunk<'a>, PngError> {
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

pub(super) fn parse_ihdr(d: &[u8]) -> Result<Header, PngError> {
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
