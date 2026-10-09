//! HTTP redirect handling for the browser's fetcher: turning a `Location`
//! header into an absolute URL, and deciding whether following it is allowed.
//!
//! The `Location` value comes from a remote server and is untrusted. Rules:
//!
//! * the scheme of the request is kept for relative references (`/x`, `x`,
//!   `?q`, `//host/x`); it is never silently swapped for `https`;
//! * `https://` to `http://` is **blocked** ([`RedirectError::Downgrade`]);
//! * only `http` and `https` targets are accepted (no `javascript:`, `ftp:`...);
//! * any control character, space or oversized value is rejected outright, so
//!   a `Location` cannot split the next request line or smuggle a header;
//! * a redirect to a URL already visited in this navigation is a loop, and at
//!   most [`MAX_REDIRECTS`] redirects are followed.
//!
//! Pure and host-tested; the kernel's `fetch` module only drives the I/O.

use crate::browsing::browser::{HOST_CAP, URL_CAP, Url, parse_url};
use alloc::vec::Vec;

/// Maximum redirects followed for one navigation.
pub const MAX_REDIRECTS: usize = 5;

/// Why a redirect was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedirectError {
    /// Empty, oversized, contains controls/spaces, non-http(s), or unparseable.
    Invalid,
    /// `https://` page redirected to `http://`.
    Downgrade,
    /// Target already visited in this navigation.
    Loop,
    /// More than [`MAX_REDIRECTS`] redirects.
    TooMany,
}

fn starts_with_ci(s: &[u8], prefix: &[u8]) -> bool {
    s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// Does `loc` start with a URI scheme (`name:`) before any `/`, `?` or `#`?
fn has_scheme(loc: &[u8]) -> bool {
    let mut it = loc.iter().enumerate();
    match it.next() {
        Some((_, c)) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    for (_, &c) in it {
        match c {
            b':' => return true,
            c if c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.') => {}
            _ => return false,
        }
    }
    false
}

/// Remove `.` and `..` segments from a path (RFC 3986 5.2.4, simplified). The
/// path starts with `/`; a query string, if any, is kept untouched.
fn normalize_path(path_and_query: &[u8]) -> Vec<u8> {
    let (path, query) = match path_and_query.iter().position(|&b| b == b'?') {
        Some(i) => path_and_query.split_at(i),
        None => (path_and_query, &[][..]),
    };
    let mut segs: Vec<&[u8]> = Vec::new();
    let mut trailing_slash = false;
    for seg in path.split(|&b| b == b'/').skip(1) {
        trailing_slash = false;
        match seg {
            b"." => trailing_slash = true,
            b".." => {
                segs.pop();
                trailing_slash = true;
            }
            s => segs.push(s),
        }
    }
    let mut out = Vec::with_capacity(path_and_query.len() + 1);
    for s in &segs {
        out.push(b'/');
        out.extend_from_slice(s);
    }
    if out.is_empty() || trailing_slash {
        out.push(b'/');
    }
    out.extend_from_slice(query);
    out
}

/// `scheme://host[:port]` for `base`, omitting the scheme's default port.
fn push_origin(out: &mut Vec<u8>, base: &Url) {
    out.extend_from_slice(if base.https { b"https://" } else { b"http://" });
    out.extend_from_slice(base.host());
    let default = if base.https { 443 } else { 80 };
    if base.port != default {
        out.push(b':');
        let mut digits = [0u8; 5];
        let mut n = 0;
        let mut p = base.port;
        while p > 0 {
            digits[n] = b'0' + (p % 10) as u8;
            p /= 10;
            n += 1;
        }
        while n > 0 {
            n -= 1;
            out.push(digits[n]);
        }
    }
}

/// Resolve the `Location` value `loc` of a redirect response received for the
/// request `base`, returning the absolute URL to fetch next.
///
/// Errors: [`RedirectError::Invalid`] for anything malformed or non-http(s),
/// [`RedirectError::Downgrade`] when `base` is `https` and the target `http`.
/// (Loops and the hop limit are tracked by [`Redirects`].)
pub fn resolve_redirect(base: &Url, loc: &[u8]) -> Result<Vec<u8>, RedirectError> {
    // Untrusted input: reject controls, spaces and over-long values up front.
    if loc.is_empty() || loc.len() > URL_CAP || loc.iter().any(|&b| b <= 0x20 || b == 0x7f) {
        return Err(RedirectError::Invalid);
    }
    // A fragment is never sent to the server.
    let loc = match loc.iter().position(|&b| b == b'#') {
        Some(i) => &loc[..i],
        None => loc,
    };

    let mut out = Vec::new();
    if starts_with_ci(loc, b"https://") || starts_with_ci(loc, b"http://") {
        out.extend_from_slice(loc);
    } else if has_scheme(loc) {
        return Err(RedirectError::Invalid); // javascript:, data:, ftp:, host:port...
    } else if loc.starts_with(b"//") {
        // Protocol-relative: keep the request's scheme.
        out.extend_from_slice(if base.https { b"https:" } else { b"http:" });
        out.extend_from_slice(loc);
    } else if loc.is_empty() {
        // Only a fragment: same document.
        push_origin(&mut out, base);
        out.extend_from_slice(base.path());
    } else if loc.starts_with(b"/") {
        push_origin(&mut out, base);
        out.extend_from_slice(&normalize_path(loc));
    } else if loc.starts_with(b"?") {
        // Query only: the base path (without its own query) plus the new one.
        push_origin(&mut out, base);
        let bp = base.path();
        let end = bp.iter().position(|&b| b == b'?').unwrap_or(bp.len());
        out.extend_from_slice(&bp[..end]);
        out.extend_from_slice(loc);
    } else {
        // Relative path: resolved against the base path's directory.
        push_origin(&mut out, base);
        let bp = base.path();
        let end = bp.iter().position(|&b| b == b'?').unwrap_or(bp.len());
        let dir_end = bp[..end]
            .iter()
            .rposition(|&b| b == b'/')
            .map_or(0, |i| i + 1);
        let mut joined = Vec::from(&bp[..dir_end]);
        if joined.is_empty() {
            joined.push(b'/');
        }
        joined.extend_from_slice(loc);
        out.extend_from_slice(&normalize_path(&joined));
    }

    // Parse what we built: it must be a valid, untruncated http(s) URL.
    let target = parse_url(&out).ok_or(RedirectError::Invalid)?;
    if target.host().len() >= HOST_CAP || target.path().len() >= URL_CAP {
        return Err(RedirectError::Invalid); // parse_url would silently cut it
    }
    if base.https && !target.https {
        return Err(RedirectError::Downgrade);
    }
    Ok(out)
}

/// Redirect bookkeeping for one navigation: hop limit and loop detection.
pub struct Redirects {
    seen: Vec<Vec<u8>>,
    hops: usize,
}

/// Canonical identity of a request target: scheme, lowercased host, port, path.
fn key(u: &Url) -> Vec<u8> {
    let mut k = Vec::new();
    push_origin(&mut k, u);
    for b in &mut k {
        b.make_ascii_lowercase(); // scheme + host only: the path is appended below
    }
    k.extend_from_slice(u.path());
    k
}

impl Redirects {
    /// Start tracking a navigation whose first request is `first`.
    pub fn new(first: &Url) -> Self {
        Redirects {
            seen: alloc::vec![key(first)],
            hops: 0,
        }
    }

    /// Account for a redirect from `base` with the given `Location`. Returns
    /// the absolute URL to fetch next, or why it must not be followed.
    pub fn follow(&mut self, base: &Url, loc: &[u8]) -> Result<Vec<u8>, RedirectError> {
        if self.hops >= MAX_REDIRECTS {
            return Err(RedirectError::TooMany);
        }
        let next = resolve_redirect(base, loc)?;
        let target = parse_url(&next).ok_or(RedirectError::Invalid)?;
        let k = key(&target);
        if self.seen.contains(&k) {
            return Err(RedirectError::Loop);
        }
        self.seen.push(k);
        self.hops += 1;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(url: &str) -> Url {
        parse_url(url.as_bytes()).expect("valid base")
    }
    fn resolve(b: &str, loc: &str) -> Result<String, RedirectError> {
        resolve_redirect(&base(b), loc.as_bytes()).map(|v| String::from_utf8(v).unwrap())
    }

    /// Regression: `resolve_redirect` rebuilt relative targets as `https://...`
    /// whatever the request used, so a plain-http site's `Location: /x` was
    /// re-fetched over https (wrong port, wrong protocol).
    #[test]
    fn relative_redirects_keep_the_request_scheme() {
        assert_eq!(resolve("http://a.com/p/q", "/x").unwrap(), "http://a.com/x");
        assert_eq!(
            resolve("https://a.com/p/q", "/x").unwrap(),
            "https://a.com/x"
        );
        assert_eq!(
            resolve("http://a.com/p/q", "x").unwrap(),
            "http://a.com/p/x"
        );
        assert_eq!(
            resolve("http://a.com/p/q", "?z=1").unwrap(),
            "http://a.com/p/q?z=1"
        );
        assert_eq!(
            resolve("http://a.com/p/q?old=1", "?z=1").unwrap(),
            "http://a.com/p/q?z=1"
        );
        assert_eq!(resolve("http://a.com", "x").unwrap(), "http://a.com/x");
        assert_eq!(
            resolve("https://a.com/d/", "e/f").unwrap(),
            "https://a.com/d/e/f"
        );
    }

    #[test]
    fn relative_redirects_keep_the_port() {
        assert_eq!(
            resolve("http://a.com:8080/p", "/x").unwrap(),
            "http://a.com:8080/x"
        );
        assert_eq!(
            resolve("https://a.com:8443/p/q", "r").unwrap(),
            "https://a.com:8443/p/r"
        );
        // Default ports are not spelled out.
        assert_eq!(
            resolve("http://a.com:80/p", "/x").unwrap(),
            "http://a.com/x"
        );
        assert_eq!(resolve("https://a.com/p", "/x").unwrap(), "https://a.com/x");
    }

    #[test]
    fn absolute_redirects_are_taken_as_is() {
        assert_eq!(
            resolve("https://a.com/p", "https://b.org:99/z?q=1").unwrap(),
            "https://b.org:99/z?q=1"
        );
        assert_eq!(
            resolve("http://a.com/p", "HTTPS://B.org/z").unwrap(),
            "HTTPS://B.org/z"
        );
        assert_eq!(
            resolve("http://a.com/p", "http://b.org").unwrap(),
            "http://b.org"
        );
    }

    #[test]
    fn protocol_relative_uses_the_request_scheme() {
        assert_eq!(
            resolve("http://a.com/p", "//b.org/x").unwrap(),
            "http://b.org/x"
        );
        assert_eq!(
            resolve("https://a.com/p", "//b.org/x").unwrap(),
            "https://b.org/x"
        );
        assert_eq!(
            resolve("https://a.com/p", "//b.org:8443/x").unwrap(),
            "https://b.org:8443/x"
        );
        assert_eq!(
            resolve("https://a.com/p", "//").unwrap_err(),
            RedirectError::Invalid
        );
    }

    /// An https page must never be sent to plain http, absolute or not.
    #[test]
    fn https_to_http_is_blocked() {
        assert_eq!(
            resolve("https://a.com/p", "http://a.com/p").unwrap_err(),
            RedirectError::Downgrade
        );
        assert_eq!(
            resolve("https://a.com/p", "HTTP://evil.example/").unwrap_err(),
            RedirectError::Downgrade
        );
        // The opposite direction, and same-scheme moves, are fine.
        assert!(resolve("http://a.com/p", "https://a.com/p").is_ok());
        assert!(resolve("http://a.com/p", "http://b.com/").is_ok());
    }

    /// Regression: a `Location` with control characters (CR/LF/NUL/TAB) went
    /// into the next request line and `Host:` header unfiltered.
    #[test]
    fn location_with_controls_or_spaces_is_rejected() {
        for bad in [
            "/x\r\nHost: evil.example",
            "/x\nSet-Cookie: a=b",
            "https://a.com/\r\n\r\nGET /admin",
            "/x\0y",
            "/x\ty",
            "/x\x7fy",
            "/a b",
            "https://a.com/a b",
            " /x",
            "",
        ] {
            assert_eq!(
                resolve("https://a.com/p", bad).unwrap_err(),
                RedirectError::Invalid,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn non_http_schemes_are_rejected() {
        for bad in [
            "javascript:alert(1)",
            "data:text/html,hi",
            "ftp://a.com/x",
            "file:///etc/passwd",
            "mailto:x@y.z",
            "a.com:8080/x",
            "x://y",
        ] {
            assert_eq!(
                resolve("http://a.com/p", bad).unwrap_err(),
                RedirectError::Invalid,
                "{bad:?}"
            );
        }
        // A colon later in the path is not a scheme.
        assert_eq!(
            resolve("http://a.com/p", "/a:b").unwrap(),
            "http://a.com/a:b"
        );
        assert_eq!(
            resolve("http://a.com/p", "a/b:c").unwrap(),
            "http://a.com/a/b:c"
        );
    }

    #[test]
    fn fragments_and_dot_segments() {
        assert_eq!(
            resolve("http://a.com/p", "/x#frag").unwrap(),
            "http://a.com/x"
        );
        assert_eq!(resolve("http://a.com/p", "#top").unwrap(), "http://a.com/p");
        assert_eq!(
            resolve("http://a.com/a/b/c", "../d").unwrap(),
            "http://a.com/a/d"
        );
        assert_eq!(
            resolve("http://a.com/a/b/c", "./d").unwrap(),
            "http://a.com/a/b/d"
        );
        assert_eq!(
            resolve("http://a.com/a/b/c", "../../../../d").unwrap(),
            "http://a.com/d"
        );
        assert_eq!(
            resolve("http://a.com/a/b/c", "/x/./y/../z").unwrap(),
            "http://a.com/x/z"
        );
        assert_eq!(
            resolve("http://a.com/a/b/c", "..").unwrap(),
            "http://a.com/a/"
        );
        assert_eq!(
            resolve("http://a.com/a/b/c", "../d?x=../y").unwrap(),
            "http://a.com/a/d?x=../y"
        );
    }

    #[test]
    fn oversized_targets_are_rejected_not_truncated() {
        let long = alloc::format!("/{}", "a".repeat(URL_CAP));
        assert_eq!(
            resolve("http://a.com/p", &long).unwrap_err(),
            RedirectError::Invalid
        );
        // Just under the cap is fine.
        let ok = alloc::format!("/{}", "a".repeat(URL_CAP - 20));
        assert!(resolve("http://a.com/p", &ok).is_ok());
        let long_host = alloc::format!("http://{}.com/", "h".repeat(HOST_CAP));
        assert_eq!(
            resolve("http://a.com/p", &long_host).unwrap_err(),
            RedirectError::Invalid
        );
    }

    #[test]
    fn redirect_loops_are_detected() {
        // a -> b -> a
        let a = base("https://a.com/x");
        let mut r = Redirects::new(&a);
        let b_url = r.follow(&a, b"https://b.com/y").unwrap();
        let b = parse_url(&b_url).unwrap();
        assert_eq!(
            r.follow(&b, b"https://A.com/x").unwrap_err(),
            RedirectError::Loop
        );
        // A page redirecting to itself is a loop on the first hop.
        let mut r = Redirects::new(&a);
        assert_eq!(r.follow(&a, b"/x").unwrap_err(), RedirectError::Loop);
        // Same host, different path or port, is not a loop.
        let mut r = Redirects::new(&a);
        assert!(r.follow(&a, b"/y").is_ok());
        let mut r = Redirects::new(&a);
        assert!(r.follow(&a, b"https://a.com:444/x").is_ok());
        // Paths are case-sensitive, hosts are not.
        let mut r = Redirects::new(&a);
        assert!(r.follow(&a, b"/X").is_ok());
    }

    #[test]
    fn hop_limit_is_enforced() {
        let mut cur = base("http://a.com/0");
        let mut r = Redirects::new(&cur);
        for i in 1..=MAX_REDIRECTS {
            let next = r.follow(&cur, alloc::format!("/{i}").as_bytes()).unwrap();
            cur = parse_url(&next).unwrap();
        }
        assert_eq!(
            r.follow(&cur, b"/next").unwrap_err(),
            RedirectError::TooMany
        );
        // And it keeps refusing.
        assert_eq!(
            r.follow(&cur, b"/again").unwrap_err(),
            RedirectError::TooMany
        );
    }

    #[test]
    fn follow_propagates_downgrade_and_invalid() {
        let a = base("https://a.com/x");
        let mut r = Redirects::new(&a);
        assert_eq!(
            r.follow(&a, b"http://a.com/x").unwrap_err(),
            RedirectError::Downgrade
        );
        assert_eq!(
            r.follow(&a, b"/a\r\nb").unwrap_err(),
            RedirectError::Invalid
        );
        // A refused redirect does not consume a hop.
        for i in 0..MAX_REDIRECTS {
            assert!(r.follow(&a, alloc::format!("/h{i}").as_bytes()).is_ok());
        }
    }
}
