use super::*;

fn ok(u: &str) -> Url {
    parse_url(u.as_bytes()).unwrap_or_else(|e| panic!("{u}: {e:?}"))
}

fn err(u: &str) -> NetError {
    parse_url(u.as_bytes()).unwrap_err()
}

#[test]
fn plain_urls() {
    let u = ok("http://example.com/a/b?x=1#frag");
    assert_eq!(
        u,
        Url {
            https: false,
            host: "example.com".into(),
            port: 80,
            path: "/a/b?x=1".into()
        }
    );
    let u = ok("https://Example.COM");
    assert_eq!((u.https, u.port, u.path.as_str()), (true, 443, "/"));
    assert_eq!(u.host, "example.com");
    assert_eq!(ok("HTTP://a.io:8080/x").port, 8080);
    assert_eq!(ok("http://a.io?q=1").path, "/?q=1");
    assert_eq!(ok("http://a.io#f").path, "/");
    assert_eq!(ok("http://a.io./").host, "a.io");
}

#[test]
fn schemes() {
    for u in [
        "ftp://a.io/",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "//a.io/",
        "a.io",
        "gopher://a.io",
        "httpx://a.io",
        "",
    ] {
        assert_eq!(err(u), NetError::Scheme, "{u}");
    }
}

#[test]
fn syntax_errors() {
    for u in [
        "http://",
        "http:///x",
        "http://a.io:/x",
        "http://a.io:0/x",
        "http://a.io:65536/x",
        "http://a.io:99999999/x",
        "http://a.io:12ab/x",
        "http://user@a.io/",
        "http://user:pw@a.io/",
        "http://a.io /x",
        "http://a.io/\x01",
        "http://a.io/\u{e9}",
        "http://a.io\\@evil.com/",
    ] {
        assert_eq!(err(u), NetError::Syntax, "{u:?}");
    }
}

#[test]
fn too_long() {
    let long = alloc::format!("http://a.io/{}", "x".repeat(MAX_URL));
    assert_eq!(err(&long), NetError::TooLong);
    let edge = alloc::format!("http://a.io/{}", "x".repeat(MAX_URL - "http://a.io/".len()));
    assert!(parse_url(edge.as_bytes()).is_ok());
}

#[test]
fn loopback_and_private_ipv4_are_refused() {
    for u in [
        "http://127.0.0.1/",
        "http://127.255.255.254/",
        "http://10.0.2.2/",
        "http://10.0.2.3:80/",
        "http://172.16.0.1/",
        "http://172.31.255.255/",
        "http://192.168.1.1/",
        "http://169.254.169.254/latest/meta-data",
        "http://0.0.0.0/",
        "http://100.64.0.1/",
        "http://100.127.255.255/",
        "http://198.18.0.1/",
        "http://192.0.0.1/",
        "http://224.0.0.1/",
        "http://239.255.255.250/",
        "http://255.255.255.255/",
        "http://240.0.0.1/",
    ] {
        assert_eq!(err(u), NetError::Forbidden, "{u}");
    }
}

#[test]
fn public_ipv4_is_allowed() {
    for u in [
        "http://8.8.8.8/",
        "http://1.1.1.1/",
        "http://172.15.0.1/",
        "http://172.32.0.1/",
        "http://100.63.0.1/",
        "http://100.128.0.1/",
        "http://192.169.0.1/",
        "http://198.20.0.1/",
        "http://223.255.255.255/",
    ] {
        assert!(parse_url(u.as_bytes()).is_ok(), "{u}");
    }
}

#[test]
fn disguised_addresses_are_refused() {
    for u in [
        "http://2130706433/",
        "http://0x7f000001/",
        "http://0x7f.0.0.1/",
        "http://0X7F.1/",
        "http://127.1/",
        "http://127.0.1/",
        "http://0177.0.0.1/",
        "http://017700000001/",
        "http://1.2.3/",
        "http://1.2.3.4.5/",
        "http://256.1.1.1/",
        "http://8.8.8.08/",
        "http://3232235777/",
    ] {
        assert_eq!(err(u), NetError::Forbidden, "{u}");
    }
}

#[test]
fn ipv6_is_always_refused() {
    for u in [
        "http://[::1]/",
        "http://[::ffff:127.0.0.1]/",
        "http://[2001:db8::1]/",
        "http://[fe80::1]:80/",
        "http://::1/",
    ] {
        assert_eq!(err(u), NetError::Forbidden, "{u}");
    }
}

#[test]
fn local_names_are_refused() {
    for u in [
        "http://localhost/",
        "http://LOCALHOST:8080/",
        "http://localhost./",
        "http://foo.localhost/",
        "http://printer.local/",
        "http://db.internal/",
        "http://nas.lan/",
        "http://router/",
        "http://intranet/",
        "http://x.home.arpa/",
        "http://localdomain/",
        "http://host.localdomain/",
    ] {
        assert_eq!(err(u), NetError::Forbidden, "{u}");
    }
}

#[test]
fn lookalike_public_names_are_allowed() {
    for u in [
        "http://notlocalhost.com/",
        "http://localhost.example.com/",
        "http://mylocal.com/",
        "http://internal.example.org/",
        "http://lan.example.org/",
        "http://a-b.example.org/",
    ] {
        assert!(parse_url(u.as_bytes()).is_ok(), "{u}");
    }
}

#[test]
fn bad_host_syntax() {
    for u in [
        "http://-a.com/",
        "http://a-.com/",
        "http://a..com/",
        "http://.com/",
        "http://a_b.com/",
        "http://a%2eb.com/",
        "http://a b.com/",
    ] {
        assert!(parse_url(u.as_bytes()).is_err(), "{u}");
    }
    let long_label = alloc::format!("http://{}.com/", "a".repeat(64));
    assert!(parse_url(long_label.as_bytes()).is_err());
}

#[test]
fn host_allowed_direct() {
    assert!(host_allowed("example.com"));
    assert!(!host_allowed(""));
    assert!(!host_allowed(&"a.".repeat(130)));
    assert!(ipv4_allowed([8, 8, 8, 8]));
    assert!(!ipv4_allowed([127, 0, 0, 1]));
}

#[test]
fn limiter_spacing() {
    let mut l = Limiter::new();
    assert!(l.allow(5000));
    assert!(!l.allow(5500));
    assert!(!l.allow(5999));
    assert!(l.allow(6000));
    assert!(!l.allow(6001));
    // clock going backwards does not wedge it
    assert!(l.allow(10));
    assert!(l.allow(u64::MAX));
}

// ---- permission, allow-list, redirects, responses ----

fn hosts(list: &[&str]) -> Vec<String> {
    list.iter().map(|h| String::from(*h)).collect()
}

#[test]
fn allow_list_exact_and_wildcard() {
    let none: Vec<String> = Vec::new();
    assert!(host_permitted(&none, "anything.example"));
    let l = hosts(&["api.example.com", "*.cdn.example.org"]);
    assert!(host_permitted(&l, "api.example.com"));
    assert!(host_permitted(&l, "a.cdn.example.org"));
    assert!(host_permitted(&l, "a.b.cdn.example.org"));
    // not the bare base of a wildcard, not a lookalike, not a different suffix
    assert!(!host_permitted(&l, "cdn.example.org"));
    assert!(!host_permitted(&l, "evilcdn.example.org"));
    assert!(!host_permitted(&l, "cdn.example.org.evil.io"));
    assert!(!host_permitted(&l, "xapi.example.com"));
    assert!(!host_permitted(&l, "api.example.com.evil.io"));
    assert!(!host_permitted(&l, "example.com"));
    assert!(!host_permitted(&l, ""));
    // a leading dot case: ".cdn.example.org" has an empty label, never a subdomain
    assert!(!host_permitted(&l, ".cdn.example.org"));
}

#[test]
fn authorize_orders_its_refusals() {
    use crate::appabi::{ERR_INVAL, ERR_NET, ERR_PERM};
    let any: Vec<String> = Vec::new();
    // no permission: ERR_PERM, whatever the URL (nothing is even parsed)
    assert_eq!(
        authorize(NetPerm::None, &any, b"http://example.com/").unwrap_err(),
        ERR_PERM
    );
    assert_eq!(
        authorize(NetPerm::None, &any, b"garbage").unwrap_err(),
        ERR_PERM
    );
    // permission, then the URL: oversized is INVAL, the filter's refusals are NET
    let long = alloc::format!("http://example.com/{}", "a".repeat(600));
    assert_eq!(
        authorize(NetPerm::Http, &any, long.as_bytes()).unwrap_err(),
        ERR_INVAL
    );
    for bad in [
        "http://localhost/",
        "http://127.0.0.1/",
        "http://10.0.2.2/",
        "http://192.168.1.1/",
        "http://[::1]/",
        "http://2130706433/",
        "ftp://example.com/",
        "http://router/",
    ] {
        assert_eq!(
            authorize(NetPerm::Http, &any, bad.as_bytes()).unwrap_err(),
            ERR_NET,
            "{bad}"
        );
    }
    // an allowed public host passes, `tcp` implies http
    let u = authorize(NetPerm::Tcp, &any, b"https://Example.com/x?y=1").unwrap();
    assert_eq!(
        (u.https, u.host.as_str(), u.port),
        (true, "example.com", 443)
    );
    // the allow-list is a permission: outside it is ERR_PERM, inside it passes, and a
    // listed-but-private name could never have been listed (the filter runs first)
    let only = hosts(&["api.example.com"]);
    assert!(authorize(NetPerm::Http, &only, b"http://api.example.com/").is_ok());
    assert_eq!(
        authorize(NetPerm::Http, &only, b"http://other.example.com/").unwrap_err(),
        ERR_PERM
    );
    assert_eq!(
        authorize(NetPerm::Http, &only, b"http://127.0.0.1/").unwrap_err(),
        ERR_NET
    );
}

#[test]
fn every_redirect_hop_is_checked_like_the_first_request() {
    // The kernel resolves each Location against the previous URL and authorizes the
    // result again: an open redirect to a private address or a host off the list is
    // refused exactly like a direct request.
    use crate::browser::parse_url as page_url;
    use crate::redirect::resolve_redirect;
    let only = hosts(&["api.example.com"]);
    let first = authorize(NetPerm::Http, &only, b"https://api.example.com/start").unwrap();
    let base = page_url(b"https://api.example.com/start").unwrap();
    let hop = |loc: &str| {
        let next = resolve_redirect(&base, loc.as_bytes()).map_err(|_| 0)?;
        authorize(NetPerm::Http, &only, &next)
    };
    assert_eq!(first.host, "api.example.com");
    assert!(hop("/other").is_ok());
    assert!(hop("https://api.example.com/x").is_ok());
    for evil in [
        "https://other.example.com/",
        "https://127.0.0.1/admin",
        "https://10.0.2.2:8080/",
        "http://api.example.com/",
        "https://api.example.com.evil.io/",
    ] {
        assert!(hop(evil).is_err(), "{evil}");
    }
}

fn response(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
    let mut r = alloc::format!("HTTP/1.1 {status}\r\n{headers}\r\n").into_bytes();
    r.extend_from_slice(body);
    r
}

#[test]
fn app_response_gives_2xx_bodies_only() {
    let ok = response("200 OK", "Content-Length: 5\r\n", b"hello");
    assert_eq!(app_response(&ok, 100).unwrap(), (b"hello".to_vec(), false));
    // cut to the app's buffer
    assert_eq!(app_response(&ok, 3).unwrap(), (b"hel".to_vec(), true));
    assert_eq!(app_response(&ok, 5).unwrap(), (b"hello".to_vec(), false));
    assert_eq!(app_response(&ok, 0).unwrap(), (Vec::new(), true));
    // chunked
    let ch = response(
        "200 OK",
        "Transfer-Encoding: chunked\r\n",
        b"3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n",
    );
    assert_eq!(app_response(&ch, 100).unwrap().0, b"abcde");
    // 204 and other 2xx
    assert!(app_response(&response("204 No Content", "", b""), 10).is_ok());
    // everything else is refused with its status
    for (st, code) in [
        ("301 Moved", 301),
        ("404 Not Found", 404),
        ("500 Oops", 500),
        ("100 Continue", 100),
    ] {
        assert_eq!(
            app_response(&response(st, "", b"body"), 10).unwrap_err(),
            ResponseError::Status(code)
        );
    }
    assert_eq!(app_response(b"", 10).unwrap_err(), ResponseError::Malformed);
    assert_eq!(
        app_response(b"not http at all", 10).unwrap_err(),
        ResponseError::Malformed
    );
    // an encoding we cannot decode is a failure, not garbage
    let br = response("200 OK", "Content-Encoding: br\r\n", b"\x01\x02\x03");
    assert_eq!(app_response(&br, 10).unwrap_err(), ResponseError::Encoding);
    let bad_gz = response("200 OK", "Content-Encoding: gzip\r\n", b"\x1f\x8b\x08junk");
    assert_eq!(
        app_response(&bad_gz, 10).unwrap_err(),
        ResponseError::Encoding
    );
    assert_eq!(ResponseError::Status(404).code(), crate::appabi::ERR_NET);
}
