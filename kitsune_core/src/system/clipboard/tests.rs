use super::*;

#[test]
fn starts_empty() {
    let c = Clipboard::new();
    assert!(c.is_empty());
    assert_eq!(c.get(), b"");
}

#[test]
fn set_then_get() {
    let mut c = Clipboard::new();
    c.set(b"hello");
    assert_eq!(c.get(), b"hello");
    assert!(!c.is_empty());
}

#[test]
fn set_overwrites() {
    let mut c = Clipboard::new();
    c.set(b"first");
    c.set(b"2nd");
    assert_eq!(c.get(), b"2nd");
}

#[test]
fn truncates_past_capacity() {
    let mut c = Clipboard::new();
    let big = [b'x'; CAP + 50];
    c.set(&big);
    assert_eq!(c.get().len(), CAP);
}

#[test]
fn clear_empties() {
    let mut c = Clipboard::new();
    c.set(b"data");
    c.clear();
    assert!(c.is_empty());
}
