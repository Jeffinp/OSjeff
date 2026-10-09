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
mod tests;
