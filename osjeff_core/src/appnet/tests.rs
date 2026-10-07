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
