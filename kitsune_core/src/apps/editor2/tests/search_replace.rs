use super::*;

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
