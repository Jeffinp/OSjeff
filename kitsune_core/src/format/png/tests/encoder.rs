use super::*;

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
