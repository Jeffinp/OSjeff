use super::*;
use crate::format::inflate::{DIST_BASE, DIST_EXTRA, adler32, inflate, zlib_decompress};
use alloc::vec;
use alloc::vec::Vec;

const BIG: usize = 1 << 26;

fn prng(seed: u32, n: usize) -> Vec<u8> {
    let mut x = seed | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 8) as u8
        })
        .collect()
}

fn roundtrip(data: &[u8]) -> usize {
    let raw = deflate_fixed(data);
    assert_eq!(
        inflate(&raw, data.len()).unwrap(),
        data,
        "raw, len {}",
        data.len()
    );
    let z = zlib_compress(data);
    assert_eq!(
        zlib_decompress(&z, data.len()).unwrap(),
        data,
        "zlib, len {}",
        data.len()
    );
    let s = deflate_stored(data);
    assert_eq!(inflate(&s, data.len()).unwrap(), data, "stored");
    let zs = zlib_stored(data);
    assert_eq!(
        zlib_decompress(&zs, data.len()).unwrap(),
        data,
        "zlib stored"
    );
    raw.len()
}

#[test]
fn empty_and_tiny_inputs_roundtrip() {
    roundtrip(b"");
    roundtrip(b"a");
    roundtrip(b"ab");
    roundtrip(b"abc");
    roundtrip(b"aaa");
    roundtrip(b"abcabcabc");
}

#[test]
fn every_length_up_to_600_roundtrips() {
    let src = prng(7, 700);
    let rep: Vec<u8> = b"0123456789abcdef"
        .iter()
        .cycle()
        .take(700)
        .copied()
        .collect();
    for len in 0..600 {
        roundtrip(&src[..len]);
        roundtrip(&rep[..len]);
    }
}

#[test]
fn flat_data_compresses_enormously() {
    let zeros = vec![0u8; 1 << 20];
    let n = roundtrip(&zeros);
    assert!(n < 7000, "{n} bytes for 1 MiB of zeros");
    let rgba: Vec<u8> = [10u8, 20, 30, 255]
        .iter()
        .cycle()
        .take(1 << 18)
        .copied()
        .collect();
    assert!(roundtrip(&rgba) < 2000);
}

#[test]
fn text_compresses() {
    let text: Vec<u8> = b"The quick brown fox jumps over the lazy dog. "
        .iter()
        .cycle()
        .take(20_000)
        .copied()
        .collect();
    let n = roundtrip(&text);
    assert!(n < 400, "{n}");
    // Less repetitive text still shrinks.
    let mut t2 = Vec::new();
    for i in 0..2000u32 {
        t2.extend_from_slice(
            alloc::format!("line {} of the file: value={}\n", i, i * i % 977).as_bytes(),
        );
    }
    let n2 = roundtrip(&t2);
    assert!(n2 < t2.len() / 2, "{n2} of {}", t2.len());
}

#[test]
fn incompressible_data_falls_back_to_stored() {
    let r = prng(99, 100_000);
    let raw = deflate_fixed(&r);
    assert_eq!(raw.len(), 100_000 + 2 * 5); // two stored blocks
    assert_eq!(inflate(&raw, BIG).unwrap(), r);
    roundtrip(&r);
}

#[test]
fn output_is_never_much_larger_than_input() {
    for seed in 1..6 {
        let r = prng(seed, 5000 + seed as usize * 1000);
        let z = zlib_compress(&r);
        assert!(z.len() <= r.len() + 6 + 5 * (r.len() / 65535 + 1));
    }
}

#[test]
fn matches_at_the_window_edge() {
    // A block repeated exactly one window apart exercises distance 32768.
    let block = prng(5, 32768);
    let mut data = block.clone();
    data.extend_from_slice(&block);
    let n = roundtrip(&data);
    assert!(n < 36_000, "second copy should be matches, got {n}");
    // And one byte farther: it must not be (mis)referenced.
    let mut data = block.clone();
    data.push(0xAB);
    data.extend_from_slice(&block);
    roundtrip(&data);
}

#[test]
fn long_data_crosses_many_windows() {
    let mut data = Vec::new();
    for i in 0..200_000u32 {
        data.push(((i / 7) ^ (i / 1000)) as u8);
    }
    roundtrip(&data);
    let mut mixed = prng(3, 90_000);
    mixed.extend_from_slice(&vec![7u8; 90_000]);
    mixed.extend_from_slice(&prng(3, 90_000));
    roundtrip(&mixed);
}

#[test]
fn overlapping_matches_and_max_length() {
    // 258-long matches at distance 1 and 2.
    let mut d = vec![b'x'; 1000];
    d.extend(b"ab".iter().cycle().take(1000));
    roundtrip(&d);
    let mut d = vec![0u8; 258];
    d.extend_from_slice(&[1, 2, 3]);
    d.extend(vec![0u8; 259]);
    roundtrip(&d);
}

#[test]
fn stored_block_structure() {
    assert_eq!(deflate_stored(b""), [1, 0, 0, 0xFF, 0xFF]);
    assert_eq!(deflate_stored(b"hi"), [1, 2, 0, 0xFD, 0xFF, b'h', b'i']);
    for len in [65534, 65535, 65536, 65537, 131_070, 131_071] {
        let d = prng(1, len);
        let s = deflate_stored(&d);
        let blocks = len.div_ceil(65535);
        assert_eq!(s.len(), len + 5 * blocks, "len {len}");
        assert_eq!(inflate(&s, BIG).unwrap(), d);
    }
}

#[test]
fn zlib_header_and_adler_are_correct() {
    let d = b"hello zlib";
    let z = zlib_compress(d);
    assert_eq!(z[0], 0x78);
    assert_eq!(((z[0] as u32) << 8 | z[1] as u32) % 31, 0);
    assert_eq!(&z[z.len() - 4..], &adler32(d).to_be_bytes());
    let zs = zlib_stored(d);
    assert_eq!(&zs[zs.len() - 4..], &adler32(d).to_be_bytes());
}

#[test]
fn distance_codes_match_the_rfc_table_for_every_distance() {
    for dist in 1..=32768usize {
        let (sym, extra, val) = dist_code(dist);
        let s = sym as usize;
        assert!(s < 30, "dist {dist}");
        assert_eq!(extra, DIST_EXTRA[s] as u32, "dist {dist}");
        assert_eq!(DIST_BASE[s] as usize + val as usize, dist, "dist {dist}");
        assert!(val < (1 << extra) || extra == 0);
    }
}

#[test]
fn length_symbols_match_the_rfc_table_for_every_length() {
    for (len, &sym) in LEN_SYM.iter().enumerate().skip(3) {
        let s = sym as usize;
        assert!(LEN_BASE[s] as usize <= len);
        let max = LEN_BASE[s] as usize + (1usize << LEN_EXTRA[s]) - 1;
        assert!(len <= max || s == 28, "len {len}");
        if s == 28 {
            assert_eq!(len, 258);
        }
        if s < 28 {
            assert!(len < LEN_BASE[s + 1] as usize, "len {len}");
        }
    }
}

#[test]
fn compression_is_deterministic() {
    let d = prng(11, 30_000);
    let mut e = d.clone();
    e.extend_from_slice(&d[..10_000]);
    assert_eq!(zlib_compress(&e), zlib_compress(&e));
}

#[test]
fn every_truncation_of_compressed_output_is_rejected() {
    let d: Vec<u8> = b"abcdefgh".iter().cycle().take(300).copied().collect();
    let z = zlib_compress(&d);
    for cut in 0..z.len() {
        assert!(zlib_decompress(&z[..cut], BIG).is_err(), "cut {cut}");
    }
}
