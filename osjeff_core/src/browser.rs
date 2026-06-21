//! Native browser logic: URL parsing, search-URL building, HTML→text
//! extraction, and the address-bar + content model.
//!
//! Pure and allocation-free (fixed buffers), so the whole parser and editing
//! model is host-testable. The kernel supplies only the networking: it pulls a
//! pending request out with [`Browser::take_request`], fetches the bytes, and
//! feeds the raw HTTP response back via [`Browser::load_response`].

use crate::Key;

/// Max bytes of a URL (address bar + resolved navigation target).
pub const URL_CAP: usize = 220;
/// Max host length.
pub const HOST_CAP: usize = 80;
/// Extracted, word-wrapped page text capacity.
pub const CONTENT_CAP: usize = 28 * 1024;
/// Wrap width in characters for the rendered text (matched to the browser
/// window's content width at the kernel's 2x font scale).
pub const COLS: usize = 70;

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

/// Parse an absolute URL. A missing scheme defaults to HTTPS. Returns `None`
/// only when there is no host at all.
pub fn parse_url(input: &[u8]) -> Option<Url> {
    let mut s = input;
    while let [b' ' | b'\t', rest @ ..] = s {
        s = rest;
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
            p = p * 10 + (*c - b'0') as u32;
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

/// The clean HTML body of a raw HTTP response: headers stripped and, if the
/// response is `Transfer-Encoding: chunked`, the chunk framing removed.
pub fn page_body(resp: &[u8]) -> alloc::vec::Vec<u8> {
    let body = http_body(resp);
    let chunked = header_value(resp, b"transfer-encoding")
        .map(|v| v.eq_ignore_ascii_case(b"chunked"))
        .unwrap_or(false);
    if chunked {
        dechunk(body)
    } else {
        body.to_vec()
    }
}

/// Decode an HTTP/1.1 chunked body into the raw payload.
fn dechunk(body: &[u8]) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec::Vec::new();
    let mut i = 0;
    while i < body.len() {
        // Chunk size in hex up to CR/LF or ';' (chunk extensions).
        let mut size = 0usize;
        let mut saw_digit = false;
        while i < body.len() {
            let c = body[i];
            if let Some(d) = (c as char).to_digit(16) {
                size = size * 16 + d as usize;
                saw_digit = true;
                i += 1;
            } else {
                break;
            }
        }
        if !saw_digit {
            break;
        }
        // Skip to end of the size line.
        while i < body.len() && body[i] != b'\n' {
            i += 1;
        }
        i += 1; // past '\n'
        if size == 0 || i >= body.len() {
            break;
        }
        let end = (i + size).min(body.len());
        out.extend_from_slice(&body[i..end]);
        i = end;
        // Skip the CRLF after the chunk data.
        while i < body.len() && (body[i] == b'\r' || body[i] == b'\n') {
            i += 1;
        }
    }
    out
}

/// True if `tag` (lowercased element name) introduces or ends a block whose
/// boundary should become a line break in the extracted text.
fn is_break_tag(tag: &[u8]) -> bool {
    const BREAKS: [&[u8]; 14] = [
        b"br",
        b"/p",
        b"/div",
        b"/h1",
        b"/h2",
        b"/h3",
        b"/h4",
        b"/li",
        b"/tr",
        b"/ul",
        b"/ol",
        b"/table",
        b"/article",
        b"hr",
    ];
    BREAKS.contains(&tag)
}

/// Lowercase element name of a tag body (the bytes between `<` and `>`),
/// e.g. `"P class=x"` → `b"p"`, `"/DIV"` → `b"/div"`. Written into `buf`.
fn tag_name<'a>(inner: &[u8], buf: &'a mut [u8; 12]) -> &'a [u8] {
    let mut n = 0;
    for &c in inner {
        if c == b' ' || c == b'\t' || c == b'\n' || c == b'>' {
            break;
        }
        if n < buf.len() {
            buf[n] = c.to_ascii_lowercase();
            n += 1;
        } else {
            break;
        }
    }
    &buf[..n]
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
pub fn html_to_text(html: &[u8], out: &mut [u8]) -> usize {
    let mut w = Wrapper::new(out);
    let mut i = 0;
    while i < html.len() {
        let c = html[i];
        if c == b'<' {
            // Find the tag's closing '>'.
            let start = i + 1;
            let mut j = start;
            while j < html.len() && html[j] != b'>' {
                j += 1;
            }
            let inner = &html[start..j.min(html.len())];
            let mut nb = [0u8; 12];
            let name = tag_name(inner, &mut nb);

            // Skip the entire body of <script> / <style>.
            if name == b"script" || name == b"style" {
                let close: &[u8] = if name == b"script" {
                    b"</script"
                } else {
                    b"</style"
                };
                if let Some(rel) = find(&html[j..], close) {
                    i = j + rel;
                    // advance past this close tag's '>'
                    while i < html.len() && html[i] != b'>' {
                        i += 1;
                    }
                    i += 1;
                    continue;
                } else {
                    break;
                }
            }

            if is_break_tag(name) {
                w.newline();
            } else {
                w.space();
            }
            i = j + 1;
        } else if c == b'&' {
            // Entity: read up to ';' (bounded).
            let mut j = i + 1;
            while j < html.len() && j < i + 12 && html[j] != b';' {
                j += 1;
            }
            if j < html.len()
                && html[j] == b';'
                && let Some(b) = decode_entity(&html[i + 1..j])
            {
                w.push(b);
                i = j + 1;
                continue;
            }
            w.push(b'&');
            i += 1;
        } else if c == b'\n' || c == b'\r' || c == b'\t' {
            w.space();
            i += 1;
        } else if c >= 0x80 {
            // Raw UTF-8: decode and fold to ASCII (the font has no accents).
            let (cp, len) = decode_utf8(&html[i..]);
            if let Some(b) = fold_ascii(cp) {
                w.push(b);
            }
            i += len;
        } else {
            w.push(c);
            i += 1;
        }
    }
    w.len()
}

/// Decode the first UTF-8 scalar in `bytes`, returning its code point and the
/// number of bytes consumed. Malformed input yields `(0, 1)` so the caller
/// skips one byte and makes progress.
pub fn decode_utf8(bytes: &[u8]) -> (u32, usize) {
    let b0 = bytes[0];
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

/// Greedy word-wrapper that writes into a fixed buffer, collapsing whitespace
/// and breaking lines at word boundaries no wider than [`COLS`]. Words are
/// buffered until a separator so a break can be decided before emitting them.
struct Wrapper<'a> {
    out: &'a mut [u8],
    len: usize,
    col: usize,
    word: [u8; COLS],
    wlen: usize,
}

impl<'a> Wrapper<'a> {
    fn new(out: &'a mut [u8]) -> Self {
        Self {
            out,
            len: 0,
            col: 0,
            word: [0; COLS],
            wlen: 0,
        }
    }

    fn raw(&mut self, b: u8) {
        if self.len < self.out.len() {
            self.out[self.len] = b;
            self.len += 1;
        }
    }

    /// Emit the buffered word, wrapping to a new line first if it would not fit.
    fn flush_word(&mut self) {
        if self.wlen == 0 {
            return;
        }
        // A leading space costs one column when continuing a line.
        let needs_space = self.col > 0;
        let cost = self.wlen + needs_space as usize;
        if needs_space && self.col + cost > COLS {
            self.raw(b'\n');
            self.col = 0;
        } else if needs_space {
            self.raw(b' ');
            self.col += 1;
        }
        for i in 0..self.wlen {
            self.raw(self.word[i]);
        }
        self.col += self.wlen;
        self.wlen = 0;
    }

    fn push(&mut self, b: u8) {
        if b == b' ' {
            self.space();
            return;
        }
        if self.wlen == self.word.len() {
            // Word longer than a full line: hard-break it onto its own line.
            self.flush_word();
            self.raw(b'\n');
            self.col = 0;
        }
        self.word[self.wlen] = b;
        self.wlen += 1;
    }

    fn space(&mut self) {
        self.flush_word();
    }

    fn newline(&mut self) {
        self.flush_word();
        self.raw(b'\n');
        self.col = 0;
    }

    fn len(&mut self) -> usize {
        self.flush_word();
        self.len
    }
}

// ---- the address-bar + content model ----

/// The browser app's editable address bar plus the rendered page text and
/// scroll position. The kernel drives networking via [`take_request`] /
/// [`load_response`].
pub struct Browser {
    url: [u8; URL_CAP],
    url_len: usize,
    caret: usize,
    content: [u8; CONTENT_CAP],
    content_len: usize,
    status: Status,
    pending: bool,
    nav: [u8; URL_CAP],
    nav_len: usize,
    home: bool,        // showing the native start page (no page loaded)
    pub scroll: usize, // first visible text line
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
            content: [0; CONTENT_CAP],
            content_len: 0,
            status: Status::Idle,
            pending: false,
            nav: [0; URL_CAP],
            nav_len: 0,
            home: true,
            scroll: 0,
        };
        b.set_url(b"");
        b
    }

    /// True while the native start page (logo + shortcuts) is shown.
    pub fn is_home(&self) -> bool {
        self.home
    }

    /// Return to the start page, clearing the loaded content and address bar.
    pub fn go_home(&mut self) {
        self.home = true;
        self.status = Status::Idle;
        self.content_len = 0;
        self.scroll = 0;
        self.set_url(b"");
    }

    /// Re-fetch the current address (no-op on the start page).
    pub fn reload(&mut self) {
        if !self.home {
            self.submit();
        }
    }

    /// Navigate straight to `url` (used by the start-page shortcuts).
    pub fn open(&mut self, url: &[u8]) {
        self.set_url(url);
        self.submit();
    }

    fn set_url(&mut self, s: &[u8]) {
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
    pub fn content(&self) -> &[u8] {
        &self.content[..self.content_len]
    }

    /// Number of wrapped text lines in the current content.
    pub fn line_count(&self) -> usize {
        if self.content_len == 0 {
            return 0;
        }
        1 + self.content[..self.content_len]
            .iter()
            .filter(|&&c| c == b'\n')
            .count()
    }

    /// The `idx`-th wrapped line (without the trailing newline), or `None`.
    pub fn line(&self, idx: usize) -> Option<&[u8]> {
        self.content[..self.content_len]
            .split(|&c| c == b'\n')
            .nth(idx)
    }

    /// Handle a key while the address bar has focus. Returns `true` if anything
    /// changed (so the caller repaints). ENTER submits a navigation/search.
    pub fn on_key(&mut self, key: Key) -> bool {
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
            Key::Up => {
                self.scroll = self.scroll.saturating_sub(1);
                true
            }
            Key::Down => {
                if self.scroll + 1 < self.line_count() {
                    self.scroll += 1;
                }
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
        self.status = Status::Loading;
        self.pending = true;
        self.home = false;
        self.scroll = 0;
    }

    /// Pull a pending navigation target (clears the pending flag). The kernel
    /// fetches it and reports back with [`load_response`] / [`fail`].
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
    }

    /// Replace the page with text extracted from a raw HTTP response.
    pub fn load_response(&mut self, resp: &[u8]) {
        let body = http_body(resp);
        self.content_len = html_to_text(body, &mut self.content);
        if self.content_len == 0 {
            let msg = b"(pagina vazia)";
            self.content_len = msg.len();
            self.content[..msg.len()].copy_from_slice(msg);
        }
        self.status = Status::Done;
        self.scroll = 0;
    }

    /// Mark the current fetch as failed and show a message.
    pub fn fail(&mut self) {
        let msg = b"Falha ao carregar a pagina (sem rede, DNS ou TLS).";
        self.content_len = msg.len();
        self.content[..msg.len()].copy_from_slice(msg);
        self.status = Status::Error;
        self.scroll = 0;
    }

    /// Clamp scroll so at least one line stays on screen for a viewport of
    /// `visible` lines.
    pub fn clamp_scroll(&mut self, visible: usize) {
        let max = self.line_count().saturating_sub(visible.max(1));
        if self.scroll > max {
            self.scroll = max;
        }
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(header_value(resp, b"location"), Some(&b"https://x.com/y"[..]));
        assert_eq!(header_value(resp, b"LOCATION"), Some(&b"https://x.com/y"[..]));
        assert_eq!(header_value(resp, b"missing"), None);
    }

    #[test]
    fn page_body_dechunks() {
        let resp = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        assert_eq!(page_body(resp), b"Wikipedia");
    }

    #[test]
    fn html_strips_tags_and_decodes_entities() {
        let html = b"<html><body><p>Hello &amp; welcome</p><p>Line 2</p></body></html>";
        let mut out = [0u8; 256];
        let n = html_to_text(html, &mut out);
        let text = core::str::from_utf8(&out[..n]).unwrap();
        assert!(text.contains("Hello & welcome"));
        assert!(text.contains("Line 2"));
        // Block boundary inserted a newline.
        assert!(text.contains('\n'));
    }

    #[test]
    fn html_skips_script_and_style() {
        let html = b"<style>.x{color:red}</style><p>Keep</p><script>var a=1<2;</script><p>This</p>";
        let mut out = [0u8; 256];
        let n = html_to_text(html, &mut out);
        let text = core::str::from_utf8(&out[..n]).unwrap();
        assert!(text.contains("Keep"));
        assert!(text.contains("This"));
        assert!(!text.contains("color"));
        assert!(!text.contains("var a"));
    }

    #[test]
    fn html_wraps_long_lines() {
        let mut html = [b'a'; 200];
        // make it words
        for i in (0..200).step_by(5) {
            html[i] = b' ';
        }
        let mut out = [0u8; 512];
        let n = html_to_text(&html, &mut out);
        for line in out[..n].split(|&c| c == b'\n') {
            assert!(line.len() <= COLS, "line too long: {}", line.len());
        }
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
        b.load_response(b"\r\n\r\n<p>hi</p>");
        assert!(!b.is_home());
        b.go_home();
        assert!(b.is_home());
        assert_eq!(b.url(), b"");
        assert_eq!(b.status(), Status::Idle);
    }

    #[test]
    fn html_folds_accented_entities_and_utf8() {
        // Numeric, hex, named entities and raw UTF-8 all fold to ASCII.
        let html = "<p>Conte&#250;do Coment&#225;rios V&#xED;deos Aten\u{e7}\u{e3}o caf\u{e9}</p>"
            .as_bytes();
        let mut out = [0u8; 256];
        let n = html_to_text(html, &mut out);
        let text = core::str::from_utf8(&out[..n]).unwrap();
        assert!(text.contains("Conteudo"));
        assert!(text.contains("Comentarios"));
        assert!(text.contains("Videos"));
        assert!(text.contains("Atencao"));
        assert!(text.contains("cafe"));
        assert!(!text.contains('&'));
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
    fn browser_loads_response_into_lines() {
        let mut b = Browser::new();
        b.load_response(b"HTTP/1.0 200 OK\r\n\r\n<p>One</p><p>Two</p>");
        assert_eq!(b.status(), Status::Done);
        assert!(b.line_count() >= 2);
        assert!(b.content().windows(3).any(|w| w == b"One"));
    }

    #[test]
    fn browser_scroll_clamps() {
        let mut b = Browser::new();
        b.load_response(b"\r\n\r\n<p>a</p><p>b</p><p>c</p>");
        b.scroll = 100;
        b.clamp_scroll(2);
        assert!(b.scroll <= b.line_count());
    }
}
