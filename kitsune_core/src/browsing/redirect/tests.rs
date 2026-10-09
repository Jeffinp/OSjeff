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
