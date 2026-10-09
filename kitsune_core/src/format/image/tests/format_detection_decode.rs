mod detect_decode {
    use super::super::*;
    use crate::format::bmp::BmpError;
    use crate::format::png::PngError;
    use crate::format::ppm::PpmError;
    use alloc::vec;

    fn sample() -> Image {
        let mut px = Vec::new();
        for y in 0..6usize {
            for x in 0..7usize {
                px.push(rgba((x * 30) as u8, (y * 40) as u8, (x * y) as u8, 255));
            }
        }
        Image::from_pixels(7, 6, px).unwrap()
    }

    #[test]
    fn detects_each_format_by_signature() {
        assert_eq!(detect(b"\x89PNG\r\n\x1a\n...."), Some(Format::Png));
        assert_eq!(detect(b"BM\x00\x00"), Some(Format::Bmp));
        assert_eq!(detect(b"P6\n1 1\n255\n"), Some(Format::Ppm));
        assert_eq!(detect(b"P3 1 1 255 0 0 0"), Some(Format::Ppm));
        assert_eq!(detect(b"P3#c\n"), Some(Format::Ppm));
        assert_eq!(detect(b"P6\t"), Some(Format::Ppm));
        assert_eq!(Format::Png.name(), "png");
        assert_eq!(Format::Bmp.name(), "bmp");
        assert_eq!(Format::Ppm.name(), "ppm");
    }

    #[test]
    fn rejects_unknown_and_short_signatures() {
        for d in [
            &b""[..],
            b"B",
            b"P",
            b"P6",
            b"P3",
            b"P1 1 1\n0",
            b"P5\n",
            b"P6x",
            b"GIF89a",
            b"\xFF\xD8\xFF\xE0",
            b"RIFF....WEBP",
            b"\x89PN",
            b"bm",
            b"\0\0\0\0",
        ] {
            assert_eq!(detect(d), None, "{d:?}");
            assert_eq!(decode(d), Err(DecodeError::UnknownFormat), "{d:?}");
        }
    }

    #[test]
    fn decode_dispatches_to_the_right_decoder() {
        let img = sample();
        for fmt in [Format::Png, Format::Bmp, Format::Ppm] {
            let bytes = encode(&img, fmt).unwrap();
            assert_eq!(detect(&bytes), Some(fmt));
            assert_eq!(decode(&bytes).unwrap(), img, "{fmt:?}");
        }
    }

    #[test]
    fn encode_picks_sensible_variants() {
        let opaque = sample();
        let png = encode(&opaque, Format::Png).unwrap();
        assert_eq!(
            crate::format::png::read_header(&png).unwrap().color_type,
            crate::format::png::ColorType::Rgb
        );
        let bmp = encode(&opaque, Format::Bmp).unwrap();
        assert_eq!(u16::from_le_bytes([bmp[28], bmp[29]]), 24);
        let mut alpha = sample();
        alpha.set(0, 0, 0x4000_00FF);
        let png = encode(&alpha, Format::Png).unwrap();
        assert_eq!(
            crate::format::png::read_header(&png).unwrap().color_type,
            crate::format::png::ColorType::Rgba
        );
        let bmp = encode(&alpha, Format::Bmp).unwrap();
        assert_eq!(u16::from_le_bytes([bmp[28], bmp[29]]), 32);
        assert_eq!(decode(&bmp).unwrap(), alpha);
        // PPM has no alpha: it is flattened over black.
        let ppm = encode(&alpha, Format::Ppm).unwrap();
        let back = decode(&ppm).unwrap();
        assert_eq!(back.get(0, 0), Some(rgba(0, 0, 64, 255)));
    }

    #[test]
    fn errors_are_typed_per_format() {
        assert_eq!(
            decode(b"\x89PNG\r\n\x1a\n"),
            Err(DecodeError::Png(PngError::Truncated))
        );
        assert_eq!(
            decode(b"\x89PNG\x00\x00\x00\x00"),
            Err(DecodeError::Png(PngError::BadSignature))
        );
        assert_eq!(decode(b"BM"), Err(DecodeError::Bmp(BmpError::Truncated)));
        assert_eq!(
            decode(b"P6 1 1 255\n\x01"),
            Err(DecodeError::Ppm(PpmError::Truncated))
        );
        assert_eq!(
            decode(b"P3 0 0 255\n"),
            Err(DecodeError::Ppm(PpmError::BadHeader))
        );
    }

    #[test]
    fn from_impls_wrap_the_module_errors() {
        assert_eq!(
            DecodeError::from(PngError::BadChunk),
            DecodeError::Png(PngError::BadChunk)
        );
        assert_eq!(
            DecodeError::from(BmpError::BadHeader),
            DecodeError::Bmp(BmpError::BadHeader)
        );
        assert_eq!(
            DecodeError::from(PpmError::BadSample),
            DecodeError::Ppm(PpmError::BadSample)
        );
    }

    #[test]
    fn decode_messages_name_the_format() {
        use alloc::string::ToString;
        assert!(DecodeError::UnknownFormat.to_string().contains("unknown"));
        assert!(
            DecodeError::Png(PngError::BadChunk)
                .to_string()
                .starts_with("png:")
        );
        assert!(
            DecodeError::Bmp(BmpError::BadHeader)
                .to_string()
                .starts_with("bmp:")
        );
        assert!(
            DecodeError::Ppm(PpmError::BadSample)
                .to_string()
                .starts_with("ppm:")
        );
    }

    #[test]
    fn decoded_images_respect_the_pixel_limit_in_every_format() {
        // Headers that claim 100000 x 100000 pixels, one per format.
        let mut png = SIGNATURE_PNG.to_vec();
        let mut ihdr = vec![];
        ihdr.extend_from_slice(&100_000u32.to_be_bytes());
        ihdr.extend_from_slice(&100_000u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut chunk = (ihdr.len() as u32).to_be_bytes().to_vec();
        chunk.extend_from_slice(b"IHDR");
        chunk.extend_from_slice(&ihdr);
        let mut crc = crate::format::inflate::Crc32::new();
        crc.update(b"IHDR");
        crc.update(&ihdr);
        chunk.extend_from_slice(&crc.finish().to_be_bytes());
        png.extend(chunk);
        assert_eq!(
            decode(&png),
            Err(DecodeError::Png(PngError::Image(ImageError::TooLarge)))
        );
        assert_eq!(
            decode(b"P6 100000 100000 255\n"),
            Err(DecodeError::Ppm(PpmError::Image(ImageError::TooLarge)))
        );
        let mut bmp = crate::format::bmp::encode_24(&sample(), 0).unwrap();
        bmp[18..22].copy_from_slice(&100_000i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&100_000i32.to_le_bytes());
        assert_eq!(
            decode(&bmp),
            Err(DecodeError::Bmp(BmpError::Image(ImageError::TooLarge)))
        );
    }

    const SIGNATURE_PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    #[test]
    fn decode_then_resize_pipeline() {
        // What the viewer does: decode, fit to a thumbnail box, rotate.
        let bytes = encode(&sample(), Format::Png).unwrap();
        let img = decode(&bytes).unwrap();
        let thumb = img.fit(4, 4, false, Filter::Auto).unwrap();
        assert!(thumb.width() <= 4 && thumb.height() <= 4);
        let rot = thumb.rotate90().unwrap();
        assert_eq!((rot.width(), rot.height()), (thumb.height(), thumb.width()));
    }

    #[test]
    fn decode_never_panics_on_garbage_with_valid_signatures() {
        let mut x = 0xDEAD_BEEFu32;
        for sig in [&b"\x89PNG\r\n\x1a\n"[..], b"BM", b"P6 ", b"P3 "] {
            for round in 0..150 {
                let mut d = sig.to_vec();
                for _ in 0..round {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    d.push(x as u8);
                }
                let _ = decode(&d);
            }
        }
    }
}
