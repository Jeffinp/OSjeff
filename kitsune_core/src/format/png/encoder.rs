//! encoder (split out of `png.rs`).

use super::*;

pub(super) fn write_chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(ty);
    out.extend_from_slice(data);
    let mut c = Crc32::new();
    c.update(ty);
    c.update(data);
    out.extend_from_slice(&c.finish().to_be_bytes());
}

/// Applies filter `ft` to `row` (previous row `prev`) into `out`.
pub(super) fn filter_row(ft: u8, row: &[u8], prev: &[u8], bpp: usize, out: &mut [u8]) {
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

pub(super) fn encode_impl(img: &Image, rgb: bool) -> Result<Vec<u8>, ImageError> {
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
