use super::*;

#[test]
fn partial_read_keeps_bytes_decoded_before_a_cut() {
    let data = text();
    let z = crate::format::deflate::zlib_compress(&data);
    for cut in 3..z.len() {
        let (out, err) = match zlib_partial(&z[..cut], 1 << 20) {
            Ok(r) => r,
            Err(_) => continue,
        };
        assert!(data.starts_with(&out), "cut {cut}: not a prefix");
        assert!(
            err.is_some(),
            "cut {cut}: a cut stream must report a problem"
        );
    }
    let (out, err) = zlib_partial(&z, 1 << 20).unwrap();
    assert_eq!((out, err), (data, None));
}

#[test]
fn partial_output_grows_with_the_input() {
    let data = window_data();
    let raw = crate::format::deflate::deflate_fixed(&data);
    let mut last = 0;
    for cut in (1..raw.len()).step_by(7) {
        let (out, err) = inflate_partial(&raw[..cut], 1 << 20);
        assert_eq!(err, Some(InflateError::Truncated), "cut {cut}");
        assert!(out.len() >= last, "cut {cut}");
        assert!(data.starts_with(&out));
        last = out.len();
    }
    assert!(last > data.len() / 2);
}

#[test]
fn partial_stored_block_returns_the_bytes_that_arrived() {
    let data = text();
    let raw = crate::format::deflate::deflate_stored(&data);
    let (out, err) = inflate_partial(&raw[..5 + 100], 1 << 20);
    assert_eq!(out, &data[..100]);
    assert_eq!(err, Some(InflateError::Truncated));
}

#[test]
fn output_limit_delivers_the_head_of_the_data() {
    let data = window_data();
    let raw = crate::format::deflate::deflate_fixed(&data);
    for limit in [1usize, 100, 4096, 33_000, data.len() - 1] {
        let (out, err) = inflate_partial(&raw, limit);
        assert_eq!(err, Some(InflateError::OutputLimit), "limit {limit}");
        assert_eq!(out.len(), limit, "limit {limit}");
        assert_eq!(&out[..], &data[..limit]);
    }
    // stored blocks too
    let raw = crate::format::deflate::deflate_stored(&data);
    let (out, err) = inflate_partial(&raw, 70_000);
    assert_eq!((out.len(), err), (70_000, Some(InflateError::OutputLimit)));
    // the strict API still refuses
    assert_eq!(inflate(&raw, 70_000), Err(InflateError::OutputLimit));
}

#[test]
fn zlib_checksum_mismatch_still_returns_all_the_data() {
    let data = text();
    let mut z = crate::format::deflate::zlib_compress(&data);
    let n = z.len();
    z[n - 1] ^= 1;
    let (out, err) = zlib_partial(&z, 1 << 20).unwrap();
    assert_eq!(out, data);
    assert_eq!(err, Some(InflateError::ChecksumMismatch));
    assert_eq!(
        zlib_decompress(&z, 1 << 20),
        Err(InflateError::ChecksumMismatch)
    );
}

#[test]
fn partial_read_errors_stay_sticky() {
    let raw = crate::format::deflate::deflate_fixed(&window_data());
    let mut inf = Inflater::new_raw(&raw[..raw.len() / 2], 1 << 20);
    let mut buf = [0u8; 100_000];
    let (n, e) = inf.read_partial(&mut buf);
    assert!(n > 0 && e == Some(InflateError::Truncated));
    assert_eq!(
        inf.read_partial(&mut buf),
        (0, Some(InflateError::Truncated))
    );
}
