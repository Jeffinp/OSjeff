use super::*;

#[test]
fn shift_arrows_select() {
    let mut e = ed("hello");
    skey(&mut e, KeyCode::Right);
    skey(&mut e, KeyCode::Right);
    assert_eq!(e.selection(), Some((0, 2)));
    assert_eq!(e.selected_bytes(), b"he");
    skey(&mut e, KeyCode::Left);
    assert_eq!(e.selection(), Some((0, 1)));
}

#[test]
fn shift_home_end_select_to_line_bounds() {
    let mut e = ed("abc def");
    e.set_cursor(0, 3);
    skey(&mut e, KeyCode::End);
    assert_eq!(e.selected_bytes(), b" def");
    e.set_cursor(0, 3);
    skey(&mut e, KeyCode::Home);
    assert_eq!(e.selected_bytes(), b"abc");
}

#[test]
fn ctrl_shift_arrows_select_words() {
    let mut e = ed("one two three");
    let mut c = Clipboard::new();
    press(&mut e, &mut c, KeyCode::Right, Mods::CTRL_SHIFT);
    press(&mut e, &mut c, KeyCode::Right, Mods::CTRL_SHIFT);
    assert_eq!(e.selected_bytes(), b"one two ");
}

#[test]
fn ctrl_a_selects_all() {
    let mut e = ed("a\nb");
    let mut c = Clipboard::new();
    press(&mut e, &mut c, KeyCode::Char('a'), Mods::CTRL);
    assert_eq!(e.selection(), Some((0, 3)));
}

#[test]
fn typing_replaces_selection() {
    let mut e = ed("hello world");
    e.select_range(0, 5);
    type_str(&mut e, "bye");
    assert_eq!(text(&e), "bye world");
    assert_eq!(e.selection(), None);
}

#[test]
fn backspace_and_delete_remove_selection() {
    let mut e = ed("abcdef");
    e.select_range(1, 4);
    e.backspace();
    assert_eq!(text(&e), "aef");
    e.select_range(0, 2);
    e.delete_forward();
    assert_eq!(text(&e), "f");
}

#[test]
fn left_right_collapse_selection() {
    let mut e = ed("abcdef");
    e.select_range(1, 4);
    e.move_left(false);
    assert_eq!(e.cursor_byte(), 1);
    assert_eq!(e.selection(), None);
    e.select_range(1, 4);
    e.move_right(false);
    assert_eq!(e.cursor_byte(), 4);
}

#[test]
fn esc_clears_selection() {
    let mut e = ed("abc");
    e.select_all();
    key(&mut e, KeyCode::Esc);
    assert!(!e.has_selection());
}

#[test]
fn multiline_selection_delete() {
    let mut e = ed("one\ntwo\nthree");
    e.select_range(2, 9);
    e.backspace();
    assert_eq!(text(&e), "onhree");
    assert_eq!(e.line_count(), 1);
}

#[test]
fn double_click_selects_word() {
    let mut e = ed("hello big_word here");
    e.mouse_down(0, 8, 2, false);
    assert_eq!(e.selected_bytes(), b"big_word");
    e.mouse_down(0, 5, 2, false);
    assert_eq!(e.selected_bytes(), b" ");
}

#[test]
fn double_click_at_line_end_selects_last_word() {
    let mut e = ed("foo bar");
    e.mouse_down(0, 70, 2, false);
    assert_eq!(e.selected_bytes(), b"bar");
}

#[test]
fn triple_click_selects_line_with_terminator() {
    let mut e = ed("first\nsecond\nthird");
    e.mouse_down(1, 2, 3, false);
    assert_eq!(e.selected_bytes(), b"second\n");
    e.mouse_down(2, 0, 3, false);
    assert_eq!(e.selected_bytes(), b"third");
}

#[test]
fn click_and_drag_select() {
    let mut e = ed("abcdefgh");
    e.mouse_down(0, 2, 1, false);
    e.mouse_drag(0, 6);
    assert_eq!(e.selected_bytes(), b"cdef");
    e.mouse_down(0, 1, 1, true);
    assert_eq!(e.selected_bytes(), b"b");
}

#[test]
fn select_line_ctrl_l() {
    let mut e = ed("aa\nbb");
    e.set_cursor(0, 1);
    let mut c = Clipboard::new();
    press(&mut e, &mut c, KeyCode::Char('l'), Mods::CTRL);
    assert_eq!(e.selected_bytes(), b"aa\n");
}

#[test]
fn status_counts_selected_chars() {
    let mut e = ed("añb");
    e.select_all();
    assert_eq!(e.status().selected_chars, 3);
}
