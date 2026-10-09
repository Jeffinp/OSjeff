use super::*;

#[test]
fn add_and_dedupe() {
    let mut h = History::new();
    assert!(h.add("ls"));
    assert!(!h.add("ls"));
    assert!(h.add("pwd"));
    assert!(h.add("ls"));
    assert_eq!(h.len(), 3);
}

#[test]
fn ignores_blank_and_space_prefixed() {
    let mut h = History::new();
    assert!(!h.add(""));
    assert!(!h.add("   "));
    assert!(!h.add(" secret"));
    assert!(h.is_empty());
}

#[test]
fn bounded_keeps_newest() {
    let mut h = History::with_max(3);
    for i in 0..10 {
        h.add(&alloc::format!("cmd{i}"));
    }
    assert_eq!(h.len(), 3);
    assert_eq!(h.get(0), Some("cmd7"));
    assert_eq!(h.get(2), Some("cmd9"));
}

#[test]
fn search_reverse() {
    let mut h = History::new();
    for c in ["echo a", "ls -l", "echo b", "pwd"] {
        h.add(c);
    }
    assert_eq!(h.search_rev("echo", 4), Some(2));
    assert_eq!(h.search_rev("echo", 2), Some(0));
    assert_eq!(h.search_rev("echo", 0), None);
    assert_eq!(h.search_rev("", 4), None);
    assert_eq!(h.search_rev("zzz", 4), None);
}

#[test]
fn persistence_round_trip() {
    let mut h = History::new();
    for c in ["one", "two words", "three"] {
        h.add(c);
    }
    let bytes = h.to_bytes();
    let mut g = History::new();
    g.load(&bytes);
    assert_eq!(g.iter().collect::<Vec<_>>(), ["one", "two words", "three"]);
}

#[test]
fn load_survives_garbage() {
    let mut g = History::with_max(5);
    g.load(&[0xFF, 0xFE, b'\n', b'o', b'k', b'\n', 0x80]);
    assert!(!g.is_empty());
}

#[test]
fn multiline_entries_are_rejected() {
    let mut h = History::new();
    assert!(!h.add("a\nb"));
}
