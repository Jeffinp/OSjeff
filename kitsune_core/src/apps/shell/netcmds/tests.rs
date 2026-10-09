use super::*;

#[test]
fn ipv4_literals() {
    assert_eq!(parse_ipv4("10.0.2.3"), Some([10, 0, 2, 3]));
    assert_eq!(parse_ipv4("0.0.0.0"), Some([0; 4]));
    for bad in [
        "",
        "1.2.3",
        "1.2.3.4.5",
        "256.1.1.1",
        "a.b.c.d",
        "1..2.3",
        "+1.2.3.4",
        "1.2.3.4 ",
    ] {
        assert_eq!(parse_ipv4(bad), None, "{bad:?}");
    }
}

#[test]
fn urls_get_a_scheme_or_are_refused() {
    assert_eq!(
        normalize_url("example.org/a").unwrap(),
        "http://example.org/a"
    );
    assert_eq!(normalize_url("https://x/").unwrap(), "https://x/");
    assert_eq!(normalize_url("HTTP://x/").unwrap(), "HTTP://x/");
    assert!(normalize_url("ftp://x/").is_err());
    assert!(normalize_url("file:///etc").is_err());
    assert!(normalize_url("http://").is_err());
}

#[test]
fn remote_names() {
    assert_eq!(remote_name("http://a.org/x/y.txt?q=1#f"), "y.txt");
    assert_eq!(remote_name("http://a.org/"), "index.html");
    assert_eq!(remote_name("http://a.org"), "index.html");
    assert_eq!(remote_name("https://a.org/dir/"), "index.html");
}
