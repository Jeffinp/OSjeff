use super::*;

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
