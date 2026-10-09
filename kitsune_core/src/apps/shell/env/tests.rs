use super::*;

#[test]
fn valid_names() {
    assert!(valid_name("A"));
    assert!(valid_name("_x1"));
    assert!(!valid_name(""));
    assert!(!valid_name("1a"));
    assert!(!valid_name("a-b"));
    assert!(!valid_name("a b"));
}

#[test]
fn defaults_exist() {
    let e = Env::new();
    assert_eq!(e.get("PATH"), Some("/bin:/usr/bin"));
    assert_eq!(e.get("HOME"), Some("/"));
    assert!(e.is_exported("PATH"));
    assert!(!e.is_exported("PS1"));
}

#[test]
fn set_get_unset() {
    let mut e = Env::empty();
    assert!(e.set("X", "1"));
    assert_eq!(e.get("X"), Some("1"));
    assert!(e.unset("X"));
    assert_eq!(e.get("X"), None);
    assert!(!e.unset("X"));
}

#[test]
fn invalid_names_rejected() {
    let mut e = Env::empty();
    assert!(!e.set("1x", "v"));
    assert!(!e.set("", "v"));
    assert!(e.is_empty());
}

#[test]
fn export_flag_survives_reassignment() {
    let mut e = Env::empty();
    e.set_exported("A", "1");
    e.set("A", "2");
    assert!(e.is_exported("A"));
    assert_eq!(e.get("A"), Some("2"));
}

#[test]
fn export_creates_empty() {
    let mut e = Env::empty();
    assert!(e.export("NEW"));
    assert_eq!(e.get("NEW"), Some(""));
}

#[test]
fn limits_are_enforced() {
    let mut e = Env::empty();
    assert!(!e.set("BIG", &"x".repeat(MAX_VALUE + 1)));
    for i in 0..MAX_VARS {
        assert!(e.set(&alloc::format!("V{i}"), "x"));
    }
    assert!(!e.set("ONE_MORE", "x"));
    assert!(e.set("V0", "still ok"));
}

#[test]
fn path_dirs_skips_empty() {
    let mut e = Env::empty();
    e.set("PATH", "/a::/b:");
    assert_eq!(e.path_dirs(), ["/a", "/b"]);
}

#[test]
fn iter_is_sorted() {
    let mut e = Env::empty();
    e.set("B", "2");
    e.set("A", "1");
    let names: Vec<&str> = e.iter().map(|(n, _, _)| n).collect();
    assert_eq!(names, ["A", "B"]);
}
