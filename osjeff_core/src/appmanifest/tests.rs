use super::*;
use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;

const MIN: &str = "id=hello\nname=Hello\nversion=1.2.3\n";

fn parse(s: &str) -> Result<Manifest, ManifestError> {
    Manifest::parse(s.as_bytes())
}

fn with(extra: &str) -> Result<Manifest, ManifestError> {
    parse(&format!("{MIN}{extra}\n"))
}

fn leb(v: &mut Vec<u8>, mut n: u32) {
    loop {
        let b = (n & 0x7F) as u8;
        n >>= 7;
        if n == 0 {
            v.push(b);
            return;
        }
        v.push(b | 0x80);
    }
}

fn custom(name: &str, data: &[u8]) -> Vec<u8> {
    let mut payload = Vec::new();
    leb(&mut payload, name.len() as u32);
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(data);
    let mut s = Vec::new();
    s.push(0);
    leb(&mut s, payload.len() as u32);
    s.extend_from_slice(&payload);
    s
}

fn module(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut v = wasmsec::HEADER.to_vec();
    for p in parts {
        v.extend_from_slice(p);
    }
    v
}

fn icon_png(w: usize, h: usize) -> Vec<u8> {
    let px = alloc::vec![0xFF2080C0u32; w * h];
    let img = Image::from_pixels(w, h, px).unwrap();
    png::encode(&img).unwrap()
}

// ---------------------------------------------------------------- basics

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

// ---------------------------------------------------------------- id / name

#[test]
fn valid_ids() {
    for id in [
        "a",
        "0",
        "notes",
        "my-app",
        "my_app",
        "a.b.c",
        "x1",
        &"a".repeat(32),
    ] {
        assert!(valid_id(id), "{id}");
    }
}

#[test]
fn invalid_ids() {
    let long = "a".repeat(33);
    for id in [
        "", "A", "My", "-a", "_a", ".a", "a..b", "..", "a/b", "a b", "a\\b", "é", "a:b", "a\0",
        "../x", "a*", &long,
    ] {
        assert!(!valid_id(id), "{id:?}");
    }
}

#[test]
fn hostile_ids_in_manifest() {
    for id in ["../etc", "a/b", "A", "", "a..b", "x y"] {
        assert_eq!(
            parse(&format!("id={id}\nname=A\nversion=1.0.0")),
            Err(ManifestError::BadValue("id")),
            "{id:?}"
        );
    }
}

#[test]
fn names() {
    assert!(parse("id=a\nname=Relogio 2\nversion=1.0.0").is_ok());
    for n in [
        "",
        " lead",
        "trail ",
        "tab\there",
        "\u{e9}",
        "123456789012345678901234x",
    ] {
        assert!(
            parse(&format!("id=a\nname={n}\nversion=1.0.0")).is_err(),
            "{n:?}"
        );
    }
    // 24 characters is the limit
    assert!(parse(&format!("id=a\nname={}\nversion=1.0.0", "n".repeat(24))).is_ok());
    assert!(parse(&format!("id=a\nname={}\nversion=1.0.0", "n".repeat(25))).is_err());
}

// ---------------------------------------------------------------- version

#[test]
fn versions() {
    for v in [
        "1.0",
        "1",
        "1.2.3.4",
        "a.b.c",
        "1.2.",
        ".1.2",
        "-1.0.0",
        "1.0.65536",
        "01.0.0",
        "1..2",
        "1. 2.3",
        "",
    ] {
        assert!(
            parse(&format!("id=a\nname=A\nversion={v}")).is_err(),
            "{v:?}"
        );
    }
    let m = parse("id=a\nname=A\nversion=65535.65535.65535").unwrap();
    assert_eq!(m.version.to_string(), "65535.65535.65535");
    assert!(
        Version {
            major: 1,
            minor: 2,
            patch: 3
        } < Version {
            major: 1,
            minor: 10,
            patch: 0
        }
    );
}

// ---------------------------------------------------------------- permissions

#[test]
fn every_permission_value() {
    assert_eq!(with("fs=none").unwrap().fs, FsPerm::None);
    assert_eq!(with("fs=own").unwrap().fs, FsPerm::Own);
    assert_eq!(with("fs=home").unwrap().fs, FsPerm::Home);
    assert_eq!(with("net=none").unwrap().net, NetPerm::None);
    assert_eq!(with("net=http").unwrap().net, NetPerm::Http);
    assert_eq!(with("net=tcp").unwrap().net, NetPerm::Tcp);
    assert_eq!(with("clipboard=none").unwrap().clipboard, ClipPerm::None);
    assert_eq!(with("clipboard=rw").unwrap().clipboard, ClipPerm::Rw);
    assert!(NetPerm::Http.allows_http() && NetPerm::Tcp.allows_http());
    assert!(!NetPerm::None.allows_http());
}

#[test]
fn nonexistent_permissions_are_refused() {
    for kv in [
        "fs=all",
        "fs=root",
        "fs=OWN",
        "fs=",
        "fs=own,home",
        "fs=/",
        "net=any",
        "net=udp",
        "net=https",
        "net=1",
        "clipboard=r",
        "clipboard=ro",
        "clipboard=yes",
        "clipboard=",
        "abi=3",
        "abi=0",
        "abi=",
        "abi=v2",
        "resizable=2",
        "resizable=true",
        "resizable=",
    ] {
        assert!(with(kv).is_err(), "{kv}");
    }
    assert_eq!(with("fs=all"), Err(ManifestError::BadValue("fs")));
    assert_eq!(with("net=udp"), Err(ManifestError::BadValue("net")));
    assert_eq!(
        with("clipboard=ro"),
        Err(ManifestError::BadValue("clipboard"))
    );
}

#[test]
fn abi_values() {
    assert_eq!(with("abi=1").unwrap().abi, Abi::V1);
    assert_eq!(with("abi=2").unwrap().abi, Abi::V2);
}

// ---------------------------------------------------------------- quotas

#[test]
fn quota_ceilings_are_enforced() {
    assert_eq!(with("mem_mib=25"), Err(ManifestError::OverLimit("mem_mib")));
    assert_eq!(with("mem_mib=24").unwrap().mem_mib, 24);
    assert_eq!(with("mem_mib=0"), Err(ManifestError::BadValue("mem_mib")));
    assert_eq!(
        with("mem_mib=99999"),
        Err(ManifestError::OverLimit("mem_mib"))
    );
    assert_eq!(
        with("fuel_frame=20000001"),
        Err(ManifestError::OverLimit("fuel_frame"))
    );
    assert_eq!(with("fuel_frame=20000000").unwrap().fuel_frame, 20_000_000);
    assert_eq!(
        with("fuel_frame=9999"),
        Err(ManifestError::BadValue("fuel_frame"))
    );
    assert_eq!(with("fuel_frame=10000").unwrap().fuel_frame, 10_000);
    assert_eq!(
        with("fuel_frame=4294967295"),
        Err(ManifestError::OverLimit("fuel_frame"))
    );
    assert_eq!(
        with("fs=own\ndisk_kib=4097"),
        Err(ManifestError::OverLimit("disk_kib"))
    );
    assert_eq!(with("fs=own\ndisk_kib=4096").unwrap().disk_kib, 4096);
    assert_eq!(with("max_fds=33"), Err(ManifestError::OverLimit("max_fds")));
    assert_eq!(with("max_fds=0"), Err(ManifestError::BadValue("max_fds")));
    assert_eq!(with("max_fds=32").unwrap().max_fds, 32);
    assert_eq!(
        with("tick_ms=60001"),
        Err(ManifestError::OverLimit("tick_ms"))
    );
    assert_eq!(with("tick_ms=15"), Err(ManifestError::BadValue("tick_ms")));
    assert_eq!(with("tick_ms=16").unwrap().tick_ms, 16);
    assert_eq!(with("tick_ms=0").unwrap().tick_ms, 0);
}

#[test]
fn numbers_are_strict_decimal() {
    for bad in [
        "+4",
        "-4",
        " 4",
        "4 ",
        "0x10",
        "1_0",
        "04",
        "4.0",
        "99999999999",
        "٣",
    ] {
        assert!(with(&format!("mem_mib={bad}")).is_err(), "{bad:?}");
    }
}

#[test]
fn disk_quota_needs_a_filesystem_permission() {
    assert_eq!(
        with("disk_kib=100"),
        Err(ManifestError::BadValue("disk_kib"))
    );
    assert_eq!(with("fs=none\ndisk_kib=0").unwrap().disk_kib, 0);
    assert_eq!(with("fs=own").unwrap().disk_kib, DEFAULT_DISK_KIB);
    assert_eq!(with("fs=home").unwrap().disk_kib, DEFAULT_DISK_KIB);
    assert_eq!(with("fs=own\ndisk_kib=0").unwrap().disk_kib, 0);
}

#[test]
fn granted_quotas() {
    let q = parse(MIN).unwrap().granted();
    assert_eq!(q.mem_bytes, 8 << 20);
    assert_eq!(q.fuel_frame, DEFAULT_FUEL_FRAME);
    assert_eq!(q.disk_bytes, 0);
    assert_eq!(q.max_fds, 16);
    // A manifest built by hand with absurd numbers is clamped again.
    let mut m = parse(MIN).unwrap();
    m.mem_mib = 4000;
    m.fuel_frame = u64::MAX;
    m.disk_kib = u32::MAX;
    m.max_fds = u32::MAX;
    let q = m.granted();
    assert_eq!(q.mem_bytes, (MAX_MEM_MIB as usize) << 20);
    assert_eq!(q.fuel_frame, MAX_FUEL_FRAME);
    assert_eq!(q.disk_bytes, MAX_DISK_KIB as u64 * 1024);
    assert_eq!(q.max_fds, MAX_FDS as usize);
    m.mem_mib = 0;
    m.fuel_frame = 0;
    m.max_fds = 0;
    let q = m.granted();
    assert_eq!(q.mem_bytes, 1 << 20);
    assert_eq!(q.fuel_frame, MIN_FUEL_FRAME);
    assert_eq!(q.max_fds, 1);
}

// ---------------------------------------------------------------- window

#[test]
fn window_limits() {
    assert_eq!(with("win_w=63"), Err(ManifestError::BadValue("win_w")));
    assert_eq!(with("win_w=1281"), Err(ManifestError::OverLimit("win_w")));
    assert_eq!(with("win_h=801"), Err(ManifestError::OverLimit("win_h")));
    assert_eq!(with("win_h=64").unwrap().win_h, 64);
    assert!(with("win_w=1280\nwin_h=800").is_ok());
    // min above default
    assert_eq!(
        with("win_w=300\nwin_min_w=301"),
        Err(ManifestError::BadValue("win_min_w"))
    );
    assert_eq!(
        with("win_h=300\nwin_min_h=301"),
        Err(ManifestError::BadValue("win_min_h"))
    );
    // a small default shrinks the default minimum with it
    let m = with("win_w=100\nwin_h=100").unwrap();
    assert_eq!((m.win_min_w, m.win_min_h), (100, 100));
}

#[test]
fn content_for_clamps_to_minimum_and_ceiling() {
    let m = with("win_w=400\nwin_h=300\nwin_min_w=200\nwin_min_h=150").unwrap();
    assert_eq!(m.content_for(10, 10), (200, 150));
    assert_eq!(m.content_for(5000, 5000), (MAX_WIN_W, MAX_WIN_H));
    assert_eq!(m.content_for(400, 300), (400, 300));
}

#[test]
fn legacy_manifest_matches_the_old_single_app() {
    let m = Manifest::legacy("snake", "Snake");
    assert_eq!(m.abi, Abi::V1);
    assert_eq!((m.win_w, m.win_h), (692, 414));
    assert!(!m.resizable);
    assert_eq!(m.fs, FsPerm::None);
    assert_eq!(m.net, NetPerm::None);
    assert_eq!(m.granted().mem_bytes, 24 << 20);
    assert_eq!(m.granted().fuel_frame, 20_000_000);
}

// ---------------------------------------------------------------- package

fn pkg(manifest: &str) -> Vec<u8> {
    module(&[custom(MANIFEST_SECTION, manifest.as_bytes())])
}

#[test]
fn package_with_manifest_only() {
    let p = parse_package(&pkg(MIN)).unwrap();
    assert_eq!(p.manifest.id, "hello");
    assert!(p.icon.is_none());
}

#[test]
fn package_with_icon() {
    let png = icon_png(32, 32);
    let w = module(&[
        custom(MANIFEST_SECTION, MIN.as_bytes()),
        custom(ICON_SECTION, &png),
    ]);
    let p = parse_package(&w).unwrap();
    let icon = p.icon.unwrap();
    assert_eq!((icon.width(), icon.height()), (32, 32));
    assert_eq!(icon.get(3, 3), Some(0xFF2080C0));
}

#[test]
fn icon_64_is_the_limit() {
    assert!(decode_icon(&icon_png(64, 64)).is_ok());
    assert_eq!(
        decode_icon(&icon_png(65, 64)).unwrap_err(),
        IconError::Dimensions
    );
    assert_eq!(
        decode_icon(&icon_png(64, 65)).unwrap_err(),
        IconError::Dimensions
    );
    assert_eq!(
        decode_icon(&icon_png(300, 300)).unwrap_err(),
        IconError::Dimensions
    );
}

#[test]
fn icon_must_be_a_png() {
    assert_eq!(decode_icon(b"GIF89a....").unwrap_err(), IconError::NotPng);
    assert_eq!(decode_icon(&[]).unwrap_err(), IconError::NotPng);
    let big = alloc::vec![0u8; MAX_ICON_BYTES + 1];
    assert_eq!(decode_icon(&big).unwrap_err(), IconError::TooLarge);
}

#[test]
fn corrupt_icon_pixels_are_refused() {
    let mut png = icon_png(16, 16);
    let n = png.len();
    // flip a byte in the IDAT payload: the CRC no longer matches
    png[n - 20] ^= 0xFF;
    assert_eq!(decode_icon(&png).unwrap_err(), IconError::Corrupt);
    // truncated file
    let png = icon_png(16, 16);
    assert!(decode_icon(&png[..png.len() - 5]).is_err());
}

#[test]
fn package_errors() {
    assert_eq!(
        parse_package(b"not wasm at all").unwrap_err(),
        PackageError::Wasm(WasmError::BadMagic)
    );
    assert_eq!(
        parse_package(&module(&[])).unwrap_err(),
        PackageError::NoManifest
    );
    let two = module(&[
        custom(MANIFEST_SECTION, MIN.as_bytes()),
        custom(MANIFEST_SECTION, MIN.as_bytes()),
    ]);
    assert_eq!(
        parse_package(&two).unwrap_err(),
        PackageError::DuplicateManifest
    );
    let png = icon_png(8, 8);
    let two = module(&[
        custom(MANIFEST_SECTION, MIN.as_bytes()),
        custom(ICON_SECTION, &png),
        custom(ICON_SECTION, &png),
    ]);
    assert_eq!(
        parse_package(&two).unwrap_err(),
        PackageError::DuplicateIcon
    );
    assert_eq!(
        parse_package(&pkg("id=a\nname=A")).unwrap_err(),
        PackageError::Manifest(ManifestError::Missing("version"))
    );
    let big = module(&[
        custom(MANIFEST_SECTION, MIN.as_bytes()),
        custom(ICON_SECTION, &icon_png(100, 100)),
    ]);
    assert_eq!(
        parse_package(&big).unwrap_err(),
        PackageError::Icon(IconError::Dimensions)
    );
}

#[test]
fn package_opt_distinguishes_legacy_from_broken() {
    assert!(parse_package_opt(&module(&[])).unwrap().is_none());
    assert!(parse_package_opt(&pkg(MIN)).unwrap().is_some());
    assert!(parse_package_opt(&pkg("garbage")).is_err());
    assert!(parse_package_opt(b"\0asm").is_err());
}

#[test]
fn manifest_in_a_regular_section_does_not_count() {
    let c = custom(MANIFEST_SECTION, MIN.as_bytes());
    // wrap the custom-section bytes as the payload of a type section (id 1)
    let mut s = Vec::new();
    s.push(1);
    leb(&mut s, c.len() as u32);
    s.extend_from_slice(&c);
    assert_eq!(
        parse_package(&module(&[s])).unwrap_err(),
        PackageError::NoManifest
    );
}

#[test]
fn error_messages_are_nonempty_and_name_the_key() {
    assert!(
        ManifestError::OverLimit("mem_mib")
            .to_string()
            .contains("mem_mib")
    );
    assert!(ManifestError::Missing("id").to_string().contains("id"));
    assert!(ManifestError::DuplicateKey("fs").to_string().contains("fs"));
    for e in [
        PackageError::NoManifest,
        PackageError::DuplicateIcon,
        PackageError::Icon(IconError::TooLarge),
        PackageError::Wasm(WasmError::BadLeb),
    ] {
        assert!(!e.to_string().is_empty());
    }
}

#[test]
fn prefixes_of_a_package_never_panic() {
    let png = icon_png(16, 16);
    let w = module(&[
        custom(MANIFEST_SECTION, MIN.as_bytes()),
        custom(ICON_SECTION, &png),
    ]);
    for n in 0..=w.len() {
        let _ = parse_package(&w[..n]);
    }
}

#[test]
fn single_byte_flips_never_panic() {
    let w = pkg("id=hello\nname=Hello\nversion=1.0.0\nfs=own\nmem_mib=4\n");
    for i in 0..w.len() {
        for b in [0u8, 0x0A, 0x3D, 0x7F, 0x80, 0xFF] {
            let mut x = w.clone();
            x[i] = b;
            let _ = parse_package(&x);
        }
    }
}

#[test]
fn values_with_stray_whitespace_are_refused() {
    for kv in [
        "fs=own ",
        "fs= own",
        "net=http\t",
        "abi=2 ",
        "resizable=1 ",
        "mem_mib=8 ",
    ] {
        assert!(with(kv).is_err(), "{kv:?}");
    }
    assert!(parse("id=a \nname=A\nversion=1.0.0").is_err());
    assert!(parse("id=a\nname=A\nversion=1.0.0 ").is_err());
}

#[test]
fn empty_values_are_refused() {
    for k in [
        "id",
        "name",
        "version",
        "fs",
        "net",
        "clipboard",
        "mem_mib",
        "win_w",
    ] {
        let text = if matches!(k, "id" | "name" | "version") {
            let mut t = String::new();
            for (kk, v) in [("id", "a"), ("name", "A"), ("version", "1.0.0")] {
                t.push_str(&format!("{kk}={}\n", if kk == k { "" } else { v }));
            }
            t
        } else {
            format!("{MIN}{k}=\n")
        };
        assert!(parse(&text).is_err(), "{k}");
    }
}

#[test]
fn keys_are_case_sensitive() {
    assert_eq!(with("FS=own"), Err(ManifestError::Syntax));
    assert_eq!(with("Fs=own"), Err(ManifestError::Syntax));
}

#[test]
fn manifest_is_equal_after_a_roundtrip_through_a_package() {
    let direct = parse(MIN).unwrap();
    let via = parse_package(&pkg(MIN)).unwrap().manifest;
    assert_eq!(direct, via);
}

#[test]
fn a_full_4k_manifest_of_x_keys_is_still_bounded() {
    // 64 lines of ~60 bytes each stays under 4096 bytes and the line limit.
    let mut t = String::from(MIN);
    for i in 0..60 {
        t.push_str(&format!("x-pad{i:02}={}\n", "y".repeat(50)));
    }
    assert!(t.len() < MAX_MANIFEST_BYTES);
    assert!(parse(&t).is_ok());
    t.push_str(&"z".repeat(MAX_MANIFEST_BYTES));
    assert_eq!(parse(&t), Err(ManifestError::TooLarge));
}

// ---- net_hosts ----

fn with_hosts(net: &str, hosts: &str) -> Result<Manifest, ManifestError> {
    Manifest::parse(
        alloc::format!("id=n\nname=N\nversion=1.0.0\nnet={net}\nnet_hosts={hosts}\n").as_bytes(),
    )
}

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
