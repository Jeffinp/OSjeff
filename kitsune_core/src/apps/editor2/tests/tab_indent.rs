use super::*;

#[test]
fn tab_inserts_spaces_to_next_stop() {
    let mut e = Editor::new();
    type_str(&mut e, "ab");
    key(&mut e, KeyCode::Tab);
    assert_eq!(text(&e), "ab  ");
    key(&mut e, KeyCode::Tab);
    assert_eq!(text(&e), "ab      ");
}

#[test]
fn tab_width_is_configurable() {
    let mut e = Editor::new();
    e.set_tab_width(8);
    key(&mut e, KeyCode::Tab);
    assert_eq!(text(&e), "        ");
}

#[test]
fn tab_can_insert_literal_tabs() {
    let mut e = Editor::new();
    e.set_use_spaces(false);
    key(&mut e, KeyCode::Tab);
    assert_eq!(text(&e), "\t");
}

#[test]
fn tab_with_multiline_selection_indents_lines() {
    let mut e = ed("a\nb\nc");
    e.select_range(0, 3);
    key(&mut e, KeyCode::Tab);
    assert_eq!(text(&e), "    a\n    b\nc");
    assert!(e.has_selection());
    e.undo();
    assert_eq!(text(&e), "a\nb\nc");
}

#[test]
fn shift_tab_outdents() {
    let mut e = ed("    a\n  b\n\tc");
    e.select_all();
    skey(&mut e, KeyCode::Tab);
    assert_eq!(text(&e), "a\nb\nc");
}

#[test]
fn shift_tab_without_selection_outdents_current_line() {
    let mut e = ed("      x");
    e.move_doc_end(false);
    skey(&mut e, KeyCode::Tab);
    assert_eq!(text(&e), "  x");
    assert_eq!(e.cursor(), (0, 3));
}

#[test]
fn outdent_without_indent_is_noop() {
    let mut e = ed("x");
    e.outdent();
    assert!(!e.is_modified());
}

#[test]
fn auto_indent_copies_leading_blanks() {
    let mut e = ed("    foo");
    e.move_doc_end(false);
    e.newline();
    assert_eq!(text(&e), "    foo\n    ");
    assert_eq!(e.cursor(), (1, 4));
    e.undo();
    assert_eq!(text(&e), "    foo");
}

#[test]
fn auto_indent_can_be_disabled() {
    let mut e = ed("    foo");
    e.set_auto_indent(false);
    e.move_doc_end(false);
    e.newline();
    assert_eq!(text(&e), "    foo\n");
}

#[test]
fn auto_indent_stops_at_cursor_inside_indent() {
    let mut e = ed("    foo");
    e.set_cursor(0, 2);
    e.newline();
    assert_eq!(text(&e), "  \n    foo");
}

#[test]
fn tab_stop_accounts_for_existing_tabs() {
    let mut e = ed("\tx");
    e.move_doc_end(false);
    key(&mut e, KeyCode::Tab);
    assert_eq!(text(&e), "\tx   ");
}
