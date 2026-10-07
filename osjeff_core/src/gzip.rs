//! `Content-Encoding` support for the browser: gzip (RFC 1952), zlib-wrapped
//! `deflate` (RFC 1950, what the HTTP spec says) and bare `deflate` (what some
//! servers actually send), all on top of [`crate::inflate`] with a hard output
//! limit so a compression bomb cannot exhaust the kernel heap.

use crate::inflate::{self, InflateError};
use alloc::vec::Vec;

/// Largest decompressed body accepted (4x the raw response cap).
pub const MAX_DECODED_BYTES: usize = 1024 * 1024;

/// Why decoding failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodingError {
    /// Not a gzip stream (bad magic, method or reserved flag bits).
    BadHeader,
    /// Truncated header, stream or trailer.
    Truncated,
    /// The DEFLATE data is malformed.
    Corrupt,
    /// CRC-32 or length in the gzip trailer does not match the data.
    BadChecksum,
    /// The output would exceed the limit.
    TooLarge,
}

impl From<InflateError> for EncodingError {
    fn from(e: InflateError) -> Self {
        match e {
            InflateError::OutputLimit => EncodingError::TooLarge,
            _ => EncodingError::Corrupt,
        }
    }
}

const FHCRC: u8 = 0x02;
const FEXTRA: u8 = 0x04;
const FNAME: u8 = 0x08;
const FCOMMENT: u8 = 0x10;
const RESERVED: u8 = 0xE0;

/// Decompress one gzip member (trailing garbage after the trailer is ignored,
/// as browsers do; further members are not followed).
pub fn gunzip(data: &[u8], max_output: usize) -> Result<Vec<u8>, EncodingError> {
    if data.len() < 18 {
        return Err(if data.len() >= 2 && data[..2] != [0x1F, 0x8B] {
            EncodingError::BadHeader
        } else {
            EncodingError::Truncated
        });
    }
    if data[0] != 0x1F || data[1] != 0x8B || data[2] != 8 {
        return Err(EncodingError::BadHeader);
    }
    let flags = data[3];
    if flags & RESERVED != 0 {
        return Err(EncodingError::BadHeader);
    }
    let mut pos = 10usize;
    if flags & FEXTRA != 0 {
        let len = usize::from(u16::from_le_bytes([
            *data.get(pos).ok_or(EncodingError::Truncated)?,
            *data.get(pos + 1).ok_or(EncodingError::Truncated)?,
        ]));
        pos = pos
            .checked_add(2 + len)
            .filter(|&p| p <= data.len())
            .ok_or(EncodingError::Truncated)?;
    }
    for flag in [FNAME, FCOMMENT] {
        if flags & flag != 0 {
            let nul = data
                .get(pos..)
                .and_then(|r| r.iter().position(|&b| b == 0))
                .ok_or(EncodingError::Truncated)?;
            pos += nul + 1;
        }
    }
    if flags & FHCRC != 0 {
        pos = pos.checked_add(2).ok_or(EncodingError::Truncated)?;
    }
    let body = data.get(pos..).ok_or(EncodingError::Truncated)?;
    let (out, used) = inflate::inflate_consumed(body, max_output)?;
    let trailer = body.get(used..used + 8).ok_or(EncodingError::Truncated)?;
    let crc = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    let isize = u32::from_le_bytes([trailer[4], trailer[5], trailer[6], trailer[7]]);
    if crc != inflate::crc32(&out) || isize != out.len() as u32 {
        return Err(EncodingError::BadChecksum);
    }
    Ok(out)
}

/// `Content-Encoding: deflate`: zlib-wrapped per RFC 9110, else bare DEFLATE.
pub fn inflate_http_deflate(data: &[u8], max_output: usize) -> Result<Vec<u8>, EncodingError> {
    match inflate::zlib_decompress(data, max_output) {
        Ok(v) => Ok(v),
        Err(InflateError::OutputLimit) => Err(EncodingError::TooLarge),
        Err(_) => Ok(inflate::inflate(data, max_output)?),
    }
}

/// How the response body is encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Identity,
    Gzip,
    Deflate,
    /// Anything else (`br`, `zstd`, stacked encodings...): not decodable here.
    Unsupported,
}

impl Encoding {
    /// Parse a `Content-Encoding` header value (case-insensitive, one coding).
    pub fn parse(value: &[u8]) -> Encoding {
        let v = value.trim_ascii();
        if v.is_empty() || v.eq_ignore_ascii_case(b"identity") {
            Encoding::Identity
        } else if v.eq_ignore_ascii_case(b"gzip") || v.eq_ignore_ascii_case(b"x-gzip") {
            Encoding::Gzip
        } else if v.eq_ignore_ascii_case(b"deflate") {
            Encoding::Deflate
        } else {
            Encoding::Unsupported
        }
    }
}

/// Decode `body` according to `enc`, within [`MAX_DECODED_BYTES`].
pub fn decode_body(enc: Encoding, body: &[u8]) -> Result<Vec<u8>, EncodingError> {
    match enc {
        Encoding::Identity => Ok(body.to_vec()),
        Encoding::Gzip => gunzip(body, MAX_DECODED_BYTES),
        Encoding::Deflate => inflate_http_deflate(body, MAX_DECODED_BYTES),
        Encoding::Unsupported => Err(EncodingError::BadHeader),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Generated with python3's gzip/zlib (mtime 0); see tools in the commit.
    const GZ_SMALL: &str = "1f8b08000000000002ffb3c928c9cdb1b349ca4fa9b4b3c930b4f3cf49b4d107d2360576e95599050ae5f945d9c50a99790afec159a9696936fa057636fa10d5fa60ad0094af60dd41000000";
    const GZ_NAMED: &str = "1f8b081c00000000000306004142020078796e616d652e747874006120636f6d6d656e7400b3c928c9cdb1b349ca4fa9b4b3c930b4f3cf49b4d107d2360576e95599050ae5f945d9c50a99790afec159a9696936fa057636fa10d5fa60ad0094af60dd41000000";
    const ZLIB_SMALL: &str = "78dab3c928c9cdb1b349ca4fa9b4b3c930b4f3cf49b4d107d2360576e95599050ae5f945d9c50a99790afec159a9696936fa057636fa10d5fa60ad00d38a15e5";
    const RAW_SMALL: &str = "b3c928c9cdb1b349ca4fa9b4b3c930b4f3cf49b4d107d2360576e95599050ae5f945d9c50a99790afec159a9696936fa057636fa10d5fa60ad00";
    const GZ_BIG: &str = "1f8b08000000000000ffedc9b10900200c00b057fcc0074a7f71e85ec4ff71f60421538644e7aeae75c643ccce504a29a594524a29a594524a29a594524a29a594524a29a59452ffd705e5abee21302a0000";
    const BIG_LEN: usize = 10800;
    const TEXT: &[u8] = b"<html><body><h1>Ola</h1><p>gzip works in OSjeff</p></body></html>";

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn gunzip_small() {
        assert_eq!(gunzip(&hex(GZ_SMALL), 4096).unwrap(), TEXT);
    }

    #[test]
    fn gunzip_with_extra_name_and_comment_fields() {
        assert_eq!(gunzip(&hex(GZ_NAMED), 4096).unwrap(), TEXT);
    }

    #[test]
    fn gunzip_repetitive_body() {
        let out = gunzip(&hex(GZ_BIG), 1 << 20).unwrap();
        assert_eq!(out.len(), BIG_LEN);
        assert!(out.starts_with(b"<p>repeat repeat repeat</p>"));
    }

    #[test]
    fn output_limit_stops_a_compression_bomb() {
        assert_eq!(gunzip(&hex(GZ_BIG), 1000), Err(EncodingError::TooLarge));
        assert_eq!(
            gunzip(&hex(GZ_BIG), BIG_LEN - 1),
            Err(EncodingError::TooLarge)
        );
        assert!(gunzip(&hex(GZ_BIG), BIG_LEN).is_ok());
    }

    #[test]
    fn bad_magic_and_method() {
        let mut v = hex(GZ_SMALL);
        v[0] = 0;
        assert_eq!(gunzip(&v, 4096), Err(EncodingError::BadHeader));
        let mut v = hex(GZ_SMALL);
        v[2] = 7;
        assert_eq!(gunzip(&v, 4096), Err(EncodingError::BadHeader));
    }

    #[test]
    fn reserved_flag_bits_refused() {
        let mut v = hex(GZ_SMALL);
        v[3] = 0x20;
        assert_eq!(gunzip(&v, 4096), Err(EncodingError::BadHeader));
    }

    #[test]
    fn corrupted_crc_or_length_detected() {
        let mut v = hex(GZ_SMALL);
        let n = v.len();
        v[n - 8] ^= 1; // CRC
        assert_eq!(gunzip(&v, 4096), Err(EncodingError::BadChecksum));
        let mut v = hex(GZ_SMALL);
        v[n - 1] ^= 1; // ISIZE
        assert_eq!(gunzip(&v, 4096), Err(EncodingError::BadChecksum));
    }

    #[test]
    fn truncation_at_every_length_is_an_error_not_a_panic() {
        let v = hex(GZ_SMALL);
        for cut in 0..v.len() {
            assert!(gunzip(&v[..cut], 4096).is_err(), "cut {cut}");
        }
        let v = hex(GZ_NAMED);
        for cut in 0..v.len() {
            assert!(gunzip(&v[..cut], 4096).is_err(), "named cut {cut}");
        }
    }

    #[test]
    fn corrupted_deflate_data_is_detected() {
        let mut v = hex(GZ_SMALL);
        v[14] ^= 0xFF;
        assert!(gunzip(&v, 4096).is_err());
    }

    #[test]
    fn trailing_garbage_after_member_is_ignored() {
        let mut v = hex(GZ_SMALL);
        v.extend_from_slice(b"garbage");
        assert_eq!(gunzip(&v, 4096).unwrap(), TEXT);
    }

    #[test]
    fn unterminated_name_field_is_truncated() {
        let mut v = vec![0x1F, 0x8B, 8, FNAME, 0, 0, 0, 0, 0, 3];
        v.extend_from_slice(&[b'a'; 30]); // no NUL
        assert_eq!(gunzip(&v, 4096), Err(EncodingError::Truncated));
    }

    #[test]
    fn huge_extra_length_does_not_overflow() {
        let mut v = vec![0x1F, 0x8B, 8, FEXTRA, 0, 0, 0, 0, 0, 3, 0xFF, 0xFF];
        v.extend_from_slice(&[0; 10]);
        assert_eq!(gunzip(&v, 4096), Err(EncodingError::Truncated));
    }

    #[test]
    fn http_deflate_accepts_zlib_and_raw() {
        assert_eq!(inflate_http_deflate(&hex(ZLIB_SMALL), 4096).unwrap(), TEXT);
        assert_eq!(inflate_http_deflate(&hex(RAW_SMALL), 4096).unwrap(), TEXT);
        assert!(inflate_http_deflate(b"not deflate at all!!", 4096).is_err());
        assert_eq!(
            inflate_http_deflate(&hex(ZLIB_SMALL), 10),
            Err(EncodingError::TooLarge)
        );
    }

    #[test]
    fn encoding_header_values() {
        assert_eq!(Encoding::parse(b"gzip"), Encoding::Gzip);
        assert_eq!(Encoding::parse(b" GZIP "), Encoding::Gzip);
        assert_eq!(Encoding::parse(b"x-gzip"), Encoding::Gzip);
        assert_eq!(Encoding::parse(b"deflate"), Encoding::Deflate);
        assert_eq!(Encoding::parse(b"identity"), Encoding::Identity);
        assert_eq!(Encoding::parse(b""), Encoding::Identity);
        assert_eq!(Encoding::parse(b"br"), Encoding::Unsupported);
        assert_eq!(Encoding::parse(b"gzip, br"), Encoding::Unsupported);
    }

    #[test]
    fn decode_body_dispatch() {
        assert_eq!(decode_body(Encoding::Identity, b"abc").unwrap(), b"abc");
        assert_eq!(decode_body(Encoding::Gzip, &hex(GZ_SMALL)).unwrap(), TEXT);
        assert_eq!(
            decode_body(Encoding::Deflate, &hex(ZLIB_SMALL)).unwrap(),
            TEXT
        );
        assert!(decode_body(Encoding::Unsupported, b"x").is_err());
    }
}
