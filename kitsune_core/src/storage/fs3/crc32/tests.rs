use super::*;

#[test]
fn standard_check_value() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
}

#[test]
fn empty_and_known_vectors() {
    assert_eq!(crc32(b""), 0);
    assert_eq!(crc32(b"a"), 0xE8B7_BE43);
    assert_eq!(
        crc32(b"The quick brown fox jumps over the lazy dog"),
        0x414F_A339
    );
}

#[test]
fn incremental_equals_one_shot_for_every_split() {
    let data: alloc::vec::Vec<u8> = (0..300u32).map(|i| (i * 7 + 3) as u8).collect();
    let whole = crc32(&data);
    for split in 0..data.len() {
        let mut c = Crc32::new();
        c.update(&data[..split]);
        c.update(&data[split..]);
        assert_eq!(c.finalize(), whole, "split {split}");
    }
}

#[test]
fn slicing_matches_bytewise_reference() {
    fn reference(d: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in d {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    let data: alloc::vec::Vec<u8> = (0..1000u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    for n in [0, 1, 7, 8, 9, 63, 64, 65, 999, 1000] {
        assert_eq!(crc32(&data[..n]), reference(&data[..n]), "len {n}");
    }
}

#[test]
fn any_single_bit_flip_changes_the_checksum() {
    let mut data = [0x5Au8; 64];
    let base = crc32(&data);
    for bit in 0..64 * 8 {
        data[bit / 8] ^= 1 << (bit % 8);
        assert_ne!(crc32(&data), base);
        data[bit / 8] ^= 1 << (bit % 8);
    }
}
