use super::*;

#[test]
fn ctrl_backspace_deletes_word() {
    let mut e = ed("foo bar");
    e.move_doc_end(false);
    ckey(&mut e, KeyCode::Backspace);
    assert_eq!(text(&e), "foo ");
    ckey(&mut e, KeyCode::Backspace);
    assert_eq!(text(&e), "");
    e.undo();
    assert_eq!(text(&e), "foo ");
}

#[test]
fn ctrl_backspace_at_line_start_joins_lines() {
    let mut e = ed("ab\ncd");
    e.set_cursor(1, 0);
    ckey(&mut e, KeyCode::Backspace);
    assert_eq!(text(&e), "abcd");
}

#[test]
fn ctrl_delete_deletes_word_right() {
    let mut e = ed("foo bar baz");
    ckey(&mut e, KeyCode::Delete);
    assert_eq!(text(&e), "bar baz");
    e.move_doc_end(false);
    ckey(&mut e, KeyCode::Delete);
    assert_eq!(text(&e), "bar baz");
}

#[test]
fn ctrl_backspace_over_punctuation_and_multibyte() {
    let mut e = ed("añb+++");
    e.move_doc_end(false);
    ckey(&mut e, KeyCode::Backspace);
    assert_eq!(text(&e), "añb");
    ckey(&mut e, KeyCode::Backspace);
    assert_eq!(text(&e), "");
}
