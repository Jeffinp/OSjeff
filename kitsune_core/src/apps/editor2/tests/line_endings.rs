use super::*;

#[test]
fn crlf_is_preserved_on_save() {
    let raw = b"one\r\ntwo\r\nthree";
    let mut e = Editor::from_bytes(raw);
    assert_eq!(e.eol(), Eol::Crlf);
    assert_eq!(e.line_string(0), "one");
    e.set_cursor(2, 5);
    type_str(&mut e, "!");
    assert_eq!(e.to_bytes(), b"one\r\ntwo\r\nthree!");
}

#[test]
fn enter_in_crlf_file_inserts_crlf() {
    let mut e = Editor::from_bytes(b"ab\r\ncd");
    e.set_cursor(0, 1);
    e.newline();
    assert_eq!(e.to_bytes(), b"a\r\nb\r\ncd");
}

#[test]
fn mixed_endings_round_trip() {
    let raw = b"a\nb\r\nc\n\r\nd";
    let e = Editor::from_bytes(raw);
    assert_eq!(e.line_count(), 5);
    assert_eq!(e.to_bytes(), raw);
}

#[test]
fn cursor_never_sits_between_cr_and_lf() {
    let mut e = Editor::from_bytes(b"ab\r\ncd");
    e.select_range(0, 3);
    assert_eq!(e.cursor_byte(), 2);
    e.set_cursor(0, 99);
    assert_eq!(e.cursor_byte(), 2);
    check(&e);
}

#[test]
fn backspace_at_line_start_removes_whole_crlf() {
    let mut e = Editor::from_bytes(b"ab\r\ncd");
    e.set_cursor(1, 0);
    e.backspace();
    assert_eq!(e.to_bytes(), b"abcd");
    e.undo();
    assert_eq!(e.to_bytes(), b"ab\r\ncd");
}

#[test]
fn delete_at_line_end_removes_whole_crlf() {
    let mut e = Editor::from_bytes(b"ab\r\ncd");
    e.set_cursor(0, 2);
    e.delete_forward();
    assert_eq!(e.to_bytes(), b"abcd");
}

#[test]
fn right_at_line_end_crosses_crlf_in_one_step() {
    let mut e = Editor::from_bytes(b"a\r\nb");
    e.set_cursor(0, 1);
    e.move_right(false);
    assert_eq!(e.cursor(), (1, 0));
    e.move_left(false);
    assert_eq!(e.cursor(), (0, 1));
}

#[test]
fn lf_file_stays_lf() {
    let mut e = ed("a\nb");
    assert_eq!(e.eol(), Eol::Lf);
    e.set_cursor(0, 1);
    e.newline();
    assert_eq!(text(&e), "a\n\nb");
}

#[test]
fn paste_with_crlf_is_kept_verbatim() {
    let mut e = Editor::new();
    e.insert_bytes(b"x\r\ny");
    assert_eq!(e.to_bytes(), b"x\r\ny");
    assert_eq!(e.line_count(), 2);
    check(&e);
}
