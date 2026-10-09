use super::vectors::*;
use super::*;
use crate::image::channels;
use crate::testutil::unhex;
use alloc::vec;

fn check(v: (&str, usize, usize, &str)) {
    let img = decode(&unhex(v.0)).unwrap();
    assert_eq!((img.width(), img.height()), (v.1, v.2));
    assert_eq!(img.to_rgba().unwrap(), unhex(v.3));
}

#[test]
fn imagemagick_p6_8bit() {
    check(IM_P6_8BIT);
}

#[test]
fn imagemagick_p3_8bit() {
    check(IM_P3_8BIT);
}

#[test]
fn imagemagick_p6_16bit_rounds_to_8() {
    check(IM_P6_16BIT);
}

#[test]
fn imagemagick_p3_16bit_rounds_to_8() {
    check(IM_P3_16BIT);
}

#[test]
fn tiny_hand_written_p3() {
    let img = decode(b"P3\n2 1\n255\n255 0 0  0 255 0\n").unwrap();
    assert_eq!(img.pixels(), &[0xFFFF_0000, 0xFF00_FF00]);
}

#[test]
fn tiny_hand_written_p6() {
    let mut d = b"P6 2 1 255\n".to_vec();
    d.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
    let img = decode(&d).unwrap();
    assert_eq!(img.pixels(), &[rgba(1, 2, 3, 255), rgba(4, 5, 6, 255)]);
}

#[test]
fn comments_anywhere_in_the_header() {
    let img = decode(b"P3 # magic comment\n# another\n1 # width\n1\n# before maxval\n255\n9 8 7")
        .unwrap();
    assert_eq!(img.get(0, 0), Some(rgba(9, 8, 7, 255)));
    // Comments between ASCII samples too.
    let img = decode(b"P3 2 1 255 1 2 3 # first pixel done\n 4 5 6").unwrap();
    assert_eq!(img.get(1, 0), Some(rgba(4, 5, 6, 255)));
    // A comment ended by CR only.
    let img = decode(b"P3 1 1 255 # c\r1 2 3").unwrap();
    assert_eq!(img.get(0, 0), Some(rgba(1, 2, 3, 255)));
}

#[test]
fn whitespace_variants() {
    let img = decode(b"P3\t1\r\n1\x0B255\x0C 1\t2\n3").unwrap();
    assert_eq!(img.get(0, 0), Some(rgba(1, 2, 3, 255)));
}

#[test]
fn maxval_scaling() {
    // maxval 1: samples 0/1 become 0/255.
    let img = decode(b"P3 2 1 1 1 0 1  0 1 0").unwrap();
    assert_eq!(img.pixels(), &[0xFFFF_00FF, 0xFF00_FF00]);
    // maxval 15: 15 -> 255, 8 -> 136 (8*255/15 = 136).
    let img = decode(b"P3 1 1 15 15 8 0").unwrap();
    assert_eq!(img.get(0, 0), Some(rgba(255, 136, 0, 255)));
    // maxval 100, rounding: 50 -> 127.5 -> 128.
    let img = decode(b"P3 1 1 100 50 100 1").unwrap();
    assert_eq!(img.get(0, 0), Some(rgba(128, 255, 3, 255)));
    // maxval 255 is the identity.
    let img = decode(b"P3 1 1 255 0 128 255").unwrap();
    assert_eq!(img.get(0, 0), Some(rgba(0, 128, 255, 255)));
}

#[test]
fn p6_sixteen_bit_samples() {
    let mut d = b"P6 1 1 65535\n".to_vec();
    d.extend_from_slice(&[0xFF, 0xFF, 0x80, 0x00, 0x00, 0x00]);
    let img = decode(&d).unwrap();
    assert_eq!(img.get(0, 0), Some(rgba(255, 128, 0, 255)));
    // maxval 256 also takes two bytes per sample.
    let mut d = b"P6 1 1 256\n".to_vec();
    d.extend_from_slice(&[1, 0, 0, 128, 0, 0]);
    let img = decode(&d).unwrap();
    assert_eq!(img.get(0, 0), Some(rgba(255, 128, 0, 255))); // 128*255/256 = 127.5 rounds up
}

#[test]
fn bad_magic() {
    for d in [
        &b""[..],
        b"P",
        b"P1 1 1\n0",
        b"P2 1 1 255 0",
        b"P4 1 1 x",
        b"P5 1 1 255 x",
        b"P7\n",
        b"Q6 1 1 255 ...",
        b"p6 1 1 255 ...",
    ] {
        assert_eq!(decode(d), Err(PpmError::BadMagic), "{d:?}");
    }
}

#[test]
fn bad_headers() {
    for d in [
        &b"P3 0 1 255\n"[..],
        b"P3 1 0 255\n",
        b"P3 1 1 0\n0 0 0",
        b"P3 1 1 65536\n0 0 0",
        b"P3 a 1 255\n",
        b"P3 1 x 255\n",
        b"P3 1 1 25x5\n0 0 0",
        b"P3 99999999999 1 255\n",
        b"P3 -1 1 255\n",
        b"P6 1 1 255#c\n...",
        b"P6 1 1 255x...",
    ] {
        assert_eq!(
            decode(d),
            Err(PpmError::BadHeader),
            "{:?}",
            core::str::from_utf8(d)
        );
    }
}

#[test]
fn truncated_headers_and_rasters() {
    for d in [
        &b"P3"[..],
        b"P3 ",
        b"P3 1",
        b"P3 1 1",
        b"P3 1 1 255",
        b"P6 1 1 255",
    ] {
        assert_eq!(
            decode(d),
            Err(PpmError::Truncated),
            "{:?}",
            core::str::from_utf8(d)
        );
    }
    assert_eq!(decode(b"P6 1 1 255\n\x01\x02"), Err(PpmError::Truncated));
    assert_eq!(decode(b"P6 2 2 255\n123456789"), Err(PpmError::Truncated));
    assert_eq!(decode(b"P3 2 1 255\n1 2 3 4 5"), Err(PpmError::Truncated));
    // Wide samples need two bytes each.
    assert_eq!(
        decode(b"P6 1 1 65535\n\x01\x02\x03\x04\x05"),
        Err(PpmError::Truncated)
    );
}

#[test]
fn p3_samples_must_be_numbers_not_above_maxval() {
    assert_eq!(decode(b"P3 1 1 10\n1 2 11"), Err(PpmError::BadSample));
    assert_eq!(decode(b"P3 1 1 255\n1 2 abc"), Err(PpmError::BadSample));
    assert_eq!(decode(b"P3 1 1 255\n1 2 -3"), Err(PpmError::BadSample));
    assert_eq!(decode(b"P3 1 1 255\n1 2 3x"), Err(PpmError::BadSample));
    assert_eq!(
        decode(b"P3 1 1 255\n1 2 99999999999"),
        Err(PpmError::BadSample)
    );
}

#[test]
fn trailing_data_after_the_raster_is_ignored() {
    let img = decode(b"P6 1 1 255\n\x01\x02\x03trailing").unwrap();
    assert_eq!(img.get(0, 0), Some(rgba(1, 2, 3, 255)));
    let img = decode(b"P3 1 1 255\n1 2 3\n\n garbage").unwrap();
    assert_eq!(img.get(0, 0), Some(rgba(1, 2, 3, 255)));
}

#[test]
fn p6_needs_exactly_one_whitespace_byte_before_the_raster() {
    // The first raster byte may itself look like whitespace (0x0A): it must not be skipped.
    let mut d = b"P6 1 1 255\n".to_vec();
    d.extend_from_slice(&[0x0A, 0x20, 0x09]);
    let img = decode(&d).unwrap();
    assert_eq!(img.get(0, 0), Some(rgba(0x0A, 0x20, 0x09, 255)));
}

#[test]
fn huge_dimensions_are_rejected_before_allocating() {
    assert_eq!(
        decode(b"P6 100000 100000 255\n"),
        Err(PpmError::Image(ImageError::TooLarge))
    );
    assert_eq!(
        decode(b"P3 4294967295 4294967295 255\n0"),
        Err(PpmError::Image(ImageError::TooLarge))
    );
    assert_eq!(decode(b"P3 4096 4096 255\n0 0 0"), Err(PpmError::Truncated)); // within the limit, but no data
    assert_eq!(
        decode(b"P6 4096 4096 255\n\0\0\0"),
        Err(PpmError::Truncated)
    );
}

#[test]
fn every_prefix_of_a_valid_file_is_an_error() {
    for v in [IM_P6_8BIT, IM_P3_8BIT, IM_P6_16BIT, IM_P3_16BIT] {
        let file = unhex(v.0);
        // A P3 file cut inside its very last number still parses (a shorter
        // value), so only the final digits are exempt there.
        let exempt = if file[1] == b'3' { 8 } else { 0 };
        for cut in 0..file.len() - exempt {
            assert!(decode(&file[..cut]).is_err(), "cut {cut} of {}", file.len());
        }
    }
}

#[test]
fn garbage_never_panics() {
    let mut x = 0xABCD_1234u32;
    for round in 0..500 {
        let mut d = if round % 2 == 0 {
            b"P3 3 2 255\n".to_vec()
        } else {
            b"P6 3 2 255\n".to_vec()
        };
        for _ in 0..round % 40 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            d.push(x as u8);
        }
        let _ = decode(&d);
    }
}

#[test]
fn encode_p6_layout_and_roundtrip() {
    let img = Image::from_pixels(
        2,
        2,
        vec![0xFF01_0203, 0xFF04_0506, 0xFF07_0809, 0xFF0A_0B0C],
    )
    .unwrap();
    let out = encode_p6(&img, 0).unwrap();
    assert!(out.starts_with(b"P6\n2 2\n255\n"));
    assert_eq!(out.len(), 11 + 12);
    assert_eq!(&out[11..14], &[1, 2, 3]);
    assert_eq!(decode(&out).unwrap(), img);
}

#[test]
fn encode_p6_flattens_alpha() {
    let img = Image::from_pixels(2, 1, vec![0x80FF_FFFF, 0x0000_0000]).unwrap();
    let back = decode(&encode_p6(&img, 0x0000_0000).unwrap()).unwrap();
    assert_eq!(channels(back.get(0, 0).unwrap()), [128, 128, 128, 255]);
    assert_eq!(back.get(1, 0), Some(0xFF00_0000));
    let back = decode(&encode_p6(&img, 0x0000_FF00).unwrap()).unwrap();
    assert_eq!(back.get(1, 0), Some(rgba(0, 255, 0, 255)));
}

#[test]
fn error_messages_are_nonempty() {
    use alloc::string::ToString;
    for e in [
        PpmError::BadMagic,
        PpmError::Truncated,
        PpmError::BadHeader,
        PpmError::BadSample,
        PpmError::Image(ImageError::TooLarge),
    ] {
        assert!(!e.to_string().is_empty());
    }
    assert_eq!(
        PpmError::from(ImageError::ZeroSize),
        PpmError::Image(ImageError::ZeroSize)
    );
}
