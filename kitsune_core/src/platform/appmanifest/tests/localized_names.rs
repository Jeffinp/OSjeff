use super::*;

#[test]
fn a_name_can_be_given_per_language() {
    let m = with("name.pt=Relógio\nname.en=Clock").unwrap();
    assert_eq!(m.name, "Hello");
    assert_eq!(m.name_in(Lang::Pt), "Relógio");
    assert_eq!(m.name_in(Lang::En), "Clock");
    assert_eq!(m.names.len(), 2);
}

#[test]
fn without_a_name_for_the_language_the_plain_name_shows() {
    let m = parse(MIN).unwrap();
    assert!(m.names.is_empty());
    for l in Lang::ALL {
        assert_eq!(m.name_in(l), "Hello");
    }
    // Only one language given: the other falls back.
    let m = with("name.pt=Olá").unwrap();
    assert_eq!(m.name_in(Lang::Pt), "Olá");
    assert_eq!(m.name_in(Lang::En), "Hello");
    // The full tag wins over the primary one; either is found.
    let m = with("name.pt=Geral\nname.pt-br=Brasil").unwrap();
    assert_eq!(m.name_in(Lang::Pt), "Brasil");
    let m = with("name.pt-br=Brasil").unwrap();
    assert_eq!(m.name_in(Lang::Pt), "Brasil");
    assert_eq!(m.name_in(Lang::En), "Hello");
}

#[test]
fn the_name_shown_follows_the_language_in_effect() {
    let m = with("name.pt=Relógio\nname.en=Clock").unwrap();
    // The default language is Portuguese (no test here changes it).
    assert_eq!(m.display_name(), m.name_in(crate::i18n::lang()));
}

#[test]
fn localized_names_allow_accents_but_not_control_characters() {
    assert!(
        with(&format!("name.pt={}", "ç".repeat(25))).is_err(),
        "25 chars"
    );
    assert!(with("name.pt=Pôr do sol").is_ok());
    assert!(with("name.en=日本語").is_ok());
    for bad in ["name.pt=", "name.pt= x", "name.pt=x ", "name.pt=a\u{7}b"] {
        assert!(with(bad).is_err(), "{bad:?}");
    }
    // The plain name stays plain ASCII.
    assert!(parse("id=a\nname=Relógio\nversion=1.0.0").is_err());
    // 24 characters, however many bytes.
    let n = "é".repeat(24);
    assert!(with(&format!("name.pt={n}")).is_ok());
    assert!(with(&format!("name.pt={n}é")).is_err());
}

#[test]
fn language_tags_are_checked_but_unknown_languages_are_kept_quietly() {
    assert!(
        with("name.fr=Horloge").is_ok(),
        "a language the system lacks"
    );
    assert_eq!(with("name.fr=Horloge").unwrap().names.len(), 1);
    for bad in [
        "name.=x",
        "name.p=x",
        "name.PT=x",
        "name.pt_br=x",
        "name.pt-=x",
        "name.pt-b=x",
        "name.abcd=x",
        "name.pt-abcdef=x",
    ] {
        assert!(with(bad).is_err(), "{bad}");
    }
    assert_eq!(with("name.pt-x"), Err(ManifestError::Syntax));
    assert_eq!(with("name.1x=a"), Err(ManifestError::UnknownKey));
    // Other dotted keys are still unknown keys.
    assert_eq!(with("fs.pt=own"), Err(ManifestError::UnknownKey));
    assert_eq!(with(".pt=a"), Err(ManifestError::Syntax));
}

#[test]
fn a_language_name_twice_or_too_many_is_an_error() {
    assert_eq!(
        with("name.pt=A\nname.pt=B"),
        Err(ManifestError::DuplicateKey("name.<lang>"))
    );
    let mut many = String::new();
    for tag in ["pt", "en", "fr", "de", "es", "it", "nl", "sv"] {
        many.push_str(&format!("name.{tag}=N\n"));
    }
    assert_eq!(
        parse(&format!("{MIN}{many}")).unwrap().names.len(),
        MAX_LOCAL_NAMES
    );
    assert_eq!(
        parse(&format!("{MIN}{many}name.da=N\n")),
        Err(ManifestError::OverLimit("name.<lang>"))
    );
}

#[test]
fn localized_names_survive_a_trip_through_a_package() {
    let text = "id=clock\nname=Clock\nname.pt=Relógio\nname.en=Clock\nversion=1.0.0\n";
    let m = parse_package(&pkg(text)).unwrap().manifest;
    assert_eq!(m.name_in(Lang::Pt), "Relógio");
    assert_eq!(m, parse(text).unwrap());
}

#[test]
fn install_and_package_errors_read_in_both_languages() {
    use crate::platform::appfs::FsError;
    let pt = |e: &PackageError| e.message_in(Lang::Pt);
    let en = |e: &PackageError| e.message_in(Lang::En);
    let all = [
        PackageError::NoManifest,
        PackageError::DuplicateManifest,
        PackageError::DuplicateIcon,
        PackageError::Icon(IconError::TooLarge),
        PackageError::Icon(IconError::NotPng),
        PackageError::Icon(IconError::Dimensions),
        PackageError::Icon(IconError::Corrupt),
        PackageError::Wasm(WasmError::TooShort),
        PackageError::Wasm(WasmError::BadMagic),
        PackageError::Wasm(WasmError::BadVersion),
        PackageError::Wasm(WasmError::BadLeb),
        PackageError::Wasm(WasmError::Truncated),
        PackageError::Wasm(WasmError::BadSectionId),
        PackageError::Wasm(WasmError::TooManySections),
        PackageError::Manifest(ManifestError::TooLarge),
        PackageError::Manifest(ManifestError::NotUtf8),
        PackageError::Manifest(ManifestError::TooManyLines),
        PackageError::Manifest(ManifestError::Syntax),
        PackageError::Manifest(ManifestError::DuplicateKey("fs")),
        PackageError::Manifest(ManifestError::UnknownKey),
        PackageError::Manifest(ManifestError::Missing("id")),
        PackageError::Manifest(ManifestError::BadValue("abi")),
        PackageError::Manifest(ManifestError::OverLimit("mem_mib")),
    ];
    for e in &all {
        let (p, n) = (pt(e), en(e));
        assert!(!p.is_empty() && !n.is_empty() && p != n, "{e:?}");
        assert!(!p.contains("apps.") && !n.contains("apps."), "{e:?}");
        assert!(!p.contains("{") && !n.contains("{"), "{e:?}");
    }
    assert_eq!(
        pt(&PackageError::Manifest(ManifestError::Missing("id"))),
        "O manifesto precisa da chave id"
    );
    assert_eq!(
        en(&PackageError::Manifest(ManifestError::OverLimit("mem_mib"))),
        "mem_mib is above the system limit"
    );
    for f in [
        FsError::NotFound,
        FsError::Exists,
        FsError::NotDir,
        FsError::IsDir,
        FsError::NotEmpty,
        FsError::NoSpace,
        FsError::Invalid,
        FsError::Perm,
        FsError::BadFd,
        FsError::TooManyFds,
        FsError::Io,
    ] {
        for l in Lang::ALL {
            assert!(
                !crate::i18n::tr_in(l, f.key()).starts_with("apps."),
                "{f:?}"
            );
        }
    }
}
