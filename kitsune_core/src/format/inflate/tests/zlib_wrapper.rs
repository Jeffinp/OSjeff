use super::*;

#[test]
fn zlib_header_validation() {
    let ok = unhex(FIXED_ZLIB);
    // Truncated header.
    assert_eq!(zlib_decompress(&[], 10), Err(InflateError::Truncated));
    assert_eq!(zlib_decompress(&ok[..1], 10), Err(InflateError::Truncated));
    // Wrong method (CM != 8).
    let mut bad = ok.clone();
    bad[0] = 0x79;
    assert_eq!(zlib_decompress(&bad, 100), Err(InflateError::BadZlibHeader));
    // Window size above 32 KiB (CINFO = 8), with a consistent FCHECK.
    let cmf = 0x88u8;
    let mut flg = 0u8;
    while !(((cmf as u32) << 8) | flg as u32).is_multiple_of(31) {
        flg += 1;
    }
    let mut bad = ok.clone();
    bad[0] = cmf;
    bad[1] = flg;
    assert_eq!(zlib_decompress(&bad, 100), Err(InflateError::BadZlibHeader));
    // Bad FCHECK.
    let mut bad = ok.clone();
    bad[1] ^= 1;
    assert_eq!(zlib_decompress(&bad, 100), Err(InflateError::BadZlibHeader));
    // FDICT set (with a valid FCHECK).
    let mut flg = 0x20u8;
    while !((0x78u32 << 8) | flg as u32).is_multiple_of(31) {
        flg += 1;
    }
    let mut bad = ok;
    bad[1] = flg;
    assert_eq!(
        zlib_decompress(&bad, 100),
        Err(InflateError::DictionaryUnsupported)
    );
}

#[test]
fn zlib_every_header_bit_flip_is_rejected_or_harmless() {
    let ok = unhex(DYN_ZLIB);
    for bit in 0..16 {
        let mut z = ok.clone();
        z[bit / 8] ^= 1 << (bit % 8);
        // A single flipped header bit can never keep (CMF*256+FLG) % 31 == 0.
        assert!(zlib_decompress(&z, BIG).is_err(), "bit {bit}");
    }
}

#[test]
fn zlib_adler_mismatch_and_missing_trailer() {
    let ok = unhex(DYN_ZLIB);
    for k in 1..=4 {
        let mut z = ok.clone();
        let n = z.len();
        z[n - k] ^= 0x40;
        assert_eq!(
            zlib_decompress(&z, BIG),
            Err(InflateError::ChecksumMismatch),
            "byte -{k}"
        );
    }
    for cut in 1..=4 {
        assert_eq!(
            zlib_decompress(&ok[..ok.len() - cut], BIG),
            Err(InflateError::Truncated),
            "cut {cut}"
        );
    }
}

#[test]
fn zlib_trailing_garbage_is_ignored_and_reported() {
    let mut z = unhex(FIXED_ZLIB);
    let len = z.len();
    z.extend_from_slice(b"garbage after the stream");
    let mut inf = Inflater::new_zlib(&z, 100).unwrap();
    let mut buf = [0u8; 64];
    let n = inf.read(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello hello hello hello");
    assert_eq!(inf.read(&mut buf).unwrap(), 0);
    assert_eq!(inf.consumed(), len);
    assert_eq!(&z[inf.consumed()..], b"garbage after the stream");
    assert_eq!(
        zlib_decompress(&z, 100).unwrap(),
        b"hello hello hello hello"
    );
}

#[test]
fn raw_consumed_counts_the_exact_stream_length() {
    for (hex, expect_eq) in [
        (STORED_RAW, true),
        (FIXED_RAW, true),
        (DYN_RAW, true),
        (MULTI_RAW, true),
    ] {
        let r = unhex(hex);
        let mut padded = r.clone();
        padded.extend_from_slice(&[0xAA; 9]);
        let (_, used) = inflate_consumed(&padded, BIG).unwrap();
        assert_eq!(used == r.len(), expect_eq, "{hex}");
    }
}

#[test]
fn zlib_wrapper_checks_adler_over_streamed_reads() {
    // The checksum must cover every byte even when reads split mid-match.
    let data = window_data();
    let z = unhex(WINDOW_ZLIB);
    for chunk in [1, 13, 1000, 32768] {
        let inf = Inflater::new_zlib(&z, BIG).unwrap();
        assert_eq!(read_chunked(inf, chunk).unwrap(), data);
    }
    let mut bad = z.clone();
    let n = bad.len();
    bad[n - 2] ^= 0xFF;
    let inf = Inflater::new_zlib(&bad, BIG).unwrap();
    assert_eq!(read_chunked(inf, 7), Err(InflateError::ChecksumMismatch));
}

#[test]
fn handmade_zlib_roundtrip_with_correct_adler() {
    let data = b"abcabcabc";
    let raw = fixed_stream(|w| {
        for &b in b"abc" {
            w.fixed_lit(b as u32);
        }
        w.fixed_match(6, 3);
    });
    assert_eq!(zlib_decompress(&zlib_wrap(&raw, data), 100).unwrap(), data);
}
