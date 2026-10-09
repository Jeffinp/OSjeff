use super::*;

#[test]
fn copy_cut_paste_roundtrip() {
    let mut e = ed("hello world");
    let mut clip = Clipboard::new();
    e.select_range(0, 5);
    assert!(e.copy(&mut clip));
    assert_eq!(clip.get(), b"hello");
    e.move_doc_end(false);
    assert!(e.paste(&clip));
    assert_eq!(text(&e), "hello worldhello");
    e.select_range(0, 6);
    assert!(e.cut(&mut clip));
    assert_eq!(text(&e), "worldhello");
    assert_eq!(clip.get(), b"hello ");
}

#[test]
fn copy_without_selection_does_nothing() {
    let e = ed("abc");
    let mut clip = Clipboard::new();
    assert!(!e.copy(&mut clip));
    assert!(clip.is_empty());
}

#[test]
fn paste_replaces_selection() {
    let mut e = ed("abc");
    let mut clip = Clipboard::new();
    clip.set(b"XY");
    e.select_range(1, 2);
    e.paste(&clip);
    assert_eq!(text(&e), "aXYc");
}

#[test]
fn copy_truncates_on_a_character_boundary() {
    let mut e = ed(&"é".repeat(200));
    let mut clip = Clipboard::new();
    e.select_all();
    e.copy(&mut clip);
    assert!(clip.get().len() <= crate::system::clipboard::CAP);
    assert!(core::str::from_utf8(clip.get()).is_ok());
}

#[test]
fn cut_of_oversized_selection_refuses() {
    let mut e = ed(&"x".repeat(1000));
    let mut clip = Clipboard::new();
    e.select_all();
    assert!(!e.cut(&mut clip));
    assert_eq!(e.len_bytes(), 1000);
}

#[test]
fn clipboard_shortcuts_via_keys() {
    let mut e = ed("abc");
    let mut clip = Clipboard::new();
    e.select_all();
    press(&mut e, &mut clip, KeyCode::Char('c'), Mods::CTRL);
    press(&mut e, &mut clip, KeyCode::End, Mods::CTRL);
    press(&mut e, &mut clip, KeyCode::Char('v'), Mods::CTRL);
    assert_eq!(text(&e), "abcabc");
    e.select_range(0, 3);
    press(&mut e, &mut clip, KeyCode::Char('x'), Mods::CTRL);
    assert_eq!(text(&e), "abc");
    press(&mut e, &mut clip, KeyCode::Delete, Mods::NONE);
    assert_eq!(text(&e), "bc");
}

#[test]
fn paste_is_one_undo_step() {
    let mut e = ed("");
    let mut clip = Clipboard::new();
    clip.set(b"multi\nline");
    e.paste(&clip);
    assert_eq!(e.line_count(), 2);
    e.undo();
    assert_eq!(text(&e), "");
}
