use super::vectors::*;
use super::*;
use crate::format::image::{alpha, channels};
use crate::testutil::unhex;
use alloc::vec;
use alloc::vec::Vec;

type Vector = (&'static str, usize, usize, &'static str);

fn check(v: Vector) {
    let (file, w, h, expect) = v;
    let img = decode(&unhex(file)).unwrap();
    assert_eq!((img.width(), img.height()), (w, h));
    assert_eq!(img.to_rgba().unwrap(), unhex(expect));
}

fn set32(v: &mut [u8], off: usize, x: u32) {
    v[off..off + 4].copy_from_slice(&x.to_le_bytes());
}

fn set16(v: &mut [u8], off: usize, x: u16) {
    v[off..off + 2].copy_from_slice(&x.to_le_bytes());
}

fn sample(w: usize, h: usize, with_alpha: bool) -> Image {
    let mut px = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let v = (x * 29 + y * 53 + x * y * 3) as u32;
            let a = if with_alpha {
                (v * 11 % 256) as u8
            } else {
                255
            };
            px.push(rgba(
                (v * 3) as u8,
                (v * 7 + 1) as u8,
                (v * 13 + 5) as u8,
                a,
            ));
        }
    }
    Image::from_pixels(w, h, px).unwrap()
}

// ---------------------------------------------------------------- vectors

#[test]
fn imagemagick_1bpp() {
    check(IM_1BPP);
}

#[test]
fn imagemagick_4bpp() {
    check(IM_4BPP);
}

#[test]
fn imagemagick_8bpp_palette() {
    check(IM_8BPP);
}

#[test]
fn imagemagick_24bpp_with_row_padding() {
    check(IM_24BPP); // width 5: 15 bytes per row padded to 16
}

#[test]
fn imagemagick_32bpp_v5_header_keeps_alpha() {
    check(IM_32BPP);
    let img = decode(&unhex(IM_32BPP.0)).unwrap();
    assert!(!img.is_opaque());
}

#[test]
fn top_down_negative_height() {
    check(TD_24BPP);
}

#[test]
fn os2_core_header_24bpp_and_8bpp() {
    check(CORE_24BPP);
    check(CORE_8BPP);
}

#[test]
fn sixteen_bit_555_and_565() {
    check(RGB555);
    check(BITFIELDS565);
}

#[test]
fn thirty_two_bit_alpha_heuristic() {
    check(RGB32_NOALPHA); // alpha bytes all zero -> opaque
    check(RGB32_ALPHA);
    assert!(decode(&unhex(RGB32_NOALPHA.0)).unwrap().is_opaque());
}

#[test]
fn v4_and_v5_headers() {
    check(V4_24BPP);
    check(V5_32BPP);
}

#[test]
fn palette_with_clr_used_and_out_of_range_index() {
    check(PAL8_CLRUSED3);
}

#[test]
fn one_bit_custom_palette_width_not_a_multiple_of_eight() {
    check(PAL1_CUSTOM);
}

#[test]
fn rle8_encoded_absolute_delta_and_untouched_pixels() {
    check(RLE8_MIXED);
    let img = decode(&unhex(RLE8_MIXED.0)).unwrap();
    // Pixels the stream never wrote are transparent.
    assert_eq!(img.get(0, 0), Some(0));
    assert_eq!(img.get(3, 1), Some(0));
}

#[test]
fn rle4_encoded_and_absolute_runs() {
    check(RLE4_MIXED);
}

// ---------------------------------------------------------------- header errors

#[test]
fn empty_and_tiny_inputs() {
    assert_eq!(decode(&[]), Err(BmpError::Truncated));
    assert_eq!(decode(b"B"), Err(BmpError::Truncated));
    assert_eq!(decode(b"BM"), Err(BmpError::Truncated));
    assert_eq!(
        decode(&[b'B', b'M', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 40]),
        Err(BmpError::Truncated)
    );
}

#[test]
fn bad_signature() {
    let mut v = unhex(IM_24BPP.0);
    v[0] = b'X';
    assert_eq!(decode(&v), Err(BmpError::BadSignature));
    assert_eq!(
        decode(b"\x89PNG\r\n\x1a\n........."),
        Err(BmpError::BadSignature)
    );
}

#[test]
fn unsupported_header_sizes() {
    for size in [0, 1, 11, 13, 16, 39, 41, 64, 100, 125, 0xFFFF_FFFF] {
        let mut v = unhex(IM_24BPP.0);
        set32(&mut v, 14, size);
        assert_eq!(decode(&v), Err(BmpError::UnsupportedHeader), "size {size}");
    }
}

#[test]
fn bad_dimensions() {
    let base = unhex(IM_24BPP.0);
    for (w, h) in [(0i32, 3i32), (-5, 3), (5, 0), (i32::MIN, 3)] {
        let mut v = base.clone();
        set32(&mut v, 18, w as u32);
        set32(&mut v, 22, h as u32);
        assert_eq!(decode(&v), Err(BmpError::BadDimensions), "{w}x{h}");
    }
}

#[test]
fn planes_must_be_one() {
    let mut v = unhex(IM_24BPP.0);
    set16(&mut v, 26, 2);
    assert_eq!(decode(&v), Err(BmpError::BadHeader));
}

#[test]
fn unsupported_depths_and_compressions() {
    for bpp in [0u16, 2, 3, 15, 48, 64] {
        let mut v = unhex(IM_24BPP.0);
        set16(&mut v, 28, bpp);
        assert_eq!(decode(&v), Err(BmpError::UnsupportedBitDepth), "bpp {bpp}");
    }
    for comp in [4u32, 5, 7, 100] {
        let mut v = unhex(IM_24BPP.0);
        set32(&mut v, 30, comp);
        assert_eq!(
            decode(&v),
            Err(BmpError::UnsupportedCompression),
            "comp {comp}"
        );
    }
    // RLE8 needs 8 bpp, RLE4 needs 4 bpp, bitfields need 16/32.
    let mut v = unhex(IM_24BPP.0);
    set32(&mut v, 30, 1);
    assert_eq!(decode(&v), Err(BmpError::UnsupportedBitDepth));
    let mut v = unhex(IM_24BPP.0);
    set32(&mut v, 30, 3);
    assert_eq!(decode(&v), Err(BmpError::UnsupportedBitDepth));
}

#[test]
fn pixel_offset_must_be_inside_the_file() {
    let mut v = unhex(IM_24BPP.0);
    let len = v.len() as u32;
    set32(&mut v, 10, len + 1);
    assert_eq!(decode(&v), Err(BmpError::Truncated));
    set32(&mut v, 10, 20); // inside the DIB header
    assert_eq!(decode(&v), Err(BmpError::BadHeader));
    set32(&mut v, 10, u32::MAX);
    assert_eq!(decode(&v), Err(BmpError::Truncated));
}

#[test]
fn palette_must_fit_and_not_overlap_the_pixels() {
    let mut v = unhex(IM_8BPP.0);
    set32(&mut v, 46, 257); // more colours than 8 bpp can index
    assert_eq!(decode(&v), Err(BmpError::BadHeader));
    let mut v = unhex(IM_8BPP.0);
    set32(&mut v, 10, 60); // pixel array starts inside the palette
    assert_eq!(decode(&v), Err(BmpError::BadHeader));
    let short = &unhex(IM_8BPP.0)[..100]; // file ends inside the palette
    assert_eq!(decode(short), Err(BmpError::Truncated));
}

#[test]
fn bad_bitfield_masks() {
    let base = unhex(BITFIELDS565.0);
    // Masks live right after the 40-byte header, at file offset 54.
    for (r, g, b) in [
        (0u32, 0x07E0u32, 0x1Fu32),
        (0xF800, 0, 0x1F),
        (0xF800, 0x07E0, 0),
        (0xF81F, 0x07E0, 0x1F0),
        (0x1_F800, 0x07E0, 0x1F),
    ] {
        let mut v = base.clone();
        set32(&mut v, 54, r);
        set32(&mut v, 58, g);
        set32(&mut v, 62, b);
        assert_eq!(
            decode(&v),
            Err(BmpError::BadBitfields),
            "{r:#x} {g:#x} {b:#x}"
        );
    }
    // A truncated mask block.
    assert_eq!(decode(&base[..60]), Err(BmpError::Truncated));
}

#[test]
fn alpha_bitfields_compression_reads_four_masks() {
    // Rebuild BITFIELDS565 as BI_ALPHABITFIELDS (6) with an extra alpha mask.
    let base = unhex(BITFIELDS565.0);
    let (hdr, pix) = base.split_at(54 + 12);
    let mut v = hdr.to_vec();
    set32(&mut v, 30, 6);
    v.extend_from_slice(&0u32.to_le_bytes()); // alpha mask = none
    v.extend_from_slice(pix);
    set32(&mut v, 10, 54 + 16);
    let img = decode(&v).unwrap();
    assert_eq!(img.to_rgba().unwrap(), unhex(BITFIELDS565.3));
}

#[test]
fn custom_masks_xrgb_2101010_style_widening() {
    // 32 bpp, R=10 bits, G=10, B=10 (alpha none): the top 8 bits are taken.
    let mut v = unhex(IM_32BPP.0);
    set32(&mut v, 30, 3);
    set32(&mut v, 54, 0x3FF0_0000);
    set32(&mut v, 58, 0x000F_FC00);
    set32(&mut v, 62, 0x0000_03FF);
    set32(&mut v, 66, 0);
    let w = 5usize;
    let off = u32::from_le_bytes([v[10], v[11], v[12], v[13]]) as usize;
    // Bottom row, first pixel: r = 1023, g = 512, b = 0.
    let px: u32 = (1023 << 20) | (512 << 10);
    v[off..off + 4].copy_from_slice(&px.to_le_bytes());
    let img = decode(&v).unwrap();
    assert_eq!(img.get(0, 2), Some(rgba(255, 128, 0, 255)));
    assert_eq!(img.width(), w);
}

#[test]
fn narrow_masks_replicate_bits() {
    // 16 bpp 4-4-4 with no alpha: 0xF -> 0xFF, 0x8 -> 0x88, 0x3 -> 0x33.
    let mut v = unhex(BITFIELDS565.0);
    set32(&mut v, 54, 0x0F00);
    set32(&mut v, 58, 0x00F0);
    set32(&mut v, 62, 0x000F);
    let off = u32::from_le_bytes([v[10], v[11], v[12], v[13]]) as usize;
    v[off..off + 2].copy_from_slice(&0x0F83u16.to_le_bytes());
    let img = decode(&v).unwrap();
    assert_eq!(img.get(0, 2), Some(rgba(0xFF, 0x88, 0x33, 255)));
}

// ---------------------------------------------------------------- size and truncation

#[test]
fn huge_dimensions_are_rejected_before_allocating() {
    let mut v = unhex(IM_24BPP.0);
    set32(&mut v, 18, 0x7FFF_FFFF);
    set32(&mut v, 22, 0x7FFF_FFFF);
    assert_eq!(decode(&v), Err(BmpError::Image(ImageError::TooLarge)));
    set32(&mut v, 18, 100_000);
    set32(&mut v, 22, 100_000);
    assert_eq!(decode(&v), Err(BmpError::Image(ImageError::TooLarge)));
    set32(&mut v, 22, (-100_000i32) as u32);
    assert_eq!(decode(&v), Err(BmpError::Image(ImageError::TooLarge)));
}

#[test]
fn plausible_dimensions_but_missing_pixels_are_truncated() {
    // 4096 x 4096 would be 64 MiB of pixels; the file has 48 bytes of them.
    let mut v = unhex(IM_24BPP.0);
    set32(&mut v, 18, 4096);
    set32(&mut v, 22, 4096);
    assert_eq!(decode(&v), Err(BmpError::Truncated));
    for bpp_comp in [(1u16, 0u32), (4, 0), (8, 0), (16, 0), (32, 0)] {
        let mut v = unhex(IM_24BPP.0);
        set32(&mut v, 18, 3000);
        set32(&mut v, 22, 3000);
        set16(&mut v, 28, bpp_comp.0);
        // (palette/offset errors may fire first; either way it is an error)
        assert!(decode(&v).is_err());
    }
}

#[test]
fn every_prefix_of_every_vector_is_an_error() {
    for v in [
        IM_1BPP,
        IM_4BPP,
        IM_24BPP,
        IM_32BPP,
        TD_24BPP,
        CORE_24BPP,
        RGB555,
        V4_24BPP,
        PAL1_CUSTOM,
    ] {
        let file = unhex(v.0);
        for cut in 0..file.len() {
            assert!(
                decode(&file[..cut]).is_err(),
                "cut {cut} of {} bytes",
                file.len()
            );
        }
    }
}

#[test]
fn rle_prefixes_never_panic() {
    for v in [RLE8_MIXED, RLE4_MIXED] {
        let file = unhex(v.0);
        for cut in 0..file.len() {
            let _ = decode(&file[..cut]);
        }
    }
}

#[test]
fn every_bit_flip_never_panics() {
    for v in [
        IM_4BPP,
        IM_24BPP,
        V5_32BPP,
        RLE8_MIXED,
        RLE4_MIXED,
        BITFIELDS565,
        CORE_8BPP,
    ] {
        let file = unhex(v.0);
        for bit in 0..file.len() * 8 {
            let mut f = file.clone();
            f[bit / 8] ^= 1 << (bit % 8);
            let _ = decode(&f);
        }
    }
}

#[test]
fn garbage_after_a_valid_header_never_panics() {
    let mut x = 0x9E37_79B9u32;
    let head = unhex(IM_24BPP.0);
    for round in 0..200 {
        let mut f = head[..54].to_vec();
        for _ in 0..(round * 3) {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            f.push(x as u8);
        }
        let _ = decode(&f);
        // And with random header bytes too.
        for b in f.iter_mut().take(54).skip(2) {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            if x & 7 == 0 {
                *b = x as u8;
            }
        }
        let _ = decode(&f);
    }
}

// ---------------------------------------------------------------- RLE details

fn rle8_file(w: i32, h: i32, rle: &[u8]) -> Vec<u8> {
    let mut v = unhex(IM_8BPP.0); // a valid 8bpp header + 256-entry palette
    let off = u32::from_le_bytes([v[10], v[11], v[12], v[13]]) as usize;
    v.truncate(off);
    set32(&mut v, 18, w as u32);
    set32(&mut v, 22, h as u32);
    set32(&mut v, 30, 1);
    v.extend_from_slice(rle);
    v
}

#[test]
fn rle_without_end_of_bitmap_is_accepted() {
    let f = rle8_file(3, 1, &[3, 7]);
    let img = decode(&f).unwrap();
    assert_eq!(img.pixels().len(), 3);
    assert_eq!(img.get(0, 0), img.get(2, 0));
    assert_ne!(img.get(0, 0), Some(0));
}

#[test]
fn rle_truncated_commands_are_errors() {
    assert_eq!(decode(&rle8_file(3, 1, &[3])), Err(BmpError::Truncated)); // count without value
    assert_eq!(
        decode(&rle8_file(3, 1, &[0, 2, 1])),
        Err(BmpError::Truncated)
    ); // delta missing dy
    assert_eq!(
        decode(&rle8_file(3, 1, &[0, 5, 1, 2])),
        Err(BmpError::Truncated)
    ); // absolute run cut short
}

#[test]
fn rle_writes_outside_the_image_are_clipped() {
    // Run of 200 pixels in a 3-wide image, then a delta far to the right/up.
    let f = rle8_file(3, 2, &[200, 5, 0, 2, 250, 250, 4, 9, 0, 1]);
    let img = decode(&f).unwrap();
    assert_eq!(img.pixels().len(), 6);
    // Bottom row is filled, top row untouched (delta moved past the image).
    assert_ne!(img.get(0, 1), Some(0));
    assert_eq!(img.get(0, 0), Some(0));
}

#[test]
fn rle_stops_after_the_last_row() {
    let f = rle8_file(2, 1, &[2, 4, 0, 0, 99, 9, 0, 1]);
    let img = decode(&f).unwrap();
    assert_eq!(img.pixels().len(), 2);
    assert_ne!(img.get(1, 0), Some(0));
}

#[test]
fn rle_top_down_flips_the_row_order() {
    // Same stream decoded bottom-up and top-down must be vertical mirrors.
    let rle = [2, 1, 0, 0, 2, 2, 0, 1];
    let up = decode(&rle8_file(2, 2, &rle)).unwrap();
    let down = decode(&rle8_file(2, -2, &rle)).unwrap();
    for x in 0..2 {
        assert_eq!(up.get(x, 0), down.get(x, 1));
        assert_eq!(up.get(x, 1), down.get(x, 0));
    }
    assert_ne!(up.get(0, 0), up.get(0, 1));
}

#[test]
fn rle_absolute_run_padding_is_honoured() {
    // Absolute run of 3 (odd -> one pad byte), then a run of 2.
    let f = rle8_file(5, 1, &[0, 3, 1, 2, 3, 0, 2, 4, 0, 1]);
    let img = decode(&f).unwrap();
    let px = img.pixels();
    assert_eq!(px[3], px[4]);
    assert_ne!(px[0], px[1]);
    assert_ne!(px[1], px[2]);
    assert_ne!(px[2], px[3]);
}

#[test]
fn rle_cannot_make_a_giant_image_without_the_limit() {
    let f = rle8_file(0x7FFF_FFFF, 0x7FFF_FFFF, &[1, 1]);
    assert_eq!(decode(&f), Err(BmpError::Image(ImageError::TooLarge)));
}

// ---------------------------------------------------------------- encoders

#[test]
fn encode_24_layout_and_roundtrip() {
    let img = sample(5, 3, false);
    let bytes = encode_24(&img, 0).unwrap();
    assert_eq!(&bytes[..2], b"BM");
    assert_eq!(
        u32::from_le_bytes(bytes[2..6].try_into().unwrap()) as usize,
        bytes.len()
    );
    assert_eq!(u32::from_le_bytes(bytes[10..14].try_into().unwrap()), 54);
    assert_eq!(bytes.len(), 54 + 16 * 3); // 15 bytes + 1 pad per row
    assert_eq!(u16::from_le_bytes([bytes[28], bytes[29]]), 24);
    assert_eq!(decode(&bytes).unwrap(), img);
}

#[test]
fn encode_24_flattens_alpha_over_the_background() {
    let img = Image::from_pixels(2, 1, vec![0x80FF_FFFF, 0x0000_0000]).unwrap();
    let out = decode(&encode_24(&img, 0x0000_0000).unwrap()).unwrap();
    assert_eq!(out.get(0, 0), Some(rgba(128, 128, 128, 255)));
    assert_eq!(out.get(1, 0), Some(0xFF00_0000));
    let out = decode(&encode_24(&img, 0x00FF_0000).unwrap()).unwrap();
    assert_eq!(out.get(1, 0), Some(rgba(255, 0, 0, 255)));
}

#[test]
fn encode_32_keeps_alpha_exactly() {
    let img = sample(7, 4, true);
    let bytes = encode_32(&img).unwrap();
    assert_eq!(bytes.len(), 14 + 108 + 7 * 4 * 4);
    assert_eq!(u32::from_le_bytes(bytes[14..18].try_into().unwrap()), 108);
    assert_eq!(u16::from_le_bytes([bytes[28], bytes[29]]), 32);
    assert_eq!(decode(&bytes).unwrap(), img);
}

#[test]
fn encode_32_fully_transparent_image_survives_only_with_alpha_present() {
    // Documented caveat: an alpha channel that is zero everywhere reads back opaque.
    let img = Image::new(2, 2, 0x0012_3456).unwrap();
    let back = decode(&encode_32(&img).unwrap()).unwrap();
    assert!(back.is_opaque());
    assert_eq!(channels(back.get(0, 0).unwrap())[..3], [0x12, 0x34, 0x56]);
    // One non-zero alpha anywhere and the channel is honoured.
    let mut img = img;
    img.set(1, 1, 0x0112_3456);
    let back = decode(&encode_32(&img).unwrap()).unwrap();
    assert_eq!(alpha(back.get(0, 0).unwrap()), 0);
    assert_eq!(alpha(back.get(1, 1).unwrap()), 1);
}

#[test]
fn encode_roundtrip_many_widths() {
    for w in 1..=9 {
        let img = sample(w, 3, true);
        assert_eq!(
            decode(&encode_32(&img).unwrap()).unwrap(),
            img,
            "32bpp width {w}"
        );
        let opaque = sample(w, 5, false);
        assert_eq!(
            decode(&encode_24(&opaque, 0).unwrap()).unwrap(),
            opaque,
            "24bpp width {w}"
        );
    }
}

#[test]
fn encode_single_pixel_and_large_image() {
    let one = Image::new(1, 1, 0xFF01_0203).unwrap();
    assert_eq!(decode(&encode_24(&one, 0).unwrap()).unwrap(), one);
    let big = sample(300, 200, false);
    let bytes = encode_24(&big, 0).unwrap();
    assert_eq!(decode(&bytes).unwrap(), big);
    assert_eq!(decode(&encode_32(&big).unwrap()).unwrap(), big);
}

#[test]
fn imagemagick_files_survive_a_reencode() {
    for v in [IM_1BPP, IM_4BPP, IM_8BPP, IM_24BPP] {
        let img = decode(&unhex(v.0)).unwrap();
        assert_eq!(decode(&encode_24(&img, 0).unwrap()).unwrap(), img);
    }
    let img = decode(&unhex(IM_32BPP.0)).unwrap();
    assert_eq!(decode(&encode_32(&img).unwrap()).unwrap(), img);
}

#[test]
fn error_messages_are_nonempty() {
    use alloc::string::ToString;
    for e in [
        BmpError::Truncated,
        BmpError::BadSignature,
        BmpError::UnsupportedHeader,
        BmpError::BadHeader,
        BmpError::BadDimensions,
        BmpError::UnsupportedBitDepth,
        BmpError::UnsupportedCompression,
        BmpError::BadBitfields,
        BmpError::Image(ImageError::TooLarge),
    ] {
        assert!(!e.to_string().is_empty());
    }
    assert_eq!(
        BmpError::from(ImageError::ZeroSize),
        BmpError::Image(ImageError::ZeroSize)
    );
}
