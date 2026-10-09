use super::vectors::*;
use super::*;
use crate::testutil::unhex;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------- helpers

const TEXT_LEN: usize = 731;

fn text() -> Vec<u8> {
    let mut v = Vec::new();
    for _ in 0..4 {
        v.extend_from_slice(
            b"The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs. \
              How vexingly quick daft zebras jump! Sphinx of black quartz, judge my vow. ",
        );
    }
    v.extend_from_slice(
        b"Kitsune image decoder: 0123456789 abcdefghijklmnopqrstuvwxyz ABCDEFGHIJKLMNOPQRSTUVWXYZ",
    );
    v
}

fn window_data() -> Vec<u8> {
    let mut pat = Vec::new();
    for k in 0..32u32 {
        pat.extend(core::iter::repeat_n(((k * 37 + 11) % 256) as u8, 1024));
    }
    let mut v = pat.clone();
    v.extend_from_slice(&pat);
    v.extend_from_slice(&pat[..5000]);
    v
}

/// LSB-first bit writer for hand-made streams.
#[derive(Default)]
struct W {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl W {
    fn bits(&mut self, v: u32, n: u32) -> &mut Self {
        self.acc |= (v as u64) << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
        self
    }
    /// A Huffman code (MSB first on the wire).
    fn code(&mut self, code: u32, len: u32) -> &mut Self {
        let rev = code.reverse_bits() >> (32 - len);
        self.bits(rev, len)
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
    fn header(&mut self, last: bool, ty: u32) -> &mut Self {
        self.bits(last as u32, 1).bits(ty, 2)
    }
    fn fixed_lit(&mut self, sym: u32) -> &mut Self {
        match sym {
            0..=143 => self.code(0x30 + sym, 8),
            144..=255 => self.code(0x190 + sym - 144, 9),
            256..=279 => self.code(sym - 256, 7),
            _ => self.code(0xC0 + sym - 280, 8),
        }
    }
    fn fixed_match(&mut self, len: usize, dist: usize) -> &mut Self {
        let li = LEN_BASE.iter().rposition(|&b| b as usize <= len).unwrap();
        self.fixed_lit(257 + li as u32);
        self.bits((len - LEN_BASE[li] as usize) as u32, LEN_EXTRA[li] as u32);
        let di = DIST_BASE.iter().rposition(|&b| b as usize <= dist).unwrap();
        self.code(di as u32, 5);
        self.bits(
            (dist - DIST_BASE[di] as usize) as u32,
            DIST_EXTRA[di] as u32,
        )
    }
}

/// Canonical codes for `lens`: `(code, len)` per symbol.
fn canon(lens: &[u8]) -> Vec<(u32, u32)> {
    let mut count = [0u32; 16];
    for &l in lens {
        count[l as usize] += 1;
    }
    count[0] = 0;
    let mut next = [0u32; 16];
    let mut code = 0;
    for b in 1..16 {
        code = (code + count[b - 1]) << 1;
        next[b] = code;
    }
    lens.iter()
        .map(|&l| {
            if l == 0 {
                (0, 0)
            } else {
                let c = next[l as usize];
                next[l as usize] += 1;
                (c, l as u32)
            }
        })
        .collect()
}

/// Starts a dynamic block whose code lengths are sent with a flat 4-bit
/// code-length code (symbols 0..=15 only, no run-length codes).
fn dyn_header(w: &mut W, last: bool, lit: &[u8], dist: &[u8]) {
    w.header(last, 2);
    w.bits(lit.len() as u32 - 257, 5);
    w.bits(dist.len() as u32 - 1, 5);
    w.bits(15, 4); // hclen = 19
    for &order in &CL_ORDER {
        w.bits(if order <= 15 { 4 } else { 0 }, 3);
    }
    for &l in lit.iter().chain(dist) {
        w.code(l as u32, 4);
    }
}

fn fixed_stream(f: impl FnOnce(&mut W)) -> Vec<u8> {
    let mut w = W::default();
    w.header(true, 1);
    f(&mut w);
    w.fixed_lit(256);
    w.finish()
}

fn lit_lens(pairs: &[(usize, u8)], n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    for &(i, l) in pairs {
        v[i] = l;
    }
    v
}

fn zlib_wrap(raw: &[u8], data: &[u8]) -> Vec<u8> {
    let mut v = vec![0x78, 0x9C];
    v.extend_from_slice(raw);
    v.extend_from_slice(&adler32(data).to_be_bytes());
    v
}

const BIG: usize = 1 << 24;

// ---------------------------------------------------------------- checksums

fn crc_bitwise(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
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

// ---------------------------------------------------------------- real zlib streams

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

// ---------------------------------------------------------------- streaming

fn read_chunked(mut inf: Inflater<'_>, chunk: usize) -> Result<Vec<u8>, InflateError> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; chunk];
    loop {
        let n = inf.read(&mut buf)?;
        if n == 0 {
            return Ok(out);
        }
        out.extend_from_slice(&buf[..n]);
    }
}

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

// ---------------------------------------------------------------- output limit

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

// ---------------------------------------------------------------- hand-made streams

#[test]
fn distance_32768_is_the_largest_valid_reference() {
    // 32768 distinct-ish literals, then a match reaching back exactly 32768.
    let lits: Vec<u8> = (0..32768u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    let stream = fixed_stream(|w| {
        for &b in &lits {
            w.fixed_lit(b as u32);
        }
        w.fixed_match(258, 32768);
        w.fixed_match(10, 32768);
    });
    let out = inflate(&stream, BIG).unwrap();
    assert_eq!(out.len(), 32768 + 268);
    assert_eq!(&out[32768..32768 + 258], &lits[..258]);
    assert_eq!(&out[32768 + 258..], &lits[258..268]);
}

#[test]
fn window_wraps_correctly_over_many_windows() {
    // 100 KB of literals, then references at several distances into the ring.
    let lits: Vec<u8> = (0..100_000u32).map(|i| (i * 7 + (i >> 8)) as u8).collect();
    let stream = fixed_stream(|w| {
        for &b in &lits {
            w.fixed_lit(b as u32);
        }
        w.fixed_match(50, 1);
        w.fixed_match(50, 32768);
        w.fixed_match(200, 12345);
    });
    let mut want = lits.clone();
    for (len, dist) in [(50usize, 1usize), (50, 32768), (200, 12345)] {
        for _ in 0..len {
            let b = want[want.len() - dist];
            want.push(b);
        }
    }
    assert_eq!(inflate(&stream, BIG).unwrap(), want);
}

#[test]
fn distance_beyond_produced_output_is_rejected() {
    let s = fixed_stream(|w| {
        w.fixed_lit(b'a' as u32);
        w.fixed_match(3, 2);
    });
    assert_eq!(inflate(&s, 100), Err(InflateError::InvalidDistance));
    let s = fixed_stream(|w| {
        w.fixed_match(3, 1); // nothing produced yet
    });
    assert_eq!(inflate(&s, 100), Err(InflateError::InvalidDistance));
    // One byte of history is enough for distance 1.
    let s = fixed_stream(|w| {
        w.fixed_lit(b'a' as u32);
        w.fixed_match(3, 1);
    });
    assert_eq!(inflate(&s, 100).unwrap(), b"aaaa");
}

#[test]
fn reserved_symbols_are_rejected() {
    for sym in [286, 287] {
        let s = fixed_stream(|w| {
            w.fixed_lit(b'a' as u32);
            w.fixed_lit(sym);
        });
        assert_eq!(
            inflate(&s, 100),
            Err(InflateError::InvalidSymbol),
            "sym {sym}"
        );
    }
    // Distance codes 30 and 31 are reserved.
    for dsym in [30, 31] {
        let s = fixed_stream(|w| {
            w.fixed_lit(b'a' as u32);
            w.fixed_lit(257);
            w.code(dsym, 5);
        });
        assert_eq!(
            inflate(&s, 100),
            Err(InflateError::InvalidSymbol),
            "dsym {dsym}"
        );
    }
}

#[test]
fn reserved_block_type_is_rejected() {
    let mut w = W::default();
    w.header(true, 3);
    assert_eq!(inflate(&w.finish(), 10), Err(InflateError::BadBlockType));
}

#[test]
fn stored_length_check_is_enforced() {
    let mut w = W::default();
    w.header(true, 0);
    let mut v = w.finish();
    v.extend_from_slice(&[3, 0, 0xFC, 0xFF, 1, 2, 3]);
    assert_eq!(inflate(&v, 10).unwrap(), [1, 2, 3]);
    v[3] = 0xFB; // NLEN no longer the complement of LEN
    assert_eq!(inflate(&v, 10), Err(InflateError::StoredLenMismatch));
}

#[test]
fn stored_payload_shorter_than_declared_is_truncated() {
    let mut w = W::default();
    w.header(true, 0);
    let mut v = w.finish();
    v.extend_from_slice(&[5, 0, 0xFA, 0xFF, 1, 2]);
    assert_eq!(inflate(&v, 10), Err(InflateError::Truncated));
}

#[test]
fn stored_block_after_unaligned_header() {
    // A fixed block, then a stored block: the stored header starts mid-byte.
    let mut w = W::default();
    w.header(false, 1);
    w.fixed_lit(b'x' as u32);
    w.fixed_lit(256);
    w.header(true, 0);
    let mut v = w.finish();
    v.extend_from_slice(&[2, 0, 0xFD, 0xFF, b'y', b'z']);
    assert_eq!(inflate(&v, 10).unwrap(), b"xyz");
}

#[test]
fn empty_stored_blocks_are_fine() {
    let mut v = Vec::new();
    for last in [false, false, true] {
        let mut w = W::default();
        w.header(last, 0);
        v.extend(w.finish());
        v.extend_from_slice(&[0, 0, 0xFF, 0xFF]);
    }
    assert_eq!(inflate(&v, 0).unwrap(), b"");
}

#[test]
fn custom_dynamic_block_decodes() {
    // lit 'a' (97) = 2 bits, EOB = 2 bits, length-4 symbol (258) = 1 bit.
    let lit = lit_lens(&[(97, 2), (256, 2), (258, 1)], 259);
    let dist = [1u8];
    let lc = canon(&lit);
    let dc = canon(&dist);
    let mut w = W::default();
    dyn_header(&mut w, true, &lit, &dist);
    w.code(lc[97].0, lc[97].1);
    w.code(lc[258].0, lc[258].1);
    w.code(dc[0].0, dc[0].1); // distance 1 (the only code)
    w.code(lc[258].0, lc[258].1);
    w.code(dc[0].0, dc[0].1);
    w.code(lc[256].0, lc[256].1);
    assert_eq!(inflate(&w.finish(), 100).unwrap(), b"aaaaaaaaa");
}

#[test]
fn dynamic_single_distance_code_rejects_the_unused_pattern() {
    let lit = lit_lens(&[(97, 2), (256, 2), (258, 1)], 259);
    let lc = canon(&lit);
    let mut w = W::default();
    dyn_header(&mut w, true, &lit, &[1]);
    w.code(lc[97].0, lc[97].1);
    w.code(lc[258].0, lc[258].1);
    w.bits(1, 1); // distance code '1' does not exist
    let mut v = w.finish();
    v.extend_from_slice(&[0xFF; 4]); // enough input that this is not "truncated"
    assert_eq!(inflate(&v, 100), Err(InflateError::InvalidCode));
}

#[test]
fn dynamic_block_without_distance_codes_is_literal_only() {
    let lit = lit_lens(&[(97, 2), (98, 2), (256, 2), (257, 2)], 258);
    let lc = canon(&lit);
    let mut w = W::default();
    dyn_header(&mut w, true, &lit, &[0]);
    w.code(lc[97].0, lc[97].1);
    w.code(lc[98].0, lc[98].1);
    w.code(lc[256].0, lc[256].1);
    assert_eq!(inflate(&w.finish(), 100).unwrap(), b"ab");
    // A length symbol would need a distance code: it does not exist.
    let mut w = W::default();
    dyn_header(&mut w, true, &lit, &[0]);
    w.code(lc[97].0, lc[97].1);
    w.code(lc[257].0, lc[257].1); // length 3
    w.bits(0, 1);
    let mut v = w.finish();
    v.extend_from_slice(&[0xFF; 4]);
    assert_eq!(inflate(&v, 100), Err(InflateError::InvalidCode));
}

#[test]
fn incomplete_and_oversubscribed_codes_are_rejected() {
    // Incomplete literal tree (1/4 + 1/4 of the code space only).
    let lit = lit_lens(&[(97, 2), (256, 2)], 257);
    let mut w = W::default();
    dyn_header(&mut w, true, &lit, &[1]);
    assert_eq!(inflate(&w.finish(), 100), Err(InflateError::BadCodeLengths));
    // Over-subscribed: three codes of length 1.
    let lit = lit_lens(&[(97, 1), (98, 1), (256, 1)], 257);
    let mut w = W::default();
    dyn_header(&mut w, true, &lit, &[1]);
    assert_eq!(inflate(&w.finish(), 100), Err(InflateError::BadCodeLengths));
    // Incomplete distance tree with two codes.
    let lit = lit_lens(&[(97, 1), (256, 1)], 257);
    let mut w = W::default();
    dyn_header(&mut w, true, &lit, &[2, 2]);
    assert_eq!(inflate(&w.finish(), 100), Err(InflateError::BadCodeLengths));
}

#[test]
fn dynamic_block_without_end_of_block_code_is_rejected() {
    let lit = lit_lens(&[(97, 1), (98, 1)], 257);
    let mut w = W::default();
    dyn_header(&mut w, true, &lit, &[0]);
    assert_eq!(
        inflate(&w.finish(), 100),
        Err(InflateError::MissingEndOfBlock)
    );
}

#[test]
fn too_many_length_or_distance_codes_are_rejected() {
    // hlit field 31 -> 288 literal/length codes (> 286).
    let mut w = W::default();
    w.header(true, 2);
    w.bits(31, 5).bits(0, 5).bits(0, 4);
    for _ in 0..4 {
        w.bits(0, 3);
    }
    assert_eq!(inflate(&w.finish(), 100), Err(InflateError::BadCodeLengths));
    // hdist field 31 -> 32 distance codes (> 30).
    let mut w = W::default();
    w.header(true, 2);
    w.bits(0, 5).bits(31, 5).bits(0, 4);
    for _ in 0..4 {
        w.bits(0, 3);
    }
    assert_eq!(inflate(&w.finish(), 100), Err(InflateError::BadCodeLengths));
}

#[test]
fn bad_code_length_code_is_rejected() {
    // Four code-length symbols of length 1: over-subscribed.
    let mut w = W::default();
    w.header(true, 2);
    w.bits(0, 5).bits(0, 5).bits(0, 4);
    for _ in 0..4 {
        w.bits(1, 3);
    }
    assert_eq!(inflate(&w.finish(), 100), Err(InflateError::BadCodeLengths));
    // No code-length symbols at all: incomplete.
    let mut w = W::default();
    w.header(true, 2);
    w.bits(0, 5).bits(0, 5).bits(0, 4);
    for _ in 0..4 {
        w.bits(0, 3);
    }
    assert_eq!(inflate(&w.finish(), 100), Err(InflateError::BadCodeLengths));
}

#[test]
fn repeat_previous_length_with_nothing_before_is_rejected() {
    // CL code: symbol 0 -> '0', symbol 16 -> '1'.
    let mut w = W::default();
    w.header(true, 2);
    w.bits(0, 5).bits(0, 5).bits(0, 4);
    w.bits(1, 3).bits(0, 3).bits(0, 3).bits(1, 3); // order: 16, 17, 18, 0
    w.code(1, 1); // symbol 16 as the very first length
    w.bits(0, 2);
    assert_eq!(inflate(&w.finish(), 100), Err(InflateError::BadCodeLengths));
}

#[test]
fn zero_run_overflowing_the_table_is_rejected() {
    // CL code: symbol 0 -> '0', symbol 18 -> '1'. 258 lengths are expected.
    let mut w = W::default();
    w.header(true, 2);
    w.bits(0, 5).bits(0, 5).bits(0, 4);
    w.bits(0, 3).bits(0, 3).bits(1, 3).bits(1, 3); // order: 16, 17, 18, 0
    w.code(1, 1).bits(127, 7); // 138 zeros
    w.code(1, 1).bits(127, 7); // 138 more: 276 > 258
    assert_eq!(inflate(&w.finish(), 100), Err(InflateError::BadCodeLengths));
}

#[test]
fn run_length_codes_17_and_18_decode() {
    // Literal/length lengths: 97 zeros, 'a' = 2, 'b' = 2, 157 zeros, EOB = 1.
    // The zero runs are sent with code-length symbols 18 (11..=138 zeros) and
    // 17 (3..=10 zeros); one distance length of 0 follows.
    let mut cl = [0u8; 19];
    for (sym, len) in [(0, 2), (1, 3), (2, 3), (17, 2), (18, 2)] {
        cl[sym] = len;
    }
    let cc = canon(&cl);
    let mut w = W::default();
    w.header(true, 2);
    w.bits(0, 5).bits(0, 5).bits(15, 4); // hlit 257, hdist 1, hclen 19
    for &sym in &CL_ORDER {
        w.bits(cl[sym] as u32, 3);
    }
    let put = |w: &mut W, sym: usize| {
        w.code(cc[sym].0, cc[sym].1);
    };
    put(&mut w, 18);
    w.bits(97 - 11, 7);
    put(&mut w, 2);
    put(&mut w, 2);
    put(&mut w, 18);
    w.bits(138 - 11, 7);
    put(&mut w, 18);
    w.bits(16 - 11, 7);
    put(&mut w, 17);
    w.bits(0, 3); // 3 zeros: 97+2+138+16+3 = 256 entries, then EOB
    put(&mut w, 1);
    put(&mut w, 0); // distance table: no codes
    let lc = canon(&lit_lens(&[(97, 2), (98, 2), (256, 1)], 257));
    for sym in [97usize, 98, 97, 256] {
        w.code(lc[sym].0, lc[sym].1);
    }
    assert_eq!(inflate(&w.finish(), 100).unwrap(), b"aba");
}

#[test]
fn literals_above_127_and_nine_bit_codes() {
    let data: Vec<u8> = (0..=255u8).collect();
    let s = fixed_stream(|w| {
        for &b in &data {
            w.fixed_lit(b as u32);
        }
    });
    assert_eq!(inflate(&s, 300).unwrap(), data);
}

#[test]
fn fixed_block_tables_survive_block_boundaries() {
    // Two fixed blocks in a row reuse the table; a stored block between does too.
    let mut w = W::default();
    w.header(false, 1);
    w.fixed_lit(b'a' as u32);
    w.fixed_lit(256);
    w.header(false, 0);
    let mut v = w.finish();
    v.extend_from_slice(&[1, 0, 0xFE, 0xFF, b'b']);
    let mut w = W::default();
    w.header(true, 1);
    w.fixed_lit(b'c' as u32);
    w.fixed_match(3, 3);
    w.fixed_lit(256);
    v.extend(w.finish());
    assert_eq!(inflate(&v, 100).unwrap(), b"abcabc");
}

// ---------------------------------------------------------------- zlib wrapper

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

// ---------------------------------------------------------------- corruption

#[test]
fn every_truncation_is_an_error_not_a_panic() {
    for (hex, zlib) in [
        (DYN_ZLIB, true),
        (DYN_RAW, false),
        (WINDOW_ZLIB, true),
        (MULTI_RAW, false),
        (STORED_ZLIB, true),
        (A1000_ZLIB, true),
        (ALL256_ZLIB, true),
    ] {
        let full = unhex(hex);
        for cut in 0..full.len() {
            let r = if zlib {
                zlib_decompress(&full[..cut], BIG)
            } else {
                inflate(&full[..cut], BIG)
            };
            assert!(r.is_err(), "{hex:.12} cut {cut} decoded");
        }
    }
}

#[test]
fn every_single_bit_flip_never_panics_and_rarely_succeeds() {
    for (hex, zlib) in [
        (DYN_ZLIB, true),
        (WINDOW_ZLIB, true),
        (FIXED_ZLIB, true),
        (MULTI_RAW, false),
    ] {
        let ok = unhex(hex);
        let reference = if zlib {
            zlib_decompress(&ok, BIG)
        } else {
            inflate(&ok, BIG)
        }
        .unwrap();
        let mut accepted = 0usize;
        for bit in 0..ok.len() * 8 {
            let mut z = ok.clone();
            z[bit / 8] ^= 1 << (bit % 8);
            let r = if zlib {
                zlib_decompress(&z, BIG)
            } else {
                inflate(&z, BIG)
            };
            if let Ok(v) = r {
                accepted += 1;
                if zlib {
                    // With the Adler-32 in place a flip can only be accepted if
                    // it did not change the output (padding bits).
                    assert_eq!(v, reference, "{hex:.12} bit {bit}");
                }
            }
        }
        if zlib {
            assert!(
                accepted * 5 < ok.len() * 8,
                "{hex:.12}: {accepted} flips accepted"
            );
        }
    }
}

#[test]
fn garbage_inputs_never_panic() {
    // Deterministic pseudo-random garbage of many lengths, with small limits.
    let mut x = 0x1234_5678u32;
    for len in 0..400usize {
        let v: Vec<u8> = (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect();
        let _ = inflate(&v, 4096);
        let _ = zlib_decompress(&v, 4096);
        let mut z = vec![0x78, 0x9C];
        z.extend_from_slice(&v);
        let _ = zlib_decompress(&z, 4096);
    }
}

#[test]
fn decoding_is_deterministic_and_inflater_reusable_per_stream() {
    let z = unhex(DYN_ZLIB);
    let a = zlib_decompress(&z, BIG).unwrap();
    let b = zlib_decompress(&z, BIG).unwrap();
    assert_eq!(a, b);
}

#[test]
fn error_messages_are_nonempty() {
    use alloc::string::ToString;
    for e in [
        InflateError::Truncated,
        InflateError::BadBlockType,
        InflateError::StoredLenMismatch,
        InflateError::BadCodeLengths,
        InflateError::MissingEndOfBlock,
        InflateError::InvalidSymbol,
        InflateError::InvalidCode,
        InflateError::InvalidDistance,
        InflateError::OutputLimit,
        InflateError::BadZlibHeader,
        InflateError::DictionaryUnsupported,
        InflateError::ChecksumMismatch,
        InflateError::OutOfMemory,
    ] {
        assert!(!e.to_string().is_empty());
    }
}

// ------------------------------------------------- partial output (cut streams)

#[test]
fn partial_read_keeps_bytes_decoded_before_a_cut() {
    let data = text();
    let z = crate::deflate::zlib_compress(&data);
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
    let raw = crate::deflate::deflate_fixed(&data);
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
    let raw = crate::deflate::deflate_stored(&data);
    let (out, err) = inflate_partial(&raw[..5 + 100], 1 << 20);
    assert_eq!(out, &data[..100]);
    assert_eq!(err, Some(InflateError::Truncated));
}

#[test]
fn output_limit_delivers_the_head_of_the_data() {
    let data = window_data();
    let raw = crate::deflate::deflate_fixed(&data);
    for limit in [1usize, 100, 4096, 33_000, data.len() - 1] {
        let (out, err) = inflate_partial(&raw, limit);
        assert_eq!(err, Some(InflateError::OutputLimit), "limit {limit}");
        assert_eq!(out.len(), limit, "limit {limit}");
        assert_eq!(&out[..], &data[..limit]);
    }
    // stored blocks too
    let raw = crate::deflate::deflate_stored(&data);
    let (out, err) = inflate_partial(&raw, 70_000);
    assert_eq!((out.len(), err), (70_000, Some(InflateError::OutputLimit)));
    // the strict API still refuses
    assert_eq!(inflate(&raw, 70_000), Err(InflateError::OutputLimit));
}

#[test]
fn zlib_checksum_mismatch_still_returns_all_the_data() {
    let data = text();
    let mut z = crate::deflate::zlib_compress(&data);
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
    let raw = crate::deflate::deflate_fixed(&window_data());
    let mut inf = Inflater::new_raw(&raw[..raw.len() / 2], 1 << 20);
    let mut buf = [0u8; 100_000];
    let (n, e) = inf.read_partial(&mut buf);
    assert!(n > 0 && e == Some(InflateError::Truncated));
    assert_eq!(
        inf.read_partial(&mut buf),
        (0, Some(InflateError::Truncated))
    );
}
