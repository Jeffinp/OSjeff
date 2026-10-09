use super::*;

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
