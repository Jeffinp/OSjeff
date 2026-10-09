use super::*;

#[test]
fn new_editor_is_empty_and_clean() {
    let e = Editor::new();
    assert!(e.is_empty());
    assert_eq!(e.line_count(), 1);
    assert_eq!(e.cursor(), (0, 0));
    assert!(!e.is_modified());
}

#[test]
fn typing_inserts_and_marks_modified() {
    let mut e = Editor::new();
    type_str(&mut e, "hello");
    assert_eq!(text(&e), "hello");
    assert_eq!(e.cursor(), (0, 5));
    assert!(e.is_modified());
}

#[test]
fn enter_splits_line() {
    let mut e = ed("abcd");
    e.set_cursor(0, 2);
    e.newline();
    assert_eq!(text(&e), "ab\ncd");
    assert_eq!(e.cursor(), (1, 0));
}

#[test]
fn backspace_deletes_char_and_joins_lines() {
    let mut e = ed("ab\ncd");
    e.set_cursor(1, 1);
    e.backspace();
    assert_eq!(text(&e), "ab\nd");
    e.backspace();
    assert_eq!(text(&e), "ab\nd".replace("\nd", "d"));
    assert_eq!(e.cursor(), (0, 2));
}

#[test]
fn backspace_at_document_start_is_noop() {
    let mut e = ed("x");
    e.backspace();
    assert_eq!(text(&e), "x");
    assert!(!e.is_modified());
}

#[test]
fn delete_forward_and_join() {
    let mut e = ed("ab\ncd");
    e.set_cursor(0, 1);
    e.delete_forward();
    assert_eq!(text(&e), "a\ncd");
    e.delete_forward();
    assert_eq!(text(&e), "acd");
}

#[test]
fn delete_at_document_end_is_noop() {
    let mut e = ed("ab");
    e.move_doc_end(false);
    e.delete_forward();
    assert_eq!(text(&e), "ab");
    assert!(!e.is_modified());
}

#[test]
fn control_chars_are_not_inserted() {
    let mut e = Editor::new();
    e.insert_char('\u{7}');
    e.insert_char('\r');
    e.insert_char('\u{1b}');
    assert_eq!(text(&e), "");
}

#[test]
fn newline_char_acts_as_enter() {
    let mut e = Editor::new();
    type_str(&mut e, "a\nb");
    assert_eq!(text(&e), "a\nb");
    assert_eq!(e.line_count(), 2);
}

#[test]
fn set_text_resets_everything() {
    let mut e = ed("old text");
    type_str(&mut e, "x");
    e.set_text(b"fresh\nfile");
    assert!(!e.is_modified());
    assert!(!e.can_undo());
    assert_eq!(e.cursor(), (0, 0));
    assert_eq!(e.line_count(), 2);
}

#[test]
fn line_accessors() {
    let e = ed("one\ntwo\n\nfour");
    assert_eq!(e.line_count(), 4);
    assert_eq!(e.line_string(1), "two");
    assert_eq!(e.line_string(2), "");
    assert_eq!(e.line_chars(3), 4);
    assert_eq!(e.line_bytes(0), b"one");
}

#[test]
fn status_reports_position() {
    let mut e = ed("ab\ncd");
    e.set_cursor(1, 1);
    let s = e.status();
    assert_eq!((s.line, s.col, s.total_lines, s.bytes), (2, 2, 2, 5));
    assert!(!s.modified);
}
