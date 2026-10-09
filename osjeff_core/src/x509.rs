//! A small, strict, panic-free DER / X.509 reader.
//!
//! This is **not** the certificate validator (that is `tlsverify`, built on the
//! reviewed `rustls-webpki` crate). It exists to look inside a certificate the
//! server sent, for three jobs the validator does not do:
//!
//! * pre-checks with hard limits before anything is handed to the validator
//!   ([`MAX_CERT_LEN`], [`MAX_CHAIN_LEN`]);
//! * telling the user *why* a chain failed (validity dates, the names the
//!   certificate is valid for, "self-signed", "no basicConstraints");
//! * a second, independent implementation of the name matching rules that the
//!   tests and the fuzzer cross-check against the validator.
//!
//! Every read is bounds-checked and returns [`Error`] on malformed input; there
//! is no indexing that can panic, no recursion (the parser is iterative and
//! nests only through the fixed X.509 structure), and lengths are limited to
//! 3 bytes (16 MiB), far beyond [`MAX_CERT_LEN`].

use crate::unixtime::DateTime;

/// Largest certificate accepted (DER bytes). Real leaf and intermediate
/// certificates are 1-3 KiB; 16 KiB leaves room for ones with many SANs.
pub const MAX_CERT_LEN: usize = 16 * 1024;
/// Most certificates in a chain from the server (leaf + intermediates).
pub const MAX_CHAIN_LEN: usize = 8;
/// Most subjectAltName entries inspected.
pub const MAX_SAN: usize = 256;

/// Why a certificate could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Ran out of bytes inside an element.
    Truncated,
    /// Wrong tag where the structure demands another.
    UnexpectedTag,
    /// Indefinite lengths, lengths over 3 bytes, non-minimal encodings.
    BadLength,
    /// Bytes after the end of the certificate.
    TrailingData,
    /// Not a version 3 certificate (or an unknown version).
    BadVersion,
    /// Validity dates malformed or `notAfter < notBefore`.
    BadTime,
    /// The input is larger than [`MAX_CERT_LEN`].
    TooLarge,
    /// A boolean or BIT STRING that violates DER.
    BadValue,
}

// ---- DER primitives ----

pub const TAG_BOOL: u8 = 0x01;
pub const TAG_INT: u8 = 0x02;
pub const TAG_BITSTR: u8 = 0x03;
pub const TAG_OCTSTR: u8 = 0x04;
pub const TAG_OID: u8 = 0x06;
pub const TAG_UTCTIME: u8 = 0x17;
pub const TAG_GENTIME: u8 = 0x18;
pub const TAG_SEQ: u8 = 0x30;
pub const TAG_SET: u8 = 0x31;

/// A cursor over DER bytes.
#[derive(Clone, Copy)]
pub struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf }
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn rest(&self) -> &'a [u8] {
        self.buf
    }

    /// The next tag without consuming it.
    pub fn peek_tag(&self) -> Option<u8> {
        self.buf.first().copied()
    }

    /// Read one element: `(tag, content)`, plus the whole encoded element
    /// (`tag`, length and content) for signature checks.
    pub fn read_full(&mut self) -> Result<(u8, &'a [u8], &'a [u8]), Error> {
        let b = self.buf;
        let tag = *b.first().ok_or(Error::Truncated)?;
        if tag & 0x1F == 0x1F {
            return Err(Error::BadLength); // multi-byte tags do not occur in X.509
        }
        let l0 = *b.get(1).ok_or(Error::Truncated)?;
        let (len, hdr) = if l0 < 0x80 {
            (usize::from(l0), 2)
        } else {
            let n = usize::from(l0 & 0x7F);
            if n == 0 || n > 3 {
                return Err(Error::BadLength); // indefinite or absurdly long
            }
            let bytes = b.get(2..2 + n).ok_or(Error::Truncated)?;
            let mut v = 0usize;
            for &x in bytes {
                v = (v << 8) | usize::from(x);
            }
            // DER: the short form must be used when it fits, no leading zeros.
            if v < 0x80 || bytes[0] == 0 {
                return Err(Error::BadLength);
            }
            (v, 2 + n)
        };
        let end = hdr.checked_add(len).ok_or(Error::BadLength)?;
        let whole = b.get(..end).ok_or(Error::Truncated)?;
        self.buf = &b[end..];
        Ok((tag, &whole[hdr..], whole))
    }

    pub fn read(&mut self) -> Result<(u8, &'a [u8]), Error> {
        self.read_full().map(|(t, c, _)| (t, c))
    }

    /// Read an element that must carry `tag`; returns its content.
    pub fn expect(&mut self, tag: u8) -> Result<&'a [u8], Error> {
        let (t, c) = self.read()?;
        if t == tag {
            Ok(c)
        } else {
            Err(Error::UnexpectedTag)
        }
    }

    /// Read an optional element with `tag` (only if it is next).
    pub fn optional(&mut self, tag: u8) -> Result<Option<&'a [u8]>, Error> {
        if self.peek_tag() == Some(tag) {
            self.expect(tag).map(Some)
        } else {
            Ok(None)
        }
    }
}

/// Parse a `UTCTime` (`YYMMDDHHMMSSZ`) or `GeneralizedTime` (`YYYYMMDDHHMMSSZ`)
/// into Unix seconds. Only the DER form (UTC, seconds present, `Z`) is valid.
pub fn parse_time(tag: u8, s: &[u8]) -> Result<u64, Error> {
    fn two(s: &[u8], i: usize) -> Result<u32, Error> {
        match (s.get(i), s.get(i + 1)) {
            (Some(a), Some(b)) if a.is_ascii_digit() && b.is_ascii_digit() => {
                Ok(u32::from(a - b'0') * 10 + u32::from(b - b'0'))
            }
            _ => Err(Error::BadTime),
        }
    }
    let (year, at) = match tag {
        TAG_UTCTIME => {
            if s.len() != 13 {
                return Err(Error::BadTime);
            }
            let yy = two(s, 0)?;
            (if yy >= 50 { 1900 + yy } else { 2000 + yy }, 2)
        }
        TAG_GENTIME => {
            if s.len() != 15 {
                return Err(Error::BadTime);
            }
            (two(s, 0)? * 100 + two(s, 2)?, 4)
        }
        _ => return Err(Error::BadTime),
    };
    if s.last() != Some(&b'Z') {
        return Err(Error::BadTime);
    }
    DateTime {
        year: year as i32,
        month: two(s, at)? as u8,
        day: two(s, at + 2)? as u8,
        hour: two(s, at + 4)? as u8,
        minute: two(s, at + 6)? as u8,
        second: two(s, at + 8)? as u8,
    }
    .to_unix()
    .ok_or(Error::BadTime)
}

// ---- the certificate ----

/// OID content bytes used below (the TLV's value, no tag or length).
mod oid {
    pub const COMMON_NAME: &[u8] = &[0x55, 0x04, 0x03];
    pub const ORGANIZATION: &[u8] = &[0x55, 0x04, 0x0A];
    pub const SUBJECT_ALT_NAME: &[u8] = &[0x55, 0x1D, 0x11];
    pub const BASIC_CONSTRAINTS: &[u8] = &[0x55, 0x1D, 0x13];
    pub const KEY_USAGE: &[u8] = &[0x55, 0x1D, 0x0F];
}

/// `keyUsage` bit for `keyCertSign` (bit 5, value 0x04 in the first byte).
pub const KU_KEY_CERT_SIGN: u16 = 0x0400;
/// `keyUsage` bit for `digitalSignature` (bit 0).
pub const KU_DIGITAL_SIGNATURE: u16 = 0x8000;

/// What `basicConstraints` said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BasicConstraints {
    pub ca: bool,
    pub path_len: Option<u32>,
}

/// The fields of one certificate that the browser's diagnostics use. Slices
/// borrow from the input.
#[derive(Clone, Copy, Debug)]
pub struct Cert<'a> {
    /// The whole certificate (the input).
    pub der: &'a [u8],
    /// `tbsCertificate`, the signed bytes (full encoding with header).
    pub tbs: &'a [u8],
    pub serial: &'a [u8],
    /// DER of the issuer `Name` (content, not the SEQUENCE header).
    pub issuer: &'a [u8],
    /// DER of the subject `Name` (content).
    pub subject: &'a [u8],
    pub not_before: u64,
    pub not_after: u64,
    /// Content of the `subjectPublicKeyInfo` SEQUENCE.
    pub spki: &'a [u8],
    /// Content of the signature `AlgorithmIdentifier`.
    pub sig_alg: &'a [u8],
    /// `subjectAltName` extension value (the inner `GeneralNames` SEQUENCE
    /// content), if present.
    san: Option<&'a [u8]>,
    pub basic_constraints: Option<BasicConstraints>,
    /// `keyUsage` as a big-endian 16-bit mask (bit 0 = MSB of the first byte).
    pub key_usage: Option<u16>,
}

/// Parse a DER certificate (strict: no trailing data, version 3 only).
pub fn parse(der: &[u8]) -> Result<Cert<'_>, Error> {
    if der.len() > MAX_CERT_LEN {
        return Err(Error::TooLarge);
    }
    let mut top = Reader::new(der);
    let cert_body = top.expect(TAG_SEQ)?;
    if !top.is_empty() {
        return Err(Error::TrailingData);
    }
    let mut cert = Reader::new(cert_body);
    let (tbs_tag, tbs_body, tbs_full) = cert.read_full()?;
    if tbs_tag != TAG_SEQ {
        return Err(Error::UnexpectedTag);
    }
    let sig_alg_outer = cert.expect(TAG_SEQ)?;
    let sig = cert.expect(TAG_BITSTR)?;
    if sig.is_empty() || sig[0] != 0 || !cert.is_empty() {
        return Err(Error::BadValue);
    }

    let mut t = Reader::new(tbs_body);
    // [0] EXPLICIT version: v3 is INTEGER 2.
    let version = t.expect(0xA0)?;
    let mut v = Reader::new(version);
    if v.expect(TAG_INT)? != [2] || !v.is_empty() {
        return Err(Error::BadVersion);
    }
    let serial = t.expect(TAG_INT)?;
    let inner_alg = t.expect(TAG_SEQ)?;
    if inner_alg != sig_alg_outer {
        // RFC 5280 4.1.1.2: the two algorithm identifiers must be equal.
        return Err(Error::BadValue);
    }
    let issuer = t.expect(TAG_SEQ)?;
    let validity = t.expect(TAG_SEQ)?;
    let subject = t.expect(TAG_SEQ)?;
    let spki = t.expect(TAG_SEQ)?;

    let mut val = Reader::new(validity);
    let (t1, nb) = val.read()?;
    let (t2, na) = val.read()?;
    if !val.is_empty() {
        return Err(Error::TrailingData);
    }
    let not_before = parse_time(t1, nb)?;
    let not_after = parse_time(t2, na)?;
    if not_after < not_before {
        return Err(Error::BadTime);
    }

    // Optional [1] issuerUniqueID, [2] subjectUniqueID, then [3] extensions.
    let _ = t.optional(0xA1)?;
    let _ = t.optional(0xA2)?;
    let mut san = None;
    let mut basic_constraints = None;
    let mut key_usage = None;
    if let Some(ext_wrap) = t.optional(0xA3)? {
        let mut w = Reader::new(ext_wrap);
        let exts = w.expect(TAG_SEQ)?;
        if !w.is_empty() {
            return Err(Error::TrailingData);
        }
        let mut er = Reader::new(exts);
        while !er.is_empty() {
            let ext = er.expect(TAG_SEQ)?;
            let mut e = Reader::new(ext);
            let id = e.expect(TAG_OID)?;
            let _critical = e.optional(TAG_BOOL)?;
            let value = e.expect(TAG_OCTSTR)?;
            if !e.is_empty() {
                return Err(Error::TrailingData);
            }
            if id == oid::SUBJECT_ALT_NAME {
                let mut vr = Reader::new(value);
                san = Some(vr.expect(TAG_SEQ)?);
            } else if id == oid::BASIC_CONSTRAINTS {
                basic_constraints = Some(parse_basic_constraints(value)?);
            } else if id == oid::KEY_USAGE {
                key_usage = Some(parse_key_usage(value)?);
            }
        }
    }
    if !t.is_empty() {
        return Err(Error::TrailingData);
    }

    Ok(Cert {
        der,
        tbs: tbs_full,
        serial,
        issuer,
        subject,
        not_before,
        not_after,
        spki,
        sig_alg: sig_alg_outer,
        san,
        basic_constraints,
        key_usage,
    })
}

fn parse_basic_constraints(value: &[u8]) -> Result<BasicConstraints, Error> {
    let mut r = Reader::new(value);
    let body = r.expect(TAG_SEQ)?;
    if !r.is_empty() {
        return Err(Error::TrailingData);
    }
    let mut b = Reader::new(body);
    let ca = match b.optional(TAG_BOOL)? {
        Some([0x00]) | None => false,
        Some([0xFF]) => true,
        Some(_) => return Err(Error::BadValue),
    };
    let path_len = match b.optional(TAG_INT)? {
        Some(v) if !v.is_empty() && v.len() <= 4 && v[0] & 0x80 == 0 => {
            Some(v.iter().fold(0u32, |a, &x| (a << 8) | u32::from(x)))
        }
        Some(_) => return Err(Error::BadValue),
        None => None,
    };
    if !b.is_empty() {
        return Err(Error::TrailingData);
    }
    Ok(BasicConstraints { ca, path_len })
}

fn parse_key_usage(value: &[u8]) -> Result<u16, Error> {
    let mut r = Reader::new(value);
    let bits = r.expect(TAG_BITSTR)?;
    if !r.is_empty() {
        return Err(Error::TrailingData);
    }
    let (&unused, data) = bits.split_first().ok_or(Error::BadValue)?;
    if unused > 7 || data.is_empty() || data.len() > 2 {
        return Err(Error::BadValue);
    }
    let hi = u16::from(data[0]);
    let lo = u16::from(data.get(1).copied().unwrap_or(0));
    Ok((hi << 8) | lo)
}

/// The first attribute `want` (an OID) of a `Name` (its content bytes), if any.
fn name_attr<'a>(name: &'a [u8], want: &[u8]) -> Option<&'a [u8]> {
    // Name ::= SEQUENCE OF RDN (SET OF AttributeTypeAndValue)
    let mut names = Reader::new(name);
    while !names.is_empty() {
        let (tag, rdn) = names.read().ok()?;
        if tag != TAG_SET {
            return None;
        }
        let mut set = Reader::new(rdn);
        while !set.is_empty() {
            let atv = set.expect(TAG_SEQ).ok()?;
            let mut a = Reader::new(atv);
            let id = a.expect(TAG_OID).ok()?;
            let (_, value) = a.read().ok()?;
            if id == want {
                return Some(value);
            }
        }
    }
    None
}

impl<'a> Cert<'a> {
    /// `issuer == subject` byte for byte (a self-issued certificate; a
    /// self-signed leaf shows up here).
    pub fn is_self_issued(&self) -> bool {
        self.issuer == self.subject
    }

    /// The first `commonName` of the subject, if any (UTF8/Printable bytes).
    pub fn subject_cn(&self) -> Option<&'a [u8]> {
        name_attr(self.subject, oid::COMMON_NAME)
    }

    /// The first `commonName` of the issuer, if any.
    pub fn issuer_cn(&self) -> Option<&'a [u8]> {
        name_attr(self.issuer, oid::COMMON_NAME)
    }

    /// The first `organizationName` of the issuer, if any.
    pub fn issuer_org(&self) -> Option<&'a [u8]> {
        name_attr(self.issuer, oid::ORGANIZATION)
    }

    /// The `dNSName` entries of `subjectAltName` (at most [`MAX_SAN`] are
    /// visited).
    pub fn san_dns_names(&self) -> SanDns<'a> {
        SanDns {
            r: Reader::new(self.san.unwrap_or(&[])),
            left: MAX_SAN,
        }
    }

    pub fn has_san(&self) -> bool {
        self.san.is_some()
    }

    /// Does this certificate name `host`? Matches only against SAN `dNSName`
    /// entries (the CA/Browser Forum rule: the CN is ignored when SAN exists
    /// and modern validators ignore the CN altogether).
    pub fn matches_host(&self, host: &str) -> bool {
        self.san_dns_names()
            .any(|n| dns_name_matches(n, host.as_bytes()))
    }

    /// True when `basicConstraints` says CA and `keyUsage` (if present) allows
    /// signing certificates.
    pub fn may_sign_certs(&self) -> bool {
        self.basic_constraints.is_some_and(|b| b.ca)
            && self.key_usage.is_none_or(|k| k & KU_KEY_CERT_SIGN != 0)
    }

    /// `notBefore <= now <= notAfter`.
    pub fn valid_at(&self, now: u64) -> bool {
        self.not_before <= now && now <= self.not_after
    }
}

/// Iterator over `dNSName` values in a `GeneralNames`.
pub struct SanDns<'a> {
    r: Reader<'a>,
    left: usize,
}

impl<'a> Iterator for SanDns<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<&'a [u8]> {
        while self.left > 0 && !self.r.is_empty() {
            self.left -= 1;
            let (tag, content) = self.r.read().ok()?;
            if tag == 0x82 {
                return Some(content); // [2] IMPLICIT IA5String
            }
        }
        None
    }
}

/// RFC 6125 style match of a certificate `pattern` against a lowercase-insensitive
/// `host`. A wildcard is accepted only as the whole left-most label (`*.a.b`),
/// matches exactly one label, never an IDN `xn--` label prefix trick, and the
/// pattern must have at least two labels after the wildcard so `*.com` is
/// useless. Trailing dots are not accepted in either input.
pub fn dns_name_matches(pattern: &[u8], host: &[u8]) -> bool {
    if pattern.is_empty() || host.is_empty() || pattern.len() > 253 || host.len() > 253 {
        return false;
    }
    if pattern.ends_with(b".") || host.ends_with(b".") {
        return false;
    }
    if !is_dns_chars(pattern, true) || !is_dns_chars(host, false) {
        return false;
    }
    // A dotted-quad is an address, not a name: never matched by a wildcard.
    let ip_like = host.iter().all(|b| b.is_ascii_digit() || *b == b'.');
    if ip_like && pattern.contains(&b'*') {
        return false;
    }
    if let Some(suffix) = pattern.strip_prefix(b"*.") {
        // Two or more labels after the wildcard.
        if !suffix.contains(&b'.') || suffix.contains(&b'*') {
            return false;
        }
        let Some(dot) = host.iter().position(|&b| b == b'.') else {
            return false;
        };
        let (label, rest) = host.split_at(dot);
        return !label.is_empty() && rest[1..].eq_ignore_ascii_case(suffix);
    }
    if pattern.contains(&b'*') {
        return false;
    }
    pattern.eq_ignore_ascii_case(host)
}

fn is_dns_chars(s: &[u8], allow_wildcard: bool) -> bool {
    let mut label_len = 0usize;
    for (i, &b) in s.iter().enumerate() {
        match b {
            b'.' => {
                if label_len == 0 {
                    return false;
                }
                label_len = 0;
            }
            b'*' if allow_wildcard && i == 0 => label_len += 1,
            b'-' | b'_' | b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' => label_len += 1,
            _ => return false,
        }
        if label_len > 63 {
            return false;
        }
    }
    label_len > 0
}

/// Split the concatenated `Certificate` entries of a TLS message into slices;
/// helper for tests and diagnostics. Returns `None` when more than
/// [`MAX_CHAIN_LEN`] certificates are present.
pub fn chain_lens_ok(entries: &[&[u8]]) -> bool {
    !entries.is_empty()
        && entries.len() <= MAX_CHAIN_LEN
        && entries
            .iter()
            .all(|e| !e.is_empty() && e.len() <= MAX_CERT_LEN)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- DER primitives ----

    #[test]
    fn short_and_long_lengths() {
        let mut r = Reader::new(&[0x04, 0x02, 0xAA, 0xBB]);
        assert_eq!(r.read().unwrap(), (0x04, &[0xAA, 0xBB][..]));
        assert!(r.is_empty());
        let mut long = vec![0x04, 0x81, 0x80];
        long.extend(std::iter::repeat_n(7u8, 0x80));
        let mut r = Reader::new(&long);
        assert_eq!(r.read().unwrap().1.len(), 0x80);
    }

    #[test]
    fn rejects_bad_lengths() {
        // indefinite
        assert_eq!(
            Reader::new(&[0x30, 0x80, 0, 0]).read().err(),
            Some(Error::BadLength)
        );
        // 4-byte length
        assert_eq!(
            Reader::new(&[0x04, 0x84, 0, 0, 0, 1, 0]).read().err(),
            Some(Error::BadLength)
        );
        // long form for a short value
        assert_eq!(
            Reader::new(&[0x04, 0x81, 0x05, 0, 0, 0, 0, 0]).read().err(),
            Some(Error::BadLength)
        );
        // leading zero in length bytes
        assert_eq!(
            Reader::new(&[0x04, 0x82, 0x00, 0x90]).read().err(),
            Some(Error::BadLength)
        );
        // truncated content
        assert_eq!(
            Reader::new(&[0x04, 0x05, 1, 2]).read().err(),
            Some(Error::Truncated)
        );
        // truncated header
        assert_eq!(Reader::new(&[0x04]).read().err(), Some(Error::Truncated));
        assert_eq!(Reader::new(&[]).read().err(), Some(Error::Truncated));
        // multi-byte tag
        assert_eq!(
            Reader::new(&[0x1F, 0x01, 0x00]).read().err(),
            Some(Error::BadLength)
        );
    }

    #[test]
    fn huge_length_does_not_overflow() {
        let r = Reader::new(&[0x04, 0x83, 0xFF, 0xFF, 0xFF]).read();
        assert_eq!(r.err(), Some(Error::Truncated));
    }

    #[test]
    fn expect_and_optional() {
        let mut r = Reader::new(&[0x02, 0x01, 0x05, 0x04, 0x00]);
        assert_eq!(r.optional(TAG_BOOL).unwrap(), None);
        assert_eq!(r.expect(TAG_INT).unwrap(), &[5]);
        assert_eq!(r.expect(TAG_INT).err(), Some(Error::UnexpectedTag));
    }

    // ---- time ----

    #[test]
    fn utc_time_pivot() {
        assert_eq!(
            parse_time(TAG_UTCTIME, b"491231235959Z").unwrap(),
            2_524_607_999 // 2049-12-31T23:59:59Z
        );
        // 50..99 pivot to 19xx, which predates the Unix epoch: refused.
        assert!(parse_time(TAG_UTCTIME, b"500101000000Z").is_err());
        assert_eq!(parse_time(TAG_UTCTIME, b"700101000000Z").unwrap(), 0);
    }

    #[test]
    fn generalized_time() {
        assert_eq!(
            parse_time(TAG_GENTIME, b"20240101000000Z").unwrap(),
            crate::unixtime::MIN_PLAUSIBLE_UNIX
        );
        assert_eq!(
            parse_time(TAG_GENTIME, b"99991231235959Z").unwrap(),
            253_402_300_799
        );
    }

    #[test]
    fn bad_times() {
        for s in [
            &b"240101000000"[..], // no Z
            b"2401010000Z",       // no seconds
            b"240230000000Z",     // Feb 30
            b"241301000000Z",     // month 13
            b"240101240000Z",     // hour 24
            b"24010100000aZ",     // non digit
            b"240101000000+0100", // offset
            b"",
        ] {
            assert!(parse_time(TAG_UTCTIME, s).is_err(), "{s:?}");
        }
        assert!(parse_time(TAG_GENTIME, b"2024010100000Z").is_err());
        assert!(parse_time(0x04, b"240101000000Z").is_err());
    }

    // ---- names ----

    #[test]
    fn exact_match_case_insensitive() {
        assert!(dns_name_matches(b"example.com", b"EXAMPLE.com"));
        assert!(!dns_name_matches(b"example.com", b"www.example.com"));
        assert!(!dns_name_matches(b"www.example.com", b"example.com"));
    }

    #[test]
    fn wildcard_one_label_only() {
        assert!(dns_name_matches(b"*.example.com", b"www.example.com"));
        assert!(dns_name_matches(b"*.example.com", b"WWW.Example.COM"));
        assert!(!dns_name_matches(b"*.example.com", b"example.com"));
        assert!(!dns_name_matches(b"*.example.com", b"a.b.example.com"));
        assert!(!dns_name_matches(b"*.example.com", b".example.com"));
    }

    #[test]
    fn wildcard_must_be_whole_leftmost_label() {
        assert!(!dns_name_matches(b"w*.example.com", b"www.example.com"));
        assert!(!dns_name_matches(b"www.*.com", b"www.a.com"));
        assert!(!dns_name_matches(b"*w.example.com", b"www.example.com"));
        assert!(!dns_name_matches(b"**.example.com", b"a.example.com"));
    }

    #[test]
    fn wildcard_needs_two_labels_after() {
        assert!(!dns_name_matches(b"*.com", b"example.com"));
        assert!(!dns_name_matches(b"*.", b"a."));
        assert!(!dns_name_matches(b"*", b"a"));
    }

    #[test]
    fn rejects_malformed_names() {
        assert!(!dns_name_matches(b"", b"a.com"));
        assert!(!dns_name_matches(b"a.com", b""));
        assert!(!dns_name_matches(b"a..com", b"a..com"));
        assert!(!dns_name_matches(b"a.com.", b"a.com."));
        assert!(!dns_name_matches(b"a b.com", b"a b.com"));
        assert!(!dns_name_matches(b"a.com\0.evil.com", b"a.com\0.evil.com"));
        let long = [b'a'; 64];
        let mut name = long.to_vec();
        name.extend_from_slice(b".com");
        assert!(!dns_name_matches(&name, &name));
    }

    #[test]
    fn ip_like_hosts_are_just_names() {
        assert!(dns_name_matches(b"10.0.0.1", b"10.0.0.1"));
        assert!(!dns_name_matches(b"*.0.0.1", b"10.0.0.1")); // no wildcard IPs
    }

    #[test]
    fn chain_limits() {
        let c = [1u8; 10];
        assert!(chain_lens_ok(&[&c]));
        assert!(!chain_lens_ok(&[]));
        let nine: Vec<&[u8]> = (0..9).map(|_| &c[..]).collect();
        assert!(!chain_lens_ok(&nine));
        let big = vec![0u8; MAX_CERT_LEN + 1];
        assert!(!chain_lens_ok(&[&big]));
        assert!(!chain_lens_ok(&[&[]]));
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse(&[]).is_err());
        assert!(parse(&[0x30, 0x00]).is_err());
        assert!(parse(&[0x30, 0x03, 0x30, 0x01, 0x00]).is_err());
        let big = vec![0x30u8; MAX_CERT_LEN + 1];
        assert_eq!(parse(&big).err(), Some(Error::TooLarge));
    }

    #[test]
    fn parse_never_panics_on_prefixes_and_flips() {
        // Truncations and single-byte corruptions of a structurally plausible
        // blob must produce errors, not panics.
        let base: Vec<u8> = (0..600u32).map(|i| (i * 31 % 251) as u8).collect();
        for n in 0..base.len() {
            let _ = parse(&base[..n]);
        }
        let mut seed = 1u32;
        for _ in 0..2000 {
            let mut b = base.clone();
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let i = (seed >> 8) as usize % b.len();
            b[i] = (seed >> 3) as u8;
            let _ = parse(&b);
        }
    }
}
