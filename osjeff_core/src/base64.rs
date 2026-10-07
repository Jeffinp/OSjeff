//! Base64 (RFC 4648, standard alphabet) decoding and `data:` URI parsing, for
//! inline images (`<img src="data:image/png;base64,...">`).
//!
//! Pure, `alloc`-only, never panics. The decoder is lenient about what real
//! pages contain (ASCII whitespace anywhere, missing `=` padding) and strict
//! about everything else (a byte outside the alphabet, padding in the middle,
//! a dangling single character), and it never produces more than `max_out`
//! bytes: the output limit is checked **before** each push, so a hostile
//! megabyte of input cannot grow a buffer past the caller's budget.

use alloc::vec::Vec;

/// Why decoding failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base64Error {
    /// A byte that is neither in the alphabet, `=`, nor ASCII whitespace.
    BadByte,
    /// A single trailing character (6 bits cannot make a byte), or bad padding.
    BadLength,
    /// `=` followed by more data.
    PaddingInMiddle,
    /// The decoded data would exceed the caller's limit.
    TooLarge,
}

impl core::fmt::Display for Base64Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Base64Error::BadByte => "invalid base64 character",
            Base64Error::BadLength => "invalid base64 length",
            Base64Error::PaddingInMiddle => "base64 padding before the end",
            Base64Error::TooLarge => "base64 data too large",
        })
    }
}

fn value(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decode `input`, producing at most `max_out` bytes.
pub fn decode(input: &[u8], max_out: usize) -> Result<Vec<u8>, Base64Error> {
    let mut out: Vec<u8> = Vec::new();
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let mut padding = 0usize;
    let mut chars = 0usize;
    for &b in input {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            padding += 1;
            continue;
        }
        if padding > 0 {
            return Err(Base64Error::PaddingInMiddle);
        }
        let v = value(b).ok_or(Base64Error::BadByte)?;
        chars += 1;
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            if out.len() >= max_out {
                return Err(Base64Error::TooLarge);
            }
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // 1 leftover character is never valid; padding, when present, must make a
    // multiple of four.
    if chars % 4 == 1 || padding > 2 || (padding > 0 && !(chars + padding).is_multiple_of(4)) {
        return Err(Base64Error::BadLength);
    }
    Ok(out)
}

/// The parts of a `data:` URI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataUri<'a> {
    /// Media type, lower-case-insensitive as written (`image/png`); empty when omitted.
    pub mime: &'a str,
    /// `;base64` was present.
    pub base64: bool,
    /// Everything after the first comma.
    pub payload: &'a str,
}

/// Split `data:[<mime>][;param]*[;base64],<payload>`. `None` if it is not a
/// `data:` URI or has no comma.
pub fn parse_data_uri(uri: &str) -> Option<DataUri<'_>> {
    let uri = uri.trim_start();
    let head = uri.get(..5)?;
    if !head.eq_ignore_ascii_case("data:") {
        return None;
    }
    let rest = &uri[5..];
    let comma = rest.find(',')?;
    let (meta, payload) = (&rest[..comma], &rest[comma + 1..]);
    let mut parts = meta.split(';');
    let mime = parts.next().unwrap_or("").trim();
    let base64 = parts.any(|p| p.trim().eq_ignore_ascii_case("base64"));
    Some(DataUri {
        mime,
        base64,
        payload,
    })
}

/// Why a `data:` image could not be extracted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataImageError {
    NotDataUri,
    /// Not `image/*`, or not base64.
    Unsupported,
    Base64(Base64Error),
}

/// The bytes of a `data:image/...;base64,...` URI, at most `max_out` of them.
pub fn decode_data_image(uri: &str, max_out: usize) -> Result<Vec<u8>, DataImageError> {
    let d = parse_data_uri(uri).ok_or(DataImageError::NotDataUri)?;
    let is_image = d.mime.len() > 6 && d.mime[..6].eq_ignore_ascii_case("image/");
    if !is_image || !d.base64 {
        return Err(DataImageError::Unsupported);
    }
    decode(d.payload.as_bytes(), max_out).map_err(DataImageError::Base64)
}

/// Encode `data` as standard padded base64 (used by tests and by the fuzz
/// target to build valid `data:` URIs).
pub fn encode(data: &[u8]) -> alloc::string::String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = alloc::string::String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        s.push(A[(n >> 18) as usize & 63] as char);
        s.push(A[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 {
            A[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        s.push(if c.len() > 2 {
            A[n as usize & 63] as char
        } else {
            '='
        });
    }
    s
}

#[cfg(test)]
mod tests {
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
}
