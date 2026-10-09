//! `Content-Encoding` support for the browser: gzip (RFC 1952), zlib-wrapped
//! `deflate` (RFC 1950, what the HTTP spec says) and bare `deflate` (what some
//! servers actually send), all on top of [`crate::inflate`] with a hard output
//! limit so a compression bomb cannot exhaust the kernel heap.

use crate::inflate::{self, InflateError};
use alloc::vec::Vec;

/// Largest decompressed body accepted. 4x the raw response cap
/// ([`crate::browser::MAX_RESPONSE_BYTES`], 1 MiB): HTML routinely compresses
/// 4-8x, so a page that fills the raw cap with gzip needs this much room. Memory
/// while decoding: the raw response + its de-chunked copy + this output (+ the
/// `Vec` growth slack, at most 2x) ~ 12 MiB worst case of the 64 MiB heap, freed
/// as soon as the DOM is built. Beyond it the decoded prefix is still shown.
pub const MAX_DECODED_BYTES: usize = 4 * 1024 * 1024;

/// Most codings accepted in one `Content-Encoding` header (`gzip, gzip, ...`).
pub const MAX_CODINGS: usize = 4;

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

/// How much of a compressed body was recovered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completeness {
    /// The stream ended cleanly and its checksum matched.
    Complete,
    /// The input ended before the stream did (a download cut at the size cap, a
    /// dropped connection). Everything decoded up to there is in `data`.
    Cut,
    /// The output limit was reached: `data` is the first `max_output` bytes.
    TooLarge,
    /// The data went bad after the part in `data` (corrupt DEFLATE).
    Damaged,
    /// All data was decoded but the trailer's checksum or length is wrong.
    BadChecksum,
}

impl Completeness {
    fn of(e: InflateError) -> Completeness {
        match e {
            InflateError::Truncated => Completeness::Cut,
            InflateError::OutputLimit | InflateError::OutOfMemory => Completeness::TooLarge,
            InflateError::ChecksumMismatch => Completeness::BadChecksum,
            _ => Completeness::Damaged,
        }
    }

    /// The strict-API error for an incomplete result.
    fn into_error(self) -> Result<(), EncodingError> {
        match self {
            Completeness::Complete => Ok(()),
            Completeness::Cut => Err(EncodingError::Truncated),
            Completeness::TooLarge => Err(EncodingError::TooLarge),
            Completeness::Damaged => Err(EncodingError::Corrupt),
            Completeness::BadChecksum => Err(EncodingError::BadChecksum),
        }
    }
}

/// A decoded body, possibly only a prefix of the real one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub data: Vec<u8>,
    pub status: Completeness,
}

impl Decoded {
    fn into_strict(self) -> Result<Vec<u8>, EncodingError> {
        self.status.into_error().map(|()| self.data)
    }
}

const FHCRC: u8 = 0x02;
const FEXTRA: u8 = 0x04;
const FNAME: u8 = 0x08;
const FCOMMENT: u8 = 0x10;
const RESERVED: u8 = 0xE0;

/// Length of the gzip member header at the start of `data`.
fn gzip_header_len(data: &[u8]) -> Result<usize, EncodingError> {
    if data.len() < 10 {
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
    if pos > data.len() {
        return Err(EncodingError::Truncated);
    }
    Ok(pos)
}

/// Decompress one gzip member (trailing garbage after the trailer is ignored,
/// as browsers do; further members are not followed). Strict: any problem,
/// including a cut stream, is an error; see [`gunzip_partial`] for the lenient
/// variant the browser uses.
pub fn gunzip(data: &[u8], max_output: usize) -> Result<Vec<u8>, EncodingError> {
    if data.len() < 18 {
        return Err(if data.len() >= 2 && data[..2] != [0x1F, 0x8B] {
            EncodingError::BadHeader
        } else {
            EncodingError::Truncated
        });
    }
    gunzip_partial(data, max_output)?.into_strict()
}

/// Decompress one gzip member, keeping whatever was decoded before a problem.
///
/// Only an unusable *start* is an `Err` (bad or incomplete header, or a stream
/// that failed before producing a single byte, reported as that failure). Once
/// bytes were decoded the result is `Ok` with a [`Completeness`] saying whether
/// they are the whole body: a download cut mid-stream, a bad trailer and a
/// corrupt tail all give the decoded prefix instead of nothing. The CRC is
/// checked only when the stream ended cleanly.
pub fn gunzip_partial(data: &[u8], max_output: usize) -> Result<Decoded, EncodingError> {
    let pos = gzip_header_len(data)?;
    let body = data.get(pos..).ok_or(EncodingError::Truncated)?;
    let (out, err, used) = inflate::inflate_consumed_partial(body, max_output);
    if let Some(e) = err {
        return finish_partial(out, e);
    }
    let Some(trailer) = body.get(used..used + 8) else {
        // The DEFLATE stream is complete but the 8 trailer bytes were cut off.
        return Ok(Decoded {
            data: out,
            status: Completeness::Cut,
        });
    };
    let crc = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    let isize = u32::from_le_bytes([trailer[4], trailer[5], trailer[6], trailer[7]]);
    let status = if crc != inflate::crc32(&out) || isize != out.len() as u32 {
        Completeness::BadChecksum
    } else {
        Completeness::Complete
    };
    Ok(Decoded { data: out, status })
}

/// A failed inflate: the prefix is the result unless there is none.
fn finish_partial(out: Vec<u8>, e: InflateError) -> Result<Decoded, EncodingError> {
    let status = Completeness::of(e);
    if out.is_empty() && status != Completeness::BadChecksum {
        // Nothing to show: report the failure itself.
        return Err(match status {
            Completeness::Cut => EncodingError::Truncated,
            Completeness::TooLarge => EncodingError::TooLarge,
            _ => EncodingError::Corrupt,
        });
    }
    Ok(Decoded { data: out, status })
}

/// Does `data` start with a plausible zlib header (RFC 1950)?
fn looks_like_zlib(data: &[u8]) -> bool {
    match data {
        [cmf, flg, ..] => {
            cmf & 0x0F == 8
                && cmf >> 4 <= 7
                && ((u32::from(*cmf) << 8) | u32::from(*flg)).is_multiple_of(31)
                && flg & 0x20 == 0
        }
        _ => false,
    }
}

/// `Content-Encoding: deflate` leniently: zlib-wrapped per RFC 9110, else bare
/// DEFLATE, keeping the decoded prefix of a cut or damaged stream. A zlib header
/// that is valid commits to zlib (so a cut zlib stream is not retried as raw and
/// turned into garbage) unless zlib decoded nothing, in which case the bytes get
/// one more chance as raw DEFLATE.
pub fn inflate_http_deflate_partial(
    data: &[u8],
    max_output: usize,
) -> Result<Decoded, EncodingError> {
    if looks_like_zlib(data)
        && let Ok((out, err)) = inflate::zlib_partial(data, max_output)
        && (!out.is_empty() || err.is_none())
    {
        let status = err.map_or(Completeness::Complete, Completeness::of);
        return Ok(Decoded { data: out, status });
    }
    let (out, err) = inflate::inflate_partial(data, max_output);
    match err {
        None => Ok(Decoded {
            data: out,
            status: Completeness::Complete,
        }),
        Some(e) => finish_partial(out, e),
    }
}

/// `Content-Encoding: deflate`, strict: zlib-wrapped per RFC 9110, else bare
/// DEFLATE.
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
    /// Anything else (`br`, `zstd`, ...): not decodable here.
    Unsupported,
}

impl Encoding {
    /// Parse a `Content-Encoding` header value holding ONE coding
    /// (case-insensitive); a list is [`Encoding::Unsupported`] here, see
    /// [`Encoding::parse_chain`].
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

    /// Parse a whole `Content-Encoding` value: codings in the order they were
    /// applied by the server (`gzip, gzip` etc.), `identity` and empty items
    /// dropped. More than [`MAX_CODINGS`] items, or any unknown one (`br`,
    /// `zstd`, ...), makes the chain `[Unsupported]`: it cannot be decoded
    /// honestly.
    pub fn parse_chain(value: &[u8]) -> Vec<Encoding> {
        let mut chain = Vec::new();
        for item in value.split(|&b| b == b',') {
            match Encoding::parse(item) {
                Encoding::Identity => {}
                Encoding::Unsupported => return alloc::vec![Encoding::Unsupported],
                e => chain.push(e),
            }
        }
        if chain.len() > MAX_CODINGS {
            return alloc::vec![Encoding::Unsupported];
        }
        chain
    }
}

/// Decode `body` according to `enc`, within [`MAX_DECODED_BYTES`]. Strict.
pub fn decode_body(enc: Encoding, body: &[u8]) -> Result<Vec<u8>, EncodingError> {
    decode_chain_partial(&[enc], body)?.into_strict()
}

/// Decode `body` through a coding chain (`Content-Encoding` order: the last
/// item is undone first), within [`MAX_DECODED_BYTES`] per stage, keeping the
/// decoded prefix of a cut or damaged stream. The result's status is that of the
/// first stage that was not [`Completeness::Complete`].
pub fn decode_chain_partial(chain: &[Encoding], body: &[u8]) -> Result<Decoded, EncodingError> {
    let mut cur = Decoded {
        data: body.to_vec(),
        status: Completeness::Complete,
    };
    for enc in chain.iter().rev() {
        let next = match enc {
            Encoding::Identity => continue,
            Encoding::Gzip => gunzip_partial(&cur.data, MAX_DECODED_BYTES)?,
            Encoding::Deflate => inflate_http_deflate_partial(&cur.data, MAX_DECODED_BYTES)?,
            Encoding::Unsupported => return Err(EncodingError::BadHeader),
        };
        let status = if cur.status == Completeness::Complete {
            next.status
        } else {
            cur.status
        };
        cur = Decoded {
            data: next.data,
            status,
        };
    }
    Ok(cur)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Generated with python3's gzip/zlib (mtime 0); see tools in the commit.
    const GZ_SMALL: &str = "1f8b08000000000002ffb3c928c9cdb1b349ca4fa9b4b3c930b4f3cf49b4d107d2360576e95599050ae5f945d9c50a99790ade9925c5a579a936fa057636fa10e5fa60bd007f2c505342000000";
    const GZ_NAMED: &str = "1f8b081c00000000000306004142020078796e616d652e747874006120636f6d6d656e7400b3c928c9cdb1b349ca4fa9b4b3c930b4f3cf49b4d107d2360576e95599050ae5f945d9c50a99790ade9925c5a579a936fa057636fa10e5fa60bd007f2c505342000000";
    const ZLIB_SMALL: &str = "78dab3c928c9cdb1b349ca4fa9b4b3c930b4f3cf49b4d107d2360576e95599050ae5f945d9c50a99790ade9925c5a579a936fa057636fa10e5fa60bd00f0c1168b";
    const RAW_SMALL: &str = "b3c928c9cdb1b349ca4fa9b4b3c930b4f3cf49b4d107d2360576e95599050ae5f945d9c50a99790ade9925c5a579a936fa057636fa10e5fa60bd00";
    const GZ_BIG: &str = "1f8b08000000000000ffedc9b10900200c00b057fcc0074a7f71e85ec4ff71f60421538644e7aeae75c643ccce504a29a594524a29a594524a29a594524a29a594524a29a59452ffd705e5abee21302a0000";
    const BIG_LEN: usize = 10800;
    const TEXT: &[u8] = b"<html><body><h1>Ola</h1><p>gzip works in Kitsune</p></body></html>";

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

    // ---- partial decoding (cut streams, bad trailers, stacked codings) ----

    fn sample(len: usize) -> Vec<u8> {
        let mut v = Vec::new();
        let mut i = 0u32;
        while v.len() < len {
            v.extend_from_slice(
                alloc::format!("<li>{} {:08x}</li>\n", i, i.wrapping_mul(2_654_435_761)).as_bytes(),
            );
            i += 1;
        }
        v
    }

    fn gz_of(plain: &[u8]) -> Vec<u8> {
        let mut v = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 3];
        v.extend_from_slice(&crate::deflate::deflate_fixed(plain));
        v.extend_from_slice(&inflate::crc32(plain).to_le_bytes());
        v.extend_from_slice(&(plain.len() as u32).to_le_bytes());
        v
    }

    #[test]
    fn gunzip_partial_of_every_cut_is_a_prefix() {
        let plain = sample(30_000);
        let gz = gz_of(&plain);
        let mut last = 0;
        for cut in 10..gz.len() {
            match gunzip_partial(&gz[..cut], 1 << 20) {
                Ok(d) => {
                    assert_eq!(d.status, Completeness::Cut, "cut {cut}");
                    assert!(plain.starts_with(&d.data), "cut {cut}");
                    assert!(d.data.len() >= last);
                    last = d.data.len();
                }
                // nothing decoded yet (the first block header is incomplete)
                Err(e) => assert_eq!(e, EncodingError::Truncated, "cut {cut}"),
            }
        }
        let whole = gunzip_partial(&gz, 1 << 20).unwrap();
        assert_eq!((whole.status, whole.data), (Completeness::Complete, plain));
        // the strict API keeps rejecting every cut
        for cut in 10..gz.len() {
            assert!(gunzip(&gz[..cut], 1 << 20).is_err());
        }
    }

    #[test]
    fn gunzip_partial_reports_checksum_and_size_limit() {
        let plain = sample(5000);
        let mut gz = gz_of(&plain);
        let n = gz.len();
        gz[n - 5] ^= 0x80; // CRC byte
        let d = gunzip_partial(&gz, 1 << 20).unwrap();
        assert_eq!(
            (d.status, d.data.as_slice()),
            (Completeness::BadChecksum, &plain[..])
        );
        let gz = gz_of(&plain);
        let d = gunzip_partial(&gz, 1000).unwrap();
        assert_eq!(d.status, Completeness::TooLarge);
        assert_eq!(d.data, &plain[..1000]);
        // trailer missing: the data is all there, the status says the end was cut
        let d = gunzip_partial(&gz[..gz.len() - 8], 1 << 20).unwrap();
        assert_eq!((d.status, d.data), (Completeness::Cut, plain));
    }

    #[test]
    fn gunzip_partial_errors_only_without_any_output() {
        assert_eq!(gunzip_partial(b"", 100), Err(EncodingError::Truncated));
        assert_eq!(
            gunzip_partial(&[0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0], 100),
            Err(EncodingError::Truncated)
        );
        assert_eq!(
            gunzip_partial(b"not gzip at all", 100),
            Err(EncodingError::BadHeader)
        );
        // a header followed by a reserved block type: damaged from the first byte
        let mut v = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 3];
        v.extend_from_slice(&[0x07, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(gunzip_partial(&v, 100), Err(EncodingError::Corrupt));
    }

    #[test]
    fn http_deflate_partial_keeps_zlib_and_raw_prefixes() {
        let plain = sample(20_000);
        let z = crate::deflate::zlib_compress(&plain);
        let d = inflate_http_deflate_partial(&z[..z.len() / 2], 1 << 20).unwrap();
        assert_eq!(d.status, Completeness::Cut);
        assert!(d.data.len() > 1000 && plain.starts_with(&d.data));
        let raw = crate::deflate::deflate_fixed(&plain);
        let d = inflate_http_deflate_partial(&raw[..raw.len() / 2], 1 << 20).unwrap();
        assert_eq!(d.status, Completeness::Cut);
        assert!(d.data.len() > 1000 && plain.starts_with(&d.data));
        // zlib with a wrong Adler: all data, flagged
        let mut z2 = z.clone();
        let n = z2.len();
        z2[n - 2] ^= 1;
        let d = inflate_http_deflate_partial(&z2, 1 << 20).unwrap();
        assert_eq!(
            (d.status, d.data),
            (Completeness::BadChecksum, plain.clone())
        );
        // whole streams are Complete
        assert_eq!(
            inflate_http_deflate_partial(&z, 1 << 20).unwrap().status,
            Completeness::Complete
        );
        assert_eq!(
            inflate_http_deflate_partial(&raw, 1 << 20).unwrap().status,
            Completeness::Complete
        );
    }

    #[test]
    fn coding_chains() {
        use Encoding::*;
        assert_eq!(Encoding::parse_chain(b"gzip"), vec![Gzip]);
        assert_eq!(
            Encoding::parse_chain(b" GZip , Deflate "),
            vec![Gzip, Deflate]
        );
        assert_eq!(Encoding::parse_chain(b"identity"), vec![]);
        assert_eq!(Encoding::parse_chain(b""), vec![]);
        assert_eq!(
            Encoding::parse_chain(b"gzip,,identity,x-gzip"),
            vec![Gzip, Gzip]
        );
        assert_eq!(Encoding::parse_chain(b"br"), vec![Unsupported]);
        assert_eq!(Encoding::parse_chain(b"gzip, zstd"), vec![Unsupported]);
        assert_eq!(Encoding::parse_chain(b"gzip,gzip,gzip,gzip"), vec![Gzip; 4]);
        assert_eq!(
            Encoding::parse_chain(b"gzip,gzip,gzip,gzip,gzip"),
            vec![Unsupported]
        );
    }

    #[test]
    fn stacked_codings_decode_outermost_last_applied_first() {
        let plain = sample(3000);
        let inner = crate::deflate::zlib_compress(&plain); // applied first: deflate
        let outer = gz_of(&inner); // then gzip
        let d = decode_chain_partial(&[Encoding::Deflate, Encoding::Gzip], &outer).unwrap();
        assert_eq!((d.status, d.data), (Completeness::Complete, plain));
        // a cut outer stage taints the status of the whole chain
        let d = decode_chain_partial(
            &[Encoding::Deflate, Encoding::Gzip],
            &outer[..outer.len() / 2],
        )
        .unwrap();
        assert_ne!(d.status, Completeness::Complete);
    }
}
