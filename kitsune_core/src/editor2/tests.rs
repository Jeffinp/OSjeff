//! Behaviour tests for the v2 editor, including a randomized comparison
//! against a trivial `Vec<String>` model.

use super::*;
use crate::input::{KeyCode, KeyEvent, Mods};
use alloc::string::ToString;
use alloc::vec;

fn ed(s: &str) -> Editor {
    Editor::from_bytes(s.as_bytes())
}

fn text(e: &Editor) -> String {
    String::from_utf8_lossy(&e.to_bytes()).into_owned()
}

fn type_str(e: &mut Editor, s: &str) {
    for c in s.chars() {
        e.insert_char(c);
    }
}

fn press(e: &mut Editor, clip: &mut Clipboard, code: KeyCode, mods: Mods) -> Event {
    e.handle_key(KeyEvent::new(code, mods), clip)
}

fn key(e: &mut Editor, code: KeyCode) {
    let mut c = Clipboard::new();
    press(e, &mut c, code, Mods::NONE);
}

fn skey(e: &mut Editor, code: KeyCode) {
    let mut c = Clipboard::new();
    press(e, &mut c, code, Mods::SHIFT);
}

fn ckey(e: &mut Editor, code: KeyCode) {
    let mut c = Clipboard::new();
    press(e, &mut c, code, Mods::CTRL);
}

fn check(e: &Editor) {
    assert!(e.text.lines_consistent(), "line index out of sync");
    let c = e.cursor;
    assert!(c <= e.text.len());
    let l = e.text.line_of(c);
    assert!(c <= e.text.line_end(l), "cursor past line content");
    assert!(c >= e.text.line_start(l));
    assert_eq!(e.normalize(c), c, "cursor not normalized");
    if let Some(a) = e.anchor {
        assert!(a <= e.text.len());
    }
}

// ---- basic editing -----------------------------------------------------

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

// ---- UTF-8 --------------------------------------------------------------

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

// ---- line endings -----------------------------------------------------

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

// ---- movement ---------------------------------------------------------

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

// ---- selection -----------------------------------------------------

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

// ---- clipboard -------------------------------------------------------

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
    assert!(clip.get().len() <= crate::clipboard::CAP);
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

// ---- undo / redo -------------------------------------------------------

#[test]
fn undo_groups_typing() {
    let mut e = Editor::new();
    type_str(&mut e, "abc");
    e.move_left(false);
    e.move_right(false);
    type_str(&mut e, "def");
    e.undo();
    assert_eq!(text(&e), "abc");
    e.undo();
    assert_eq!(text(&e), "");
    assert!(!e.undo());
}

#[test]
fn undo_splits_typing_at_word_boundary() {
    let mut e = Editor::new();
    type_str(&mut e, "ab cd");
    e.undo();
    assert_eq!(text(&e), "ab ");
    e.undo();
    assert_eq!(text(&e), "");
}

#[test]
fn redo_reapplies() {
    let mut e = Editor::new();
    type_str(&mut e, "abc");
    e.undo();
    assert!(e.can_redo());
    assert!(e.redo());
    assert_eq!(text(&e), "abc");
    assert_eq!(e.cursor(), (0, 3));
    assert!(!e.redo());
}

#[test]
fn new_edit_discards_redo() {
    let mut e = Editor::new();
    type_str(&mut e, "abc");
    e.undo();
    type_str(&mut e, "x");
    assert!(!e.can_redo());
    assert_eq!(text(&e), "x");
}

#[test]
fn undo_restores_cursor() {
    let mut e = ed("hello");
    e.set_cursor(0, 5);
    e.backspace();
    e.backspace();
    assert_eq!(e.cursor(), (0, 3));
    e.undo();
    assert_eq!(text(&e), "hello");
    assert_eq!(e.cursor(), (0, 5));
}

#[test]
fn backspace_burst_is_one_undo() {
    let mut e = ed("abcdef");
    e.move_doc_end(false);
    for _ in 0..4 {
        e.backspace();
    }
    assert_eq!(text(&e), "ab");
    e.undo();
    assert_eq!(text(&e), "abcdef");
}

#[test]
fn delete_burst_is_one_undo() {
    let mut e = ed("abcdef");
    e.set_cursor(0, 1);
    for _ in 0..3 {
        e.delete_forward();
    }
    assert_eq!(text(&e), "aef");
    e.undo();
    assert_eq!(text(&e), "abcdef");
    e.redo();
    assert_eq!(text(&e), "aef");
}

#[test]
fn enter_is_its_own_undo_step() {
    let mut e = Editor::new();
    type_str(&mut e, "ab");
    e.newline();
    type_str(&mut e, "cd");
    e.undo();
    assert_eq!(text(&e), "ab\n");
    e.undo();
    assert_eq!(text(&e), "ab");
}

#[test]
fn modified_flag_follows_undo_to_saved_state() {
    let mut e = ed("base");
    assert!(!e.is_modified());
    e.move_doc_end(false);
    type_str(&mut e, "x");
    assert!(e.is_modified());
    e.undo();
    assert!(!e.is_modified());
    e.redo();
    assert!(e.is_modified());
    e.mark_saved();
    assert!(!e.is_modified());
    type_str(&mut e, "y");
    assert!(e.is_modified());
    e.undo();
    assert!(!e.is_modified());
}

#[test]
fn modified_stays_true_after_diverging_from_saved_state() {
    let mut e = Editor::new();
    type_str(&mut e, "a");
    e.mark_saved();
    e.undo();
    assert!(e.is_modified());
    type_str(&mut e, "b");
    e.undo();
    assert!(e.is_modified());
}

#[test]
fn undo_unlimited_depth() {
    let mut e = Editor::new();
    for i in 0..3000 {
        e.insert_char('x');
        e.newline();
        if i % 7 == 0 {
            e.move_left(false);
            e.move_right(false);
        }
    }
    let mut n = 0;
    while e.undo() {
        n += 1;
    }
    assert!(n >= 3000);
    assert_eq!(text(&e), "");
    while e.redo() {}
    assert_eq!(e.line_count(), 3001);
}

#[test]
fn undo_limit_drops_oldest_but_stays_consistent() {
    let mut e = Editor::new();
    e.set_undo_limit(2000);
    for _ in 0..200 {
        e.insert_str("0123456789");
    }
    assert!(e.hist.memory() <= 2200);
    while e.undo() {}
    assert!(e.len_bytes() > 0);
    check(&e);
}

#[test]
fn undo_through_selection_replace() {
    let mut e = ed("hello world");
    e.select_range(0, 5);
    type_str(&mut e, "bye");
    e.undo();
    assert_eq!(text(&e), "hello world");
}

#[test]
fn ctrl_z_and_ctrl_y_keys() {
    let mut e = Editor::new();
    let mut clip = Clipboard::new();
    type_str(&mut e, "abc");
    press(&mut e, &mut clip, KeyCode::Char('z'), Mods::CTRL);
    assert_eq!(text(&e), "");
    press(&mut e, &mut clip, KeyCode::Char('y'), Mods::CTRL);
    assert_eq!(text(&e), "abc");
    press(&mut e, &mut clip, KeyCode::Char('z'), Mods::CTRL);
    press(&mut e, &mut clip, KeyCode::Char('z'), Mods::CTRL_SHIFT);
    assert_eq!(text(&e), "abc");
}

// ---- search / replace -------------------------------------------------

#[test]
fn find_next_selects_and_wraps() {
    let mut e = ed("foo bar foo baz foo");
    e.set_search("foo", true);
    assert!(e.find_next());
    assert_eq!(e.selection(), Some((0, 3)));
    assert!(e.find_next());
    assert_eq!(e.selection(), Some((8, 11)));
    assert!(e.find_next());
    assert_eq!(e.selection(), Some((16, 19)));
    assert!(e.find_next());
    assert_eq!(e.selection(), Some((0, 3)));
    assert_eq!(e.notice(), Notice::Wrapped);
}

#[test]
fn find_prev_walks_backwards_and_wraps() {
    let mut e = ed("foo bar foo");
    e.set_search("foo", true);
    e.move_doc_end(false);
    assert!(e.find_prev());
    assert_eq!(e.selection(), Some((8, 11)));
    assert!(e.find_prev());
    assert_eq!(e.selection(), Some((0, 3)));
    assert!(e.find_prev());
    assert_eq!(e.selection(), Some((8, 11)));
    assert_eq!(e.notice(), Notice::Wrapped);
}

#[test]
fn find_not_found_keeps_state() {
    let mut e = ed("abc");
    e.set_search("zzz", true);
    assert!(!e.find_next());
    assert_eq!(e.notice(), Notice::NotFound);
    assert_eq!(e.cursor_byte(), 0);
}

#[test]
fn find_empty_query_is_noop() {
    let mut e = ed("abc");
    e.set_search("", true);
    assert!(!e.find_next());
    assert!(!e.find_prev());
    assert_eq!(e.count_matches(), 0);
    assert_eq!(e.replace_all(), 0);
}

#[test]
fn find_case_sensitivity() {
    let mut e = ed("Foo foo FOO");
    e.set_search("foo", true);
    assert_eq!(e.count_matches(), 1);
    e.set_search("foo", false);
    assert_eq!(e.count_matches(), 3);
    assert!(e.find_next());
    assert_eq!(e.selection(), Some((0, 3)));
}

#[test]
fn find_case_insensitive_unicode() {
    let mut e = ed("ÁRVORE árvore Árvore");
    e.set_search("árvore", false);
    assert_eq!(e.count_matches(), 3);
    e.set_search("árvore", true);
    assert_eq!(e.count_matches(), 1);
}

#[test]
fn find_multibyte_needle_exact() {
    let mut e = ed("a€b€c");
    e.set_search("€", true);
    e.find_next();
    assert_eq!(e.selection(), Some((1, 4)));
    e.find_next();
    assert_eq!(e.selection(), Some((5, 8)));
}

#[test]
fn find_across_lines_and_gap() {
    let mut e = ed("alpha\nbeta\ngamma");
    e.set_cursor(1, 2);
    type_str(&mut e, "XY");
    e.set_search("XYta\ngam", true);
    assert!(e.find_next());
    assert_eq!(e.selected_bytes(), b"XYta\ngam");
}

#[test]
fn find_selects_match_then_typing_replaces_it() {
    let mut e = ed("one two three");
    e.set_search("two", true);
    e.find_next();
    type_str(&mut e, "2");
    assert_eq!(text(&e), "one 2 three");
}

#[test]
fn replace_current_replaces_and_advances() {
    let mut e = ed("a-a-a");
    e.set_search("a", true);
    e.set_replacement("XX");
    e.find_next();
    assert!(e.replace_current());
    assert_eq!(text(&e), "XX-a-a");
    assert_eq!(e.selection(), Some((3, 4)));
    assert!(e.replace_current());
    assert_eq!(text(&e), "XX-XX-a");
}

#[test]
fn replace_current_without_match_selected_just_finds() {
    let mut e = ed("a b a");
    e.set_search("a", true);
    e.set_replacement("Z");
    assert!(!e.replace_current());
    assert_eq!(text(&e), "a b a");
    assert!(e.has_selection());
}

#[test]
fn replace_all_counts_and_undoes_in_one_step() {
    let mut e = ed("cat dog cat\ncat");
    e.set_search("cat", true);
    e.set_replacement("tiger");
    assert_eq!(e.replace_all(), 3);
    assert_eq!(text(&e), "tiger dog tiger\ntiger");
    assert_eq!(e.notice(), Notice::Replaced(3));
    e.undo();
    assert_eq!(text(&e), "cat dog cat\ncat");
    e.redo();
    assert_eq!(text(&e), "tiger dog tiger\ntiger");
    check(&e);
}

#[test]
fn replace_all_with_empty_replacement_and_overlaps() {
    let mut e = ed("aaaa");
    e.set_search("aa", true);
    e.set_replacement("");
    assert_eq!(e.replace_all(), 2);
    assert_eq!(text(&e), "");
}

#[test]
fn replace_all_no_match_returns_zero() {
    let mut e = ed("abc");
    e.set_search("q", true);
    assert_eq!(e.replace_all(), 0);
    assert!(!e.is_modified());
}

#[test]
fn replace_all_keeps_line_index_valid() {
    let mut e = ed("x\nx\nx\nx");
    e.set_search("x", true);
    e.set_replacement("a\nb");
    e.replace_all();
    assert_eq!(e.line_count(), 8);
    check(&e);
}

#[test]
fn find_prompt_flow_with_keys() {
    let mut e = ed("alpha beta alpha");
    let mut clip = Clipboard::new();
    press(&mut e, &mut clip, KeyCode::Char('f'), Mods::CTRL);
    assert!(e.is_prompt_open());
    for c in "alp".chars() {
        press(&mut e, &mut clip, KeyCode::Char(c), Mods::NONE);
    }
    assert_eq!(e.selection(), Some((0, 3)));
    press(&mut e, &mut clip, KeyCode::Enter, Mods::NONE);
    assert_eq!(e.selection(), Some((11, 14)));
    press(&mut e, &mut clip, KeyCode::Enter, Mods::SHIFT);
    assert_eq!(e.selection(), Some((0, 3)));
    press(&mut e, &mut clip, KeyCode::Esc, Mods::NONE);
    assert!(!e.is_prompt_open());
    assert_eq!(e.selection(), Some((0, 3)));
}

#[test]
fn find_prompt_is_incremental_and_backspace_shrinks() {
    let mut e = ed("abc abd");
    let mut clip = Clipboard::new();
    e.open_find();
    press(&mut e, &mut clip, KeyCode::Char('a'), Mods::NONE);
    press(&mut e, &mut clip, KeyCode::Char('b'), Mods::NONE);
    press(&mut e, &mut clip, KeyCode::Char('d'), Mods::NONE);
    assert_eq!(e.selection(), Some((4, 7)));
    press(&mut e, &mut clip, KeyCode::Backspace, Mods::NONE);
    assert_eq!(e.selection(), Some((0, 2)));
    assert_eq!(e.prompt().unwrap().text, "ab");
}

#[test]
fn find_prompt_prefills_from_selection() {
    let mut e = ed("hello world");
    e.select_range(6, 11);
    e.open_find();
    assert_eq!(e.prompt().unwrap().text, "world");
}

#[test]
fn alt_c_toggles_case_in_prompt() {
    let mut e = ed("Abc");
    let mut clip = Clipboard::new();
    e.open_find();
    press(&mut e, &mut clip, KeyCode::Char('a'), Mods::NONE);
    assert!(e.has_selection());
    press(&mut e, &mut clip, KeyCode::Char('c'), Mods::ALT);
    assert!(e.config().case_sensitive);
    press(&mut e, &mut clip, KeyCode::Char('z'), Mods::NONE);
    assert_eq!(e.notice(), Notice::NotFound);
}

#[test]
fn replace_prompt_flow() {
    let mut e = ed("one one one");
    let mut clip = Clipboard::new();
    press(&mut e, &mut clip, KeyCode::Char('h'), Mods::CTRL);
    for c in "one".chars() {
        press(&mut e, &mut clip, KeyCode::Char(c), Mods::NONE);
    }
    press(&mut e, &mut clip, KeyCode::Tab, Mods::NONE);
    for c in "1".chars() {
        press(&mut e, &mut clip, KeyCode::Char(c), Mods::NONE);
    }
    assert_eq!(e.prompt().unwrap().active, 1);
    press(&mut e, &mut clip, KeyCode::Enter, Mods::NONE);
    assert_eq!(text(&e), "1 one one");
    press(&mut e, &mut clip, KeyCode::Char('a'), Mods::ALT);
    assert_eq!(text(&e), "1 1 1");
}

#[test]
fn goto_prompt_jumps_and_rejects_garbage() {
    let doc: String = (0..50).map(|i| format!("{i}\n")).collect();
    let mut e = ed(&doc);
    let mut clip = Clipboard::new();
    press(&mut e, &mut clip, KeyCode::Char('g'), Mods::CTRL);
    press(&mut e, &mut clip, KeyCode::Char('x'), Mods::NONE);
    assert_eq!(e.prompt().unwrap().text, "");
    press(&mut e, &mut clip, KeyCode::Enter, Mods::NONE);
    assert_eq!(e.notice(), Notice::InvalidLine);
    assert!(e.is_prompt_open());
    for c in "25".chars() {
        press(&mut e, &mut clip, KeyCode::Char(c), Mods::NONE);
    }
    press(&mut e, &mut clip, KeyCode::Enter, Mods::NONE);
    assert!(!e.is_prompt_open());
    assert_eq!(e.cursor().0, 24);
}

#[test]
fn prompt_swallows_editing_keys() {
    let mut e = ed("abc");
    let mut clip = Clipboard::new();
    e.open_find();
    press(&mut e, &mut clip, KeyCode::Char('z'), Mods::NONE);
    assert_eq!(text(&e), "abc");
    assert!(!e.is_modified());
}

#[test]
fn a_click_on_a_bar_field_moves_the_focus() {
    let mut e = ed("abc abc");
    let mut clip = Clipboard::new();
    e.open_replace();
    assert_eq!(e.prompt().map(|p| p.active), Some(0));
    e.prompt_focus(1);
    assert_eq!(e.prompt().map(|p| p.active), Some(1));
    press(&mut e, &mut clip, KeyCode::Char('x'), Mods::NONE);
    assert_eq!(e.prompt().and_then(|p| p.text2), Some("x"));
    e.prompt_focus(0);
    assert_eq!(e.prompt().map(|p| p.active), Some(0));
    // The find prompt has no replacement field to focus.
    e.close_prompt();
    e.open_find();
    e.prompt_focus(1);
    assert_eq!(e.prompt().map(|p| p.active), Some(0));
}

#[test]
fn f3_finds_next() {
    let mut e = ed("x y x");
    e.set_search("x", true);
    let mut clip = Clipboard::new();
    press(&mut e, &mut clip, KeyCode::F(3), Mods::NONE);
    assert_eq!(e.selection(), Some((0, 1)));
    press(&mut e, &mut clip, KeyCode::F(3), Mods::NONE);
    assert_eq!(e.selection(), Some((4, 5)));
    press(&mut e, &mut clip, KeyCode::F(3), Mods::SHIFT);
    assert_eq!(e.selection(), Some((0, 1)));
}

// ---- tab / indent -----------------------------------------------------

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

// ---- word deletion ------------------------------------------------------

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

// ---- viewport ----------------------------------------------------------

fn numbered(n: usize) -> String {
    (0..n).map(|i| format!("line {i}\n")).collect()
}

#[test]
fn view_follows_cursor_down_and_up() {
    let mut e = ed(&numbered(100));
    e.resize(10, 40);
    for _ in 0..30 {
        e.move_vertical(1, false);
    }
    assert_eq!(e.top_line(), 21);
    assert_eq!(e.cursor_screen(), Some((9, 0)));
    for _ in 0..30 {
        e.move_vertical(-1, false);
    }
    assert_eq!(e.top_line(), 0);
    assert_eq!(e.cursor_screen(), Some((0, 0)));
}

#[test]
fn view_scrolls_horizontally() {
    let long = "x".repeat(100);
    let mut e = ed(&long);
    e.resize(5, 20);
    e.move_doc_end(false);
    assert_eq!(e.left_col(), 100 - 20 + 1);
    let (r, c) = e.cursor_screen().unwrap();
    assert_eq!((r, c), (0, 19));
    e.move_doc_start(false);
    assert_eq!(e.left_col(), 0);
}

#[test]
fn horizontal_viewport_rows_show_the_right_slice() {
    let line: String = (0..60).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
    let mut e = ed(&line);
    e.resize(3, 10);
    e.move_doc_end(false);
    let row = e.visible_rows().next().unwrap();
    let s: String = row.cells().map(|c| c.ch).collect();
    assert_eq!(s.chars().count(), 9);
    assert_eq!(s, line[51..].to_string());
}

#[test]
fn resize_keeps_cursor_visible() {
    let mut e = ed(&numbered(50));
    e.resize(30, 80);
    e.goto_line(40);
    e.resize(5, 20);
    assert!(e.cursor_screen().is_some());
    e.resize(100, 200);
    assert!(e.cursor_screen().is_some());
}

#[test]
fn tiny_windows_do_not_panic() {
    let mut e = ed("héllo\nwörld\n\tx");
    for (r, c) in [(0, 0), (1, 1), (1, 2), (2, 1), (1, 40)] {
        e.resize(r, c);
        e.set_line_numbers(true);
        e.set_soft_wrap(true);
        e.move_doc_end(false);
        for row in e.visible_rows() {
            for _ in row.cells() {}
        }
        e.move_doc_start(false);
        let _ = e.cursor_screen();
        let _ = e.pos_at_screen(0, 0);
        check(&e);
    }
}

#[test]
fn line_number_gutter_width_grows_with_digits() {
    let mut e = ed(&numbered(8));
    e.set_line_numbers(true);
    assert_eq!(e.gutter_width(), 2);
    let mut e2 = ed(&numbered(1000));
    e2.set_line_numbers(true);
    assert_eq!(e2.gutter_width(), 5);
    assert_eq!(e2.text_cols(), 80 - 5);
    e2.set_line_numbers(false);
    assert_eq!(e2.gutter_width(), 0);
}

#[test]
fn visible_rows_report_line_numbers() {
    let mut e = ed("a\nb\nc");
    e.set_line_numbers(true);
    e.resize(2, 20);
    let nums: Vec<Option<usize>> = e.visible_rows().map(|r| r.line_number).collect();
    assert_eq!(nums, vec![Some(1), Some(2)]);
}

#[test]
fn visible_rows_stop_at_document_end() {
    let e = ed("a\nb");
    assert_eq!(e.visible_rows().count(), 2);
}

#[test]
fn visible_rows_limited_by_window() {
    let mut e = ed(&numbered(50));
    e.resize(7, 30);
    assert_eq!(e.visible_rows().count(), 7);
}

#[test]
fn tabs_expand_in_rows_and_selection_marks_cells() {
    let mut e = ed("a\tb");
    e.select_range(1, 2);
    let row = e.visible_rows().next().unwrap();
    let cells: Vec<Cell> = row.cells().collect();
    assert_eq!(cells.len(), 5);
    assert_eq!(
        cells[0],
        Cell {
            ch: 'a',
            selected: false
        }
    );
    assert!(cells[1].selected && cells[2].selected && cells[3].selected);
    assert!(!cells[4].selected);
    assert_eq!(cells[4].ch, 'b');
}

#[test]
fn soft_wrap_splits_long_lines_into_rows() {
    let mut e = ed(&"abcdefghij".repeat(3));
    e.set_soft_wrap(true);
    e.resize(10, 10);
    let rows: Vec<RowView> = e.visible_rows().collect();
    assert_eq!(rows.len(), 4);
    assert!(!rows[0].continuation && rows[1].continuation);
    let s: String = rows[1].cells().map(|c| c.ch).collect();
    assert_eq!(s, "abcdefghij");
    assert_eq!(rows[3].cells().count(), 0);
}

#[test]
fn soft_wrap_cursor_and_scroll() {
    let mut e = ed(&"x".repeat(95));
    e.set_soft_wrap(true);
    e.resize(3, 10);
    e.move_doc_end(false);
    // 95 chars at width 10: the cursor is on wrapped row 9.
    let (r, c) = e.cursor_screen().unwrap();
    assert_eq!((r, c), (2, 5));
    assert_eq!(e.top_line(), 0);
    assert_eq!(e.left_col(), 0);
    e.move_doc_start(false);
    assert_eq!(e.cursor_screen(), Some((0, 0)));
}

#[test]
fn soft_wrap_vertical_moves_by_visual_row() {
    let mut e = ed(&"y".repeat(25));
    e.set_soft_wrap(true);
    e.resize(5, 10);
    e.set_cursor(0, 3);
    e.move_vertical(1, false);
    assert_eq!(e.cursor(), (0, 13));
    e.move_vertical(1, false);
    assert_eq!(e.cursor(), (0, 23));
    e.move_vertical(-1, false);
    assert_eq!(e.cursor(), (0, 13));
}

#[test]
fn soft_wrap_over_multiple_lines() {
    let doc = format!("{}\nshort\n{}", "a".repeat(30), "b".repeat(30));
    let mut e = ed(&doc);
    e.set_soft_wrap(true);
    e.resize(4, 10);
    e.move_doc_end(false);
    assert!(e.cursor_screen().is_some());
    let lines: Vec<usize> = e.visible_rows().map(|r| r.line).collect();
    assert_eq!(lines.last(), Some(&2));
    assert_eq!(e.rows_of_line(0), 4);
    assert_eq!(e.rows_of_line(1), 1);
}

#[test]
fn pos_at_screen_maps_clicks() {
    let mut e = ed("abc\ndefgh\ni");
    assert_eq!(e.pos_at_screen(0, 1), 1);
    assert_eq!(e.pos_at_screen(1, 3), 7);
    assert_eq!(e.pos_at_screen(0, 50), 3);
    assert_eq!(e.pos_at_screen(20, 0), e.len_bytes());
    e.set_line_numbers(true);
    assert_eq!(e.pos_at_screen(1, 2 + 4), 8);
    assert_eq!(e.pos_at_screen(1, 0), 4);
}

#[test]
fn click_moves_cursor() {
    let mut e = ed("hello\nworld");
    e.mouse_down(1, 3, 1, false);
    assert_eq!(e.cursor(), (1, 3));
}

#[test]
fn scroll_by_does_not_move_cursor() {
    let mut e = ed(&numbered(100));
    e.resize(10, 30);
    e.scroll_by(20);
    assert_eq!(e.top_line(), 20);
    assert_eq!(e.cursor(), (0, 0));
    assert_eq!(e.cursor_screen(), None);
    e.scroll_by(-500);
    assert_eq!(e.top_line(), 0);
    e.scroll_by(10_000);
    assert_eq!(e.top_line(), 100);
}

#[test]
fn horizontal_scroll_by() {
    let mut e = ed(&"z".repeat(100));
    e.resize(3, 10);
    e.scroll_x_by(15);
    assert_eq!(e.left_col(), 15);
    e.scroll_x_by(-100);
    assert_eq!(e.left_col(), 0);
}

#[test]
fn ctrl_up_down_scroll_one_line() {
    let mut e = ed(&numbered(30));
    e.resize(5, 20);
    ckey(&mut e, KeyCode::Down);
    assert_eq!(e.top_line(), 1);
    ckey(&mut e, KeyCode::Up);
    assert_eq!(e.top_line(), 0);
}

// ---- keys ----------------------------------------------------------------

#[test]
fn key_events_from_legacy_keymap_work() {
    use crate::keymap::Key;
    let mut e = Editor::new();
    let mut clip = Clipboard::new();
    for k in [
        Key::Char(b'h'),
        Key::Char(b'i'),
        Key::Enter,
        Key::Char(b'!'),
    ] {
        e.handle_key(KeyEvent::from(k), &mut clip);
    }
    assert_eq!(text(&e), "hi\n!");
    e.handle_key(KeyEvent::from(Key::Backspace), &mut clip);
    e.handle_key(KeyEvent::from(Key::Left), &mut clip);
    assert_eq!(e.cursor(), (0, 2));
}

#[test]
fn save_and_quit_shortcuts_report_events() {
    let mut e = Editor::new();
    let mut clip = Clipboard::new();
    assert_eq!(
        e.handle_key(KeyEvent::ctrl('s'), &mut clip),
        Event::SaveRequested
    );
    assert_eq!(
        e.handle_key(KeyEvent::ctrl('q'), &mut clip),
        Event::QuitRequested
    );
    assert_eq!(e.handle_key(KeyEvent::ctrl('9'), &mut clip), Event::Ignored);
    assert_eq!(
        e.handle_key(KeyEvent::plain(KeyCode::F(7)), &mut clip),
        Event::Ignored
    );
    assert_eq!(e.handle_key(KeyEvent::ch('a'), &mut clip), Event::Handled);
}

#[test]
fn alt_chars_are_not_typed() {
    let mut e = Editor::new();
    let mut clip = Clipboard::new();
    e.handle_key(KeyEvent::new(KeyCode::Char('x'), Mods::ALT), &mut clip);
    assert_eq!(text(&e), "");
}

#[test]
fn mark_saved_flow_with_keys() {
    let mut e = Editor::new();
    let mut clip = Clipboard::new();
    e.handle_key(KeyEvent::ch('a'), &mut clip);
    assert!(e.is_modified());
    if e.handle_key(KeyEvent::ctrl('s'), &mut clip) == Event::SaveRequested {
        e.mark_saved();
    }
    assert!(!e.is_modified());
}

// ---- large inputs --------------------------------------------------------

fn big_doc(bytes: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(bytes + 100);
    let mut i = 0usize;
    while v.len() < bytes {
        v.extend_from_slice(
            format!("line {i}: the quick brown fox jumps over ñandú €\n").as_bytes(),
        );
        i += 1;
    }
    v
}

#[test]
fn two_megabyte_file_loads_and_round_trips() {
    let doc = big_doc(2 * 1024 * 1024);
    let mut e = Editor::from_bytes(&doc);
    assert!(e.line_count() > 30_000);
    assert_eq!(e.to_bytes(), doc);
    e.goto_line(e.line_count() / 2);
    type_str(&mut e, "MIDDLE");
    assert_eq!(e.len_bytes(), doc.len() + 6);
    e.undo();
    assert_eq!(e.to_bytes(), doc);
    check(&e);
}

#[test]
fn search_in_two_megabyte_file_is_fast() {
    let mut doc = big_doc(2 * 1024 * 1024);
    doc.extend_from_slice(b"NEEDLE_AT_THE_END");
    let mut e = Editor::from_bytes(&doc);
    let t = std::time::Instant::now();
    e.set_search("needle_at_the_end", false);
    assert!(e.find_next());
    assert_eq!(e.selected_bytes(), b"NEEDLE_AT_THE_END");
    assert!(e.find_prev());
    e.set_search("needle_at_the_end", true);
    assert!(!e.find_next());
    assert!(t.elapsed().as_secs_f64() < 5.0, "search too slow");
}

#[test]
fn replace_all_in_big_file() {
    let doc = big_doc(1024 * 1024);
    let mut e = Editor::from_bytes(&doc);
    e.set_search("fox", true);
    e.set_replacement("wolf");
    let n = e.replace_all();
    assert_eq!(n, e.line_count() - 1);
    assert_eq!(e.len_bytes(), doc.len() + n);
    e.undo();
    assert_eq!(e.to_bytes(), doc);
    check(&e);
}

#[test]
fn single_two_megabyte_line_works() {
    let doc = vec![b'q'; 2 * 1024 * 1024];
    let mut e = Editor::from_bytes(&doc);
    assert_eq!(e.line_count(), 1);
    e.move_doc_end(false);
    assert_eq!(e.cursor().1, 2 * 1024 * 1024);
    type_str(&mut e, "!");
    e.set_cursor(0, 1_000_000);
    e.backspace();
    assert_eq!(e.len_bytes(), 2 * 1024 * 1024);
    let row = e.visible_rows().next().unwrap();
    assert!(row.cells().count() <= 80);
    check(&e);
}

#[test]
fn typing_at_the_start_of_a_big_file_stays_correct() {
    let doc = big_doc(512 * 1024);
    let mut e = Editor::from_bytes(&doc);
    for _ in 0..200 {
        e.insert_char('x');
        e.newline();
    }
    assert_eq!(
        e.line_count(),
        doc.iter().filter(|&&b| b == b'\n').count() + 201
    );
    check(&e);
}

#[test]
fn many_small_edits_keep_index_consistent() {
    let mut e = Editor::new();
    for i in 0..500 {
        type_str(&mut e, "line");
        e.newline();
        if i % 3 == 0 {
            e.move_vertical(-1, false);
            e.move_end(false);
        }
    }
    check(&e);
}

// ---- randomized model comparison -----------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[derive(Clone)]
struct Model {
    lines: Vec<String>,
    cur: (usize, usize),
    anchor: Option<(usize, usize)>,
    pref: usize,
}

fn cl(s: &str) -> usize {
    s.chars().count()
}

fn bi(s: &str, c: usize) -> usize {
    s.char_indices().nth(c).map_or(s.len(), |x| x.0)
}

impl Model {
    fn new(init: &str) -> Self {
        Self {
            lines: init.split('\n').map(String::from).collect(),
            cur: (0, 0),
            anchor: None,
            pref: 0,
        }
    }

    fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn offset(&self, p: (usize, usize)) -> usize {
        let n: usize = self.lines.iter().take(p.0).map(|s| s.len() + 1).sum();
        n + bi(&self.lines[p.0], p.1)
    }

    fn sel(&self) -> Option<((usize, usize), (usize, usize))> {
        let a = self.anchor?;
        if a == self.cur {
            None
        } else {
            Some((a.min(self.cur), a.max(self.cur)))
        }
    }

    fn delete_range(&mut self, a: (usize, usize), b: (usize, usize)) {
        if a.0 == b.0 {
            let s = &mut self.lines[a.0];
            let (i, j) = (bi(s, a.1), bi(s, b.1));
            s.replace_range(i..j, "");
        } else {
            let tail = self.lines[b.0][bi(&self.lines[b.0], b.1)..].to_string();
            let h = bi(&self.lines[a.0], a.1);
            self.lines[a.0].truncate(h);
            self.lines[a.0].push_str(&tail);
            self.lines.drain(a.0 + 1..=b.0);
        }
        self.cur = a;
        self.anchor = None;
    }

    fn del_sel(&mut self) -> bool {
        if let Some((a, b)) = self.sel() {
            self.delete_range(a, b);
            true
        } else {
            false
        }
    }

    fn type_char(&mut self, c: char) {
        self.del_sel();
        let (l, col) = self.cur;
        let i = bi(&self.lines[l], col);
        self.lines[l].insert(i, c);
        self.cur.1 += 1;
        self.anchor = None;
        self.pref = self.cur.1;
    }

    fn enter(&mut self) {
        self.del_sel();
        let (l, col) = self.cur;
        let i = bi(&self.lines[l], col);
        let tail = self.lines[l].split_off(i);
        self.lines.insert(l + 1, tail);
        self.cur = (l + 1, 0);
        self.anchor = None;
        self.pref = 0;
    }

    fn backspace(&mut self) {
        if self.del_sel() {
            self.pref = self.cur.1;
            return;
        }
        let (l, c) = self.cur;
        if c > 0 {
            self.delete_range((l, c - 1), (l, c));
        } else if l > 0 {
            self.delete_range((l - 1, cl(&self.lines[l - 1])), (l, 0));
        } else {
            // No-op at the start of the document: the column is untouched.
            return;
        }
        self.pref = self.cur.1;
    }

    fn delete(&mut self) {
        if self.del_sel() {
            self.pref = self.cur.1;
            return;
        }
        let (l, c) = self.cur;
        if c < cl(&self.lines[l]) {
            self.delete_range((l, c), (l, c + 1));
            self.cur = (l, c);
        } else if l + 1 < self.lines.len() {
            self.delete_range((l, c), (l + 1, 0));
        } else {
            return;
        }
        self.pref = self.cur.1;
    }

    fn prep(&mut self, shift: bool) {
        if shift {
            if self.anchor.is_none() {
                self.anchor = Some(self.cur);
            }
        } else {
            self.anchor = None;
        }
    }

    fn left(&mut self, shift: bool) {
        if !shift && let Some((a, _)) = self.sel() {
            self.cur = a;
            self.anchor = None;
            self.pref = a.1;
            return;
        }
        self.prep(shift);
        let (l, c) = self.cur;
        self.cur = if c > 0 {
            (l, c - 1)
        } else if l > 0 {
            (l - 1, cl(&self.lines[l - 1]))
        } else {
            (0, 0)
        };
        self.pref = self.cur.1;
    }

    fn right(&mut self, shift: bool) {
        if !shift && let Some((_, b)) = self.sel() {
            self.cur = b;
            self.anchor = None;
            self.pref = b.1;
            return;
        }
        self.prep(shift);
        let (l, c) = self.cur;
        self.cur = if c < cl(&self.lines[l]) {
            (l, c + 1)
        } else if l + 1 < self.lines.len() {
            (l + 1, 0)
        } else {
            (l, c)
        };
        self.pref = self.cur.1;
    }

    fn up(&mut self, shift: bool) {
        self.prep(shift);
        let (l, _) = self.cur;
        self.cur = if l == 0 {
            (0, 0)
        } else {
            (l - 1, self.pref.min(cl(&self.lines[l - 1])))
        };
    }

    fn down(&mut self, shift: bool) {
        self.prep(shift);
        let (l, _) = self.cur;
        let last = self.lines.len() - 1;
        self.cur = if l == last {
            (last, cl(&self.lines[last]))
        } else {
            (l + 1, self.pref.min(cl(&self.lines[l + 1])))
        };
    }

    fn end(&mut self, shift: bool) {
        self.prep(shift);
        self.cur.1 = cl(&self.lines[self.cur.0]);
        self.pref = self.cur.1;
    }

    fn select_all(&mut self) {
        self.anchor = Some((0, 0));
        let l = self.lines.len() - 1;
        self.cur = (l, cl(&self.lines[l]));
        self.pref = self.cur.1;
    }
}

const ALPHABET: [char; 10] = ['a', 'b', ' ', 'z', 'é', '€', '😀', 'x', '.', 'Q'];

fn random_session(seed: u64, steps: usize, init: &str) {
    let mut rng = Rng(seed);
    let mut e = ed(init);
    e.set_auto_indent(false);
    e.resize(1 + rng.below(12), 1 + rng.below(30));
    let mut m = Model::new(init);
    for step in 0..steps {
        let last_op = rng.below(16);
        match last_op {
            0..=4 => {
                let c = ALPHABET[rng.below(ALPHABET.len())];
                e.insert_char(c);
                m.type_char(c);
            }
            5 => {
                e.newline();
                m.enter();
            }
            6 => {
                e.backspace();
                m.backspace();
            }
            7 => {
                e.delete_forward();
                m.delete();
            }
            8 => {
                let s = rng.below(2) == 1;
                e.move_left(s);
                m.left(s);
            }
            9 => {
                let s = rng.below(2) == 1;
                e.move_right(s);
                m.right(s);
            }
            10 => {
                let s = rng.below(2) == 1;
                e.move_vertical(-1, s);
                m.up(s);
            }
            11 => {
                let s = rng.below(2) == 1;
                e.move_vertical(1, s);
                m.down(s);
            }
            12 => {
                let s = rng.below(2) == 1;
                e.move_end(s);
                m.end(s);
            }
            13 => {
                if rng.below(4) == 0 {
                    e.select_all();
                    m.select_all();
                }
            }
            14 => {
                // Undo then redo is the identity.
                let before = e.to_bytes();
                if e.undo() {
                    assert!(e.redo());
                    assert_eq!(e.to_bytes(), before, "seed {seed} step {step}");
                    // Selection and pref column are not restored by undo.
                    m.anchor = None;
                    e.clear_selection();
                    m.cur = {
                        let (l, c) = e.cursor();
                        (l, c)
                    };
                    m.pref = m.cur.1;
                }
            }
            _ => {
                e.resize(1 + rng.below(12), 1 + rng.below(30));
            }
        }
        assert_eq!(
            String::from_utf8_lossy(&e.to_bytes()),
            m.text(),
            "text diverged: seed {seed} step {step}"
        );
        assert_eq!(
            e.cursor(),
            m.cur,
            "cursor diverged: seed {seed} step {step} op {last_op}"
        );
        let want = m.sel().map(|(a, b)| (m.offset(a), m.offset(b)));
        assert_eq!(e.selection(), want, "selection: seed {seed} step {step}");
        if step % 25 == 0 {
            check(&e);
        }
    }
    check(&e);
    // Undo everything: back to the original; redo everything: back to the end.
    let end_text = e.to_bytes();
    while e.undo() {}
    assert_eq!(e.to_bytes(), init.as_bytes(), "undo-all seed {seed}");
    assert!(!e.is_modified());
    while e.redo() {}
    assert_eq!(e.to_bytes(), end_text, "redo-all seed {seed}");
    check(&e);
}

#[test]
fn model_comparison_empty_start() {
    for seed in 1..=20u64 {
        random_session(seed * 7919, 600, "");
    }
}

#[test]
fn model_comparison_multibyte_start() {
    for seed in 1..=20u64 {
        random_session(seed * 104_729, 600, "héllo wörld\n€uro\n\n😀 end");
    }
}

#[test]
fn model_comparison_long_run() {
    random_session(0xDEADBEEF, 6000, "seed text\nsecond line\n");
}

#[test]
fn random_keys_never_break_invariants() {
    let codes = [
        KeyCode::Char('a'),
        KeyCode::Char('Z'),
        KeyCode::Char(' '),
        KeyCode::Char('é'),
        KeyCode::Char('f'),
        KeyCode::Char('g'),
        KeyCode::Char('h'),
        KeyCode::Char('z'),
        KeyCode::Char('y'),
        KeyCode::Char('x'),
        KeyCode::Char('v'),
        KeyCode::Char('c'),
        KeyCode::Char('9'),
        KeyCode::Enter,
        KeyCode::Backspace,
        KeyCode::Delete,
        KeyCode::Tab,
        KeyCode::Esc,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::F(3),
        KeyCode::F(5),
    ];
    for seed in 1..=15u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut e = Editor::from_bytes("a\tb\r\nc€\n\n😀 word word".as_bytes());
        let mut clip = Clipboard::new();
        e.resize(1 + rng.below(8), 1 + rng.below(25));
        e.set_line_numbers(rng.below(2) == 0);
        e.set_soft_wrap(rng.below(2) == 0);
        for _ in 0..1500 {
            let code = codes[rng.below(codes.len())];
            let r = rng.below(8);
            let mods = Mods {
                ctrl: r & 1 != 0,
                shift: r & 2 != 0,
                alt: r & 4 != 0 && rng.below(4) == 0,
            };
            let ev = e.handle_key(KeyEvent::new(code, mods), &mut clip);
            if ev == Event::SaveRequested {
                e.mark_saved();
            }
            if rng.below(40) == 0 {
                e.resize(rng.below(10), rng.below(30));
            }
            if rng.below(30) == 0 {
                e.mouse_down(rng.below(12), rng.below(35), 1 + rng.below(3) as u8, false);
            }
            check(&e);
            for row in e.visible_rows() {
                assert!(row.cells().count() <= e.text_cols());
            }
            if let Some((r, c)) = e.cursor_screen() {
                let (rows, cols) = e.viewport();
                assert!(r < rows && c < cols);
            }
        }
        while e.undo() {}
        while e.redo() {}
        check(&e);
    }
}

#[test]
fn undo_redo_roundtrip_property() {
    let mut rng = Rng(424242);
    let mut e = Editor::new();
    for _ in 0..800 {
        match rng.below(6) {
            0 | 1 => e.insert_char(ALPHABET[rng.below(ALPHABET.len())]),
            2 => e.newline(),
            3 => e.backspace(),
            4 => e.move_left(false),
            _ => e.delete_forward(),
        }
    }
    let end = e.to_bytes();
    let k = 1 + rng.below(50);
    let mut steps = 0;
    for _ in 0..k {
        if e.undo() {
            steps += 1;
        }
    }
    for _ in 0..steps {
        assert!(e.redo());
    }
    assert_eq!(e.to_bytes(), end);
}

#[test]
fn random_replace_and_search_stay_consistent() {
    let mut rng = Rng(777);
    let mut e = ed("abcabc ABCabc\nabc");
    for _ in 0..300 {
        match rng.below(6) {
            0 => {
                e.set_search(
                    ["a", "bc", "abc", "é", "C"][rng.below(5)],
                    rng.below(2) == 0,
                );
                e.find_next();
            }
            1 => {
                e.find_prev();
            }
            2 => {
                e.set_replacement(["", "x", "éé", "\n", "abc"][rng.below(5)]);
                e.replace_current();
            }
            3 => {
                if rng.below(10) == 0 {
                    e.replace_all();
                }
            }
            4 => {
                e.undo();
            }
            _ => {
                e.redo();
            }
        }
        check(&e);
    }
}

#[test]
fn modified_flag_matches_text_difference_after_random_undo_redo() {
    let mut rng = Rng(99);
    let base = "start\ntext";
    let mut e = ed(base);
    for i in 0..500 {
        match rng.below(5) {
            0 => e.insert_char('k'),
            1 => e.backspace(),
            2 => {
                e.undo();
            }
            3 => {
                e.redo();
            }
            _ => {
                if i % 50 == 0 {
                    e.mark_saved();
                }
                e.move_right(false);
            }
        }
    }
    // Undoing everything lands on the initial text: clean only if the save
    // point was never moved.
    while e.undo() {}
    assert_eq!(text(&e), base);
}

#[test]
fn check_invariants_hold_on_fresh_and_edited_editors() {
    assert_eq!(Editor::new().check_invariants(), Ok(()));
    let mut e = ed("héllo\r\nwörld\n\tx");
    e.set_soft_wrap(true);
    e.resize(3, 7);
    e.move_doc_end(false);
    type_str(&mut e, "€€€");
    assert_eq!(e.check_invariants(), Ok(()));
    e.resize(0, 0);
    assert_eq!(e.check_invariants(), Ok(()));
}
