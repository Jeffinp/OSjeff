use super::*;

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
