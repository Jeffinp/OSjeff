use super::vectors::*;
use super::*;
use crate::format::deflate::{zlib_compress, zlib_stored};
use crate::format::inflate::{crc32, zlib_decompress};
use crate::testutil::unhex;
use alloc::vec;
use alloc::vec::Vec;

fn chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    write_chunk(&mut v, ty, data);
    v
}

fn ihdr(w: u32, h: u32, depth: u8, ct: u8, interlace: u8) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&w.to_be_bytes());
    d.extend_from_slice(&h.to_be_bytes());
    d.extend_from_slice(&[depth, ct, 0, 0, interlace]);
    d
}

/// A PNG from raw scanline bytes (filter bytes included), compressed with our
/// own zlib encoder. `before` chunks go between IHDR and IDAT.
fn build(ihdr_data: &[u8], before: &[Vec<u8>], raw: &[u8]) -> Vec<u8> {
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"IHDR", ihdr_data));
    for c in before {
        v.extend_from_slice(c);
    }
    v.extend(chunk(b"IDAT", &zlib_compress(raw)));
    v.extend(chunk(b"IEND", &[]));
    v
}

/// Rows of `w` RGBA8 pixels each preceded by filter byte 0.
fn rgba_raw(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 4]) -> Vec<u8> {
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            raw.extend_from_slice(&f(x, y));
        }
    }
    raw
}

fn plain_rgba(w: usize, h: usize) -> Vec<u8> {
    let raw = rgba_raw(w, h, |x, y| {
        [x as u8 * 10, y as u8 * 10, (x + y) as u8, 255 - x as u8]
    });
    build(&ihdr(w as u32, h as u32, 8, 6, 0), &[], &raw)
}

/// Rewrites the CRC of every chunk (so tests can mutate bytes and still pass framing).
fn fix_crcs(png: &mut [u8]) {
    let mut pos = 8;
    while pos + 12 <= png.len() {
        let len = u32::from_be_bytes([png[pos], png[pos + 1], png[pos + 2], png[pos + 3]]) as usize;
        let end = pos + 8 + len;
        if end + 4 > png.len() {
            return;
        }
        let crc = crc32(&png[pos + 4..end]);
        png[end..end + 4].copy_from_slice(&crc.to_be_bytes());
        pos = end + 4;
    }
}

fn first_chunk_of(png: &[u8], ty: &[u8; 4]) -> (usize, usize) {
    let mut pos = 8;
    loop {
        let len = u32::from_be_bytes([png[pos], png[pos + 1], png[pos + 2], png[pos + 3]]) as usize;
        if &png[pos + 4..pos + 8] == ty {
            return (pos, len);
        }
        pos += 12 + len;
    }
}

fn with_ihdr(patch: impl Fn(&mut [u8])) -> Vec<u8> {
    let mut v = plain_rgba(4, 4);
    let (pos, len) = first_chunk_of(&v, b"IHDR");
    assert_eq!(len, 13);
    patch(&mut v[pos + 8..pos + 8 + 13]);
    fix_crcs(&mut v);
    v
}

fn alpha(p: u32) -> u8 {
    image::alpha(p)
}

fn sample(w: usize, h: usize, with_alpha: bool) -> Image {
    let mut px = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let v = (x * 31 + y * 17 + x * y) as u32;
            let a = if with_alpha { (v * 7 % 256) as u8 } else { 255 };
            px.push(rgba(
                (v * 3) as u8,
                (v * 5 + 11) as u8,
                (v * 13 + 29) as u8,
                a,
            ));
        }
    }
    Image::from_pixels(w, h, px).unwrap()
}

mod chunk_order_palette;
mod encoder;
mod geometry_conversion;
mod idat_structure;
mod ihdr;
mod signature_framing;
mod unfilter;
