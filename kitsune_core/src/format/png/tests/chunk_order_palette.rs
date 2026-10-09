use super::*;

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
