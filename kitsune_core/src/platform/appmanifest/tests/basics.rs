use super::*;

#[test]
fn minimal_manifest_gets_defaults() {
    let m = parse(MIN).unwrap();
    assert_eq!(m.id, "hello");
    assert_eq!(m.name, "Hello");
    assert_eq!(
        m.version,
        Version {
            major: 1,
            minor: 2,
            patch: 3
        }
    );
    assert_eq!(m.abi, Abi::V2);
    assert_eq!(m.fs, FsPerm::None);
    assert_eq!(m.net, NetPerm::None);
    assert_eq!(m.clipboard, ClipPerm::None);
    assert_eq!(m.mem_mib, DEFAULT_MEM_MIB);
    assert_eq!(m.fuel_frame, DEFAULT_FUEL_FRAME);
    assert_eq!(m.disk_kib, 0, "no fs permission means no disk quota");
    assert_eq!(m.max_fds, DEFAULT_MAX_FDS);
    assert_eq!(m.tick_ms, 0);
    assert_eq!((m.win_w, m.win_h), (DEFAULT_WIN_W, DEFAULT_WIN_H));
    assert_eq!((m.win_min_w, m.win_min_h), (200, 120));
    assert!(m.resizable);
}

#[test]
fn full_manifest() {
    let m = parse(
        "# a comment\nid=notes\nname=Bloco de notas\nversion=0.9.12\nabi=2\nfs=own\nnet=http\n\
         clipboard=rw\nmem_mib=12\nfuel_frame=8000000\ndisk_kib=512\nmax_fds=8\ntick_ms=1000\n\
         win_w=700\nwin_h=500\nwin_min_w=300\nwin_min_h=200\nresizable=0\nx-future=whatever\n",
    )
    .unwrap();
    assert_eq!(m.fs, FsPerm::Own);
    assert_eq!(m.net, NetPerm::Http);
    assert_eq!(m.clipboard, ClipPerm::Rw);
    assert_eq!(m.mem_mib, 12);
    assert_eq!(m.fuel_frame, 8_000_000);
    assert_eq!(m.disk_kib, 512);
    assert_eq!(m.max_fds, 8);
    assert_eq!(m.tick_ms, 1000);
    assert_eq!(
        (m.win_w, m.win_h, m.win_min_w, m.win_min_h),
        (700, 500, 300, 200)
    );
    assert!(!m.resizable);
}

#[test]
fn crlf_blank_lines_and_comments_are_accepted() {
    let m = parse("id=a\r\n\r\n# c\r\nname=A\r\nversion=1.0.0\r\n").unwrap();
    assert_eq!(m.id, "a");
    assert_eq!(m.name, "A");
}

#[test]
fn no_trailing_newline_is_fine() {
    assert!(parse("id=a\nname=A\nversion=1.0.0").is_ok());
}

#[test]
fn value_may_contain_equals_sign() {
    // only the first `=` splits; a name with `=` is a valid printable name
    let m = parse("id=a\nname=a=b\nversion=1.0.0").unwrap();
    assert_eq!(m.name, "a=b");
}

#[test]
fn required_keys() {
    assert_eq!(
        parse("name=A\nversion=1.0.0"),
        Err(ManifestError::Missing("id"))
    );
    assert_eq!(
        parse("id=a\nversion=1.0.0"),
        Err(ManifestError::Missing("name"))
    );
    assert_eq!(
        parse("id=a\nname=A"),
        Err(ManifestError::Missing("version"))
    );
    assert_eq!(parse(""), Err(ManifestError::Missing("id")));
}

#[test]
fn syntax_errors() {
    assert_eq!(with("novalue"), Err(ManifestError::Syntax));
    assert_eq!(with("=x"), Err(ManifestError::Syntax));
    assert_eq!(with("Bad Key=1"), Err(ManifestError::Syntax));
    assert_eq!(with(" fs=own"), Err(ManifestError::Syntax));
    assert_eq!(with("fs =own"), Err(ManifestError::Syntax));
    assert_eq!(with("fs=own\u{7}"), Err(ManifestError::Syntax));
    assert_eq!(with("fs=own\0"), Err(ManifestError::Syntax));
}

#[test]
fn unknown_key_is_an_error_but_x_prefix_is_not() {
    assert_eq!(with("shell=yes"), Err(ManifestError::UnknownKey));
    assert_eq!(with("root=1"), Err(ManifestError::UnknownKey));
    assert!(with("x-vendor=acme").is_ok());
    // a duplicate x- key is also ignored (reserved namespace)
    assert!(with("x-a=1\nx-a=2").is_ok());
}

#[test]
fn duplicate_keys_are_errors() {
    assert_eq!(
        parse("id=a\nid=b\nname=A\nversion=1.0.0"),
        Err(ManifestError::DuplicateKey("id"))
    );
    assert_eq!(
        with("fs=own\nfs=home"),
        Err(ManifestError::DuplicateKey("fs"))
    );
    assert_eq!(
        with("net=none\nnet=none"),
        Err(ManifestError::DuplicateKey("net"))
    );
    assert_eq!(
        with("mem_mib=2\nmem_mib=3"),
        Err(ManifestError::DuplicateKey("mem_mib"))
    );
}

#[test]
fn size_and_line_limits() {
    let big = alloc::vec![b'#'; MAX_MANIFEST_BYTES + 1];
    assert_eq!(Manifest::parse(&big), Err(ManifestError::TooLarge));
    let mut many = String::from(MIN);
    for i in 0..MAX_MANIFEST_LINES {
        many.push_str(&format!("x-k{i}=1\n"));
    }
    assert_eq!(parse(&many), Err(ManifestError::TooManyLines));
    // exactly at the line limit passes
    let mut ok = String::from(MIN); // 3 lines
    for i in 0..MAX_MANIFEST_LINES - 3 {
        ok.push_str(&format!("x-k{i}=1\n"));
    }
    assert!(parse(&ok).is_ok());
}

#[test]
fn comments_do_not_count_as_lines() {
    let mut s = String::from(MIN);
    for _ in 0..200 {
        s.push_str("# note\n");
    }
    // 200 * 7 = 1400 bytes, below 4096
    assert!(parse(&s).is_ok());
}

#[test]
fn not_utf8() {
    assert_eq!(
        Manifest::parse(&[0xFF, 0xFE, b'=']),
        Err(ManifestError::NotUtf8)
    );
}
