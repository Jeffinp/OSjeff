use super::*;

#[test]
fn horizontal_movement_wraps_across_lines() {
    let mut e = ed("ab\ncd");
    e.set_cursor(0, 2);
    e.move_right(false);
    assert_eq!(e.cursor(), (1, 0));
    e.move_left(false);
    assert_eq!(e.cursor(), (0, 2));
    e.move_doc_start(false);
    e.move_left(false);
    assert_eq!(e.cursor(), (0, 0));
    e.move_doc_end(false);
    e.move_right(false);
    assert_eq!(e.cursor(), (1, 2));
}

#[test]
fn vertical_movement_keeps_preferred_column() {
    let mut e = ed("abcdef\nab\nabcdef");
    e.set_cursor(0, 5);
    e.move_vertical(1, false);
    assert_eq!(e.cursor(), (1, 2));
    e.move_vertical(1, false);
    assert_eq!(e.cursor(), (2, 5));
}

#[test]
fn up_on_first_line_goes_to_start_and_down_on_last_to_end() {
    let mut e = ed("abc\ndef");
    e.set_cursor(0, 2);
    e.move_vertical(-1, false);
    assert_eq!(e.cursor(), (0, 0));
    e.set_cursor(1, 1);
    e.move_vertical(1, false);
    assert_eq!(e.cursor(), (1, 3));
}

#[test]
fn smart_home_toggles_between_indent_and_column_zero() {
    let mut e = ed("    code");
    e.move_end(false);
    e.move_home(false);
    assert_eq!(e.cursor(), (0, 4));
    e.move_home(false);
    assert_eq!(e.cursor(), (0, 0));
    e.move_home(false);
    assert_eq!(e.cursor(), (0, 4));
}

#[test]
fn end_goes_to_line_end() {
    let mut e = ed("hello\nworld");
    e.move_end(false);
    assert_eq!(e.cursor(), (0, 5));
}

#[test]
fn ctrl_right_moves_by_word() {
    let mut e = ed("foo bar_baz  qux");
    ckey(&mut e, KeyCode::Right);
    assert_eq!(e.cursor(), (0, 4));
    ckey(&mut e, KeyCode::Right);
    assert_eq!(e.cursor(), (0, 13));
    ckey(&mut e, KeyCode::Right);
    assert_eq!(e.cursor(), (0, 16));
}

#[test]
fn ctrl_left_moves_by_word() {
    let mut e = ed("foo bar_baz  qux");
    e.move_doc_end(false);
    ckey(&mut e, KeyCode::Left);
    assert_eq!(e.cursor(), (0, 13));
    ckey(&mut e, KeyCode::Left);
    assert_eq!(e.cursor(), (0, 4));
    ckey(&mut e, KeyCode::Left);
    assert_eq!(e.cursor(), (0, 0));
}

#[test]
fn word_movement_treats_punctuation_as_a_word() {
    let mut e = ed("a+++b");
    ckey(&mut e, KeyCode::Right);
    assert_eq!(e.cursor(), (0, 1));
    ckey(&mut e, KeyCode::Right);
    assert_eq!(e.cursor(), (0, 4));
}

#[test]
fn word_movement_crosses_lines() {
    let mut e = ed("ab\ncd");
    e.set_cursor(0, 2);
    ckey(&mut e, KeyCode::Right);
    assert_eq!(e.cursor(), (1, 0));
    ckey(&mut e, KeyCode::Left);
    assert_eq!(e.cursor(), (0, 2));
}

#[test]
fn ctrl_home_end() {
    let mut e = ed("a\nb\nc");
    ckey(&mut e, KeyCode::End);
    assert_eq!(e.cursor(), (2, 1));
    ckey(&mut e, KeyCode::Home);
    assert_eq!(e.cursor(), (0, 0));
}

#[test]
fn page_down_and_up_move_by_window_height() {
    let doc: String = (0..100).map(|i| format!("line{i}\n")).collect();
    let mut e = ed(&doc);
    e.resize(10, 40);
    key(&mut e, KeyCode::PageDown);
    assert_eq!(e.cursor().0, 9);
    key(&mut e, KeyCode::PageDown);
    assert_eq!(e.cursor().0, 18);
    key(&mut e, KeyCode::PageUp);
    assert_eq!(e.cursor().0, 9);
    check(&e);
}

#[test]
fn goto_line_clamps_and_centres() {
    let doc: String = (0..200).map(|i| format!("l{i}\n")).collect();
    let mut e = ed(&doc);
    e.resize(20, 40);
    e.goto_line(100);
    assert_eq!(e.cursor(), (99, 0));
    assert!(e.top_line() <= 99 && e.top_line() + 20 > 99);
    e.goto_line(10_000);
    assert_eq!(e.cursor().0, 200);
    e.goto_line(0);
    assert_eq!(e.cursor().0, 0);
}
