//! Fuzz target: everything the image viewer does with attacker-controlled
//! bytes (a downloaded or on-disk picture): DEFLATE/zlib, PNG, BMP, PPM, the
//! signature detector, and the pixel operations on whatever decodes.
//!
//! Input layout: `[mode, p0, p1, p2, ...bytes]`.
//!
//! * `mode & 3 == 0`: `bytes` go to `image::decode` as they are (raw).
//! * `mode & 3 == 1`: `bytes` are treated as a PNG: the signature is
//!   prepended if missing and every chunk CRC is repaired, so the fuzzer gets
//!   past the checksum and reaches IHDR/PLTE/tRNS/IDAT/inflate handling.
//! * `mode & 3 == 2`: a structurally valid PNG is *synthesised*: `p0..p2`
//!   pick width, height, colour type, depth and interlacing, `bytes` become
//!   the scanline data (compressed with our own zlib encoder, CRCs and Adler
//!   right), so filters, Adam7 and every pixel format run on random data.
//! * `mode & 3 == 3`: `bytes` after a `BM` signature are decoded as a BMP,
//!   with the pixel offset patched to a plausible value so headers, palettes,
//!   bit masks and RLE streams are exercised.
//! * `mode & 7 == 4` / `5`: `bytes` after a `GIF89a` / `FF D8 FF` signature, so the GIF
//!   (LZW, palettes, interlace) and JPEG (Huffman, restart, IDCT) decoders get structure.
//!   (`mode & 7 == 6, 7` are raw like 0.)
//!
//! Always: `inflate`/`zlib_decompress` on the raw bytes with a small
//! `max_output`, and the streaming `Inflater` with a tiny buffer.
//!
//! Whatever decodes is then resized (all three filters), fitted, rotated,
//! flipped, cropped, flattened, blitted, and re-encoded as PNG/BMP/PPM; the
//! PNG and 32-bit BMP encoders must round-trip exactly (a property check, not
//! just "does not crash").
#![no_main]

use libfuzzer_sys::fuzz_target;
use kitsune_core::image::{self, Filter, Format, Image};
use kitsune_core::inflate::{self, Crc32, Inflater};
use kitsune_core::{bmp, png};

const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Rewrites the CRC of every chunk that is fully inside `png`.
fn fix_crcs(png: &mut [u8]) {
    let mut pos = 8usize;
    while pos + 12 <= png.len() {
        let len = u32::from_be_bytes([png[pos], png[pos + 1], png[pos + 2], png[pos + 3]]) as usize;
        let Some(end) = (pos + 8).checked_add(len) else { return };
        if end + 4 > png.len() {
            return;
        }
        let mut c = Crc32::new();
        c.update(&png[pos + 4..end]);
        png[end..end + 4].copy_from_slice(&c.finish().to_be_bytes());
        pos = end + 4;
    }
}

fn chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(ty);
    out.extend_from_slice(data);
    let mut c = Crc32::new();
    c.update(ty);
    c.update(data);
    out.extend_from_slice(&c.finish().to_be_bytes());
}

/// A valid PNG container around `scan` (the filtered scanline bytes).
fn synth_png(p: [u8; 3], scan: &[u8]) -> Vec<u8> {
    let w = (p[0] % 40) as u32 + 1;
    let h = (p[1] % 40) as u32 + 1;
    let (ct, depth) = match p[2] % 15 {
        0 => (0, 1),
        1 => (0, 2),
        2 => (0, 4),
        3 => (0, 8),
        4 => (0, 16),
        5 => (2, 8),
        6 => (2, 16),
        7 => (3, 1),
        8 => (3, 2),
        9 => (3, 4),
        10 => (3, 8),
        11 => (4, 8),
        12 => (4, 16),
        13 => (6, 8),
        _ => (6, 16),
    };
    let interlace = (p[2] >> 4) & 1;
    let mut out = SIG.to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[depth, ct, 0, 0, interlace]);
    chunk(&mut out, b"IHDR", &ihdr);
    if ct == 3 {
        // A palette with a size taken from the data (1..=256 colours).
        let n = (scan.first().copied().unwrap_or(1) as usize % (1usize << depth)) + 1;
        let mut pal = Vec::new();
        for i in 0..n * 3 {
            pal.push(scan.get(i + 1).copied().unwrap_or(i as u8));
        }
        chunk(&mut out, b"PLTE", &pal);
        if p[2] & 0x20 != 0 {
            chunk(&mut out, b"tRNS", &pal[..n.min(pal.len())]);
        }
    } else if (ct == 0 || ct == 2) && p[2] & 0x20 != 0 {
        let key: Vec<u8> = (0..if ct == 0 { 2 } else { 6 }).map(|i| scan.get(i).copied().unwrap_or(0)).collect();
        chunk(&mut out, b"tRNS", &key);
    }
    // Make the IDAT the right size most of the time (so the pixel code runs),
    // but let `p[2] & 0x40` leave it as the fuzzer wrote it.
    let mut raw = scan.to_vec();
    if p[2] & 0x40 == 0 {
        if let Ok(hdr) = png::read_header(&out_with_iend(&out)) {
            let want = expected_len(&hdr);
            raw.resize(want, 0);
            // Keep filter bytes valid: they sit at known row starts.
            sanitize_filters(&hdr, &mut raw);
        }
    }
    let z = kitsune_core::deflate::zlib_compress(&raw);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn out_with_iend(partial: &[u8]) -> Vec<u8> {
    let mut v = partial.to_vec();
    chunk(&mut v, b"IEND", &[]);
    v
}

fn bits_per_pixel(h: &png::Header) -> usize {
    use png::ColorType::*;
    let ch = match h.color_type {
        Gray | Palette => 1,
        GrayAlpha => 2,
        Rgb => 3,
        Rgba => 4,
    };
    ch * h.bit_depth as usize
}

/// Passes as (width, height) of each (non-empty) pass.
fn pass_dims(h: &png::Header) -> Vec<(usize, usize)> {
    let (w, hh) = (h.width as usize, h.height as usize);
    if !h.interlaced {
        return vec![(w, hh)];
    }
    let adam7 = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];
    adam7
        .iter()
        .filter_map(|&(x0, y0, dx, dy)| {
            let pw = if w > x0 { (w - x0).div_ceil(dx) } else { 0 };
            let ph = if hh > y0 { (hh - y0).div_ceil(dy) } else { 0 };
            (pw > 0 && ph > 0).then_some((pw, ph))
        })
        .collect()
}

fn expected_len(h: &png::Header) -> usize {
    let bpp = bits_per_pixel(h);
    pass_dims(h).iter().map(|&(pw, ph)| ph * (1 + (pw * bpp).div_ceil(8))).sum()
}

fn sanitize_filters(h: &png::Header, raw: &mut [u8]) {
    let bpp = bits_per_pixel(h);
    let mut pos = 0;
    for (pw, ph) in pass_dims(h) {
        let rb = 1 + (pw * bpp).div_ceil(8);
        for _ in 0..ph {
            if let Some(b) = raw.get_mut(pos) {
                *b %= 5;
            }
            pos += rb;
        }
    }
}

/// True for a BMP with RLE compression whose dimensions exceed ~1 Mpx.
fn big_rle_bmp(d: &[u8]) -> bool {
    if d.len() < 34 || &d[..2] != b"BM" {
        return false;
    }
    let w = i32::from_le_bytes([d[18], d[19], d[20], d[21]]) as i64;
    let h = i32::from_le_bytes([d[22], d[23], d[24], d[25]]) as i64;
    let comp = u32::from_le_bytes([d[30], d[31], d[32], d[33]]);
    (comp == 1 || comp == 2) && w > 0 && w * h.abs() > 1 << 20
}

fn exercise(img: &Image, params: [u8; 3]) {
    let (w, h) = (img.width(), img.height());
    assert_eq!(img.pixels().len(), w * h);
    // Keep the per-input cost bounded: big images only get the cheap checks.
    if w * h > 1 << 14 {
        let _ = img.fit(64, 64, false, Filter::Nearest);
        let t = img.fit(64, 64, false, Filter::Box).expect("fit box");
        assert!(t.width() <= 64 && t.height() <= 64);
        return;
    }
    let tw = params[0] as usize % 97 + 1;
    let th = params[1] as usize % 97 + 1;
    for f in [Filter::Nearest, Filter::Bilinear, Filter::Box, Filter::Auto] {
        let r = img.resize(tw, th, f).expect("small resize");
        assert_eq!((r.width(), r.height()), (tw, th));
    }
    let t = img.fit(tw, th, params[2] & 1 != 0, Filter::Auto).expect("fit");
    assert!(t.width() >= 1 && t.height() >= 1);
    let r90 = img.rotate90().expect("rotate90");
    assert_eq!((r90.width(), r90.height()), (h, w));
    assert_eq!(r90.rotate270().expect("rotate270"), *img);
    let mut m = img.clone();
    m.flip_horizontal();
    m.flip_vertical();
    m.rotate180();
    assert_eq!(&m, img);
    let (cx, cy) = (params[0] as usize % w, params[1] as usize % h);
    let c = img.crop(cx, cy, w - cx, h - cy).expect("crop");
    assert_eq!(c.get(0, 0), img.get(cx, cy));
    let mut bg = Image::new(w.min(9), h.min(9), 0xFF30_6090).expect("bg");
    bg.blit_over(img, params[0] as i32 - 100, params[1] as i32 - 100);
    let mut flat = img.clone();
    flat.flatten(0xFF11_2233);
    assert!(flat.is_opaque());

    // Encoders must round-trip exactly.
    let png_bytes = png::encode(img).expect("png encode");
    assert_eq!(&png::decode(&png_bytes).expect("png re-decode"), img);
    let png_bytes = png::encode_rgba(img).expect("png encode rgba");
    assert_eq!(&png::decode(&png_bytes).expect("png re-decode"), img);
    if img.pixels().iter().any(|&p| image::alpha(p) != 0) {
        let b = bmp::encode_32(img).expect("bmp encode");
        assert_eq!(&bmp::decode(&b).expect("bmp re-decode"), img);
    }
    let b = bmp::encode_24(img, 0xFF00_0000).expect("bmp24 encode");
    let back = bmp::decode(&b).expect("bmp24 re-decode");
    let mut want = img.clone();
    want.flatten(0xFF00_0000);
    assert_eq!(back, want);
    for fmt in [Format::Png, Format::Bmp, Format::Ppm] {
        let bytes = image::encode(img, fmt).expect("encode");
        assert_eq!(image::detect(&bytes), Some(fmt));
        let _ = image::decode(&bytes).expect("decode what we encoded");
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }
    let mode = data[0];
    let params = [data[1], data[2], data[3]];
    let bytes = &data[4..];

    // ---- DEFLATE / zlib on the raw bytes, small output cap ----
    let _ = inflate::inflate(bytes, 4096);
    let _ = inflate::zlib_decompress(bytes, 4096);
    let _ = inflate::inflate_consumed(bytes, 1 << 16);
    if let Ok(mut inf) = Inflater::new_zlib(bytes, 1 << 16) {
        let mut buf = [0u8; 7];
        let mut total = 0usize;
        while let Ok(n) = inf.read(&mut buf) {
            total += n;
            if n == 0 || total > 1 << 16 {
                break;
            }
        }
        assert!(inf.total_out() <= 1 << 16);
    }
    let mut inf = Inflater::new_raw(bytes, 1 << 16);
    let mut buf = [0u8; 4096];
    let mut total = 0usize;
    while let Ok(n) = inf.read(&mut buf) {
        total += n;
        if n == 0 || total > 1 << 16 {
            break;
        }
    }
    assert!(total <= 1 << 16);

    // ---- build the picture bytes according to the mode ----
    let picture: Vec<u8> = match mode & 7 {
        0 | 6 | 7 => bytes.to_vec(),
        4 => {
            let mut v = if bytes.starts_with(b"GIF8") { Vec::new() } else { b"GIF89a".to_vec() };
            v.extend_from_slice(bytes);
            v
        }
        5 => {
            let mut v = if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) { Vec::new() } else { vec![0xFF, 0xD8, 0xFF] };
            v.extend_from_slice(bytes);
            v
        }
        1 => {
            let mut v = if bytes.starts_with(&SIG) { Vec::new() } else { SIG.to_vec() };
            v.extend_from_slice(bytes);
            fix_crcs(&mut v);
            v
        }
        2 => synth_png(params, bytes),
        _ => {
            let mut v = b"BM".to_vec();
            v.extend_from_slice(bytes);
            // Point the pixel array just past the header(s) when it is wildly off.
            if v.len() >= 18 {
                let off = u32::from_le_bytes([v[10], v[11], v[12], v[13]]) as usize;
                if off > v.len() {
                    let dib = u32::from_le_bytes([v[14], v[15], v[16], v[17]]) as usize;
                    let guess = (14 + dib.min(124) + 1024).min(v.len()) as u32;
                    v[10..14].copy_from_slice(&guess.to_le_bytes());
                }
            }
            v
        }
    };

    // ---- decode, then use the result ----
    let _ = image::detect(&picture);
    let _ = png::read_header(&picture);
    // A GIF or JPEG may legitimately claim up to MAX_PIXELS from a few bytes; as for the RLE
    // BMP, that limit is covered by unit tests and would only make the fuzzer memset.
    let claimed = kitsune_core::format::gif::peek_dims(&picture)
        .or_else(|| kitsune_core::format::jpeg::peek_dims(&picture));
    if claimed.is_some_and(|(w, h)| w * h > 1 << 20) {
        return;
    }
    if big_rle_bmp(&picture) {
        // A run-length BMP may legitimately claim up to MAX_PIXELS (64 MiB of
        // zeroed pixels) from a few bytes; that limit is covered by unit tests
        // and would only make the fuzzer spend its time in memset.
        return;
    }
    if let Ok(img) = image::decode(&picture) {
        exercise(&img, params);
    }
});
