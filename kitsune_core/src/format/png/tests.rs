use super::vectors::*;
use super::*;
use crate::format::deflate::{zlib_compress, zlib_stored};
use crate::format::inflate::{crc32, zlib_decompress};
use crate::testutil::unhex;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------- builders

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

// ---------------------------------------------------------------- signature, framing

#[test]
fn signature_is_checked() {
    assert_eq!(decode(&[]), Err(PngError::BadSignature));
    assert_eq!(decode(&SIGNATURE[..7]), Err(PngError::BadSignature));
    assert_eq!(decode(b"GIF89a.........."), Err(PngError::BadSignature));
    // Line-ending damage (a classic FTP text-mode corruption) is detected.
    let mut v = plain_rgba(2, 2);
    v.remove(4); // drop the \r
    assert_eq!(decode(&v), Err(PngError::BadSignature));
    assert_eq!(read_header(b"\x89PNG"), Err(PngError::BadSignature));
}

#[test]
fn signature_only_is_truncated() {
    assert_eq!(decode(&SIGNATURE), Err(PngError::Truncated));
}

#[test]
fn first_chunk_must_be_ihdr() {
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"IEND", &[]));
    assert_eq!(decode(&v), Err(PngError::MissingIhdr));
    assert_eq!(read_header(&v), Err(PngError::MissingIhdr));
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"tEXt", b"a\0b"));
    v.extend(chunk(b"IHDR", &ihdr(1, 1, 8, 6, 0)));
    assert_eq!(decode(&v), Err(PngError::MissingIhdr));
}

#[test]
fn huge_chunk_length_is_rejected() {
    let mut v = SIGNATURE.to_vec();
    v.extend_from_slice(&0x8000_0000u32.to_be_bytes());
    v.extend_from_slice(b"IHDR");
    v.extend_from_slice(&[0; 20]);
    assert_eq!(decode(&v), Err(PngError::BadChunk));
    // A length that is legal but far past the end of the file.
    let mut v = SIGNATURE.to_vec();
    v.extend_from_slice(&0x7FFF_FFFFu32.to_be_bytes());
    v.extend_from_slice(b"IHDR");
    v.extend_from_slice(&[0; 20]);
    assert_eq!(decode(&v), Err(PngError::Truncated));
}

#[test]
fn chunk_type_must_be_letters() {
    let mut v = plain_rgba(2, 2);
    let (pos, _) = first_chunk_of(&v, b"IHDR");
    v[pos + 4] = b'1';
    assert_eq!(decode(&v), Err(PngError::BadChunk));
}

#[test]
fn every_chunk_crc_is_verified() {
    for ty in [b"IHDR", b"IDAT", b"IEND"] {
        let mut v = plain_rgba(3, 3);
        let (pos, len) = first_chunk_of(&v, ty);
        v[pos + 8 + len] ^= 0x01; // first CRC byte
        assert_eq!(
            decode(&v),
            Err(PngError::CrcMismatch),
            "{}",
            core::str::from_utf8(ty).unwrap()
        );
    }
    // A skipped ancillary chunk with a bad CRC is still an error.
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"IHDR", &ihdr(1, 1, 8, 6, 0)));
    let mut t = chunk(b"tEXt", b"k\0v");
    let n = t.len();
    t[n - 1] ^= 0xFF;
    v.extend(t);
    v.extend(chunk(b"IDAT", &zlib_compress(&[0, 1, 2, 3, 4])));
    v.extend(chunk(b"IEND", &[]));
    assert_eq!(decode(&v), Err(PngError::CrcMismatch));
}

#[test]
fn missing_iend_is_truncated() {
    let v = plain_rgba(2, 2);
    let (pos, _) = first_chunk_of(&v, b"IEND");
    assert_eq!(decode(&v[..pos]), Err(PngError::Truncated));
}

#[test]
fn iend_must_be_empty() {
    let mut v = plain_rgba(2, 2);
    let (pos, _) = first_chunk_of(&v, b"IEND");
    v.truncate(pos);
    v.extend(chunk(b"IEND", &[1]));
    assert_eq!(decode(&v), Err(PngError::BadChunk));
}

#[test]
fn trailing_bytes_after_iend_are_ignored() {
    let mut v = plain_rgba(2, 2);
    let want = decode(&v).unwrap();
    v.extend_from_slice(b"trailing garbage that is not a chunk at all");
    assert_eq!(decode(&v).unwrap(), want);
}

#[test]
fn every_prefix_of_the_vectors_is_an_error() {
    for v in [
        IM_RGB8,
        IM_PAL8_TRNS,
        IM_RGBA8_ADAM7,
        CR_GRAY1_FILTERS,
        CR_RGBA8_MULTI_IDAT,
    ] {
        let file = unhex(v.0);
        for cut in 0..file.len() {
            assert!(decode(&file[..cut]).is_err(), "cut {cut}/{}", file.len());
        }
    }
}

#[test]
fn every_bit_flip_before_the_end_is_detected() {
    // CRC-32 catches every single-bit error inside a chunk, and a flipped
    // length breaks the framing, so no flip before the end of IEND may decode.
    for v in [
        IM_RGB8,
        IM_PAL4,
        CR_PAL2_FILTERS,
        CR_RGB8_KEY,
        IM_GRAY1_ADAM7,
        CR_RGB8_ANCILLARY,
    ] {
        let file = unhex(v.0);
        let iend_end = file.len(); // the vectors end exactly at IEND's CRC
        for bit in 0..iend_end * 8 {
            let mut f = file.clone();
            f[bit / 8] ^= 1 << (bit % 8);
            assert!(decode(&f).is_err(), "flip of bit {bit} decoded");
        }
    }
}

#[test]
fn bit_flips_with_fixed_crcs_never_panic() {
    // Mutating the payload and repairing the CRCs reaches the deeper checks.
    for v in [
        IM_RGB8,
        IM_PAL4,
        IM_RGBA16,
        IM_RGB8_ADAM7,
        CR_PAL2_FILTERS,
        CR_GRAY8_KEY,
        IM_GA8_ADAM7,
    ] {
        let file = unhex(v.0);
        for bit in 0..file.len() * 8 {
            let mut f = file.clone();
            f[bit / 8] ^= 1 << (bit % 8);
            fix_crcs(&mut f);
            let _ = decode(&f);
        }
    }
}

#[test]
fn garbage_never_panics() {
    let mut x = 0xC0FF_EE11u32;
    let head = unhex(IM_RGB8.0);
    for round in 0..300 {
        let mut f = head[..33.min(head.len())].to_vec();
        for _ in 0..round * 2 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            f.push(x as u8);
        }
        let _ = decode(&f);
        let _ = read_header(&f);
    }
}

// ---------------------------------------------------------------- IHDR

#[test]
fn read_header_reports_the_fields() {
    let h = read_header(&unhex(IM_RGB16_ADAM7.0)).unwrap();
    assert_eq!(
        (h.width, h.height, h.bit_depth, h.color_type, h.interlaced),
        (9, 7, 16, ColorType::Rgb, true)
    );
    let h = read_header(&unhex(IM_PAL4.0)).unwrap();
    assert_eq!(
        (h.bit_depth, h.color_type, h.interlaced),
        (4, ColorType::Palette, false)
    );
    let h = read_header(&unhex(IM_GA8.0)).unwrap();
    assert_eq!(h.color_type, ColorType::GrayAlpha);
    let h = read_header(&unhex(IM_GRAY1.0)).unwrap();
    assert_eq!((h.bit_depth, h.color_type), (1, ColorType::Gray));
    let h = read_header(&unhex(IM_RGBA8.0)).unwrap();
    assert_eq!(h.color_type, ColorType::Rgba);
}

fn with_ihdr(patch: impl Fn(&mut [u8])) -> Vec<u8> {
    let mut v = plain_rgba(4, 4);
    let (pos, len) = first_chunk_of(&v, b"IHDR");
    assert_eq!(len, 13);
    patch(&mut v[pos + 8..pos + 8 + 13]);
    fix_crcs(&mut v);
    v
}

#[test]
fn invalid_depth_and_color_combinations() {
    for (depth, ct) in [
        (0u8, 6u8),
        (3, 6),
        (5, 2),
        (7, 0),
        (32, 6),
        (1, 2),
        (2, 2),
        (4, 6),
        (16, 3),
        (8, 1),
        (8, 5),
        (8, 7),
        (8, 255),
    ] {
        let v = with_ihdr(|d| {
            d[8] = depth;
            d[9] = ct;
        });
        assert_eq!(
            decode(&v),
            Err(PngError::BadIhdr),
            "depth {depth} type {ct}"
        );
        assert_eq!(read_header(&v), Err(PngError::BadIhdr));
    }
}

#[test]
fn every_legal_depth_and_color_combination_parses() {
    for (depth, ct) in [
        (1u8, 0u8),
        (2, 0),
        (4, 0),
        (8, 0),
        (16, 0),
        (8, 2),
        (16, 2),
        (1, 3),
        (2, 3),
        (4, 3),
        (8, 3),
        (8, 4),
        (16, 4),
        (8, 6),
        (16, 6),
    ] {
        let v = with_ihdr(|d| {
            d[8] = depth;
            d[9] = ct;
        });
        assert!(read_header(&v).is_ok(), "depth {depth} type {ct}");
    }
}

#[test]
fn invalid_methods_and_interlace() {
    for (idx, val) in [
        (10usize, 1u8),
        (10, 2),
        (11, 1),
        (11, 5),
        (12, 2),
        (12, 255),
    ] {
        let v = with_ihdr(|d| d[idx] = val);
        assert_eq!(decode(&v), Err(PngError::BadIhdr), "byte {idx} = {val}");
    }
}

#[test]
fn ihdr_length_must_be_13() {
    for n in [0usize, 12, 14, 25] {
        let mut v = SIGNATURE.to_vec();
        v.extend(chunk(b"IHDR", &vec![1u8; n]));
        v.extend(chunk(b"IEND", &[]));
        assert_eq!(decode(&v), Err(PngError::BadIhdr), "len {n}");
    }
}

#[test]
fn zero_and_oversized_dimensions() {
    for (w, h) in [
        (0u32, 4u32),
        (4, 0),
        (0, 0),
        (0x8000_0000, 4),
        (4, 0x8000_0000),
        (u32::MAX, u32::MAX),
    ] {
        let v = with_ihdr(|d| {
            d[..4].copy_from_slice(&w.to_be_bytes());
            d[4..8].copy_from_slice(&h.to_be_bytes());
        });
        assert_eq!(decode(&v), Err(PngError::BadDimensions), "{w}x{h}");
    }
}

#[test]
fn giant_images_hit_the_pixel_limit_without_allocating() {
    for (w, h) in [
        (65_536u32, 65_536u32),
        (0x7FFF_FFFF, 0x7FFF_FFFF),
        (100_000, 100_000),
        (4097, 4096),
        (16_777_217, 1),
        (1, 16_777_217),
    ] {
        let v = with_ihdr(|d| {
            d[..4].copy_from_slice(&w.to_be_bytes());
            d[4..8].copy_from_slice(&h.to_be_bytes());
        });
        assert_eq!(
            decode(&v),
            Err(PngError::Image(ImageError::TooLarge)),
            "{w}x{h}"
        );
        assert_eq!(read_header(&v), Err(PngError::Image(ImageError::TooLarge)));
    }
}

#[test]
fn a_header_promising_far_more_than_the_data_is_rejected_early() {
    // 4096 x 4096 RGBA (64 MiB) backed by a few hundred bytes of IDAT.
    let raw = vec![0u8; 4096];
    let v = build(&ihdr(4096, 4096, 8, 6, 0), &[], &raw);
    assert_eq!(decode(&v), Err(PngError::ImageDataTooShort));
    let v = build(&ihdr(16_777_216, 1, 8, 6, 0), &[], &raw);
    assert_eq!(decode(&v), Err(PngError::ImageDataTooShort));
}

// ---------------------------------------------------------------- chunk order / palette

#[test]
fn duplicate_ihdr_and_late_plte_are_rejected() {
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"IHDR", &ihdr(1, 1, 8, 6, 0)));
    v.extend(chunk(b"IHDR", &ihdr(1, 1, 8, 6, 0)));
    v.extend(chunk(b"IEND", &[]));
    assert_eq!(decode(&v), Err(PngError::ChunkOrder));
    let pal = chunk(b"PLTE", &[1, 2, 3]);
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"IHDR", &ihdr(1, 1, 8, 3, 0)));
    v.extend(chunk(b"IDAT", &zlib_compress(&[0, 0])));
    v.extend(pal.clone());
    v.extend(chunk(b"IEND", &[]));
    assert_eq!(decode(&v), Err(PngError::ChunkOrder));
    // PLTE twice.
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"IHDR", &ihdr(1, 1, 8, 3, 0)));
    v.extend(pal.clone());
    v.extend(pal);
    v.extend(chunk(b"IDAT", &zlib_compress(&[0, 0])));
    v.extend(chunk(b"IEND", &[]));
    assert_eq!(decode(&v), Err(PngError::ChunkOrder));
}

#[test]
fn trns_before_plte_and_after_idat_are_rejected() {
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"IHDR", &ihdr(1, 1, 8, 3, 0)));
    v.extend(chunk(b"tRNS", &[0]));
    v.extend(chunk(b"PLTE", &[1, 2, 3]));
    v.extend(chunk(b"IDAT", &zlib_compress(&[0, 0])));
    v.extend(chunk(b"IEND", &[]));
    assert_eq!(decode(&v), Err(PngError::ChunkOrder));
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"IHDR", &ihdr(1, 1, 8, 3, 0)));
    v.extend(chunk(b"PLTE", &[1, 2, 3]));
    v.extend(chunk(b"IDAT", &zlib_compress(&[0, 0])));
    v.extend(chunk(b"tRNS", &[0]));
    v.extend(chunk(b"IEND", &[]));
    assert_eq!(decode(&v), Err(PngError::ChunkOrder));
}

#[test]
fn palette_image_needs_plte() {
    let v = build(&ihdr(1, 1, 8, 3, 0), &[], &[0, 0]);
    assert_eq!(decode(&v), Err(PngError::MissingPalette));
}

#[test]
fn plte_size_rules() {
    let mk =
        |depth: u8, plte: &[u8]| build(&ihdr(1, 1, depth, 3, 0), &[chunk(b"PLTE", plte)], &[0, 0]);
    assert!(decode(&mk(8, &[1, 2, 3])).is_ok());
    assert_eq!(decode(&mk(8, &[])), Err(PngError::BadPalette));
    assert_eq!(decode(&mk(8, &[1, 2, 3, 4])), Err(PngError::BadPalette));
    assert_eq!(decode(&mk(8, &[1, 2])), Err(PngError::BadPalette));
    assert_eq!(
        decode(&mk(8, &vec![0u8; 257 * 3])),
        Err(PngError::BadPalette)
    );
    assert!(decode(&mk(8, &vec![0u8; 256 * 3])).is_ok());
    // 2-bit images can have at most 4 colours.
    assert!(decode(&mk(2, &[0u8; 4 * 3])).is_ok());
    assert_eq!(decode(&mk(2, &[0u8; 5 * 3])), Err(PngError::BadPalette));
    assert_eq!(decode(&mk(1, &[0u8; 3 * 3])), Err(PngError::BadPalette));
}

#[test]
fn plte_is_illegal_for_grey_and_ignored_for_rgb() {
    let pal = chunk(b"PLTE", &[1, 2, 3]);
    let v = build(&ihdr(1, 1, 8, 0, 0), core::slice::from_ref(&pal), &[0, 7]);
    assert_eq!(decode(&v), Err(PngError::BadPalette));
    let v = build(
        &ihdr(1, 1, 8, 4, 0),
        core::slice::from_ref(&pal),
        &[0, 7, 9],
    );
    assert_eq!(decode(&v), Err(PngError::BadPalette));
    let v = build(&ihdr(1, 1, 8, 2, 0), &[pal], &[0, 10, 20, 30]);
    assert_eq!(decode(&v).unwrap().get(0, 0), Some(rgba(10, 20, 30, 255)));
}

#[test]
fn trns_size_rules() {
    let plte = chunk(b"PLTE", &[1, 2, 3, 4, 5, 6]);
    let pal = |t: &[u8]| {
        build(
            &ihdr(2, 1, 8, 3, 0),
            &[plte.clone(), chunk(b"tRNS", t)],
            &[0, 0, 1],
        )
    };
    let img = decode(&pal(&[10])).unwrap();
    assert_eq!(img.pixels(), &[rgba(1, 2, 3, 10), rgba(4, 5, 6, 255)]);
    assert!(decode(&pal(&[10, 20])).is_ok());
    assert_eq!(decode(&pal(&[1, 2, 3])), Err(PngError::BadPalette)); // more alphas than colours
    let gray = |t: &[u8]| build(&ihdr(1, 1, 8, 0, 0), &[chunk(b"tRNS", t)], &[0, 5]);
    assert!(decode(&gray(&[0, 5])).is_ok());
    assert_eq!(decode(&gray(&[0])), Err(PngError::BadPalette));
    assert_eq!(decode(&gray(&[0, 5, 0])), Err(PngError::BadPalette));
    let rgb = |t: &[u8]| build(&ihdr(1, 1, 8, 2, 0), &[chunk(b"tRNS", t)], &[0, 1, 2, 3]);
    assert!(decode(&rgb(&[0, 1, 0, 2, 0, 3])).is_ok());
    assert_eq!(decode(&rgb(&[0, 1])), Err(PngError::BadPalette));
}

#[test]
fn color_key_makes_exact_matches_transparent() {
    let key = chunk(b"tRNS", &[0, 1, 0, 2, 0, 3]);
    let v = build(&ihdr(2, 1, 8, 2, 0), &[key], &[0, 1, 2, 3, 1, 2, 4]);
    let img = decode(&v).unwrap();
    assert_eq!(alpha(img.get(0, 0).unwrap()), 0);
    assert_eq!(img.get(1, 0), Some(rgba(1, 2, 4, 255)));
    // A 2-bit grey key only compares the low two bits.
    let key = chunk(b"tRNS", &[0xFF, 0xFE]);
    let v = build(&ihdr(4, 1, 2, 0, 0), &[key], &[0, 0b0001_1011]);
    let img = decode(&v).unwrap();
    let a: Vec<u8> = img.pixels().iter().map(|&p| alpha(p)).collect();
    assert_eq!(a, [255, 255, 0, 255]);
}

fn alpha(p: u32) -> u8 {
    image::alpha(p)
}

#[test]
fn trns_is_ignored_for_alpha_color_types() {
    let v = build(
        &ihdr(1, 1, 8, 6, 0),
        &[chunk(b"tRNS", &[0, 0])],
        &[0, 1, 2, 3, 200],
    );
    assert_eq!(decode(&v).unwrap().get(0, 0), Some(rgba(1, 2, 3, 200)));
}

#[test]
fn unknown_chunks() {
    // Ancillary (lowercase first letter): skipped.
    let v = build(
        &ihdr(1, 1, 8, 6, 0),
        &[chunk(b"teSt", &[1, 2, 3]), chunk(b"gAMA", &[0, 0, 0, 1])],
        &[0, 1, 2, 3, 4],
    );
    assert_eq!(decode(&v).unwrap().get(0, 0), Some(rgba(1, 2, 3, 4)));
    // Critical (uppercase first letter): an error.
    let v = build(
        &ihdr(1, 1, 8, 6, 0),
        &[chunk(b"TeSt", &[1, 2, 3])],
        &[0, 1, 2, 3, 4],
    );
    assert_eq!(decode(&v), Err(PngError::UnknownCriticalChunk));
}

// ---------------------------------------------------------------- IDAT structure

#[test]
fn missing_idat() {
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"IHDR", &ihdr(1, 1, 8, 6, 0)));
    v.extend(chunk(b"IEND", &[]));
    assert_eq!(decode(&v), Err(PngError::MissingIdat));
}

#[test]
fn idat_chunks_must_be_consecutive() {
    let z = zlib_compress(&[0, 1, 2, 3, 4]);
    let (a, b) = z.split_at(z.len() / 2);
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"IHDR", &ihdr(1, 1, 8, 6, 0)));
    v.extend(chunk(b"IDAT", a));
    v.extend(chunk(b"tEXt", b"k\0v"));
    v.extend(chunk(b"IDAT", b));
    v.extend(chunk(b"IEND", &[]));
    assert_eq!(decode(&v), Err(PngError::ChunkOrder));
}

#[test]
fn idat_split_at_every_byte_still_decodes() {
    let raw = rgba_raw(3, 3, |x, y| [x as u8, y as u8, 7, 255]);
    let z = zlib_compress(&raw);
    for split in 0..=z.len() {
        let (a, b) = z.split_at(split);
        let mut v = SIGNATURE.to_vec();
        v.extend(chunk(b"IHDR", &ihdr(3, 3, 8, 6, 0)));
        v.extend(chunk(b"IDAT", a)); // may be empty: a zero-length IDAT is legal
        v.extend(chunk(b"IDAT", b));
        v.extend(chunk(b"IEND", &[]));
        let img = decode(&v).unwrap_or_else(|e| panic!("split {split}: {e}"));
        assert_eq!(img.get(2, 2), Some(rgba(2, 2, 7, 255)));
    }
}

#[test]
fn too_little_image_data() {
    let raw = rgba_raw(4, 4, |_, _| [1, 2, 3, 4]);
    for cut in [1, 5, raw.len() / 2, raw.len() - 1] {
        let v = build(&ihdr(4, 4, 8, 6, 0), &[], &raw[..cut]);
        assert_eq!(decode(&v), Err(PngError::ImageDataTooShort), "cut {cut}");
    }
    let v = build(&ihdr(4, 4, 8, 6, 0), &[], &[]);
    assert_eq!(decode(&v), Err(PngError::ImageDataTooShort));
}

#[test]
fn too_much_image_data() {
    let mut raw = rgba_raw(4, 4, |_, _| [1, 2, 3, 4]);
    raw.push(0);
    assert_eq!(
        decode(&build(&ihdr(4, 4, 8, 6, 0), &[], &raw)),
        Err(PngError::ImageDataTooLong)
    );
    raw.extend(vec![0u8; 1 << 20]);
    assert_eq!(
        decode(&build(&ihdr(4, 4, 8, 6, 0), &[], &raw)),
        Err(PngError::ImageDataTooLong)
    );
}

#[test]
fn interlaced_size_mismatch_is_detected_per_pass() {
    // A non-interlaced stream under an interlaced header is the wrong size.
    let raw = rgba_raw(5, 5, |_, _| [9, 9, 9, 9]);
    let v = build(&ihdr(5, 5, 8, 6, 1), &[], &raw);
    // The pixel bytes get read as filter bytes (or the stream length is off):
    // whichever check fires first, it must be an error.
    assert!(matches!(
        decode(&v),
        Err(PngError::ImageDataTooShort | PngError::ImageDataTooLong | PngError::BadFilter)
    ));
}

#[test]
fn invalid_filter_types() {
    for ft in [5u8, 6, 100, 255] {
        let mut raw = rgba_raw(2, 2, |_, _| [1, 2, 3, 4]);
        raw[9] = ft; // second row's filter byte
        assert_eq!(
            decode(&build(&ihdr(2, 2, 8, 6, 0), &[], &raw)),
            Err(PngError::BadFilter),
            "ft {ft}"
        );
    }
}

#[test]
fn palette_index_out_of_range() {
    let pal = chunk(b"PLTE", &[1, 2, 3, 4, 5, 6]);
    let v = build(
        &ihdr(2, 1, 8, 3, 0),
        core::slice::from_ref(&pal),
        &[0, 0, 1],
    );
    assert!(decode(&v).is_ok());
    let v = build(
        &ihdr(2, 1, 8, 3, 0),
        core::slice::from_ref(&pal),
        &[0, 0, 2],
    );
    assert_eq!(decode(&v), Err(PngError::BadPaletteIndex));
    // Packed: 2-bit index 3 with only 2 entries.
    let v = build(&ihdr(2, 1, 2, 3, 0), &[pal], &[0, 0b0011_0000]);
    assert_eq!(decode(&v), Err(PngError::BadPaletteIndex));
}

#[test]
fn corrupt_zlib_wrapper() {
    let raw = rgba_raw(2, 2, |_, _| [1, 2, 3, 4]);
    let good = build(&ihdr(2, 2, 8, 6, 0), &[], &raw);
    let (pos, len) = first_chunk_of(&good, b"IDAT");
    // Header.
    let mut v = good.clone();
    v[pos + 8] = 0x79;
    fix_crcs(&mut v);
    assert_eq!(
        decode(&v),
        Err(PngError::Inflate(InflateError::BadZlibHeader))
    );
    // Adler-32.
    let mut v = good.clone();
    v[pos + 8 + len - 1] ^= 1;
    fix_crcs(&mut v);
    assert_eq!(
        decode(&v),
        Err(PngError::Inflate(InflateError::ChecksumMismatch))
    );
    // Truncated IDAT payload (CRC repaired).
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"IHDR", &ihdr(2, 2, 8, 6, 0)));
    let z = zlib_compress(&raw);
    v.extend(chunk(b"IDAT", &z[..z.len() - 6]));
    v.extend(chunk(b"IEND", &[]));
    assert!(matches!(
        decode(&v),
        Err(PngError::Inflate(_) | PngError::ImageDataTooShort)
    ));
}

#[test]
fn trailing_bytes_inside_idat_after_the_zlib_stream_are_ignored() {
    let raw = rgba_raw(2, 2, |x, y| [x as u8, y as u8, 3, 4]);
    let mut z = zlib_compress(&raw);
    z.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"IHDR", &ihdr(2, 2, 8, 6, 0)));
    v.extend(chunk(b"IDAT", &z));
    v.extend(chunk(b"IEND", &[]));
    assert_eq!(decode(&v).unwrap().get(1, 1), Some(rgba(1, 1, 3, 4)));
}

#[test]
fn stored_zlib_blocks_decode() {
    let raw = rgba_raw(3, 2, |x, y| [x as u8 * 50, y as u8 * 90, 1, 255]);
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"IHDR", &ihdr(3, 2, 8, 6, 0)));
    v.extend(chunk(b"IDAT", &zlib_stored(&raw)));
    v.extend(chunk(b"IEND", &[]));
    assert_eq!(decode(&v).unwrap().get(2, 1), Some(rgba(100, 90, 1, 255)));
}

#[test]
fn decompression_bomb_inside_idat_is_cut_off() {
    // Header says 8x8; the stream inflates to 8 MiB of zeros.
    let raw = vec![0u8; 8 << 20];
    let v = build(&ihdr(8, 8, 8, 6, 0), &[], &raw);
    assert_eq!(decode(&v), Err(PngError::ImageDataTooLong));
}

// ---------------------------------------------------------------- unfilter

#[test]
fn unfilter_none_sub_up() {
    let mut r = [1u8, 2, 3, 4];
    unfilter(0, &mut r, &[9; 4], 1).unwrap();
    assert_eq!(r, [1, 2, 3, 4]);
    let mut r = [1u8, 1, 1, 1];
    unfilter(1, &mut r, &[0; 4], 1).unwrap();
    assert_eq!(r, [1, 2, 3, 4]);
    let mut r = [1u8, 1, 1, 1, 1, 1];
    unfilter(1, &mut r, &[0; 6], 3).unwrap();
    assert_eq!(r, [1, 1, 1, 2, 2, 2]);
    let mut r = [1u8, 2, 3];
    unfilter(2, &mut r, &[10, 20, 30], 1).unwrap();
    assert_eq!(r, [11, 22, 33]);
    // Wrapping arithmetic.
    let mut r = [250u8, 10];
    unfilter(2, &mut r, &[10, 250], 1).unwrap();
    assert_eq!(r, [4, 4]);
}

#[test]
fn unfilter_average_values_by_hand() {
    // x0 = 10 + (0 + 20)/2 = 20 ; x1 = 10 + (20 + 20)/2 = 30 ; x2 = 10 + (30 + 20)/2 = 35
    let mut r = [10u8, 10, 10];
    unfilter(3, &mut r, &[20, 20, 20], 1).unwrap();
    assert_eq!(r, [20, 30, 35]);
    // Sum of left and up above 255 must not overflow: (255 + 255) / 2 = 255: x1 = 0 + (127 + 255) / 2 = 191.
    let mut r = [0u8, 0];
    unfilter(3, &mut r, &[255, 255], 1).unwrap();
    assert_eq!(r, [127, 191]);
}

#[test]
fn paeth_predictor_known_cases() {
    assert_eq!(paeth(0, 0, 0), 0);
    assert_eq!(paeth(10, 20, 10), 20); // p = 20: closest to b
    assert_eq!(paeth(20, 10, 10), 20); // p = 20: closest to a
    assert_eq!(paeth(10, 10, 20), 10); // p = 0: a and b tie (10), a wins
    assert_eq!(paeth(100, 50, 75), 75); // p = 75 exactly c
    assert_eq!(paeth(255, 0, 255), 0);
    assert_eq!(paeth(0, 255, 0), 255);
    assert_eq!(paeth(7, 7, 7), 7);
}

#[test]
fn unfilter_paeth_first_row_and_with_history() {
    // First row (prev all zero) degenerates to Sub.
    let mut r = [1u8, 1, 1, 1];
    unfilter(4, &mut r, &[0; 4], 1).unwrap();
    assert_eq!(r, [1, 2, 3, 4]);
    // With a previous row, bpp 2.
    let mut r = [5u8, 5, 5, 5, 5, 5];
    let prev = [10u8, 20, 30, 40, 50, 60];
    unfilter(4, &mut r, &prev, 2).unwrap();
    assert_eq!(r[0], 15);
    assert_eq!(r[1], 25);
    let p2 = paeth(r[0], prev[2], prev[0]);
    assert_eq!(r[2], 5u8.wrapping_add(p2));
}

#[test]
fn unfilter_rejects_unknown_types_and_handles_tiny_rows() {
    let mut r = [0u8; 3];
    assert_eq!(unfilter(5, &mut r, &[0; 3], 1), Err(PngError::BadFilter));
    // A row shorter than bpp (e.g. a 1-pixel RGBA8 row has len 4, bpp 4).
    for ft in 0..5u8 {
        let mut r = [1u8, 2, 3, 4];
        unfilter(ft, &mut r, &[10, 20, 30, 40], 4).unwrap();
    }
    let mut r = [9u8];
    unfilter(1, &mut r, &[0], 4).unwrap();
    assert_eq!(r, [9]);
    unfilter(3, &mut r, &[2], 4).unwrap();
    unfilter(4, &mut r, &[2], 4).unwrap();
    let mut empty: [u8; 0] = [];
    unfilter(4, &mut empty, &[], 1).unwrap();
}

// ---------------------------------------------------------------- geometry and conversion

#[test]
fn adam7_passes_cover_every_pixel_exactly_once() {
    for w in 1..=40usize {
        for h in 1..=40usize {
            let (ps, n) = passes(w, h, true);
            assert_eq!(n, 7);
            let mut seen = vec![0u8; w * h];
            for p in ps.iter().take(n) {
                for j in 0..p.h {
                    for i in 0..p.w {
                        seen[(p.y0 + j * p.dy) * w + p.x0 + i * p.dx] += 1;
                    }
                }
            }
            assert!(seen.iter().all(|&c| c == 1), "{w}x{h}");
        }
    }
}

#[test]
fn non_interlaced_is_a_single_pass_and_tiny_images_skip_passes() {
    let (ps, n) = passes(5, 3, false);
    assert_eq!((n, ps[0].w, ps[0].h, ps[0].dx), (1, 5, 3, 1));
    // 1x1: only pass 1 has pixels.
    let (ps, n) = passes(1, 1, true);
    let nonempty: Vec<bool> = ps.iter().take(n).map(|p| p.w > 0 && p.h > 0).collect();
    assert_eq!(nonempty, [true, false, false, false, false, false, false]);
}

#[test]
fn raw_size_formula() {
    let h = |w, hh, d, ct, il| Header {
        width: w,
        height: hh,
        bit_depth: d,
        color_type: parse_ihdr(&ihdr(w, hh, d, ct, il)).unwrap().color_type,
        interlaced: il == 1,
    };
    assert_eq!(raw_size(&h(4, 4, 8, 6, 0)), 4 * (1 + 16));
    assert_eq!(raw_size(&h(3, 2, 8, 2, 0)), 2 * (1 + 9));
    assert_eq!(raw_size(&h(9, 2, 1, 0, 0)), 2 * (1 + 2));
    assert_eq!(raw_size(&h(5, 5, 16, 6, 0)), 5 * (1 + 40));
    // Adam7 on 1x1 is a single 1-pixel row.
    assert_eq!(raw_size(&h(1, 1, 8, 6, 1)), 1 + 4);
    // Interlacing adds filter bytes, never removes pixels.
    assert!(raw_size(&h(8, 8, 8, 6, 1)) > raw_size(&h(8, 8, 8, 6, 0)));
}

#[test]
fn sixteen_to_eight_bit_rounding() {
    assert_eq!(s16(0), 0);
    assert_eq!(s16(65535), 255);
    assert_eq!(s16(128), 0);
    assert_eq!(s16(129), 1);
    assert_eq!(s16(257), 1);
    assert_eq!(s16(0x8080), 128);
    for v in 0..=255u32 {
        assert_eq!(s16((v * 257) as u16) as u32, v); // exact for replicated bytes
    }
}

#[test]
fn packed_samples() {
    assert_eq!(packed_sample(&[0b1010_0101], 0, 1), 1);
    assert_eq!(packed_sample(&[0b1010_0101], 1, 1), 0);
    assert_eq!(packed_sample(&[0b1010_0101], 7, 1), 1);
    assert_eq!(packed_sample(&[0b1110_0100], 0, 2), 3);
    assert_eq!(packed_sample(&[0b1110_0100], 1, 2), 2);
    assert_eq!(packed_sample(&[0b1110_0100], 3, 2), 0);
    assert_eq!(packed_sample(&[0xAB, 0xCD], 0, 4), 0xA);
    assert_eq!(packed_sample(&[0xAB, 0xCD], 1, 4), 0xB);
    assert_eq!(packed_sample(&[0xAB, 0xCD], 3, 4), 0xD);
    assert_eq!(packed_sample(&[], 5, 4), 0); // out of range reads as 0
}

#[test]
fn grey_levels_scale_to_full_range() {
    for (depth, max) in [(1u8, 1u8), (2, 3), (4, 15)] {
        // One pixel with the maximum sample value.
        let byte = ((1u16 << depth) as u8).wrapping_sub(1) << (8 - depth);
        let v = build(&ihdr(1, 1, depth, 0, 0), &[], &[0, byte]);
        assert_eq!(
            decode(&v).unwrap().get(0, 0),
            Some(0xFFFF_FFFF),
            "depth {depth} max {max}"
        );
        let v = build(&ihdr(1, 1, depth, 0, 0), &[], &[0, 0]);
        assert_eq!(decode(&v).unwrap().get(0, 0), Some(0xFF00_0000));
    }
}

// ---------------------------------------------------------------- encoder

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

#[test]
fn encode_decode_roundtrip_opaque_and_alpha() {
    for (w, h) in [(1, 1), (2, 1), (1, 2), (7, 5), (16, 16), (33, 17), (100, 3)] {
        for alpha in [false, true] {
            let img = sample(w, h, alpha);
            let png = encode(&img).unwrap();
            assert_eq!(decode(&png).unwrap(), img, "{w}x{h} alpha {alpha}");
            let png = encode_rgba(&img).unwrap();
            assert_eq!(decode(&png).unwrap(), img, "{w}x{h} alpha {alpha} (rgba)");
        }
    }
}

#[test]
fn encode_picks_rgb_for_opaque_images_and_rgba_otherwise() {
    let opaque = sample(8, 8, false);
    let h = read_header(&encode(&opaque).unwrap()).unwrap();
    assert_eq!(
        (h.bit_depth, h.color_type, h.interlaced),
        (8, ColorType::Rgb, false)
    );
    let h = read_header(&encode_rgba(&opaque).unwrap()).unwrap();
    assert_eq!(h.color_type, ColorType::Rgba);
    let h = read_header(&encode(&sample(8, 8, true)).unwrap()).unwrap();
    assert_eq!(h.color_type, ColorType::Rgba);
}

#[test]
fn encoded_png_has_valid_structure() {
    let png = encode(&sample(20, 10, true)).unwrap();
    assert_eq!(&png[..8], &SIGNATURE);
    let mut pos = 8;
    let mut types = Vec::new();
    while pos < png.len() {
        let c = next_chunk(&png, &mut pos).unwrap(); // verifies every CRC
        types.push(c.ty);
    }
    assert_eq!(types.first(), Some(b"IHDR"));
    assert_eq!(types.last(), Some(b"IEND"));
    assert!(types.iter().any(|t| t == b"IDAT"));
    assert_eq!(pos, png.len());
}

#[test]
fn flat_images_compress_well_and_gradients_use_several_filters() {
    let flat = Image::new(256, 256, 0xFF20_4060).unwrap();
    let png = encode(&flat).unwrap();
    assert!(png.len() < 2500, "{} bytes for a flat 256x256", png.len());
    // A smooth gradient: filtering should help, and the filter bytes vary.
    let mut px = Vec::new();
    for y in 0..64u32 {
        for x in 0..64u32 {
            px.push(rgba((x * 4) as u8, (y * 4) as u8, ((x + y) * 2) as u8, 255));
        }
    }
    let img = Image::from_pixels(64, 64, px).unwrap();
    let png = encode(&img).unwrap();
    let (pos, len) = first_chunk_of(&png, b"IDAT");
    let raw = zlib_decompress(&png[pos + 8..pos + 8 + len], 1 << 20).unwrap();
    let stride = 1 + 64 * 3;
    let mut used = [false; 5];
    for r in 0..64 {
        used[raw[r * stride] as usize] = true;
    }
    assert!(used.iter().filter(|&&u| u).count() >= 1);
    assert!(png.len() < 64 * 64 * 3 / 2, "{} bytes", png.len());
    assert_eq!(decode(&png).unwrap(), img);
}

#[test]
fn encoder_mixes_filters_across_rows_with_different_content() {
    // Rows with different statistics should make the heuristic pick several filters.
    let w = 64usize;
    let mut px = Vec::new();
    for y in 0..40usize {
        for x in 0..w {
            let v = match y % 5 {
                0 => (x * 3) as u8,                   // horizontal ramp -> Sub
                1 => 90,                              // same as previous row -> Up
                2 => ((x * 7 + y * 13) % 251) as u8,  // noisy
                3 => (x as u8).wrapping_mul(y as u8), // structured noise
                _ => ((x + y) * 4) as u8,             // diagonal ramp
            };
            px.push(rgba(v, v.wrapping_add(40), v / 2, 255));
        }
    }
    let img = Image::from_pixels(w, 40, px).unwrap();
    let png = encode(&img).unwrap();
    let (pos, len) = first_chunk_of(&png, b"IDAT");
    let raw = zlib_decompress(&png[pos + 8..pos + 8 + len], 1 << 20).unwrap();
    let stride = 1 + w * 3;
    let mut used = [false; 5];
    for r in 0..40 {
        used[raw[r * stride] as usize] = true;
    }
    assert!(
        used.iter().filter(|&&u| u).count() >= 2,
        "filters used: {used:?}"
    );
    assert_eq!(decode(&png).unwrap(), img);
}

#[test]
fn large_image_roundtrip_and_multiple_idat_chunks() {
    let img = sample(300, 200, true);
    let png = encode(&img).unwrap();
    assert_eq!(decode(&png).unwrap(), img);
    // Incompressible-ish content > 64 KiB forces several IDAT chunks.
    let mut x = 12345u32;
    let px: Vec<u32> = (0..200 * 200)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x | 0xFF00_0000
        })
        .collect();
    let noisy = Image::from_pixels(200, 200, px).unwrap();
    let png = encode(&noisy).unwrap();
    let mut pos = 8;
    let mut idats = 0;
    while pos < png.len() {
        if &next_chunk(&png, &mut pos).unwrap().ty == b"IDAT" {
            idats += 1;
        }
    }
    assert!(idats >= 2, "{idats} IDAT chunks");
    assert_eq!(decode(&png).unwrap(), noisy);
}

#[test]
fn encoded_vectors_roundtrip_through_the_decoder() {
    for v in [
        IM_RGB8,
        IM_RGBA8,
        IM_PAL8_TRNS,
        IM_GRAY16,
        CR_RGBA8_ADAM7_FILTERS,
        CR_GRAY8_KEY,
        IM_RGB16,
    ] {
        let img = decode(&unhex(v.0)).unwrap();
        assert_eq!(decode(&encode(&img).unwrap()).unwrap(), img);
        assert_eq!(decode(&encode_rgba(&img).unwrap()).unwrap(), img);
    }
}

#[test]
fn error_messages_are_nonempty() {
    use alloc::string::ToString;
    for e in [
        PngError::BadSignature,
        PngError::Truncated,
        PngError::BadChunk,
        PngError::CrcMismatch,
        PngError::MissingIhdr,
        PngError::BadIhdr,
        PngError::BadDimensions,
        PngError::BadPalette,
        PngError::MissingPalette,
        PngError::MissingIdat,
        PngError::ChunkOrder,
        PngError::UnknownCriticalChunk,
        PngError::Inflate(InflateError::Truncated),
        PngError::ImageDataTooShort,
        PngError::ImageDataTooLong,
        PngError::BadFilter,
        PngError::BadPaletteIndex,
        PngError::Image(ImageError::TooLarge),
    ] {
        assert!(!e.to_string().is_empty());
    }
    assert_eq!(
        PngError::from(InflateError::Truncated),
        PngError::Inflate(InflateError::Truncated)
    );
    assert_eq!(
        PngError::from(ImageError::ZeroSize),
        PngError::Image(ImageError::ZeroSize)
    );
}

#[test]
fn encoder_uses_paeth_on_planar_content() {
    // v = 3x + 5y is predicted exactly by Paeth: it must beat Sub/Up/Average.
    let (w, h) = (48usize, 24usize);
    let px: Vec<u32> = (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| {
                let v = (3 * x + 5 * y) as u8;
                rgba(v, v, v, 255)
            })
        })
        .collect();
    let img = Image::from_pixels(w, h, px).unwrap();
    let png = encode(&img).unwrap();
    let (pos, len) = first_chunk_of(&png, b"IDAT");
    let raw = zlib_decompress(&png[pos + 8..pos + 8 + len], 1 << 20).unwrap();
    let stride = 1 + w * 3;
    let paeth_rows = (1..h).filter(|&r| raw[r * stride] == 4).count();
    assert!(paeth_rows >= h / 2, "only {paeth_rows} Paeth rows");
    assert_eq!(decode(&png).unwrap(), img);
}
