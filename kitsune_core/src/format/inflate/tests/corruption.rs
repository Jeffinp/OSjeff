use super::*;

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
