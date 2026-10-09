use super::*;

#[test]
fn stored_block_zlib_and_raw() {
    assert_eq!(
        zlib_decompress(&unhex(STORED_ZLIB), 100).unwrap(),
        b"hello stored"
    );
    assert_eq!(inflate(&unhex(STORED_RAW), 100).unwrap(), b"hello stored");
}

#[test]
fn fixed_huffman_zlib_and_raw() {
    let want = b"hello hello hello hello";
    assert_eq!(zlib_decompress(&unhex(FIXED_ZLIB), 100).unwrap(), want);
    assert_eq!(inflate(&unhex(FIXED_RAW), 100).unwrap(), want);
}

#[test]
fn dynamic_huffman_zlib_and_raw() {
    let want = text();
    assert_eq!(want.len(), TEXT_LEN);
    assert_eq!(zlib_decompress(&unhex(DYN_ZLIB), BIG).unwrap(), want);
    assert_eq!(inflate(&unhex(DYN_RAW), BIG).unwrap(), want);
}

#[test]
fn long_overlapping_references() {
    // "a" * 1000 is one literal plus length-258 matches at distance 1.
    assert_eq!(
        zlib_decompress(&unhex(A1000_ZLIB), BIG).unwrap(),
        vec![b'a'; 1000]
    );
    // "abc" * 500: matches at distance 3 overlap their own output.
    let want: Vec<u8> = b"abc".iter().cycle().take(1500).copied().collect();
    assert_eq!(inflate(&unhex(ABC500_RAW), BIG).unwrap(), want);
}

#[test]
fn empty_streams() {
    assert_eq!(zlib_decompress(&unhex(EMPTY_ZLIB), 0).unwrap(), b"");
    assert_eq!(inflate(&unhex(EMPTY_RAW), 0).unwrap(), b"");
}

#[test]
fn window_spanning_32k_history() {
    let want = window_data();
    assert_eq!(want.len(), 70536);
    assert_eq!(zlib_decompress(&unhex(WINDOW_ZLIB), BIG).unwrap(), want);
}

#[test]
fn multiple_blocks_with_sync_flush() {
    let mut want = b"first block ".to_vec();
    want.extend_from_slice(b"second block second block ");
    want.extend(b"third ".repeat(20));
    assert_eq!(inflate(&unhex(MULTI_RAW), BIG).unwrap(), want);
}

#[test]
fn incompressible_and_all_byte_values() {
    let out = zlib_decompress(&unhex(RND_ZLIB), BIG).unwrap();
    assert_eq!(out.len(), 300);
    assert_eq!(out[..4], [0xA5, 0x4D, 0xCA, 0x18][..]); // spot check, stored block
    let all: Vec<u8> = (0..=255u8).cycle().take(768).collect();
    assert_eq!(zlib_decompress(&unhex(ALL256_ZLIB), BIG).unwrap(), all);
}
