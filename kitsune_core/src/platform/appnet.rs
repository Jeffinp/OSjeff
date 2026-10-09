//! Network policy for apps with `net=http`: URL parsing, the destination
//! filter and the limits. The kernel only transports; what an app may reach is
//! decided (and tested) here.
//!
//! **Default filter.** Refused with `ERR_NET`: names `localhost`, `*.localhost`,
//! `*.local`, `*.internal`, `*.lan`, `*.home.arpa` and single-label names (LAN
//! hosts such as `router`); IPv4 literals in `0/8`, `10/8`, `100.64/10`,
//! `127/8`, `169.254/16`, `172.16/12`, `192.0.0/24`, `192.168/16`, `198.18/15`,
//! and everything `>= 224.0.0.0` (multicast, reserved, broadcast); every IPv6
//! literal (`[...]`); and any numeric-looking host that is not a canonical
//! dotted quad (`2130706433`, `0x7f.1`, `127.1`, `0177.0.0.1`), because those
//! are the classic ways to smuggle a loopback address past a name filter. The
//! QEMU gateway (`10.0.2.2`) and the LAN are therefore out of reach of apps.
//! The kernel must repeat the address check on the *resolved* IP.

use crate::platform::appabi::{ERR_INVAL, ERR_NET, ERR_PERM};
use crate::platform::appmanifest::NetPerm;
use alloc::string::String;
use alloc::vec::Vec;

pub const MAX_URL: usize = 512;
/// Largest response body handed to the guest.
pub const MAX_BODY: usize = 256 * 1024;
/// Total time budget for one plain-HTTP request.
pub const TIMEOUT_MS: u64 = 8_000;
/// The same for HTTPS: the software TLS handshake alone can take seconds.
pub const TIMEOUT_HTTPS_MS: u64 = 20_000;
/// Minimum spacing between two requests of the same app.
pub const MIN_INTERVAL_MS: u64 = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetError {
    TooLong,
    /// Not `http://` or `https://`.
    Scheme,
    /// Syntax problem (empty host, bad port, control/space characters, `user@`).
    Syntax,
    /// Valid URL, forbidden destination.
    Forbidden,
}

/// A parsed, filter-approved URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    pub https: bool,
    /// Lower-case host without a trailing dot.
    pub host: String,
    pub port: u16,
    /// Path and query, always starting with `/`; fragment removed.
    pub path: String,
}

/// Is the IPv4 address (as four octets) one an app may reach?
pub fn ipv4_allowed(o: [u8; 4]) -> bool {
    let [a, b, _, _] = o;
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 192 && b == 0 && o[2] == 0)
        || (a == 198 && (18..=19).contains(&b)))
}

fn parse_dotted_quad(h: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut n = 0;
    for part in h.split('.') {
        if n == 4 || part.is_empty() || part.len() > 3 || !part.bytes().all(|c| c.is_ascii_digit())
        {
            return None;
        }
        if part.len() > 1 && part.starts_with('0') {
            return None; // octal-looking
        }
        out[n] = part.parse::<u8>().ok()?;
        n += 1;
    }
    (n == 4).then_some(out)
}

/// Does the host look numeric (an IP in some notation)?
fn numeric_looking(h: &str) -> bool {
    let last = h.rsplit('.').next().unwrap_or("");
    let hexish = h
        .split('.')
        .any(|l| l.starts_with("0x") || l.starts_with("0X"));
    hexish || (!last.is_empty() && last.bytes().all(|c| c.is_ascii_digit()))
}

/// Is `host` (lower-case, no trailing dot) an allowed destination name or IP?
pub fn host_allowed(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    if numeric_looking(host) {
        return match parse_dotted_quad(host) {
            Some(o) => ipv4_allowed(o),
            None => false,
        };
    }
    if !host.contains('.') {
        return false; // single label: a LAN name
    }
    for bad in ["localhost", "local", "internal", "lan", "localdomain"] {
        if host == bad || host.ends_with(&alloc::format!(".{bad}")) {
            return false;
        }
    }
    if host.ends_with(".home.arpa") || host == "home.arpa" {
        return false;
    }
    host.split('.').all(|l| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
    })
}

/// Parses `url` and applies the destination filter.
pub fn parse_url(url: &[u8]) -> Result<Url, NetError> {
    if url.len() > MAX_URL {
        return Err(NetError::TooLong);
    }
    if !url.iter().all(|&c| (0x21..=0x7E).contains(&c)) {
        return Err(NetError::Syntax);
    }
    // all bytes are printable ASCII
    let s: String = url.iter().map(|&b| b as char).collect();
    let (https, rest) = if let Some(r) = strip_prefix_ci(&s, "https://") {
        (true, r)
    } else if let Some(r) = strip_prefix_ci(&s, "http://") {
        (false, r)
    } else {
        return Err(NetError::Scheme);
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (auth, tail) = rest.split_at(end);
    if auth.is_empty() || auth.contains('@') || auth.starts_with('[') || auth.contains('\\') {
        return Err(if auth.starts_with('[') {
            NetError::Forbidden
        } else {
            NetError::Syntax
        });
    }
    let (host, port) = match auth.rsplit_once(':') {
        Some((h, p)) => {
            if p.is_empty() || p.len() > 5 || !p.bytes().all(|c| c.is_ascii_digit()) {
                return Err(NetError::Syntax);
            }
            let n: u32 = p.parse().map_err(|_| NetError::Syntax)?;
            if n == 0 || n > 65535 {
                return Err(NetError::Syntax);
            }
            (h, n as u16)
        }
        None => (auth, if https { 443 } else { 80 }),
    };
    let host = host.strip_suffix('.').unwrap_or(host);
    let host = host.to_ascii_lowercase();
    if host.contains(':') {
        return Err(NetError::Forbidden); // IPv6 without brackets or odd syntax
    }
    if !host_allowed(&host) {
        return Err(NetError::Forbidden);
    }
    // path + query, fragment dropped
    let tail = tail.split('#').next().unwrap_or("");
    let path = if tail.is_empty() {
        String::from("/")
    } else if tail.starts_with('/') {
        String::from(tail)
    } else {
        // "?query" with no path
        let mut p = String::from("/");
        p.push_str(tail);
        p
    };
    Ok(Url {
        https,
        host,
        port,
        path,
    })
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// Does the app's `net_hosts` allow `host` (lower-case, as [`Url::host`])? An empty
/// list allows every host the destination filter accepted; otherwise `host` must
/// equal an entry or, for a `*.d` entry, be a subdomain of `d` (`d` itself is not
/// covered: list it too). Used on the request URL and again on every redirect hop.
pub fn host_permitted(net_hosts: &[String], host: &str) -> bool {
    if net_hosts.is_empty() {
        return true;
    }
    net_hosts.iter().any(|e| match e.strip_prefix("*.") {
        Some(base) => host
            .strip_suffix(base)
            .is_some_and(|head| head.len() > 1 && head.ends_with('.')),
        None => e == host,
    })
}

/// May an app with permission `perm` and allow-list `hosts` fetch `url`? On success
/// the parsed URL (the destination filter and the allow-list both passed); on
/// failure the ABI error code to hand to the guest: `ERR_PERM` for no network
/// permission or a host outside `net_hosts`, `ERR_INVAL` for an oversized URL,
/// `ERR_NET` for anything else the filter refuses. Called for the request and,
/// with the redirect target, for every hop.
pub fn authorize(perm: NetPerm, hosts: &[String], url: &[u8]) -> Result<Url, i32> {
    if !perm.allows_http() {
        return Err(ERR_PERM);
    }
    let u = parse_url(url).map_err(|e| match e {
        NetError::TooLong => ERR_INVAL,
        _ => ERR_NET,
    })?;
    if !host_permitted(hosts, &u.host) {
        return Err(ERR_PERM);
    }
    Ok(u)
}

/// Why a response is not handed to the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseError {
    /// Not an HTTP response (no status line).
    Malformed,
    /// A status other than 2xx (the app only sees successful bodies).
    Status(u16),
    /// The body uses an encoding that could not be decoded.
    Encoding,
}

impl ResponseError {
    /// The ABI code for the guest.
    pub fn code(self) -> i32 {
        ERR_NET
    }
}

/// The body an app receives for the raw HTTP `response`: decoded (chunked, gzip,
/// deflate) and cut to `cap` bytes; the flag says it was cut. Only a 2xx status
/// yields a body.
pub fn app_response(response: &[u8], cap: usize) -> Result<(Vec<u8>, bool), ResponseError> {
    let status = crate::browsing::browser::status_code(response).ok_or(ResponseError::Malformed)?;
    if !(200..300).contains(&status) {
        return Err(ResponseError::Status(status));
    }
    // A cut or damaged compressed body still hands the app its decoded prefix; the
    // flag tells it the body is partial (as it does for a body over `cap`).
    let page = crate::browsing::browser::body_partial(response, false)
        .map_err(|_| ResponseError::Encoding)?;
    let mut body = page.body;
    let cut = body.len() > cap || page.note.is_some();
    body.truncate(cap);
    Ok((body, cut))
}

/// One request at a time, at most one per [`MIN_INTERVAL_MS`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Limiter {
    last: Option<u64>,
}

impl Limiter {
    pub const fn new() -> Limiter {
        Limiter { last: None }
    }

    /// May a request start at `now_ms`? Records it when allowed.
    pub fn allow(&mut self, now_ms: u64) -> bool {
        match self.last {
            Some(t) if now_ms.saturating_sub(t) < MIN_INTERVAL_MS && now_ms >= t => false,
            _ => {
                self.last = Some(now_ms);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests;
