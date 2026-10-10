//! http (split out of `browser.rs`).

use super::*;

/// Return the body slice of a raw HTTP response (everything past the blank line
/// that ends the headers). If no header terminator is found, the whole input is
/// treated as the body.
pub fn http_body(resp: &[u8]) -> &[u8] {
    if let Some(i) = find(resp, b"\r\n\r\n") {
        &resp[i + 4..]
    } else if let Some(i) = find(resp, b"\n\n") {
        &resp[i + 2..]
    } else {
        resp
    }
}

pub(super) fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// Parse the numeric status code from an HTTP response's status line.
pub fn status_code(resp: &[u8]) -> Option<u16> {
    let line = resp.split(|&b| b == b'\r' || b == b'\n').next()?;
    let code = line.split(|&b| b == b' ').nth(1)?;
    core::str::from_utf8(code).ok()?.parse().ok()
}

/// Look up a response header (case-insensitive), returning its trimmed value.
pub fn header_value<'a>(resp: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    let end = find(resp, b"\r\n\r\n")
        .or_else(|| find(resp, b"\n\n"))
        .unwrap_or(resp.len());
    for line in resp[..end].split(|&b| b == b'\n') {
        let line = trim_ascii(line);
        if let Some(pos) = line.iter().position(|&b| b == b':')
            && line[..pos].eq_ignore_ascii_case(name)
        {
            return Some(trim_ascii(&line[pos + 1..]));
        }
    }
    None
}

pub(super) fn trim_ascii(mut s: &[u8]) -> &[u8] {
    while let [f, rest @ ..] = s {
        if f.is_ascii_whitespace() {
            s = rest;
        } else {
            break;
        }
    }
    while let [rest @ .., l] = s {
        if l.is_ascii_whitespace() {
            s = rest;
        } else {
            break;
        }
    }
    s
}

/// The payload of a raw HTTP response: headers stripped, the chunk framing removed
/// if the response is `Transfer-Encoding: chunked`, and `Content-Encoding: gzip` /
/// `deflate` decompressed (bounded by [`crate::format::gzip::MAX_DECODED_BYTES`]). Strict:
/// a cut or corrupt compressed body is an error. The page view uses the lenient
/// [`page_body_partial`] instead.
pub fn body_bytes(resp: &[u8]) -> Result<alloc::vec::Vec<u8>, crate::format::gzip::EncodingError> {
    let body = http_body(resp);
    let raw = if is_chunked(resp) {
        dechunk(body)
    } else {
        body.to_vec()
    };
    let chain = content_encodings(resp);
    if chain.is_empty() {
        return Ok(raw);
    }
    let d = crate::format::gzip::decode_chain_partial(&chain, &raw)?;
    match d.status {
        crate::format::gzip::Completeness::Complete => Ok(d.data),
        crate::format::gzip::Completeness::Cut => {
            Err(crate::format::gzip::EncodingError::Truncated)
        }
        crate::format::gzip::Completeness::TooLarge => {
            Err(crate::format::gzip::EncodingError::TooLarge)
        }
        crate::format::gzip::Completeness::Damaged => {
            Err(crate::format::gzip::EncodingError::Corrupt)
        }
        crate::format::gzip::Completeness::BadChecksum => {
            Err(crate::format::gzip::EncodingError::BadChecksum)
        }
    }
}

/// `Transfer-Encoding` ends in `chunked` (case-insensitive; `gzip, chunked` too).
pub(super) fn is_chunked(resp: &[u8]) -> bool {
    header_value(resp, b"transfer-encoding")
        .and_then(|v| v.rsplit(|&b| b == b',').next())
        .is_some_and(|last| last.trim_ascii().eq_ignore_ascii_case(b"chunked"))
}

/// The `Content-Encoding` codings, in the order the server applied them
/// (empty = identity).
pub(super) fn content_encodings(resp: &[u8]) -> alloc::vec::Vec<crate::format::gzip::Encoding> {
    header_value(resp, b"content-encoding")
        .map(crate::format::gzip::Encoding::parse_chain)
        .unwrap_or_default()
}

impl PageNote {
    /// Catalog key of the banner text.
    pub fn label_key(self) -> &'static str {
        match self {
            PageNote::Truncated => tk!("web.note.truncated"),
            PageNote::Incomplete => tk!("web.note.incomplete"),
            PageNote::Damaged => tk!("web.note.damaged"),
            PageNote::BadChecksum => tk!("web.note.checksum"),
        }
    }

    /// Banner text in the language in effect.
    pub fn label(self) -> &'static str {
        i18n::tr(self.label_key())
    }
}

/// Like [`body_bytes`] but never throws away what arrived: a body that is cut
/// (`cut`: the fetch layer stopped reading at the size cap), declared longer than
/// received (`Content-Length`, or a chunked stream without its last chunk) or
/// whose compressed data is damaged yields the decoded prefix plus a [`PageNote`].
/// `Err` only when nothing at all could be decoded (unsupported coding, not a
/// gzip stream, damaged from the first byte).
pub fn body_partial(
    resp: &[u8],
    cut: bool,
) -> Result<PageBody, crate::format::gzip::EncodingError> {
    use crate::format::gzip::Completeness;
    let body = http_body(resp);
    let chunked = is_chunked(resp);
    let (raw, mut whole) = if chunked {
        dechunk_checked(body)
    } else {
        (body.to_vec(), true)
    };
    if !chunked
        && let Some(want) = header_value(resp, b"content-length")
            .and_then(|v| core::str::from_utf8(v).ok())
            .and_then(|v| v.trim().parse::<usize>().ok())
    {
        whole &= raw.len() >= want;
    }
    let chain = content_encodings(resp);
    let (data, status) = if chain.is_empty() {
        (raw, Completeness::Complete)
    } else {
        let d = crate::format::gzip::decode_chain_partial(&chain, &raw)?;
        (d.data, d.status)
    };
    let note = match status {
        Completeness::Complete if cut => Some(PageNote::Truncated),
        Completeness::Complete if !whole => Some(PageNote::Incomplete),
        Completeness::Complete => None,
        Completeness::TooLarge => Some(PageNote::Truncated),
        // The compressed bytes ran out: because we stopped reading, or because the
        // connection ended (a cut stream the server itself did not finish).
        Completeness::Cut | Completeness::Damaged if cut => Some(PageNote::Truncated),
        Completeness::Cut => Some(PageNote::Incomplete),
        Completeness::Damaged => Some(PageNote::Damaged),
        Completeness::BadChecksum if cut => Some(PageNote::Truncated),
        Completeness::BadChecksum => Some(PageNote::BadChecksum),
    };
    Ok(PageBody { body: data, note })
}

/// The page to render for a raw response: [`body_partial`], with a body that
/// cannot be decoded at all turned into a short explanatory page. `cut` is the
/// fetch layer's "stopped at the size cap" flag.
pub fn page_body_partial(resp: &[u8], cut: bool) -> PageBody {
    match body_partial(resp, cut) {
        Ok(p) => p,
        Err(e) => {
            let key = match e {
                crate::format::gzip::EncodingError::TooLarge => tk!("web.body.too_big"),
                _ if content_encodings(resp)
                    .contains(&crate::format::gzip::Encoding::Unsupported) =>
                {
                    tk!("web.body.encoding")
                }
                _ => tk!("web.body.damaged"),
            };
            PageBody {
                body: alloc::format!("<p>{}</p>", html_escape(i18n::tr(key))).into_bytes(),
                note: cut.then_some(PageNote::Truncated),
            }
        }
    }
}

/// The clean HTML body of a raw HTTP response ([`page_body_partial`] without the
/// cut flag and the note); a body that cannot be decoded at all becomes a short
/// explanatory page instead of garbage.
pub fn page_body(resp: &[u8]) -> alloc::vec::Vec<u8> {
    page_body_partial(resp, false).body
}

/// Decode an HTTP/1.1 chunked body into the raw payload.
pub(super) fn dechunk(body: &[u8]) -> alloc::vec::Vec<u8> {
    dechunk_checked(body).0
}

/// [`dechunk`] plus whether the terminating zero-size chunk was seen (a body cut
/// anywhere before it is `false`).
pub(super) fn dechunk_checked(body: &[u8]) -> (alloc::vec::Vec<u8>, bool) {
    let mut out = alloc::vec::Vec::new();
    let mut i = 0;
    while i < body.len() {
        // Chunk size in hex up to CR/LF or ';' (chunk extensions).
        let mut size = 0usize;
        let mut saw_digit = false;
        while i < body.len() {
            let c = body[i];
            if let Some(d) = (c as char).to_digit(16) {
                // The size is attacker-controlled: an absurdly long hex run
                // must end decoding, not overflow (and later wrap `i + size`).
                match size.checked_mul(16).and_then(|v| v.checked_add(d as usize)) {
                    Some(v) => size = v,
                    None => return (out, false),
                }
                saw_digit = true;
                i += 1;
            } else {
                break;
            }
        }
        if !saw_digit {
            return (out, false);
        }
        // Skip to end of the size line.
        while i < body.len() && body[i] != b'\n' {
            i += 1;
        }
        i += 1; // past '\n'
        if size == 0 {
            return (out, true);
        }
        if i >= body.len() {
            return (out, false);
        }
        // `i < body.len()` here, so this cannot underflow or overflow.
        let end = i + size.min(body.len() - i);
        out.extend_from_slice(&body[i..end]);
        i = end;
        // Skip the CRLF after the chunk data.
        while i < body.len() && (body[i] == b'\r' || body[i] == b'\n') {
            i += 1;
        }
    }
    (out, false)
}

/// Fold a Unicode code point to a single printable ASCII byte for our bitmap
/// font (which only has ASCII). Accented Latin letters collapse to their base
/// letter; a few punctuation marks map to ASCII look-alikes. Returns `None` for
/// code points with no sensible ASCII rendering (the caller drops them).
pub fn fold_ascii(cp: u32) -> Option<u8> {
    if (0x20..0x7f).contains(&cp) {
        return Some(cp as u8);
    }
    let b = match cp {
        0x00A0 => b' ',                                              // nbsp
        0x00C0..=0x00C5 | 0x00E0..=0x00E5 => b'a',                   // À-Å à-å
        0x00C7 | 0x00E7 => b'c',                                     // Ç ç
        0x00C8..=0x00CB | 0x00E8..=0x00EB => b'e',                   // È-Ë è-ë
        0x00CC..=0x00CF | 0x00EC..=0x00EF => b'i',                   // Ì-Ï ì-ï
        0x00D1 | 0x00F1 => b'n',                                     // Ñ ñ
        0x00D2..=0x00D6 | 0x00D8 | 0x00F2..=0x00F6 | 0x00F8 => b'o', // Ò-Ö Ø ò-ö ø
        0x00D9..=0x00DC | 0x00F9..=0x00FC => b'u',                   // Ù-Ü ù-ü
        0x00DD | 0x00FD | 0x00FF => b'y',                            // Ý ý ÿ
        0x2018 | 0x2019 | 0x201B => b'\'',                           // ' ' ‛
        0x201C | 0x201D => b'"',                                     // " "
        0x2013 | 0x2014 | 0x2212 => b'-',                            // – — −
        0x2026 => b'.',                                              // …
        0x00A9 => b'c',                                              // ©
        0x00AE => b'r',                                              // ®
        _ => return None,
    };
    Some(b)
}

/// Decode an HTML entity. `name` is the text between `&` and `;`. Returns the
/// decoded byte (ASCII-folded), or `None` to keep the literal text.
pub fn decode_entity(name: &[u8]) -> Option<u8> {
    match name {
        b"amp" => Some(b'&'),
        b"lt" => Some(b'<'),
        b"gt" => Some(b'>'),
        b"quot" | b"ldquo" | b"rdquo" => Some(b'"'),
        b"apos" | b"lsquo" | b"rsquo" => Some(b'\''),
        b"nbsp" => Some(b' '),
        b"copy" => Some(b'c'),
        b"reg" => Some(b'r'),
        b"hellip" => Some(b'.'),
        b"mdash" | b"ndash" => Some(b'-'),
        b"aacute" | b"agrave" | b"acirc" | b"atilde" | b"auml" => Some(b'a'),
        b"eacute" | b"egrave" | b"ecirc" | b"euml" => Some(b'e'),
        b"iacute" | b"igrave" | b"icirc" | b"iuml" => Some(b'i'),
        b"oacute" | b"ograve" | b"ocirc" | b"otilde" | b"ouml" => Some(b'o'),
        b"uacute" | b"ugrave" | b"ucirc" | b"uuml" => Some(b'u'),
        b"ccedil" => Some(b'c'),
        b"ntilde" => Some(b'n'),
        _ => {
            // Numeric entity: &#NN; (decimal) or &#xHH; (hex).
            if let [b'#', rest @ ..] = name
                && !rest.is_empty()
            {
                let cp = if let [b'x' | b'X', hex @ ..] = rest {
                    parse_radix(hex, 16)
                } else {
                    parse_radix(rest, 10)
                };
                return cp.and_then(fold_ascii);
            }
            None
        }
    }
}

/// Parse `digits` in `radix` (10 or 16), or `None` on any invalid digit.
pub(super) fn parse_radix(digits: &[u8], radix: u32) -> Option<u32> {
    if digits.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    for &d in digits {
        let n = (d as char).to_digit(radix)?;
        v = v.checked_mul(radix)?.checked_add(n)?;
    }
    Some(v)
}

/// Extract readable, word-wrapped text from an HTML document into `out`.
/// Strips tags and `<script>`/`<style>` bodies, decodes common entities,
/// collapses runs of whitespace, and inserts line breaks at block boundaries.
/// Returns the number of bytes written.
/// Decode the first UTF-8 scalar in `bytes`, returning its code point and the
/// number of bytes consumed. Malformed input yields `(0, 1)` so the caller
/// skips one byte and makes progress; empty input yields `(0, 0)`.
pub fn decode_utf8(bytes: &[u8]) -> (u32, usize) {
    let Some(&b0) = bytes.first() else {
        return (0, 0);
    };
    let (len, init) = match b0 {
        0x00..=0x7f => return (b0 as u32, 1),
        0xC0..=0xDF => (2, (b0 & 0x1F) as u32),
        0xE0..=0xEF => (3, (b0 & 0x0F) as u32),
        0xF0..=0xF7 => (4, (b0 & 0x07) as u32),
        _ => return (0, 1),
    };
    if bytes.len() < len {
        return (0, 1);
    }
    let mut cp = init;
    for &b in &bytes[1..len] {
        if b & 0xC0 != 0x80 {
            return (0, 1); // not a continuation byte
        }
        cp = (cp << 6) | (b & 0x3F) as u32;
    }
    (cp, len)
}
