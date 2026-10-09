use super::*;

#[test]
fn streaming_in_tiny_pieces_matches_whole_buffer() {
    let cases: [(&str, Vec<u8>); 4] = [
        (DYN_ZLIB, text()),
        (WINDOW_ZLIB, window_data()),
        (A1000_ZLIB, vec![b'a'; 1000]),
        (FIXED_ZLIB, b"hello hello hello hello".to_vec()),
    ];
    for (hex, want) in &cases {
        let z = unhex(hex);
        for chunk in [1, 2, 3, 7, 100, 4096, 70000] {
            let inf = Inflater::new_zlib(&z, BIG).unwrap();
            assert_eq!(&read_chunked(inf, chunk).unwrap(), want, "chunk {chunk}");
        }
    }
}

#[test]
fn streaming_handles_stored_blocks_across_reads() {
    let z = unhex(STORED_ZLIB);
    for chunk in [1, 5, 12, 13] {
        let inf = Inflater::new_zlib(&z, 100).unwrap();
        assert_eq!(read_chunked(inf, chunk).unwrap(), b"hello stored");
    }
}

#[test]
fn inflater_reports_progress_and_end() {
    let z = unhex(DYN_ZLIB);
    let mut inf = Inflater::new_zlib(&z, BIG).unwrap();
    assert!(!inf.is_done());
    let mut buf = vec![0u8; 10];
    assert_eq!(inf.read(&mut buf).unwrap(), 10);
    assert_eq!(inf.total_out(), 10);
    assert!(inf.consumed() < z.len());
    let mut rest = vec![0u8; 5000];
    let n = inf.read(&mut rest).unwrap();
    assert_eq!(n, TEXT_LEN - 10);
    assert_eq!(inf.read(&mut rest).unwrap(), 0);
    assert!(inf.is_done());
    assert_eq!(inf.consumed(), z.len());
    // Reading again after the end stays at 0.
    assert_eq!(inf.read(&mut rest).unwrap(), 0);
}

#[test]
fn read_with_empty_buffer_is_a_no_op() {
    let z = unhex(FIXED_ZLIB);
    let mut inf = Inflater::new_zlib(&z, 100).unwrap();
    assert_eq!(inf.read(&mut []).unwrap(), 0);
    assert!(!inf.is_done());
    assert_eq!(read_chunked(inf, 64).unwrap(), b"hello hello hello hello");
}

#[test]
fn errors_are_sticky() {
    let mut z = unhex(DYN_ZLIB);
    let n = z.len();
    z[n - 1] ^= 1; // corrupt the Adler-32
    let mut inf = Inflater::new_zlib(&z, BIG).unwrap();
    let mut buf = vec![0u8; 4096];
    let e = inf.read(&mut buf).unwrap_err();
    assert_eq!(e, InflateError::ChecksumMismatch);
    assert_eq!(inf.read(&mut buf).unwrap_err(), e);
    assert!(!inf.is_done());
}
