//! url (split out of `browser.rs`).

use super::*;

impl Url {
    pub fn host(&self) -> &[u8] {
        &self.host[..self.host_len]
    }
    pub fn path(&self) -> &[u8] {
        &self.path[..self.path_len]
    }
}

pub(super) fn starts_with_ci(s: &[u8], prefix: &[u8]) -> bool {
    s.len() >= prefix.len()
        && s[..prefix.len()]
            .iter()
            .zip(prefix)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// True for ASCII control bytes (C0 and DEL). A URL carrying one could split
/// the request line / smuggle a header once host and path are written into the
/// HTTP request.
pub(super) fn is_control(b: u8) -> bool {
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
