use super::*;

#[test]
fn signature_is_checked() {
    assert_eq!(decode(&[]), Err(PngError::BadSignature));
    assert_eq!(decode(&SIGNATURE[..7]), Err(PngError::BadSignature));
    assert_eq!(decode(b"GIF89a.........."), Err(PngError::BadSignature));
    // Line-ending damage (a classic FTP text-mode corruption) is detected.
    let mut v = plain_rgba(2, 2);
    v.remove(4); // drop the \r
    assert_eq!(decode(&v), Err(PngError::BadSignature));
    assert_eq!(read_header(b"\x89PNG"), Err(PngError::BadSignature));
}

#[test]
fn signature_only_is_truncated() {
    assert_eq!(decode(&SIGNATURE), Err(PngError::Truncated));
}

#[test]
fn first_chunk_must_be_ihdr() {
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"IEND", &[]));
    assert_eq!(decode(&v), Err(PngError::MissingIhdr));
    assert_eq!(read_header(&v), Err(PngError::MissingIhdr));
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"tEXt", b"a\0b"));
    v.extend(chunk(b"IHDR", &ihdr(1, 1, 8, 6, 0)));
    assert_eq!(decode(&v), Err(PngError::MissingIhdr));
}

#[test]
fn huge_chunk_length_is_rejected() {
    let mut v = SIGNATURE.to_vec();
    v.extend_from_slice(&0x8000_0000u32.to_be_bytes());
    v.extend_from_slice(b"IHDR");
    v.extend_from_slice(&[0; 20]);
    assert_eq!(decode(&v), Err(PngError::BadChunk));
    // A length that is legal but far past the end of the file.
    let mut v = SIGNATURE.to_vec();
    v.extend_from_slice(&0x7FFF_FFFFu32.to_be_bytes());
    v.extend_from_slice(b"IHDR");
    v.extend_from_slice(&[0; 20]);
    assert_eq!(decode(&v), Err(PngError::Truncated));
}

#[test]
fn chunk_type_must_be_letters() {
    let mut v = plain_rgba(2, 2);
    let (pos, _) = first_chunk_of(&v, b"IHDR");
    v[pos + 4] = b'1';
    assert_eq!(decode(&v), Err(PngError::BadChunk));
}

#[test]
fn every_chunk_crc_is_verified() {
    for ty in [b"IHDR", b"IDAT", b"IEND"] {
        let mut v = plain_rgba(3, 3);
        let (pos, len) = first_chunk_of(&v, ty);
        v[pos + 8 + len] ^= 0x01; // first CRC byte
        assert_eq!(
            decode(&v),
            Err(PngError::CrcMismatch),
            "{}",
            core::str::from_utf8(ty).unwrap()
        );
    }
    // A skipped ancillary chunk with a bad CRC is still an error.
    let mut v = SIGNATURE.to_vec();
    v.extend(chunk(b"IHDR", &ihdr(1, 1, 8, 6, 0)));
    let mut t = chunk(b"tEXt", b"k\0v");
    let n = t.len();
    t[n - 1] ^= 0xFF;
    v.extend(t);
    v.extend(chunk(b"IDAT", &zlib_compress(&[0, 1, 2, 3, 4])));
    v.extend(chunk(b"IEND", &[]));
    assert_eq!(decode(&v), Err(PngError::CrcMismatch));
}

#[test]
fn missing_iend_is_truncated() {
    let v = plain_rgba(2, 2);
    let (pos, _) = first_chunk_of(&v, b"IEND");
    assert_eq!(decode(&v[..pos]), Err(PngError::Truncated));
}

#[test]
fn iend_must_be_empty() {
    let mut v = plain_rgba(2, 2);
    let (pos, _) = first_chunk_of(&v, b"IEND");
    v.truncate(pos);
    v.extend(chunk(b"IEND", &[1]));
    assert_eq!(decode(&v), Err(PngError::BadChunk));
}

#[test]
fn trailing_bytes_after_iend_are_ignored() {
    let mut v = plain_rgba(2, 2);
    let want = decode(&v).unwrap();
    v.extend_from_slice(b"trailing garbage that is not a chunk at all");
    assert_eq!(decode(&v).unwrap(), want);
}

#[test]
fn every_prefix_of_the_vectors_is_an_error() {
    for v in [
        IM_RGB8,
        IM_PAL8_TRNS,
        IM_RGBA8_ADAM7,
        CR_GRAY1_FILTERS,
        CR_RGBA8_MULTI_IDAT,
    ] {
        let file = unhex(v.0);
        for cut in 0..file.len() {
            assert!(decode(&file[..cut]).is_err(), "cut {cut}/{}", file.len());
        }
    }
}

#[test]
fn every_bit_flip_before_the_end_is_detected() {
    // CRC-32 catches every single-bit error inside a chunk, and a flipped
    // length breaks the framing, so no flip before the end of IEND may decode.
    for v in [
        IM_RGB8,
        IM_PAL4,
        CR_PAL2_FILTERS,
        CR_RGB8_KEY,
        IM_GRAY1_ADAM7,
        CR_RGB8_ANCILLARY,
    ] {
        let file = unhex(v.0);
        let iend_end = file.len(); // the vectors end exactly at IEND's CRC
        for bit in 0..iend_end * 8 {
            let mut f = file.clone();
            f[bit / 8] ^= 1 << (bit % 8);
            assert!(decode(&f).is_err(), "flip of bit {bit} decoded");
        }
    }
}

#[test]
fn bit_flips_with_fixed_crcs_never_panic() {
    // Mutating the payload and repairing the CRCs reaches the deeper checks.
    for v in [
        IM_RGB8,
        IM_PAL4,
        IM_RGBA16,
        IM_RGB8_ADAM7,
        CR_PAL2_FILTERS,
        CR_GRAY8_KEY,
        IM_GA8_ADAM7,
    ] {
        let file = unhex(v.0);
        for bit in 0..file.len() * 8 {
            let mut f = file.clone();
            f[bit / 8] ^= 1 << (bit % 8);
            fix_crcs(&mut f);
            let _ = decode(&f);
        }
    }
}

#[test]
fn garbage_never_panics() {
    let mut x = 0xC0FF_EE11u32;
    let head = unhex(IM_RGB8.0);
    for round in 0..300 {
        let mut f = head[..33.min(head.len())].to_vec();
        for _ in 0..round * 2 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            f.push(x as u8);
        }
        let _ = decode(&f);
        let _ = read_header(&f);
    }
}
