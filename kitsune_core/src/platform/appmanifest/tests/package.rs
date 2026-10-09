use super::*;

#[test]
fn package_with_manifest_only() {
    let p = parse_package(&pkg(MIN)).unwrap();
    assert_eq!(p.manifest.id, "hello");
    assert!(p.icon.is_none());
}

#[test]
fn package_with_the_old_section_names_still_parses() {
    let png = icon_png(32, 32);
    let w = module(&[
        custom(LEGACY_MANIFEST_SECTION, MIN.as_bytes()),
        custom(LEGACY_ICON_SECTION, &png),
    ]);
    assert_eq!(LEGACY_MANIFEST_SECTION, "osjeff.manifest");
    let p = parse_package(&w).unwrap();
    assert_eq!(p.manifest.id, "hello");
    assert_eq!(p.icon.unwrap().width(), 32);
    // Mixed old manifest + new icon is fine too.
    let w = module(&[
        custom(LEGACY_MANIFEST_SECTION, MIN.as_bytes()),
        custom(ICON_SECTION, &png),
    ]);
    assert!(parse_package(&w).unwrap().icon.is_some());
}

#[test]
fn both_section_names_together_count_as_a_duplicate() {
    let w = module(&[
        custom(MANIFEST_SECTION, MIN.as_bytes()),
        custom(LEGACY_MANIFEST_SECTION, MIN.as_bytes()),
    ]);
    assert_eq!(
        parse_package(&w).unwrap_err(),
        PackageError::DuplicateManifest
    );
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
