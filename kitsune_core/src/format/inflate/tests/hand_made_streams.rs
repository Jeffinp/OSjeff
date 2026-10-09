use super::*;

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
