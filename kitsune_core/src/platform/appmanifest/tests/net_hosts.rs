use super::*;

#[test]
fn net_hosts_parse_and_default() {
    let m = Manifest::parse(b"id=n\nname=N\nversion=1.0.0\nnet=http\n").unwrap();
    assert!(m.net_hosts.is_empty());
    let m = with_hosts("http", "api.example.com,*.cdn.example.org").unwrap();
    assert_eq!(m.net_hosts, ["api.example.com", "*.cdn.example.org"]);
    // tcp implies http, so the list is fine there too
    assert!(with_hosts("tcp", "a.example.com").is_ok());
    assert!(Manifest::legacy("x", "X").net_hosts.is_empty());
}

#[test]
fn net_hosts_need_the_permission_and_valid_public_names() {
    // An allow-list for a permission the app does not have is an error.
    assert_eq!(
        with_hosts("none", "a.example.com").unwrap_err(),
        ManifestError::BadValue("net_hosts")
    );
    for bad in [
        "",
        ",",
        "a.example.com,",
        ",a.example.com",
        "a.example.com,,b.example.com",
        "localhost",
        "router",
        "127.0.0.1",
        "10.0.0.1",
        "192.168.1.1",
        "*.localhost",
        "*.local",
        "*.com", // a wildcard needs a base with a dot of its own... (single label base)
        "*",
        "*.",
        "**.example.com",
        "*example.com",
        "exa*mple.com",
        "Example.com", // upper case is not canonical
        "a.example.com ",
        " a.example.com",
        "a b.example.com",
        "-a.example.com",
        "a.example.com:8080",
        "http://a.example.com",
        "a.example.com/x",
        "a_b.example.com",
        "\u{e9}.example.com",
        "[::1]",
        "a.example.com,a.example.com", // duplicate
    ] {
        assert_eq!(
            with_hosts("http", bad).unwrap_err(),
            ManifestError::BadValue("net_hosts"),
            "{bad:?}"
        );
    }
}

#[test]
fn net_hosts_have_a_ceiling() {
    let eight: Vec<String> = (0..MAX_NET_HOSTS)
        .map(|i| alloc::format!("h{i}.example.com"))
        .collect();
    assert_eq!(
        with_hosts("http", &eight.join(","))
            .unwrap()
            .net_hosts
            .len(),
        8
    );
    let nine: Vec<String> = (0..=MAX_NET_HOSTS)
        .map(|i| alloc::format!("h{i}.example.com"))
        .collect();
    assert_eq!(
        with_hosts("http", &nine.join(",")).unwrap_err(),
        ManifestError::OverLimit("net_hosts")
    );
    let long = alloc::format!("{}.example.com", "a".repeat(MAX_NET_HOSTS_LEN));
    assert!(with_hosts("http", &long).is_err());
    // repeated key
    assert_eq!(
        Manifest::parse(
            b"id=n\nname=N\nversion=1.0.0\nnet=http\nnet_hosts=a.example.com\nnet_hosts=b.example.com\n"
        )
        .unwrap_err(),
        ManifestError::DuplicateKey("net_hosts")
    );
}
