//! Native browser logic: URL parsing, search-URL building, HTML→text
//! extraction, and the address-bar + content model.
//!
//! Pure and allocation-free (fixed buffers), so the whole parser and editing
//! model is host-testable. The kernel supplies only the networking: it pulls a
//! pending request out with [`Browser::take_request`], fetches the bytes, and
//! fetches the bytes; the kernel renders them with the `web` engine.

pub mod cert;
pub mod errors;
pub mod motion;
pub mod tabs;

pub use cert::CertInfo;

use crate::Key;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

/// Max bytes of a URL (address bar + resolved navigation target).
pub const URL_CAP: usize = 480;
/// Max host length.
pub const HOST_CAP: usize = 80;

/// Maximum bytes of one HTTP(S) response accepted from the network (headers +
/// body, still compressed). Shared by the plain-HTTP and the TLS path so neither
/// can be talked into exhausting the kernel's single heap. An oversized response
/// is cut at this size and the page is flagged as truncated (a cut gzip/deflate
/// body still renders its decoded prefix, see [`page_body_partial`]).
///
/// Memory budget of one page load, worst case, of the 64 MiB heap (freed once the
/// DOM exists; the DOM itself is bounded by `web::MAX_NODES`): the raw response
/// (this, 1 MiB) + its de-chunked copy (1 MiB) + the decoded body
/// ([`crate::gzip::MAX_DECODED_BYTES`], 4 MiB, up to 2x transient `Vec` slack)
/// ~ 12 MiB. It was 256 KiB, which real pages (a gzip home page of a CDN vendor
/// is ~300 KiB on the wire) overflowed.
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Append `data` to `out`, never letting `out` grow past `cap` bytes. Returns
/// `true` when some of `data` had to be dropped (the response is truncated).
pub fn append_capped(out: &mut alloc::vec::Vec<u8>, data: &[u8], cap: usize) -> bool {
    let room = cap.saturating_sub(out.len());
    let take = data.len().min(room);
    out.extend_from_slice(&data[..take]);
    take < data.len()
}

/// What the browser can honestly say about the connection that produced the
/// page on screen. "Secure" exists only as [`Security::HttpsVerified`], which
/// the kernel may report only after the server's certificate chain was
/// validated against the embedded trust store, the host name matched and the
/// handshake signature verified; there is no way to reach it without that.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Security {
    /// Nothing loaded (start page), or an `https://` load still in flight.
    None,
    /// Plain `http://`: not encrypted.
    Http,
    /// `https://` with a verified certificate chain for this host.
    HttpsVerified,
    /// `https://` where verification failed and the user chose to continue
    /// anyway for this origin, for this session only: encrypted but the peer
    /// is not authenticated.
    HttpsInvalid,
}

impl Security {
    /// Short label for the address bar, `None` when there is nothing to say.
    /// ASCII only (the bitmap font has no accents).
    pub fn label(self) -> Option<&'static str> {
        match self {
            Security::None => None,
            Security::Http => Some("Nao seguro"),
            Security::HttpsVerified => Some("Conexão segura"),
            Security::HttpsInvalid => Some("Certificado inválido"),
        }
    }
}

/// How a loaded page actually arrived, reported by the kernel's fetcher.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Conn {
    /// Plain `http://`.
    Plain,
    /// TLS with a validated chain and a matching host name.
    Verified,
    /// TLS where validation failed and the user allowed this origin.
    Insecure,
}

/// Why a navigation failed, so the UI can say more than "failed".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailReason {
    /// DNS, connect, TLS or timeout: no usable response (cause not known).
    Network,
    /// The host name does not resolve.
    Dns,
    /// The server refused the TCP connection.
    Refused,
    /// No answer in time (connect, handshake or response).
    Timeout,
    /// The TLS handshake failed for a reason other than the certificate.
    Tls,
    /// The server certificate was refused.
    Cert(crate::tlsverify::CertError),
    /// A redirect tried to move from `https://` to `http://`; blocked.
    RedirectDowngrade,
    /// A redirect `Location` was malformed or unsupported.
    RedirectInvalid,
    /// A redirect pointed back at a URL already visited in this navigation.
    RedirectLoop,
    /// More than [`crate::redirect::MAX_REDIRECTS`] redirects.
    TooManyRedirects,
    /// The background fetcher thread died (panic or CPU fault) and cannot serve requests.
    WorkerDied,
}

impl FailReason {
    /// One-line message for the page area (ASCII only).
    pub fn message(self) -> &'static str {
        match self {
            FailReason::Network => "Falha ao carregar a pagina.",
            FailReason::Dns => "Nome nao encontrado: confira o endereco (DNS).",
            FailReason::Refused => "Conexão recusada pelo servidor.",
            FailReason::Timeout => "Tempo esgotado: o servidor nao respondeu.",
            FailReason::Tls => "Falha na negociacao TLS (conexao segura).",
            FailReason::Cert(e) => e.page_message(),
            FailReason::RedirectDowngrade => "Bloqueado: redirecionamento de HTTPS para HTTP.",
            FailReason::RedirectInvalid => "Redirecionamento invalido.",
            FailReason::RedirectLoop => "Redirecionamento em ciclo.",
            FailReason::TooManyRedirects => "Redirecionamentos demais.",
            FailReason::WorkerDied => "O carregador de paginas falhou (thread encerrada).",
        }
    }

    /// Map a refused redirect onto the reason shown to the user.
    pub fn from_redirect(e: crate::redirect::RedirectError) -> Self {
        use crate::redirect::RedirectError as E;
        match e {
            E::Invalid => FailReason::RedirectInvalid,
            E::Downgrade => FailReason::RedirectDowngrade,
            E::Loop => FailReason::RedirectLoop,
            E::TooMany => FailReason::TooManyRedirects,
        }
    }
}

/// Where a fetch stands, surfaced in the UI.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Idle,
    Loading,
    Done,
    Error,
}

// ---- URL parsing ----

/// A parsed absolute URL split into fixed buffers.
pub struct Url {
    pub https: bool,
    pub port: u16,
    host: [u8; HOST_CAP],
    host_len: usize,
    path: [u8; URL_CAP],
    path_len: usize,
}

impl Url {
    pub fn host(&self) -> &[u8] {
        &self.host[..self.host_len]
    }
    pub fn path(&self) -> &[u8] {
        &self.path[..self.path_len]
    }
}

fn starts_with_ci(s: &[u8], prefix: &[u8]) -> bool {
    s.len() >= prefix.len()
        && s[..prefix.len()]
            .iter()
            .zip(prefix)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// True for ASCII control bytes (C0 and DEL). A URL carrying one could split
/// the request line / smuggle a header once host and path are written into the
/// HTTP request.
fn is_control(b: u8) -> bool {
    b < 0x20 || b == 0x7f
}

/// Parse an absolute URL. A missing scheme defaults to HTTPS. Returns `None`
/// when there is no host at all, or when the URL contains an ASCII control
/// character (CR/LF/NUL/TAB...) after the leading blanks: such input is never
/// a legitimate address and would let a hostile `Location` inject headers.
pub fn parse_url(input: &[u8]) -> Option<Url> {
    let mut s = input;
    while let [b' ' | b'\t', rest @ ..] = s {
        s = rest;
    }
    if s.iter().any(|&b| is_control(b)) {
        return None;
    }

    let (https, mut rest) = if starts_with_ci(s, b"https://") {
        (true, &s[8..])
    } else if starts_with_ci(s, b"http://") {
        (false, &s[7..])
    } else {
        (true, s)
    };

    // Host ends at the first '/', ':' or '?'.
    let mut host = [0u8; HOST_CAP];
    let mut host_len = 0;
    while let [c, tail @ ..] = rest {
        if matches!(c, b'/' | b':' | b'?' | b' ') {
            break;
        }
        if host_len < HOST_CAP {
            host[host_len] = *c;
            host_len += 1;
        }
        rest = tail;
    }
    if host_len == 0 {
        return None;
    }

    // Optional explicit port.
    let mut port = if https { 443u16 } else { 80u16 };
    if let [b':', tail @ ..] = rest {
        rest = tail;
        let mut p = 0u32;
        while let [c @ b'0'..=b'9', tt @ ..] = rest {
            // Saturate: anything above 65535 is rejected below anyway.
            p = p.saturating_mul(10).saturating_add((*c - b'0') as u32);
            rest = tt;
        }
        if p > 0 && p <= 65535 {
            port = p as u16;
        }
    }

    // Path (everything else); default "/".
    let mut path = [0u8; URL_CAP];
    let mut path_len = 0;
    if matches!(rest.first(), Some(b'/') | Some(b'?')) {
        for &c in rest {
            if c == b' ' {
                break;
            }
            if path_len < URL_CAP {
                path[path_len] = c;
                path_len += 1;
            }
        }
    }
    if path_len == 0 {
        path[0] = b'/';
        path_len = 1;
    }

    Some(Url {
        https,
        port,
        host,
        host_len,
        path,
        path_len,
    })
}

/// True when `input` looks like a navigable address (has a dot in the host part
/// and no spaces) rather than a free-text search query.
pub fn looks_like_url(input: &[u8]) -> bool {
    let s = input.trim_ascii();
    if s.is_empty() {
        return false;
    }
    if starts_with_ci(s, b"http://") || starts_with_ci(s, b"https://") {
        return true;
    }
    // No spaces, and a dot before any slash → domain-like.
    let host_part = s.split(|&c| c == b'/').next().unwrap_or(s);
    !s.contains(&b' ') && host_part.contains(&b'.')
}

/// Percent-encode `query` into `out` as an `application/x-www-form-urlencoded`
/// value (spaces become `+`). Returns the number of bytes written.
pub fn encode_query(query: &[u8], out: &mut [u8]) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut n = 0;
    let push = |b: u8, out: &mut [u8], n: &mut usize| {
        if *n < out.len() {
            out[*n] = b;
            *n += 1;
        }
    };
    for &c in query {
        match c {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                push(c, out, &mut n)
            }
            b' ' => push(b'+', out, &mut n),
            _ => {
                push(b'%', out, &mut n);
                push(HEX[(c >> 4) as usize], out, &mut n);
                push(HEX[(c & 0xF) as usize], out, &mut n);
            }
        }
    }
    n
}

/// Build the Bing search URL for `query` into `out`. Bing's TLS 1.3 endpoint
/// accepts our P-256 / AES-128-GCM handshake (DuckDuckGo's Azure edge rejects
/// it) and returns a plain `200 OK` HTML page our extractor handles. Returns
/// the URL length.
pub fn build_search_url(query: &[u8], out: &mut [u8]) -> usize {
    let prefix = b"https://www.bing.com/search?q=";
    let mut n = 0;
    for &b in prefix {
        if n < out.len() {
            out[n] = b;
            n += 1;
        }
    }
    n + encode_query(query, &mut out[n..])
}

// ---- HTTP / HTML ----

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

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
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

fn trim_ascii(mut s: &[u8]) -> &[u8] {
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
/// `deflate` decompressed (bounded by [`crate::gzip::MAX_DECODED_BYTES`]). Strict:
/// a cut or corrupt compressed body is an error. The page view uses the lenient
/// [`page_body_partial`] instead.
pub fn body_bytes(resp: &[u8]) -> Result<alloc::vec::Vec<u8>, crate::gzip::EncodingError> {
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
    let d = crate::gzip::decode_chain_partial(&chain, &raw)?;
    match d.status {
        crate::gzip::Completeness::Complete => Ok(d.data),
        crate::gzip::Completeness::Cut => Err(crate::gzip::EncodingError::Truncated),
        crate::gzip::Completeness::TooLarge => Err(crate::gzip::EncodingError::TooLarge),
        crate::gzip::Completeness::Damaged => Err(crate::gzip::EncodingError::Corrupt),
        crate::gzip::Completeness::BadChecksum => Err(crate::gzip::EncodingError::BadChecksum),
    }
}

/// `Transfer-Encoding` ends in `chunked` (case-insensitive; `gzip, chunked` too).
fn is_chunked(resp: &[u8]) -> bool {
    header_value(resp, b"transfer-encoding")
        .and_then(|v| v.rsplit(|&b| b == b',').next())
        .is_some_and(|last| last.trim_ascii().eq_ignore_ascii_case(b"chunked"))
}

/// The `Content-Encoding` codings, in the order the server applied them
/// (empty = identity).
fn content_encodings(resp: &[u8]) -> alloc::vec::Vec<crate::gzip::Encoding> {
    header_value(resp, b"content-encoding")
        .map(crate::gzip::Encoding::parse_chain)
        .unwrap_or_default()
}

/// Why a page body on screen is not the whole document. Shown as a banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageNote {
    /// Cut at [`MAX_RESPONSE_BYTES`] (or the decoded size limit).
    Truncated,
    /// The connection ended before the body did.
    Incomplete,
    /// The compressed data went bad; the part decoded before that is shown.
    Damaged,
    /// Fully decoded, but the gzip/zlib checksum did not match.
    BadChecksum,
}

impl PageNote {
    /// Banner text.
    pub fn label(self) -> &'static str {
        match self {
            PageNote::Truncated => "Página cortada no limite de tamanho",
            PageNote::Incomplete => "Página incompleta: a conexão foi interrompida",
            PageNote::Damaged => "Página parcial: os dados recebidos estão corrompidos",
            PageNote::BadChecksum => "A verificação da página falhou",
        }
    }
}

/// A decoded page body plus, when it is not the whole document, why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageBody {
    pub body: alloc::vec::Vec<u8>,
    pub note: Option<PageNote>,
}

/// Like [`body_bytes`] but never throws away what arrived: a body that is cut
/// (`cut`: the fetch layer stopped reading at the size cap), declared longer than
/// received (`Content-Length`, or a chunked stream without its last chunk) or
/// whose compressed data is damaged yields the decoded prefix plus a [`PageNote`].
/// `Err` only when nothing at all could be decoded (unsupported coding, not a
/// gzip stream, damaged from the first byte).
pub fn body_partial(resp: &[u8], cut: bool) -> Result<PageBody, crate::gzip::EncodingError> {
    use crate::gzip::Completeness;
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
        let d = crate::gzip::decode_chain_partial(&chain, &raw)?;
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
            let msg: &[u8] = match e {
                crate::gzip::EncodingError::TooLarge => {
                    b"<p>Pagina descompactada grande demais.</p>"
                }
                _ if content_encodings(resp).contains(&crate::gzip::Encoding::Unsupported) => {
                    b"<p>Codificacao de conteudo nao suportada.</p>"
                }
                _ => b"<p>Falha ao descompactar a pagina (dados corrompidos ou cortados).</p>",
            };
            PageBody {
                body: msg.to_vec(),
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
fn dechunk(body: &[u8]) -> alloc::vec::Vec<u8> {
    dechunk_checked(body).0
}

/// [`dechunk`] plus whether the terminating zero-size chunk was seen (a body cut
/// anywhere before it is `false`).
fn dechunk_checked(body: &[u8]) -> (alloc::vec::Vec<u8>, bool) {
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
fn parse_radix(digits: &[u8], radix: u32) -> Option<u32> {
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

// ---- favourites, suggestions, internal pages ----

/// Scheme of the browser's own pages.
pub const INTERNAL_SCHEME: &[u8] = b"osjeff:";

/// Most favourites kept.
pub const MAX_BOOKMARKS: usize = 64;
/// Most suggestions shown under the address bar.
pub const MAX_SUGGESTIONS: usize = 6;

/// A favourite: an absolute URL and the page's title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    pub url: String,
    pub title: String,
}

/// Where favourites live. The browser only talks to this trait, so persistence
/// (a file on the filesystem) is a matter of giving [`Browser::with_store`] another
/// implementation; [`MemoryBookmarks`] forgets everything at power-off.
pub trait BookmarkStore {
    /// Every favourite, oldest first.
    fn all(&self) -> Vec<Bookmark>;
    /// Add one; `false` when it is already there or the store is full.
    fn add(&mut self, b: Bookmark) -> bool;
    /// Remove the favourite with this URL; `false` when there is none.
    fn remove(&mut self, url: &str) -> bool;
    fn contains(&self, url: &str) -> bool {
        self.all().iter().any(|b| b.url == url)
    }
}

/// Favourites in memory, at most [`MAX_BOOKMARKS`].
#[derive(Default)]
pub struct MemoryBookmarks {
    items: Vec<Bookmark>,
}

impl BookmarkStore for MemoryBookmarks {
    fn all(&self) -> Vec<Bookmark> {
        self.items.clone()
    }
    fn add(&mut self, b: Bookmark) -> bool {
        if self.items.len() >= MAX_BOOKMARKS || self.items.iter().any(|x| x.url == b.url) {
            return false;
        }
        self.items.push(b);
        true
    }
    fn remove(&mut self, url: &str) -> bool {
        let n = self.items.len();
        self.items.retain(|b| b.url != url);
        self.items.len() != n
    }
    fn contains(&self, url: &str) -> bool {
        self.items.iter().any(|b| b.url == url)
    }
}

/// The text form of a favourites list: one `url<TAB>title` line each (tabs and
/// newlines inside a title become spaces).
pub fn bookmarks_to_text(items: &[Bookmark]) -> String {
    let mut out = String::new();
    for b in items {
        let clean = |s: &str| -> String {
            s.chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect()
        };
        out.push_str(&clean(&b.url));
        out.push('\t');
        out.push_str(&clean(&b.title));
        out.push('\n');
    }
    out
}

/// Parse [`bookmarks_to_text`] output. Total: bad UTF-8, lines without a URL and
/// duplicates are skipped, at most [`MAX_BOOKMARKS`] are kept.
pub fn bookmarks_from_text(text: &[u8]) -> Vec<Bookmark> {
    let mut items: Vec<Bookmark> = Vec::new();
    for line in text.split(|&b| b == b'\n') {
        let Ok(line) = core::str::from_utf8(line) else {
            continue;
        };
        let (url, title) = line.split_once('\t').unwrap_or((line, ""));
        let url = url.trim();
        if url.is_empty() || items.iter().any(|b| b.url == url) {
            continue;
        }
        if items.len() >= MAX_BOOKMARKS {
            break;
        }
        items.push(Bookmark {
            url: String::from(url),
            title: String::from(title.trim()),
        });
    }
    items
}

/// Favourites that write themselves out after every change: `save` receives the
/// text form of the whole list (the kernel stores it as a file). A failed save
/// keeps the change in memory.
pub struct SavedBookmarks<F: FnMut(&[u8])> {
    inner: MemoryBookmarks,
    save: F,
}

impl<F: FnMut(&[u8])> SavedBookmarks<F> {
    /// Start from the saved `text` (as read from the file, possibly empty or damaged).
    pub fn load(text: &[u8], save: F) -> Self {
        let mut inner = MemoryBookmarks::default();
        for b in bookmarks_from_text(text) {
            inner.add(b);
        }
        Self { inner, save }
    }

    fn flush(&mut self) {
        let text = bookmarks_to_text(&self.inner.items);
        (self.save)(text.as_bytes());
    }
}

impl<F: FnMut(&[u8])> BookmarkStore for SavedBookmarks<F> {
    fn all(&self) -> Vec<Bookmark> {
        self.inner.all()
    }
    fn add(&mut self, b: Bookmark) -> bool {
        let ok = self.inner.add(b);
        if ok {
            self.flush();
        }
        ok
    }
    fn remove(&mut self, url: &str) -> bool {
        let ok = self.inner.remove(url);
        if ok {
            self.flush();
        }
        ok
    }
    fn contains(&self, url: &str) -> bool {
        self.inner.contains(url)
    }
}

/// One line of the suggestion list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub url: String,
    /// Text shown: the page title for a favourite, else the URL.
    pub label: String,
    pub bookmark: bool,
}

/// An address without its scheme and a leading `www.`, lower-cased, for matching.
fn bare(url: &str) -> String {
    let l = url.trim().to_ascii_lowercase();
    let l = l
        .strip_prefix("https://")
        .or_else(|| l.strip_prefix("http://"))
        .unwrap_or(&l);
    l.strip_prefix("www.").unwrap_or(l).into()
}

/// Rank `url`/`title` against the query: 0 = prefix of the address, 1 = contained in the
/// address or the title, `None` = no match.
fn rank(q: &str, url: &str, title: &str) -> Option<u8> {
    if bare(url).starts_with(q) {
        Some(0)
    } else if bare(url).contains(q) || title.to_ascii_lowercase().contains(q) {
        Some(1)
    } else {
        None
    }
}

/// Build the suggestion list for `query` from favourites and history (newest first).
pub fn suggest(query: &str, bookmarks: &[Bookmark], history: &[String]) -> Vec<Suggestion> {
    let q = bare(query);
    if q.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<Suggestion> = Vec::new();
    for want in [0u8, 1] {
        for b in bookmarks {
            if rank(&q, &b.url, &b.title) == Some(want) && !out.iter().any(|s| s.url == b.url) {
                out.push(Suggestion {
                    url: b.url.clone(),
                    label: if b.title.is_empty() {
                        b.url.clone()
                    } else {
                        b.title.clone()
                    },
                    bookmark: true,
                });
            }
        }
        for u in history {
            if rank(&q, u, "") == Some(want) && !out.iter().any(|s| s.url == *u) {
                out.push(Suggestion {
                    url: u.clone(),
                    label: u.clone(),
                    bookmark: false,
                });
            }
        }
    }
    // A single suggestion that is exactly what was typed adds nothing.
    if out.len() == 1 && bare(&out[0].url) == q {
        out.clear();
    }
    out.truncate(MAX_SUGGESTIONS);
    out
}

/// Escape `&`, `<`, `>`, `"` for HTML text and attribute values.
pub fn html_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}

// ---- the address-bar model ----

/// The browser app's editable address bar and navigation state. The rendered
/// page itself is produced by the `web` engine and owned by the kernel; this
/// only tracks the URL input, status, and start-page flag. The kernel drives
/// networking via [`take_request`].
pub struct Browser {
    url: [u8; URL_CAP],
    url_len: usize,
    caret: usize,
    status: Status,
    pending: bool,
    nav: [u8; URL_CAP],
    nav_len: usize,
    home: bool, // showing the native start page (no page loaded)
    security: Security,
    note: Option<PageNote>,
    fail_reason: FailReason,
    insecure: Rc<RefCell<InsecureHosts>>,
    history: History,
    bookmarks: Rc<RefCell<Box<dyn BookmarkStore>>>,
    /// Keyboard focus is in the address bar (else on the page).
    bar_focus: bool,
    /// Highlighted suggestion (Up/Down in the address bar).
    sugg_sel: Option<usize>,
    /// Esc closed the suggestion list; it stays closed until the text changes.
    sugg_dismissed: bool,
    /// The page on screen is an `osjeff://` page generated by the browser.
    internal: bool,
    /// An internal page was just opened: the kernel must fetch its HTML with
    /// [`Browser::take_internal`].
    internal_ready: bool,
    /// `<title>` of the page on screen (set by the kernel after layout).
    page_title: String,
    /// The whole address is selected (Ctrl+L, a click on the bar): the next edit replaces it.
    bar_selected: bool,
}

/// Pages kept in the in-memory history (the oldest is dropped when full).
pub const MAX_HISTORY: usize = 64;

/// Back/forward list: absolute URLs, the cursor on the page being shown. Never
/// persisted (a store behind a trait comes with the persistent filesystem).
#[derive(Default)]
struct History {
    urls: alloc::vec::Vec<alloc::vec::Vec<u8>>,
    /// Index of the current entry (meaningful when `urls` is not empty).
    cur: usize,
    /// The next successful load comes from back/forward: do not record it.
    replay: bool,
}

impl History {
    /// Record a finished navigation to `url`: drops the forward entries, ignores
    /// a reload of the same page and honors a pending back/forward replay.
    fn record(&mut self, url: &[u8]) {
        if core::mem::take(&mut self.replay) {
            return;
        }
        if self.urls.get(self.cur).is_some_and(|u| u == url) {
            return;
        }
        if !self.urls.is_empty() {
            self.urls.truncate(self.cur + 1);
        }
        if self.urls.len() == MAX_HISTORY {
            self.urls.remove(0);
        }
        self.urls.push(url.to_vec());
        self.cur = self.urls.len() - 1;
    }
}

/// Most origins the user can allow to continue past a certificate error in one
/// session.
pub const MAX_INSECURE_HOSTS: usize = 8;

/// Hosts the user chose to open despite a certificate error. Lives only in
/// memory: it is never saved, so it ends with the browser window's state.
struct InsecureHosts {
    hosts: [[u8; HOST_CAP]; MAX_INSECURE_HOSTS],
    lens: [u8; MAX_INSECURE_HOSTS],
    n: usize,
}

impl InsecureHosts {
    const fn new() -> Self {
        Self {
            hosts: [[0; HOST_CAP]; MAX_INSECURE_HOSTS],
            lens: [0; MAX_INSECURE_HOSTS],
            n: 0,
        }
    }

    fn find(&self, host: &[u8]) -> Option<usize> {
        (0..self.n).find(|&i| self.hosts[i][..usize::from(self.lens[i])].eq_ignore_ascii_case(host))
    }

    /// Remember `host` (the oldest entry is dropped when full).
    fn add(&mut self, host: &[u8]) {
        if host.is_empty() || host.len() > HOST_CAP || self.find(host).is_some() {
            return;
        }
        if self.n == MAX_INSECURE_HOSTS {
            for i in 1..MAX_INSECURE_HOSTS {
                self.hosts[i - 1] = self.hosts[i];
                self.lens[i - 1] = self.lens[i];
            }
            self.n -= 1;
        }
        let i = self.n;
        self.hosts[i] = [0; HOST_CAP];
        self.hosts[i][..host.len()].copy_from_slice(host);
        self.lens[i] = host.len() as u8;
        self.n += 1;
    }
}

/// Quick-link shortcuts shown on the start page (label, URL). All chosen to
/// accept our P-256 TLS 1.3 handshake.
pub const QUICK_LINKS: [(&str, &str); 4] = [
    ("Bing", "www.bing.com"),
    ("Wikipedia", "en.wikipedia.org/wiki/Operating_system"),
    ("Cloudflare", "www.cloudflare.com"),
    ("Exemplo", "example.com"),
];

impl Default for Browser {
    fn default() -> Self {
        Self::new()
    }
}

impl Browser {
    pub fn new() -> Self {
        let mut b = Self {
            url: [0; URL_CAP],
            url_len: 0,
            caret: 0,
            status: Status::Idle,
            pending: false,
            nav: [0; URL_CAP],
            nav_len: 0,
            home: true,
            security: Security::None,
            note: None,
            fail_reason: FailReason::Network,
            insecure: Rc::new(RefCell::new(InsecureHosts::new())),
            history: History::default(),
            bookmarks: Rc::new(RefCell::new(Box::new(MemoryBookmarks::default()))),
            bar_focus: true,
            sugg_sel: None,
            sugg_dismissed: false,
            internal: false,
            internal_ready: false,
            page_title: String::new(),
            bar_selected: false,
        };
        b.set_url(b"");
        b
    }

    /// A browser whose favourites live in `store` (the one place the kernel plugs persistence in).
    pub fn with_store(store: Box<dyn BookmarkStore>) -> Self {
        let mut b = Self::new();
        b.bookmarks = Rc::new(RefCell::new(store));
        b
    }

    /// A fresh browser (a new tab) that shares this one's favourites and the hosts the user
    /// allowed past a certificate error: both belong to the window, not to a tab.
    pub fn sibling(&self) -> Self {
        let mut b = Self::new();
        b.bookmarks = Rc::clone(&self.bookmarks);
        b.insecure = Rc::clone(&self.insecure);
        b
    }

    /// True while the native start page (logo + shortcuts) is shown.
    pub fn is_home(&self) -> bool {
        self.home
    }

    /// Return to the start page, clearing the address bar.
    pub fn go_home(&mut self) {
        self.home = true;
        self.status = Status::Idle;
        self.security = Security::None;
        self.note = None;
        self.set_url(b"");
    }

    /// Re-fetch the page that is shown (no-op on the start page). It is the address that was
    /// navigated to, not whatever has been typed in the bar since; an unsent edit is dropped.
    pub fn reload(&mut self) {
        if self.home || self.nav_len == 0 {
            return;
        }
        let nav = self.nav[..self.nav_len].to_vec();
        self.set_url(&nav);
        self.submit();
    }

    /// "Tentar novamente" on an error page: the same navigation again.
    pub fn retry(&mut self) {
        self.reload();
    }

    /// Navigate straight to `url` (used by the start-page shortcuts).
    pub fn open(&mut self, url: &[u8]) {
        self.set_url(url);
        self.submit();
    }

    fn set_url(&mut self, s: &[u8]) {
        self.bar_selected = false;
        self.url_len = s.len().min(URL_CAP);
        self.url[..self.url_len].copy_from_slice(&s[..self.url_len]);
        self.caret = self.url_len;
    }

    pub fn url(&self) -> &[u8] {
        &self.url[..self.url_len]
    }
    pub fn caret(&self) -> usize {
        self.caret
    }
    pub fn status(&self) -> Status {
        self.status
    }

    /// What can be said about the connection behind the current page. While a
    /// load is in flight this reflects the *requested* scheme; once it
    /// completes ([`Browser::loaded_with`]) it reflects the final one, after
    /// any redirects. Never reports a verified connection (see [`Security`]).
    pub fn security(&self) -> Security {
        self.security
    }

    /// True when the loaded page is not the whole document (see [`Browser::note`]).
    pub fn truncated(&self) -> bool {
        self.note.is_some()
    }

    /// Why the loaded page is only part of the document, if it is.
    pub fn note(&self) -> Option<PageNote> {
        self.note
    }

    /// Why the last navigation failed (meaningful when `status()` is `Error`).
    pub fn fail_reason(&self) -> FailReason {
        self.fail_reason
    }

    /// Handle a key while the address bar has focus. Returns `true` if anything
    /// changed (so the caller repaints). ENTER submits a navigation/search.
    pub fn on_key(&mut self, key: Key) -> bool {
        // Suggestion list: Up/Down move through it, Enter opens the highlighted one.
        let n = self.suggestions().len();
        match key {
            Key::Down if n > 0 => {
                self.sugg_sel = Some(self.sugg_sel.map_or(0, |i| (i + 1).min(n - 1)));
                return true;
            }
            Key::Up if n > 0 => {
                self.sugg_sel = match self.sugg_sel {
                    Some(0) | None => None,
                    Some(i) => Some(i - 1),
                };
                return true;
            }
            Key::Enter if self.sugg_sel.is_some() => {
                let i = self.sugg_sel.unwrap_or(0);
                if let Some(s) = self.suggestions().get(i) {
                    let url = s.url.clone();
                    self.open(url.as_bytes());
                }
                return true;
            }
            Key::Esc if n > 0 => {
                self.sugg_dismissed = true;
                self.sugg_sel = None;
                return true;
            }
            Key::Char(_) | Key::Backspace | Key::Delete => {
                self.sugg_sel = None;
                self.sugg_dismissed = false;
                // A selected address is replaced by what is typed (or just deleted).
                if core::mem::take(&mut self.bar_selected) {
                    self.url_len = 0;
                    self.caret = 0;
                    if matches!(key, Key::Backspace | Key::Delete) {
                        return true;
                    }
                }
            }
            Key::Left | Key::Right | Key::Home | Key::End => self.bar_selected = false,
            _ => {}
        }
        match key {
            Key::Char(c) => {
                if self.url_len < URL_CAP {
                    // insert at caret
                    let mut i = self.url_len;
                    while i > self.caret {
                        self.url[i] = self.url[i - 1];
                        i -= 1;
                    }
                    self.url[self.caret] = c;
                    self.url_len += 1;
                    self.caret += 1;
                }
                true
            }
            Key::Backspace => {
                if self.caret > 0 {
                    for i in self.caret..self.url_len {
                        self.url[i - 1] = self.url[i];
                    }
                    self.url_len -= 1;
                    self.caret -= 1;
                }
                true
            }
            Key::Delete => {
                if self.caret < self.url_len {
                    for i in self.caret + 1..self.url_len {
                        self.url[i - 1] = self.url[i];
                    }
                    self.url_len -= 1;
                }
                true
            }
            Key::Left => {
                self.caret = self.caret.saturating_sub(1);
                true
            }
            Key::Right => {
                if self.caret < self.url_len {
                    self.caret += 1;
                }
                true
            }
            Key::Home => {
                self.caret = 0;
                true
            }
            Key::End => {
                self.caret = self.url_len;
                true
            }
            Key::Enter => {
                self.submit();
                true
            }
            _ => false,
        }
    }

    /// Resolve the address bar into a navigation target and mark a fetch pending.
    pub fn submit(&mut self) {
        let input = &self.url[..self.url_len];
        if input.trim_ascii().is_empty() {
            return;
        }
        self.sugg_sel = None;
        self.sugg_dismissed = true;
        if starts_with_ci(input.trim_ascii(), INTERNAL_SCHEME) {
            let url = input.trim_ascii().to_vec();
            self.open_internal(&url);
            return;
        }
        self.internal = false;
        let mut nav = [0u8; URL_CAP];
        let n = if looks_like_url(input) {
            // Normalize: prepend https:// if no scheme was given.
            if starts_with_ci(input, b"http://") || starts_with_ci(input, b"https://") {
                let n = input.len().min(URL_CAP);
                nav[..n].copy_from_slice(&input[..n]);
                n
            } else {
                let pre = b"https://";
                let mut n = pre.len();
                nav[..n].copy_from_slice(pre);
                let take = input.len().min(URL_CAP - n);
                nav[n..n + take].copy_from_slice(&input[..take]);
                n += take;
                n
            }
        } else {
            build_search_url(input, &mut nav)
        };
        self.nav_len = n;
        self.nav[..n].copy_from_slice(&nav[..n]);
        // A verified connection is only ever reported by `loaded_with`.
        self.security = if starts_with_ci(&nav[..n], b"http://") {
            Security::Http
        } else {
            Security::None
        };
        self.note = None;
        self.status = Status::Loading;
        self.pending = true;
        self.home = false;
    }

    /// Pull a pending navigation target (clears the pending flag). The kernel
    /// fetches it and reports back with [`loaded`] / [`fail`].
    pub fn take_request(&mut self) -> Option<&[u8]> {
        if self.pending {
            self.pending = false;
            Some(&self.nav[..self.nav_len])
        } else {
            None
        }
    }

    /// Mark a navigation as successfully loaded (the kernel renders the page via
    /// the `web` engine and owns the display list).
    pub fn loaded(&mut self) {
        self.status = Status::Done;
        self.home = false;
        let url = self.nav[..self.nav_len].to_vec();
        self.history.record(&url);
    }

    /// True when there is an earlier page in the history.
    pub fn can_back(&self) -> bool {
        self.history.cur > 0 && !self.history.urls.is_empty()
    }

    /// True when there is a later page in the history.
    pub fn can_forward(&self) -> bool {
        self.history.cur + 1 < self.history.urls.len()
    }

    /// Number of pages in the history.
    pub fn history_len(&self) -> usize {
        self.history.urls.len()
    }

    /// Go to the previous page of the history (no-op at the start).
    pub fn back(&mut self) {
        if self.can_back() {
            self.history.cur -= 1;
            self.replay_current();
        }
    }

    /// Go to the next page of the history (no-op at the end).
    pub fn forward(&mut self) {
        if self.can_forward() {
            self.history.cur += 1;
            self.replay_current();
        }
    }

    fn replay_current(&mut self) {
        let url = self.history.urls[self.history.cur].clone();
        // Set before `submit`: an `osjeff://` page is "loaded" inside it.
        self.history.replay = true;
        self.set_url(&url);
        self.submit();
    }

    /// The user clicked a link whose `href` is `href` on the current page:
    /// resolve it against the page URL (relative, absolute and protocol-relative
    /// forms; `javascript:`, `data:` and an https -> http downgrade are refused)
    /// and navigate there. Returns `false` when the link was refused.
    pub fn open_link(&mut self, href: &[u8]) -> bool {
        if starts_with_ci(href.trim_ascii(), INTERNAL_SCHEME) {
            self.set_url(href.trim_ascii());
            self.submit();
            return true;
        }
        // The browser's own pages have no real address: resolve against a neutral http base so
        // their absolute links are not mistaken for an https -> http downgrade.
        let base = if self.internal {
            parse_url(b"http://osjeff.local/")
        } else {
            parse_url(self.nav_url())
        };
        let Some(base) = base else {
            return false;
        };
        match crate::redirect::resolve_redirect(&base, href) {
            Ok(target) => {
                self.set_url(&target);
                self.submit();
                true
            }
            Err(_) => false,
        }
    }

    /// Like [`Browser::loaded`], recording how the page actually arrived: `conn`
    /// describes the *final* connection (after redirects) and `truncated` says
    /// the response hit [`MAX_RESPONSE_BYTES`].
    pub fn loaded_with(&mut self, conn: Conn, truncated: bool) {
        self.loaded_with_note(conn, truncated.then_some(PageNote::Truncated));
    }

    /// [`Browser::loaded_with`] with the precise reason the page is partial
    /// (from [`page_body_partial`]), `None` for a whole page.
    pub fn loaded_with_note(&mut self, conn: Conn, note: Option<PageNote>) {
        self.security = match conn {
            Conn::Plain => Security::Http,
            Conn::Verified => Security::HttpsVerified,
            Conn::Insecure => Security::HttpsInvalid,
        };
        self.note = note;
        self.loaded();
    }

    /// The navigation target of the last (or pending) request.
    pub fn nav_url(&self) -> &[u8] {
        &self.nav[..self.nav_len]
    }

    /// The host of the current navigation when the user allowed it to proceed
    /// despite a certificate error (the fetcher then skips validation for that
    /// host only, on every hop of this navigation).
    pub fn insecure_host(&self) -> Option<Vec<u8>> {
        let u = parse_url(self.nav_url())?;
        if !u.https {
            return None;
        }
        let ins = self.insecure.borrow();
        let i = ins.find(u.host())?;
        Some(ins.hosts[i][..usize::from(ins.lens[i])].to_vec())
    }

    /// True when the failed navigation can be retried anyway: the failure is a
    /// certificate error (nothing else offers an unsafe override).
    pub fn can_continue_insecure(&self) -> bool {
        self.status == Status::Error && matches!(self.fail_reason, FailReason::Cert(_))
    }

    /// The user's explicit "continue anyway (insecure)": remember this host for
    /// the session and load the page again.
    pub fn continue_insecure(&mut self) {
        if !self.can_continue_insecure() {
            return;
        }
        if let Some(u) = parse_url(self.nav_url()) {
            self.insecure.borrow_mut().add(u.host());
        }
        self.pending = true;
        self.status = Status::Loading;
    }

    // ---- the browser's own pages, favourites and suggestions ----

    /// True while an `osjeff://` page is on screen.
    pub fn is_internal(&self) -> bool {
        self.internal
    }

    /// Open the internal page `url` (`osjeff://inicio`, `favoritos`, `historico`, `sobre`).
    /// Unknown names show the "about" page's list of pages.
    fn open_internal(&mut self, url: &[u8]) {
        let text = String::from_utf8_lossy(url).to_ascii_lowercase();
        let rest = text.trim_start_matches("osjeff:").trim_start_matches('/');
        let (name, query) = rest.split_once('?').unwrap_or((rest, ""));
        let name = name.trim_end_matches('/');
        if name == "inicio" || name.is_empty() {
            self.internal = false;
            self.go_home();
            return;
        }
        // `osjeff://favoritos?rm=N` removes the N-th favourite, then shows the list.
        let mut shown = alloc::format!("osjeff://{name}");
        let doomed = (name == "favoritos")
            .then(|| {
                query
                    .strip_prefix("rm=")
                    .and_then(|v| v.parse::<usize>().ok())
            })
            .flatten()
            .and_then(|n| self.bookmarks.borrow().all().get(n).cloned());
        if let Some(b) = doomed {
            self.bookmarks.borrow_mut().remove(&b.url);
            shown = String::from("osjeff://favoritos");
        }
        self.set_url(shown.as_bytes());
        let n = self.url_len;
        self.nav[..n].copy_from_slice(&self.url[..n]);
        self.nav_len = n;
        self.internal = true;
        self.internal_ready = true;
        self.security = Security::None;
        self.note = None;
        self.pending = false;
        self.home = false;
        self.loaded();
    }

    /// The HTML of an internal page that was just opened (once), for the kernel to lay out.
    pub fn take_internal(&mut self) -> Option<Vec<u8>> {
        if !core::mem::take(&mut self.internal_ready) {
            return None;
        }
        let name = String::from_utf8_lossy(self.nav_url()).to_ascii_lowercase();
        let name = name.trim_start_matches("osjeff://");
        Some(match name {
            "favoritos" => self.page_bookmarks(),
            "historico" => self.page_history(),
            _ => self.page_about(),
        })
    }

    fn page_bookmarks(&self) -> Vec<u8> {
        let mut h =
            String::from("<html><head><title>Favoritos</title></head><body><h1>Favoritos</h1>");
        let all = self.bookmarks.borrow().all();
        if all.is_empty() {
            h.push_str("<p>Nenhum favorito ainda. Abra uma pagina e aperte Ctrl+D.</p>");
        }
        for (i, b) in all.iter().enumerate() {
            h.push_str(&alloc::format!(
                "<p><a href=\"{u}\">{t}</a> <a href=\"osjeff://favoritos?rm={i}\">[remover]</a><br>{u}</p>",
                u = html_escape(&b.url),
                t = html_escape(if b.title.is_empty() { &b.url } else { &b.title }),
            ));
        }
        h.push_str("<p><a href=\"osjeff://inicio\">Inicio</a> | <a href=\"osjeff://historico\">Historico</a></p></body></html>");
        h.into_bytes()
    }

    fn page_history(&self) -> Vec<u8> {
        let mut h =
            String::from("<html><head><title>Historico</title></head><body><h1>Historico</h1>");
        let mut any = false;
        for u in self.history.urls.iter().rev() {
            let u = String::from_utf8_lossy(u);
            if u.starts_with("osjeff://") {
                continue;
            }
            any = true;
            h.push_str(&alloc::format!(
                "<p><a href=\"{0}\">{0}</a></p>",
                html_escape(&u)
            ));
        }
        if !any {
            h.push_str("<p>Nada visitado ainda.</p>");
        }
        h.push_str("<p><a href=\"osjeff://inicio\">Inicio</a> | <a href=\"osjeff://favoritos\">Favoritos</a></p></body></html>");
        h.into_bytes()
    }

    fn page_about(&self) -> Vec<u8> {
        let mut h = String::from(
            "<html><head><title>Sobre o navegador</title></head><body><h1>Navegador OSjeff</h1>",
        );
        h.push_str("<p>HTTPS com certificado verificado (TLS 1.3), links, historico, imagens PNG/BMP/PPM, formularios GET, favoritos, busca na pagina e zoom.</p>");
        h.push_str("<h3>Teclas</h3><ul><li>Alt+Esq / Alt+Dir: voltar e avancar</li><li>Ctrl+D: favorito</li><li>Ctrl+F: buscar na pagina</li><li>Ctrl+ + / - / 0: zoom</li><li>PgUp, PgDn, Home, End, Espaco: rolar</li><li>Ctrl+C: copia o texto selecionado</li></ul>");
        h.push_str("<h3>Paginas</h3><ul><li><a href=\"osjeff://inicio\">osjeff://inicio</a></li><li><a href=\"osjeff://favoritos\">osjeff://favoritos</a></li><li><a href=\"osjeff://historico\">osjeff://historico</a></li><li><a href=\"osjeff://sobre\">osjeff://sobre</a></li></ul></body></html>");
        h.into_bytes()
    }

    /// Remember the `<title>` of the page on screen (for favourites and the window title).
    pub fn set_page_title(&mut self, title: &str) {
        self.page_title.clear();
        // Cut at a character boundary: a title is page-controlled UTF-8.
        let mut end = title.len().min(80);
        while !title.is_char_boundary(end) {
            end -= 1;
        }
        self.page_title.push_str(&title[..end]);
    }

    /// `<title>` of the page on screen (empty when none).
    pub fn page_title(&self) -> &str {
        &self.page_title
    }

    /// Is the page on screen a favourite?
    pub fn is_bookmarked(&self) -> bool {
        !self.home
            && !self.nav_url().is_empty()
            && self
                .bookmarks
                .borrow()
                .contains(&String::from_utf8_lossy(self.nav_url()))
    }

    /// Ctrl+D / the star: add the page to the favourites, or remove it. Returns the new state
    /// (`true` = now a favourite) or `None` when there is nothing to bookmark (start page, full).
    pub fn toggle_bookmark(&mut self) -> Option<bool> {
        if self.home || self.nav_url().is_empty() {
            return None;
        }
        let url = String::from_utf8_lossy(self.nav_url()).into_owned();
        if self.bookmarks.borrow().contains(&url) {
            self.bookmarks.borrow_mut().remove(&url);
            return Some(false);
        }
        let title = if self.page_title.is_empty() {
            url.clone()
        } else {
            self.page_title.clone()
        };
        self.bookmarks
            .borrow_mut()
            .add(Bookmark { url, title })
            .then_some(true)
    }

    /// The favourites.
    pub fn bookmarks(&self) -> Vec<Bookmark> {
        self.bookmarks.borrow().all()
    }

    /// Select the whole address (Ctrl+L): typing replaces it.
    pub fn select_bar(&mut self) {
        self.bar_focus = true;
        self.bar_selected = self.url_len > 0;
        self.sugg_dismissed = true;
    }

    /// Is the whole address selected?
    pub fn bar_selected(&self) -> bool {
        self.bar_selected
    }

    /// Keyboard focus is in the address bar (true) or on the page (false).
    pub fn bar_focus(&self) -> bool {
        self.bar_focus
    }

    pub fn set_bar_focus(&mut self, on: bool) {
        self.bar_focus = on;
        if !on {
            self.sugg_sel = None;
        }
    }

    /// The history, oldest first (absolute URLs).
    pub fn history_urls(&self) -> impl Iterator<Item = &[u8]> {
        self.history.urls.iter().map(|u| u.as_slice())
    }

    /// Address-bar suggestions for the text typed so far: favourites then history, prefix matches
    /// before substring matches, at most [`MAX_SUGGESTIONS`]. Empty when the bar has no text, does
    /// not have the focus, was dismissed with Esc, or only the typed address itself matches.
    pub fn suggestions(&self) -> Vec<Suggestion> {
        if !self.bar_focus || self.sugg_dismissed || self.url_len == 0 {
            return Vec::new();
        }
        let q = String::from_utf8_lossy(self.url());
        let hist: Vec<String> = self
            .history
            .urls
            .iter()
            .rev()
            .map(|u| String::from_utf8_lossy(u).into_owned())
            .filter(|u| !u.starts_with("osjeff://"))
            .collect();
        suggest(&q, &self.bookmarks.borrow().all(), &hist)
    }

    /// The highlighted suggestion index.
    pub fn suggestion_selected(&self) -> Option<usize> {
        self.sugg_sel
    }

    /// Choose suggestion `i` (a click): open it.
    pub fn pick_suggestion(&mut self, i: usize) -> bool {
        match self.suggestions().get(i) {
            Some(s) => {
                let url = s.url.clone();
                self.open(url.as_bytes());
                true
            }
            None => false,
        }
    }

    /// Stop loading: a request that was not taken yet is dropped, one in flight is ignored by
    /// the caller. The page on screen (or the start page) stays.
    pub fn stop(&mut self) {
        if self.status == Status::Loading {
            self.pending = false;
            self.history.replay = false;
            self.status = Status::Idle;
        }
    }

    /// Is a navigation in progress?
    pub fn is_loading(&self) -> bool {
        self.status == Status::Loading
    }

    /// Up to `n` most recently visited addresses, newest first, each once, without the
    /// browser's own pages.
    pub fn recent(&self, n: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for u in self.history.urls.iter().rev() {
            let u = String::from_utf8_lossy(u);
            if u.starts_with("osjeff://") || out.iter().any(|o| *o == u) {
                continue;
            }
            out.push(u.into_owned());
            if out.len() >= n {
                break;
            }
        }
        out
    }

    /// Mark the current fetch as failed (the kernel shows the error state).
    pub fn fail(&mut self) {
        self.fail_with(FailReason::Network);
    }

    /// Mark the current fetch as failed for a specific reason.
    pub fn fail_with(&mut self, reason: FailReason) {
        self.history.replay = false;
        self.status = Status::Error;
        self.fail_reason = reason;
        self.note = None;
        self.home = false;
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn bookmarks_text_roundtrip_and_damage() {
        let items = vec![
            Bookmark {
                url: "http://a.example/".into(),
                title: "A\tpage\n".into(),
            },
            Bookmark {
                url: "https://b.example/x".into(),
                title: String::new(),
            },
        ];
        let text = bookmarks_to_text(&items);
        let back = bookmarks_from_text(text.as_bytes());
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].title, "A page");
        assert_eq!(back[1].url, "https://b.example/x");
        // damage: bad UTF-8, blank lines, duplicates, no tab
        let junk = b"\xff\xfe\n\n\thttp://x\nhttp://d/\tD\nhttp://d/\tagain\nhttp://e/\n";
        let got = bookmarks_from_text(junk);
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[0].url, "http://d/");
        assert_eq!(got[1].title, "");
        // never more than MAX_BOOKMARKS
        let mut big = String::new();
        for i in 0..(MAX_BOOKMARKS + 20) {
            big.push_str(&format!("http://h{i}/\tt\n"));
        }
        assert_eq!(bookmarks_from_text(big.as_bytes()).len(), MAX_BOOKMARKS);
    }

    #[test]
    fn saved_bookmarks_flush_on_change_only() {
        use alloc::rc::Rc;
        use core::cell::RefCell;
        let saved: Rc<RefCell<Vec<Vec<u8>>>> = Rc::default();
        let sink = saved.clone();
        let mut s = SavedBookmarks::load(b"http://a/\tA\n", move |t: &[u8]| {
            sink.borrow_mut().push(t.to_vec())
        });
        assert!(s.contains("http://a/"));
        assert!(!s.add(Bookmark {
            url: "http://a/".into(),
            title: "dup".into()
        }));
        assert!(saved.borrow().is_empty(), "a refused add must not write");
        assert!(s.add(Bookmark {
            url: "http://b/".into(),
            title: "B".into()
        }));
        assert_eq!(saved.borrow().len(), 1);
        assert_eq!(saved.borrow()[0], b"http://a/\tA\nhttp://b/\tB\n");
        assert!(s.remove("http://a/"));
        assert!(!s.remove("http://a/"));
        assert_eq!(saved.borrow().len(), 2);
        assert_eq!(saved.borrow()[1], b"http://b/\tB\n");
    }
    use super::*;

    #[test]
    fn parse_plain_host_defaults_https() {
        let u = parse_url(b"example.com").unwrap();
        assert!(u.https);
        assert_eq!(u.port, 443);
        assert_eq!(u.host(), b"example.com");
        assert_eq!(u.path(), b"/");
    }

    #[test]
    fn parse_http_scheme_and_path_and_port() {
        let u = parse_url(b"http://example.com:8080/a/b?x=1").unwrap();
        assert!(!u.https);
        assert_eq!(u.port, 8080);
        assert_eq!(u.host(), b"example.com");
        assert_eq!(u.path(), b"/a/b?x=1");
    }

    #[test]
    fn parse_https_default_port_and_query_only_path() {
        let u = parse_url(b"https://duckduckgo.com/html/?q=rust").unwrap();
        assert!(u.https);
        assert_eq!(u.port, 443);
        assert_eq!(u.host(), b"duckduckgo.com");
        assert_eq!(u.path(), b"/html/?q=rust");
    }

    /// Regression: an over-long port (`host:99999999999`) overflowed the u32
    /// accumulator (panic with overflow checks, silent wrap -> wrong port in
    /// release). Out-of-range ports now fall back to the scheme default.
    #[test]
    fn parse_url_oversized_port_falls_back_to_default() {
        let u = parse_url(b"http://example.com:99999999999/x").unwrap();
        assert_eq!(u.port, 80);
        assert_eq!(u.path(), b"/x");
        // 4294967296 + 80 would wrap to 80 in a u32 accumulator.
        let u = parse_url(b"https://example.com:4294967376/").unwrap();
        assert_eq!(u.port, 443);
        let u = parse_url(b"https://example.com:65536/").unwrap();
        assert_eq!(u.port, 443);
        let u = parse_url(b"https://example.com:65535/").unwrap();
        assert_eq!(u.port, 65535);
    }

    #[test]
    fn parse_rejects_empty_host() {
        assert!(parse_url(b"   ").is_none());
        assert!(parse_url(b"https://").is_none());
    }

    #[test]
    fn url_vs_search_heuristic() {
        assert!(looks_like_url(b"example.com"));
        assert!(looks_like_url(b"http://foo.bar/baz"));
        assert!(!looks_like_url(b"rust programming"));
        assert!(!looks_like_url(b"hello"));
        assert!(!looks_like_url(b"two words.with dot"));
    }

    #[test]
    fn encode_query_escapes() {
        let mut out = [0u8; 64];
        let n = encode_query(b"rust lang & co", &mut out);
        assert_eq!(&out[..n], b"rust+lang+%26+co");
    }

    #[test]
    fn search_url_built() {
        let mut out = [0u8; 128];
        let n = build_search_url(b"rust", &mut out);
        assert_eq!(&out[..n], b"https://www.bing.com/search?q=rust");
    }

    #[test]
    fn http_body_after_headers() {
        let resp = b"HTTP/1.0 200 OK\r\nContent-Type: text/html\r\n\r\n<p>hi</p>";
        assert_eq!(http_body(resp), b"<p>hi</p>");
    }

    #[test]
    fn parses_status_and_headers() {
        let resp = b"HTTP/1.1 301 Moved\r\nLocation: https://x.com/y\r\nServer: t\r\n\r\nbody";
        assert_eq!(status_code(resp), Some(301));
        assert_eq!(
            header_value(resp, b"location"),
            Some(&b"https://x.com/y"[..])
        );
        assert_eq!(
            header_value(resp, b"LOCATION"),
            Some(&b"https://x.com/y"[..])
        );
        assert_eq!(header_value(resp, b"missing"), None);
    }

    #[test]
    fn page_body_dechunks() {
        let resp = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        assert_eq!(page_body(resp), b"Wikipedia");
    }

    /// Regression: a chunk-size line with too many hex digits overflowed
    /// `size * 16` (debug: panic) and, wrapping in release, made `i + size`
    /// wrap below `i` so `&body[i..end]` panicked. A remote server controls
    /// this, so it must be handled without panicking.
    #[test]
    fn dechunk_huge_chunk_size_does_not_panic() {
        // 17 hex digits: overflows usize on the multiply.
        let resp = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nFFFFFFFFFFFFFFFFF\r\nabc";
        let _ = page_body(resp);
        // 16 hex digits == usize::MAX: no multiply overflow, but `i + size`
        // wraps below `i` in release builds.
        let resp = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nFFFFFFFFFFFFFFFF\r\nabc";
        let _ = page_body(resp);
        // A valid chunk before the bad one is kept.
        let resp =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\nFFFFFFFFFFFFFFFFFFFF\r\nxyz";
        assert_eq!(page_body(resp), b"abc");
    }

    #[test]
    fn browser_typing_and_caret() {
        let mut b = Browser::new();
        // clear the default url
        while b.caret() > 0 {
            b.on_key(Key::Backspace);
        }
        for &c in b"abc" {
            b.on_key(Key::Char(c));
        }
        assert_eq!(b.url(), b"abc");
        b.on_key(Key::Left);
        b.on_key(Key::Char(b'X'));
        assert_eq!(b.url(), b"abXc");
    }

    #[test]
    fn browser_submit_navigates_url() {
        let mut b = Browser::new();
        assert!(b.is_home());
        for &c in b"example.com" {
            b.on_key(Key::Char(c));
        }
        b.on_key(Key::Enter);
        assert!(!b.is_home());
        let req = b.take_request().unwrap();
        assert_eq!(req, b"https://example.com");
        assert_eq!(b.status(), Status::Loading);
        assert!(b.take_request().is_none()); // consumed
    }

    #[test]
    fn browser_go_home_resets() {
        let mut b = Browser::new();
        b.open(b"example.com");
        let _ = b.take_request();
        b.loaded();
        assert!(!b.is_home());
        b.go_home();
        assert!(b.is_home());
        assert_eq!(b.url(), b"");
        assert_eq!(b.status(), Status::Idle);
    }

    #[test]
    fn browser_submit_searches_free_text() {
        let mut b = Browser::new();
        while b.caret() > 0 {
            b.on_key(Key::Backspace);
        }
        for &c in b"rust lang" {
            b.on_key(Key::Char(c));
        }
        b.on_key(Key::Enter);
        let req = b.take_request().unwrap();
        assert_eq!(req, b"https://www.bing.com/search?q=rust+lang");
    }

    #[test]
    fn browser_loaded_sets_done() {
        let mut b = Browser::new();
        b.open(b"example.com");
        let _ = b.take_request();
        b.loaded();
        assert_eq!(b.status(), Status::Done);
        assert!(!b.is_home());
    }

    /// Regression: host/path go straight into the request line and `Host:`
    /// header, so a CR/LF in a URL (typed, or from a server's `Location`)
    /// allowed request splitting. Any ASCII control now makes the URL invalid.
    #[test]
    fn parse_url_rejects_control_characters() {
        for bad in [
            &b"http://a.com/x\r\nHost: evil"[..],
            b"http://a.com\r\nX: y/",
            b"http://a.com/\0",
            b"http://a.com/p\tq",
            b"http://a.com/p\x7fq",
            b"ex\x01ample.com",
            b"http://a.com/x y\nz",
        ] {
            assert!(parse_url(bad).is_none(), "{:?}", core::str::from_utf8(bad));
        }
        // Leading blanks are still tolerated; spaces and non-ASCII are not controls.
        assert!(parse_url(b" \t http://a.com/x").is_some());
        assert!(parse_url(b"http://a.com/caf\xc3\xa9").is_some());
    }

    /// Regression: `decode_utf8(&[])` indexed `bytes[0]` and panicked.
    #[test]
    fn decode_utf8_handles_empty_and_truncated_input() {
        assert_eq!(decode_utf8(&[]), (0, 0));
        assert_eq!(decode_utf8(b"a"), (0x61, 1));
        assert_eq!(decode_utf8("\u{e9}".as_bytes()), (0xE9, 2));
        assert_eq!(decode_utf8(&[0xE2, 0x82]), (0, 1)); // cut mid-sequence
        assert_eq!(decode_utf8(&[0xFF]), (0, 1));
    }

    /// Regression: `http_get` appended every received byte to a `Vec` with no
    /// ceiling (the TLS path had its own 256 KiB check), so a server could
    /// stream until the 64 MiB heap was gone. Both paths now share this cap.
    #[test]
    fn append_capped_never_exceeds_the_cap() {
        let mut v = alloc::vec::Vec::new();
        assert!(!append_capped(&mut v, b"abcd", 10));
        assert!(!append_capped(&mut v, b"efghij", 10)); // exactly full: not truncated
        assert_eq!(v, b"abcdefghij");
        assert!(append_capped(&mut v, b"k", 10)); // one byte over
        assert_eq!(v.len(), 10);
        let mut v = alloc::vec::Vec::new();
        assert!(append_capped(&mut v, &[7u8; 100], 30));
        assert_eq!(v.len(), 30);
        assert!(append_capped(&mut v, b"x", 30));
        assert_eq!(v.len(), 30);
        // Already above the cap (cap lowered): nothing added, no underflow.
        assert!(append_capped(&mut v, b"yz", 5));
        assert_eq!(v.len(), 30);
        // Streaming a huge body in small pieces stays at the cap.
        let mut v = alloc::vec::Vec::new();
        let mut truncated = false;
        for _ in 0..(MAX_RESPONSE_BYTES / 1024 + 100) {
            truncated |= append_capped(&mut v, &[0u8; 1024], MAX_RESPONSE_BYTES);
        }
        assert_eq!(v.len(), MAX_RESPONSE_BYTES);
        assert!(truncated);
        // A body that fits is untouched.
        let mut v = alloc::vec::Vec::new();
        assert!(!append_capped(&mut v, &[1u8; 1024], MAX_RESPONSE_BYTES));
        assert_eq!(v.len(), 1024);
    }

    #[test]
    fn https_is_never_secure_until_the_fetcher_says_verified() {
        let mut b = Browser::new();
        assert_eq!(b.security(), Security::None);
        assert_eq!(b.security().label(), None);
        b.open(b"https://example.com");
        // In flight: no claim either way (and certainly no padlock).
        assert_eq!(b.security(), Security::None);
        let _ = b.take_request();
        b.loaded_with(Conn::Verified, false);
        assert_eq!(b.security(), Security::HttpsVerified);
        assert_eq!(b.security().label(), Some("Conexão segura"));
        assert!(!b.truncated());
        // A bare host is normalised to https: a new load drops the padlock at once.
        b.open(b"example.com");
        assert_eq!(b.security(), Security::None);
        // Plain http says so.
        b.open(b"http://example.com");
        assert_eq!(b.security(), Security::Http);
        assert_eq!(b.security().label(), Some("Nao seguro"));
    }

    #[test]
    fn only_a_verified_load_shows_the_padlock() {
        for conn in [Conn::Plain, Conn::Insecure] {
            let mut b = Browser::new();
            b.open(b"https://example.com");
            let _ = b.take_request();
            b.loaded_with(conn, false);
            assert_ne!(b.security(), Security::HttpsVerified, "{conn:?}");
        }
        let mut b = Browser::new();
        b.open(b"https://example.com");
        let _ = b.take_request();
        b.loaded_with(Conn::Insecure, false);
        assert_eq!(b.security(), Security::HttpsInvalid);
        assert_eq!(b.security().label(), Some("Certificado inválido"));
        // The page-load entry points that do not carry a connection state never
        // produce a padlock.
        b.loaded();
        assert_ne!(b.security(), Security::HttpsVerified);
    }

    #[test]
    fn final_scheme_after_redirect_wins_over_requested_one() {
        let mut b = Browser::new();
        b.open(b"http://example.com"); // asked for http...
        let _ = b.take_request();
        b.loaded_with(Conn::Verified, false); // ...but ended on verified https
        assert_eq!(b.security(), Security::HttpsVerified);
        b.open(b"https://example.com");
        let _ = b.take_request();
        b.loaded_with(Conn::Plain, false); // (the kernel blocks this; the model stays honest)
        assert_eq!(b.security(), Security::Http);
    }

    #[test]
    fn cert_failure_offers_a_per_origin_session_override() {
        use crate::tlsverify::CertError;
        let mut b = Browser::new();
        b.open(b"https://expired.example.com/page");
        let _ = b.take_request();
        assert_eq!(b.insecure_host(), None);
        b.fail_with(FailReason::Cert(CertError::Expired));
        assert!(b.can_continue_insecure());
        assert_eq!(b.fail_reason().message(), "Certificado inválido: expirado");
        b.continue_insecure();
        assert_eq!(b.status(), Status::Loading);
        assert_eq!(
            b.take_request(),
            Some(&b"https://expired.example.com/page"[..])
        );
        assert_eq!(
            b.insecure_host().as_deref(),
            Some(&b"expired.example.com"[..])
        );
        b.loaded_with(Conn::Insecure, false);
        assert_eq!(b.security(), Security::HttpsInvalid);
        // Another origin is not covered.
        b.open(b"https://other.example.com/");
        assert_eq!(b.insecure_host(), None);
        // Same origin, other path: covered (it is per origin, case-insensitive).
        b.open(b"https://EXPIRED.example.com/x");
        assert_eq!(
            b.insecure_host().as_deref(),
            Some(&b"expired.example.com"[..])
        );
        // http never uses the override.
        b.open(b"http://expired.example.com/");
        assert_eq!(b.insecure_host(), None);
    }

    #[test]
    fn override_is_only_offered_for_certificate_errors() {
        let mut b = Browser::new();
        b.open(b"https://x.example.com");
        let _ = b.take_request();
        for r in [
            FailReason::Network,
            FailReason::Dns,
            FailReason::Refused,
            FailReason::Timeout,
            FailReason::Tls,
            FailReason::RedirectDowngrade,
        ] {
            b.fail_with(r);
            assert!(!b.can_continue_insecure(), "{r:?}");
            b.continue_insecure(); // must be a no-op
            assert_eq!(b.status(), Status::Error);
            assert_eq!(b.insecure_host(), None);
        }
    }

    #[test]
    fn override_list_is_bounded_and_forgets_the_oldest() {
        use crate::tlsverify::CertError;
        let mut b = Browser::new();
        for i in 0..MAX_INSECURE_HOSTS + 2 {
            let url = alloc::format!("https://h{i}.example.com/");
            b.open(url.as_bytes());
            let _ = b.take_request();
            b.fail_with(FailReason::Cert(CertError::NameMismatch));
            b.continue_insecure();
            let _ = b.take_request();
        }
        b.open(b"https://h0.example.com/");
        assert_eq!(b.insecure_host(), None, "oldest was evicted");
        b.open(b"https://h9.example.com/");
        assert!(b.insecure_host().is_some());
    }

    #[test]
    fn every_failure_message_is_distinct_for_network_causes() {
        use crate::tlsverify::CertError;
        let causes = [
            FailReason::Network,
            FailReason::Dns,
            FailReason::Refused,
            FailReason::Timeout,
            FailReason::Tls,
            FailReason::Cert(CertError::Expired),
        ];
        for (i, a) in causes.iter().enumerate() {
            // Latin-1 at most: the interface font covers Portuguese accents.
            assert!(a.message().chars().all(|c| (c as u32) < 0x100));
            for b in &causes[i + 1..] {
                assert_ne!(a.message(), b.message());
            }
        }
        assert!(
            FailReason::Cert(CertError::NameMismatch)
                .message()
                .starts_with("Certificado inválido:")
        );
    }

    #[test]
    fn truncation_and_failure_state() {
        let mut b = Browser::new();
        b.open(b"example.com");
        let _ = b.take_request();
        b.loaded_with(Conn::Verified, true);
        assert!(b.truncated());
        assert_eq!(b.status(), Status::Done);
        // A new navigation clears the flag.
        b.open(b"example.org");
        assert!(!b.truncated());
        let _ = b.take_request();
        b.fail_with(FailReason::RedirectDowngrade);
        assert_eq!(b.status(), Status::Error);
        assert_eq!(b.fail_reason(), FailReason::RedirectDowngrade);
        assert!(b.fail_reason().message().contains("HTTPS"));
        b.fail();
        assert_eq!(b.fail_reason(), FailReason::Network);
        b.go_home();
        assert_eq!(b.security(), Security::None);
    }

    #[test]
    fn redirect_errors_map_to_distinct_reasons() {
        use crate::redirect::RedirectError as E;
        let all = [E::Invalid, E::Downgrade, E::Loop, E::TooMany];
        let reasons: alloc::vec::Vec<_> =
            all.iter().map(|&e| FailReason::from_redirect(e)).collect();
        for (i, a) in reasons.iter().enumerate() {
            for b in &reasons[i + 1..] {
                assert_ne!(a, b);
            }
            // Latin-1 at most: the interface font covers Portuguese accents.
            assert!(a.message().chars().all(|c| (c as u32) < 0x100));
        }
        assert_eq!(
            FailReason::from_redirect(E::Downgrade),
            FailReason::RedirectDowngrade
        );
    }

    #[test]
    fn worker_died_is_a_distinct_ascii_failure() {
        let mut b = Browser::new();
        b.open(b"example.com");
        let _ = b.take_request();
        b.fail_with(FailReason::WorkerDied);
        assert_eq!(b.status(), Status::Error);
        assert_eq!(b.fail_reason(), FailReason::WorkerDied);
        assert_ne!(FailReason::WorkerDied, FailReason::Network);
        assert!(FailReason::WorkerDied.message().is_ascii());
        // The next navigation starts clean.
        b.open(b"example.org");
        assert_eq!(b.status(), Status::Loading);
    }

    const GZ_PAGE: &str = "1f8b08000000000002ffb3c928c9cdb1b349ca4fa9b4b3c930b4f3cf49b4d107d2360576e95599050ae5f945d9c50a99790afec159a9696936fa057636fa10d5fa60ad0094af60dd41000000";

    fn hexv(s: &str) -> alloc::vec::Vec<u8> {
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn page_body_gunzips_content_encoding_gzip() {
        let mut resp =
            b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Type: text/html\r\n\r\n"
                .to_vec();
        resp.extend_from_slice(&hexv(GZ_PAGE));
        let body = page_body(&resp);
        assert!(body.starts_with(b"<html><body><h1>Ola</h1>"));
    }

    #[test]
    fn page_body_dechunks_then_gunzips() {
        let gz = hexv(GZ_PAGE);
        let (a, b) = gz.split_at(20);
        let mut resp =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Encoding: GZIP\r\n\r\n"
                .to_vec();
        for part in [a, b] {
            resp.extend_from_slice(alloc::format!("{:x}\r\n", part.len()).as_bytes());
            resp.extend_from_slice(part);
            resp.extend_from_slice(b"\r\n");
        }
        resp.extend_from_slice(b"0\r\n\r\n");
        assert!(page_body(&resp).starts_with(b"<html><body><h1>Ola</h1>"));
    }

    #[test]
    fn page_body_bad_or_unsupported_encoding_gives_a_notice() {
        let resp = b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\nnot gzip at all, sorry";
        let body = page_body(resp);
        assert!(String::from_utf8_lossy(&body).contains("Falha ao descompactar"));
        let resp = b"HTTP/1.1 200 OK\r\nContent-Encoding: br\r\n\r\n\x01\x02";
        assert!(String::from_utf8_lossy(&page_body(resp)).contains("nao suportada"));
        // identity is a no-op
        let resp = b"HTTP/1.1 200 OK\r\nContent-Encoding: identity\r\n\r\n<p>x</p>";
        assert_eq!(page_body(resp), b"<p>x</p>");
    }

    fn load(b: &mut Browser, url: &[u8]) {
        b.open(url);
        assert!(b.take_request().is_some());
        b.loaded_with(Conn::Verified, false);
    }

    #[test]
    fn history_back_forward_and_truncation() {
        let mut b = Browser::new();
        assert!(!b.can_back() && !b.can_forward());
        load(&mut b, b"https://a.example/");
        load(&mut b, b"https://b.example/");
        load(&mut b, b"https://c.example/");
        assert_eq!(b.history_len(), 3);
        assert!(b.can_back() && !b.can_forward());
        b.back();
        assert_eq!(b.take_request(), Some(&b"https://b.example/"[..]));
        b.loaded_with(Conn::Verified, false);
        assert_eq!(b.history_len(), 3, "back does not add an entry");
        assert!(b.can_back() && b.can_forward());
        b.back();
        assert_eq!(b.take_request(), Some(&b"https://a.example/"[..]));
        b.loaded_with(Conn::Verified, false);
        assert!(!b.can_back());
        b.back(); // no-op at the start
        assert!(b.take_request().is_none());
        b.forward();
        assert_eq!(b.take_request(), Some(&b"https://b.example/"[..]));
        b.loaded_with(Conn::Verified, false);
        // A new navigation from the middle drops the forward entries.
        load(&mut b, b"https://d.example/");
        assert_eq!(b.history_len(), 3);
        assert!(!b.can_forward());
        assert_eq!(b.url(), b"https://d.example/");
    }

    #[test]
    fn reload_does_not_duplicate_history() {
        let mut b = Browser::new();
        load(&mut b, b"https://a.example/");
        b.reload();
        let _ = b.take_request();
        b.loaded_with(Conn::Verified, false);
        assert_eq!(b.history_len(), 1);
    }

    #[test]
    fn history_is_bounded() {
        let mut b = Browser::new();
        for i in 0..MAX_HISTORY + 10 {
            let u = alloc::format!("https://h{i}.example/");
            load(&mut b, u.as_bytes());
        }
        assert_eq!(b.history_len(), MAX_HISTORY);
        b.back();
        let want = alloc::format!("https://h{}.example/", MAX_HISTORY + 8);
        assert_eq!(b.take_request(), Some(want.as_bytes()));
    }

    #[test]
    fn failed_load_is_not_recorded() {
        let mut b = Browser::new();
        load(&mut b, b"https://a.example/");
        b.open(b"https://broken.example/");
        let _ = b.take_request();
        b.fail_with(FailReason::Dns);
        assert_eq!(b.history_len(), 1);
    }

    #[test]
    fn open_link_resolves_relative_absolute_and_refuses_unsafe() {
        let mut b = Browser::new();
        load(&mut b, b"https://site.example/dir/page.html");
        assert!(b.open_link(b"other.html"));
        assert_eq!(
            b.take_request(),
            Some(&b"https://site.example/dir/other.html"[..])
        );
        b.loaded_with(Conn::Verified, false);
        assert!(b.open_link(b"/root.html"));
        assert_eq!(
            b.take_request(),
            Some(&b"https://site.example/root.html"[..])
        );
        b.loaded_with(Conn::Verified, false);
        assert!(b.open_link(b"https://elsewhere.example/x"));
        assert_eq!(b.take_request(), Some(&b"https://elsewhere.example/x"[..]));
        b.loaded_with(Conn::Verified, false);
        for bad in [
            &b"javascript:alert(1)"[..],
            b"data:text/html,x",
            b"http://plain.example/",
            b"",
            b"a b",
        ] {
            assert!(!b.open_link(bad), "{bad:?}");
        }
        assert!(b.take_request().is_none());
    }
}

#[cfg(test)]
mod ui_tests;

#[cfg(test)]
mod body_tests;
