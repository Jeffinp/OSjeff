use super::*;
use alloc::string::ToString;

#[test]
fn rfc4648_vectors() {
    let v: [(&str, &str); 7] = [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ];
    for (plain, enc) in v {
        assert_eq!(
            decode(enc.as_bytes(), 100).unwrap(),
            plain.as_bytes(),
            "{enc}"
        );
        assert_eq!(encode(plain.as_bytes()), enc);
    }
}

#[test]
fn missing_padding_is_accepted() {
    assert_eq!(decode(b"Zg", 10).unwrap(), b"f");
    assert_eq!(decode(b"Zm8", 10).unwrap(), b"fo");
}

#[test]
fn whitespace_anywhere_is_ignored() {
    assert_eq!(decode(b" Zm9v\r\nYmFy\t", 10).unwrap(), b"foobar");
    assert_eq!(decode(b"Z m 9 v", 10).unwrap(), b"foo");
}

#[test]
fn bad_bytes_are_rejected() {
    assert_eq!(decode(b"Zm9!", 10), Err(Base64Error::BadByte));
    assert_eq!(decode(b"Zm9v-_", 10), Err(Base64Error::BadByte)); // url-safe is not standard
    assert_eq!(decode(&[0xFF, b'A'], 10), Err(Base64Error::BadByte));
}

#[test]
fn dangling_character_is_rejected() {
    assert_eq!(decode(b"Z", 10), Err(Base64Error::BadLength));
    assert_eq!(decode(b"Zm9vY", 10), Err(Base64Error::BadLength));
}

#[test]
fn padding_rules() {
    assert_eq!(decode(b"Zg=", 10), Err(Base64Error::BadLength));
    assert_eq!(decode(b"Zg===", 10), Err(Base64Error::BadLength));
    assert_eq!(decode(b"Zg==Zg==", 10), Err(Base64Error::PaddingInMiddle));
    assert_eq!(decode(b"=Zg", 10), Err(Base64Error::PaddingInMiddle));
}

#[test]
fn output_limit_is_exact() {
    assert_eq!(decode(b"Zm9v", 3).unwrap(), b"foo");
    assert_eq!(decode(b"Zm9v", 2), Err(Base64Error::TooLarge));
    assert_eq!(decode(b"Zm9vYmFy", 5), Err(Base64Error::TooLarge));
    assert_eq!(decode(b"Zm9v", 0), Err(Base64Error::TooLarge));
}

#[test]
fn a_huge_input_never_exceeds_the_limit() {
    let big = "QUJD".repeat(100_000);
    assert_eq!(decode(big.as_bytes(), 1000), Err(Base64Error::TooLarge));
}

#[test]
fn every_byte_round_trips() {
    let data: alloc::vec::Vec<u8> = (0..=255u8).collect();
    let e = encode(&data);
    assert_eq!(decode(e.as_bytes(), 300).unwrap(), data);
}

#[test]
fn round_trip_all_lengths() {
    for n in 0..40usize {
        let data: alloc::vec::Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
        assert_eq!(decode(encode(&data).as_bytes(), 64).unwrap(), data, "{n}");
    }
}

#[test]
fn data_uri_parts() {
    let d = parse_data_uri("data:image/png;base64,AAAA").unwrap();
    assert_eq!(d.mime, "image/png");
    assert!(d.base64);
    assert_eq!(d.payload, "AAAA");
    let d = parse_data_uri("DATA:image/png;charset=x;BASE64,QQ==").unwrap();
    assert!(d.base64);
    let d = parse_data_uri("data:,hello").unwrap();
    assert_eq!((d.mime, d.base64, d.payload), ("", false, "hello"));
}

#[test]
fn not_a_data_uri() {
    assert!(parse_data_uri("http://x/").is_none());
    assert!(parse_data_uri("data:image/png;base64").is_none()); // no comma
    assert!(parse_data_uri("dat").is_none());
    assert!(parse_data_uri("").is_none());
    assert!(parse_data_uri("dата:x,y").is_none()); // non-ASCII lookalike
}

#[test]
fn data_image_extraction() {
    let uri = alloc::format!("data:image/png;base64,{}", encode(b"\x89PNGxx"));
    assert_eq!(decode_data_image(&uri, 100).unwrap(), b"\x89PNGxx");
    assert_eq!(
        decode_data_image("data:text/html;base64,QQ==", 10),
        Err(DataImageError::Unsupported)
    );
    assert_eq!(
        decode_data_image("data:image/png,QQ==", 10),
        Err(DataImageError::Unsupported)
    );
    assert_eq!(
        decode_data_image("http://a/b.png", 10),
        Err(DataImageError::NotDataUri)
    );
    assert_eq!(
        decode_data_image("data:image/png;base64,Q", 10),
        Err(DataImageError::Base64(Base64Error::BadLength))
    );
    assert_eq!(
        decode_data_image("data:image/png;base64,QUJDREVG", 2),
        Err(DataImageError::Base64(Base64Error::TooLarge))
    );
}

#[test]
fn mime_is_case_insensitive_and_needs_a_subtype() {
    assert!(decode_data_image("data:IMAGE/PNG;base64,QQ==", 4).is_ok());
    assert_eq!(
        decode_data_image("data:image/;base64,QQ==", 4),
        Err(DataImageError::Unsupported)
    );
}

#[test]
fn display_messages_are_ascii() {
    for e in [
        Base64Error::BadByte,
        Base64Error::BadLength,
        Base64Error::PaddingInMiddle,
        Base64Error::TooLarge,
    ] {
        assert!(e.to_string().is_ascii());
    }
}
