use super::*;

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
    assert!(parse("id=a\nname=Clock 2\nversion=1.0.0").is_ok());
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
