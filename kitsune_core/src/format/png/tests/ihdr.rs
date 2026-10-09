use super::*;

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
