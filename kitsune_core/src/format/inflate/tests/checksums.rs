use super::*;

#[test]
fn crc32_known_vectors() {
    assert_eq!(crc32(b""), 0);
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(b"a"), 0xE8B7_BE43);
    assert_eq!(
        crc32(b"The quick brown fox jumps over the lazy dog"),
        0x414F_A339
    );
    // The CRC of the PNG IEND chunk type is a famous constant.
    assert_eq!(crc32(b"IEND"), 0xAE42_6082);
}

#[test]
fn crc32_matches_bitwise_reference_for_all_alignments() {
    let data: Vec<u8> = (0..300u32).map(|i| (i * 131 + 7) as u8).collect();
    for start in 0..9 {
        for len in 0..70 {
            let s = &data[start..start + len];
            assert_eq!(crc32(s), crc_bitwise(s), "start {start} len {len}");
        }
    }
    assert_eq!(crc32(&data), crc_bitwise(&data));
}

#[test]
fn crc32_incremental_equals_one_shot() {
    let data: Vec<u8> = (0..1000u32).map(|i| (i ^ (i >> 3)) as u8).collect();
    for split in [0, 1, 7, 8, 9, 500, 999, 1000] {
        let mut c = Crc32::new();
        c.update(&data[..split]);
        c.update(&data[split..]);
        assert_eq!(c.finish(), crc32(&data), "split {split}");
    }
    let mut c = Crc32::default();
    c.update(b"12345");
    assert_eq!(c.finish(), crc32(b"12345")); // finish() does not consume the state
    c.update(b"6789");
    assert_eq!(c.finish(), 0xCBF4_3926);
}

#[test]
fn adler32_known_vectors() {
    assert_eq!(adler32(b""), 1);
    assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    assert_eq!(adler32(b"a"), 0x0062_0062);
}

#[test]
fn adler32_large_input_does_not_overflow() {
    // 0xFF everywhere maximises the running sums; compare against u64 maths.
    let data = vec![0xFFu8; 200_000];
    let (mut a, mut b) = (1u64, 0u64);
    for &x in &data {
        a = (a + x as u64) % 65521;
        b = (b + a) % 65521;
    }
    assert_eq!(adler32(&data), ((b << 16) | a) as u32);
    let mut inc = Adler32::new();
    inc.update(&data[..12345]);
    inc.update(&data[12345..]);
    assert_eq!(inc.finish(), adler32(&data));
}
