use super::*;

#[test]
fn max_output_is_exact() {
    let z = unhex(DYN_ZLIB);
    assert_eq!(zlib_decompress(&z, TEXT_LEN).unwrap().len(), TEXT_LEN);
    assert_eq!(
        zlib_decompress(&z, TEXT_LEN - 1),
        Err(InflateError::OutputLimit)
    );
    assert_eq!(zlib_decompress(&z, 0), Err(InflateError::OutputLimit));
    let r = unhex(STORED_RAW);
    assert_eq!(inflate(&r, 12).unwrap().len(), 12);
    assert_eq!(inflate(&r, 11), Err(InflateError::OutputLimit));
    assert_eq!(inflate(&r, 5), Err(InflateError::OutputLimit));
}

#[test]
fn max_output_stops_overlapping_match_runs() {
    let a = unhex(A1000_ZLIB);
    for limit in [0, 1, 2, 257, 258, 259, 999] {
        assert_eq!(
            zlib_decompress(&a, limit),
            Err(InflateError::OutputLimit),
            "{limit}"
        );
    }
    assert!(zlib_decompress(&a, 1000).is_ok());
}

#[test]
fn zip_bomb_is_cut_off_at_the_limit() {
    // One literal then 20000 matches of 258 at distance 1: ~5 MB from ~50 KB.
    let stream = fixed_stream(|w| {
        w.fixed_lit(b'z' as u32);
        for _ in 0..20_000 {
            w.fixed_match(258, 1);
        }
    });
    assert!(stream.len() < 60_000);
    assert_eq!(inflate(&stream, 1000), Err(InflateError::OutputLimit));
    assert_eq!(inflate(&stream, 1_000_000), Err(InflateError::OutputLimit));
    let full = inflate(&stream, 258 * 20_000 + 1).unwrap();
    assert_eq!(full.len(), 258 * 20_000 + 1);
    assert!(full.iter().all(|&b| b == b'z'));
    assert_eq!(
        inflate(&stream, 258 * 20_000),
        Err(InflateError::OutputLimit)
    );
}

#[test]
fn limit_applies_to_stored_blocks_too() {
    let mut w = W::default();
    w.header(true, 0);
    let mut v = w.finish();
    v.extend_from_slice(&[10, 0, 0xF5, 0xFF]);
    v.extend_from_slice(b"0123456789");
    assert_eq!(inflate(&v, 10).unwrap(), b"0123456789");
    assert_eq!(inflate(&v, 9), Err(InflateError::OutputLimit));
}

#[test]
fn huge_limit_does_not_preallocate() {
    // usize::MAX must be harmless for a tiny stream (no up-front reservation).
    let z = unhex(FIXED_ZLIB);
    assert_eq!(
        zlib_decompress(&z, usize::MAX).unwrap(),
        b"hello hello hello hello"
    );
}
