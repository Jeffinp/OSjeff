use super::*;

#[test]
fn utf8_typing_and_cursor_by_character() {
    let mut e = Editor::new();
    type_str(&mut e, "añ€😀");
    assert_eq!(e.cursor(), (0, 4));
    assert_eq!(e.cursor_byte(), 1 + 2 + 3 + 4);
    e.move_left(false);
    assert_eq!(e.cursor(), (0, 3));
    e.move_left(false);
    e.move_left(false);
    assert_eq!(e.cursor(), (0, 1));
    check(&e);
}

#[test]
fn utf8_backspace_removes_whole_char() {
    let mut e = ed("a€b");
    e.set_cursor(0, 2);
    e.backspace();
    assert_eq!(text(&e), "ab");
}

#[test]
fn utf8_delete_removes_whole_char() {
    let mut e = ed("a😀b");
    e.set_cursor(0, 1);
    e.delete_forward();
    assert_eq!(text(&e), "ab");
}

#[test]
fn invalid_utf8_is_preserved_on_save() {
    let raw = [b'a', 0xFF, 0xFE, b'\n', 0xC3, b'b'];
    let mut e = Editor::from_bytes(&raw);
    type_str(&mut e, "Z");
    assert!(e.to_bytes().ends_with(&[0xC3, b'b']));
    e.undo();
    assert_eq!(e.to_bytes(), raw);
}

#[test]
fn invalid_bytes_are_single_cursor_steps() {
    let mut e = Editor::from_bytes(&[0xFF, 0xFF, 0xFF, b'x']);
    e.move_right(false);
    e.move_right(false);
    assert_eq!(e.cursor(), (0, 2));
    e.backspace();
    assert_eq!(e.to_bytes(), vec![0xFF, 0xFF, b'x']);
    check(&e);
}

#[test]
fn invalid_bytes_render_as_replacement() {
    let e = Editor::from_bytes(&[b'a', 0x80, b'b']);
    let row = e.visible_rows().next().unwrap();
    let s: String = row.cells().map(|c| c.ch).collect();
    assert_eq!(s, "a\u{FFFD}b");
}

#[test]
fn cursor_snaps_out_of_a_multibyte_char() {
    let mut e = ed("€x");
    e.select_range(1, 2);
    assert_eq!(e.selection(), None);
    assert_eq!(e.cursor_byte(), 0);
}

#[test]
fn wide_codepoints_have_width_one() {
    let e = ed("日本語");
    let row = e.visible_rows().next().unwrap();
    assert_eq!(row.cells().count(), 3);
}
