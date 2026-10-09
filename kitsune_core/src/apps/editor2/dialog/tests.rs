use super::*;
use crate::apps::editor2::Editor;
use crate::i18n::{Lang, testlang::LangGuard};
use crate::system::input::Mods;

fn k(code: KeyCode) -> KeyEvent {
    KeyEvent::plain(code)
}

fn row(name: &str, dir: bool) -> PickRow {
    PickRow {
        name: name.into(),
        dir,
        size: 10,
    }
}

fn listing() -> Vec<PickRow> {
    alloc::vec![
        row("b.txt", false),
        row("Zeta", true),
        row("a10.txt", false),
        row("a2.txt", false),
        row("alpha", true),
    ]
}

fn picker(mode: PickMode, dir: &str, name: &str) -> Picker {
    let mut p = Picker::new(mode, dir, name);
    p.set_entries(dir, listing());
    p
}

fn names(p: &Picker) -> Vec<&str> {
    p.rows().iter().map(|r| r.name.as_str()).collect()
}

#[test]
fn listing_is_folders_first_natural_with_parent_row() {
    let p = picker(PickMode::Open, "/docs", "");
    assert_eq!(
        names(&p),
        ["..", "alpha", "Zeta", "a2.txt", "a10.txt", "b.txt"]
    );
    let root = picker(PickMode::Open, "/", "");
    assert_eq!(names(&root)[0], "alpha", "no .. at the root");
    assert_eq!(p.dir(), "/docs");
}

#[test]
fn the_file_named_in_the_field_is_preselected() {
    let p = picker(PickMode::SaveAs, "/", "a10.txt");
    assert_eq!(p.rows()[p.selected()].name, "a10.txt");
    let (text, caret) = p.field();
    assert_eq!((text.as_str(), caret), ("a10.txt", 7));
}

#[test]
fn arrows_select_and_files_fill_the_field() {
    let mut p = picker(PickMode::Open, "/", "");
    assert_eq!(p.key(k(KeyCode::Down)), PickEvent::Redraw); // Zeta
    assert_eq!(p.field().0, "", "folders do not fill the field");
    p.key(k(KeyCode::Down)); // a2.txt
    assert_eq!(p.field().0, "a2.txt");
    p.key(k(KeyCode::PageDown));
    assert_eq!(p.rows()[p.selected()].name, "b.txt");
    assert_eq!(p.field().0, "b.txt");
    p.key(k(KeyCode::Down));
    assert_eq!(p.rows()[p.selected()].name, "b.txt", "stops at the end");
    p.key(k(KeyCode::PageUp));
    assert_eq!(p.selected(), 0);
}

#[test]
fn enter_opens_folders_and_chooses_files() {
    let mut p = picker(PickMode::Open, "/docs", "");
    // ".." is selected first: Enter goes up.
    assert_eq!(p.key(k(KeyCode::Enter)), PickEvent::Navigate("/".into()));
    p.key(k(KeyCode::Down)); // alpha
    assert_eq!(
        p.key(k(KeyCode::Enter)),
        PickEvent::Navigate("/docs/alpha".into())
    );
    p.key(k(KeyCode::Down));
    p.key(k(KeyCode::Down)); // Zeta, a2.txt
    assert_eq!(
        p.key(k(KeyCode::Enter)),
        PickEvent::Choose("/docs/a2.txt".into())
    );
}

#[test]
fn typed_names_are_resolved_against_the_folder() {
    let mut p = picker(PickMode::SaveAs, "/docs", "");
    for c in "new.txt".chars() {
        p.key(KeyEvent::ch(c));
    }
    assert_eq!(
        p.key(k(KeyCode::Enter)),
        PickEvent::Choose("/docs/new.txt".into())
    );
    let mut p = picker(PickMode::SaveAs, "/docs", "");
    for c in "../x/y.txt".chars() {
        p.key(KeyEvent::ch(c));
    }
    assert_eq!(
        p.key(k(KeyCode::Enter)),
        PickEvent::Choose("/x/y.txt".into())
    );
    let mut p = picker(PickMode::Open, "/docs", "");
    for c in "/etc/hosts".chars() {
        p.key(KeyEvent::ch(c));
    }
    assert_eq!(
        p.key(k(KeyCode::Enter)),
        PickEvent::Choose("/etc/hosts".into())
    );
}

#[test]
fn a_typed_folder_name_navigates() {
    let mut p = picker(PickMode::Open, "/", "");
    for c in "alpha".chars() {
        p.key(KeyEvent::ch(c));
    }
    assert_eq!(
        p.key(k(KeyCode::Enter)),
        PickEvent::Navigate("/alpha".into())
    );
    assert_eq!(p.field().0, "", "the field is cleared for the next name");
    let mut p = picker(PickMode::Open, "/", "");
    for c in "sub/".chars() {
        p.key(KeyEvent::ch(c));
    }
    assert_eq!(p.key(k(KeyCode::Enter)), PickEvent::Navigate("/sub".into()));
}

#[test]
fn backspace_on_an_empty_field_goes_up_but_never_above_the_root() {
    let mut p = picker(PickMode::Open, "/a/b", "");
    assert_eq!(
        p.key(k(KeyCode::Backspace)),
        PickEvent::Navigate("/a".into())
    );
    let mut p = picker(PickMode::Open, "/", "");
    assert_eq!(p.key(k(KeyCode::Backspace)), PickEvent::Redraw);
}

#[test]
fn field_editing_is_character_aware() {
    let mut p = picker(PickMode::SaveAs, "/", "");
    for c in "açãe".chars() {
        p.key(KeyEvent::ch(c));
    }
    p.key(k(KeyCode::Left));
    p.key(k(KeyCode::Backspace));
    assert_eq!(p.field(), ("açe".into(), 2));
    p.key(k(KeyCode::Home));
    p.key(k(KeyCode::Delete));
    assert_eq!(p.field().0, "çe");
    p.key(k(KeyCode::End));
    p.key(KeyEvent::ctrl('u'));
    assert_eq!(p.field(), (String::new(), 0));
    // Control characters and Alt chords are not text; the field is capped.
    p.key(KeyEvent::ch('\u{7}'));
    p.key(KeyEvent::new(KeyCode::Char('x'), Mods::ALT));
    assert_eq!(p.field().0, "");
    for _ in 0..(MAX_FIELD + 20) {
        p.key(KeyEvent::ch('z'));
    }
    assert_eq!(p.field().0.len(), MAX_FIELD);
}

#[test]
fn the_first_typed_character_replaces_the_starting_name() {
    let mut p = picker(PickMode::SaveAs, "/", "sem-nome.txt");
    p.key(KeyEvent::ch('n'));
    p.key(KeyEvent::ch('o'));
    assert_eq!(p.field().0, "no");
    // Backspace clears the whole starting name, once.
    let mut p = picker(PickMode::SaveAs, "/", "sem-nome.txt");
    p.key(k(KeyCode::Backspace));
    assert_eq!(p.field().0, "");
    p.key(KeyEvent::ch('a'));
    p.key(k(KeyCode::Backspace));
    assert_eq!(p.field().0, "");
    // Moving the caret first keeps the name for editing.
    let mut p = picker(PickMode::SaveAs, "/", "sem-nome.txt");
    p.key(k(KeyCode::End));
    p.key(KeyEvent::ch('2'));
    assert_eq!(p.field().0, "sem-nome.txt2");
    // Choosing with Enter keeps it as typed.
    let mut p = picker(PickMode::SaveAs, "/", "sem-nome.txt");
    assert_eq!(
        p.key(k(KeyCode::Enter)),
        PickEvent::Choose("/sem-nome.txt".into())
    );
}

#[test]
fn tab_completes_the_longest_common_prefix() {
    let mut p = picker(PickMode::Open, "/", "");
    p.key(KeyEvent::ch('a'));
    p.key(k(KeyCode::Tab));
    assert_eq!(p.field().0, "a", "alpha, a2.txt, a10.txt share only 'a'");
    p.key(KeyEvent::ch('l'));
    p.key(k(KeyCode::Tab));
    assert_eq!(p.field().0, "alpha");
    let mut p = picker(PickMode::Open, "/", "b");
    p.key(k(KeyCode::Tab));
    assert_eq!(p.field().0, "b.txt");
    let mut p = picker(PickMode::Open, "/", "q");
    p.key(k(KeyCode::Tab));
    assert_eq!(p.field().0, "q", "no match leaves the text alone");
}

#[test]
fn mouse_clicks_select_and_double_clicks_open() {
    let mut p = picker(PickMode::Open, "/", "");
    assert_eq!(p.click(1, false), PickEvent::Redraw);
    assert_eq!(p.selected(), 1);
    assert_eq!(p.click(1, true), PickEvent::Navigate("/Zeta".into()));
    assert_eq!(p.click(2, false), PickEvent::Redraw);
    assert_eq!(p.field().0, "a2.txt");
    assert_eq!(p.click(2, true), PickEvent::Choose("/a2.txt".into()));
    assert_eq!(p.click(99, true), PickEvent::None);
}

#[test]
fn the_list_scrolls_with_the_selection_and_the_wheel() {
    let mut p = Picker::new(PickMode::Open, "/", "");
    let many: Vec<PickRow> = (0..30)
        .map(|i| row(&alloc::format!("f{i:02}"), false))
        .collect();
    p.set_entries("/", many);
    p.set_visible(5);
    for _ in 0..12 {
        p.key(k(KeyCode::Down));
    }
    assert_eq!(p.selected(), 12);
    assert_eq!(p.scroll(), 8);
    p.scroll_by(-100);
    assert_eq!(p.scroll(), 0);
    assert_eq!(p.selected(), 12, "the wheel leaves the selection alone");
    p.scroll_by(1000);
    assert_eq!(p.scroll(), 25);
    // Shrinking the window keeps the selection in view.
    p.set_visible(3);
    assert!(p.selected() >= p.scroll() && p.selected() < p.scroll() + 3);
}

#[test]
fn errors_show_until_the_next_key() {
    let mut p = picker(PickMode::Open, "/", "");
    p.set_error("Pasta não encontrada");
    assert_eq!(p.error(), Some("Pasta não encontrada"));
    p.key(KeyEvent::ch('x'));
    assert_eq!(p.error(), None);
    // A listing that fails leaves the old rows in place.
    let before = names(&p).len();
    p.set_error("x");
    assert_eq!(names(&p).len(), before);
}

#[test]
fn the_overwrite_question_takes_over_the_keys() {
    let mut p = picker(PickMode::SaveAs, "/", "a2.txt");
    assert_eq!(
        p.key(k(KeyCode::Enter)),
        PickEvent::Choose("/a2.txt".into())
    );
    p.confirm_overwrite("/a2.txt");
    assert_eq!(p.asking(), Some("/a2.txt"));
    // Typing is ignored while the question is open.
    assert_eq!(p.key(KeyEvent::ch('q')), PickEvent::None);
    assert_eq!(p.click(1, true), PickEvent::None);
    assert_eq!(
        p.key(k(KeyCode::Esc)),
        PickEvent::Redraw,
        "Esc only closes the question"
    );
    assert_eq!(p.asking(), None);
    p.confirm_overwrite("/a2.txt");
    assert_eq!(p.key(KeyEvent::ch('n')), PickEvent::Redraw);
    p.confirm_overwrite("/a2.txt");
    assert_eq!(
        p.key(k(KeyCode::Enter)),
        PickEvent::Overwrite("/a2.txt".into())
    );
    p.confirm_overwrite("/a2.txt");
    assert_eq!(
        p.key(KeyEvent::ch('s')),
        PickEvent::Overwrite("/a2.txt".into())
    );
    assert_eq!(
        p.key(k(KeyCode::Esc)),
        PickEvent::Cancel,
        "now Esc cancels the dialog"
    );
}

#[test]
fn close_question_answers() {
    let mut a = CloseAsk::new();
    assert_eq!(a.selected(), CloseChoice::Save);
    assert_eq!(a.key(k(KeyCode::Enter)), Some(CloseChoice::Save));
    assert_eq!(a.key(k(KeyCode::Right)), None);
    assert_eq!(a.selected(), CloseChoice::Discard);
    assert_eq!(a.key(k(KeyCode::Enter)), Some(CloseChoice::Discard));
    a.key(k(KeyCode::Tab));
    assert_eq!(a.selected(), CloseChoice::Cancel);
    a.key(k(KeyCode::Tab));
    assert_eq!(a.selected(), CloseChoice::Save, "wraps around");
    a.key(k(KeyCode::Left));
    assert_eq!(a.selected(), CloseChoice::Cancel);
    a.key(KeyEvent::plain(KeyCode::Tab).shifted());
    assert_eq!(a.selected(), CloseChoice::Discard);
    assert_eq!(a.key(KeyEvent::ch('S')), Some(CloseChoice::Save));
    assert_eq!(a.key(KeyEvent::ch('d')), Some(CloseChoice::Discard));
    assert_eq!(a.key(KeyEvent::ch('c')), Some(CloseChoice::Cancel));
    assert_eq!(a.key(k(KeyCode::Esc)), Some(CloseChoice::Cancel));
    assert_eq!(a.key(KeyEvent::ch('x')), None);
    assert_eq!(a.key(KeyEvent::ctrl('s')), None, "chords are not answers");
    a.select(CloseChoice::Cancel);
    assert_eq!(a.key(k(KeyCode::Enter)), Some(CloseChoice::Cancel));
}

#[test]
fn status_bar_pieces_follow_the_editor() {
    let _g = LangGuard::new(Lang::Pt);
    let mut e = Editor::from_bytes(b"ola\nmundo\n");
    e.set_cursor(1, 2);
    let b = status_bar(&e.status());
    assert_eq!(b.position, "Ln 2, Col 3");
    assert_eq!(b.facts[0], "UTF-8");
    assert_eq!(b.facts[1], "LF");
    assert_eq!(b.facts[2], "10 B");
    assert_eq!(b.facts[3], "3 linhas");
    assert!(!b.modified);
    let big = status_bar(&Editor::from_bytes(&alloc::vec![b'x'; 1536]).status());
    assert_eq!(big.facts[2], "1,5 KiB");
    e.select_all();
    let b = status_bar(&e.status());
    assert!(b.position.ends_with("(10 selecionados)"), "{}", b.position);
    let one = status_bar(&Editor::from_bytes(b"x").status());
    assert_eq!(one.facts[3], "1 linha");
    let mut crlf = Editor::from_bytes(b"a\r\nb\r\n");
    crlf.insert_char('z');
    let b = status_bar(&crlf.status());
    assert_eq!(b.facts[1], "CRLF");
    assert!(b.modified);
    let mut sel1 = Editor::from_bytes(b"ab");
    sel1.select_range(0, 1);
    assert!(
        status_bar(&sel1.status())
            .position
            .ends_with("(1 selecionado)")
    );
}

#[test]
fn status_line_reports_position_size_and_state() {
    let _g = LangGuard::new(Lang::Pt);
    let mut e = Editor::from_bytes(b"one\r\ntwo\r\nthree");
    e.set_cursor(1, 2);
    let s = status_line(&e.status(), false);
    assert!(
        s.starts_with("Ln 2, Col 3   3 linhas   15 B   UTF-8   CRLF"),
        "{s}"
    );
    assert!(!s.contains("modificado"));
    e.insert_str("x");
    let s = status_line(&e.status(), false);
    assert!(s.ends_with("* modificado"), "{s}");
    assert!(status_line(&e.status(), true).ends_with("SOMENTE LEITURA"));
    e.select_all();
    assert!(status_line(&e.status(), false).contains("sel 16"));
    let big = Editor::from_bytes(&alloc::vec![b'a'; 1_572_864]);
    assert!(status_line(&big.status(), false).contains("1,5 MiB"));
}

#[test]
fn status_texts_in_english() {
    let _g = LangGuard::new(Lang::En);
    let mut e = Editor::from_bytes(b"one\r\ntwo\r\nthree");
    e.set_cursor(1, 2);
    let s = status_line(&e.status(), false);
    assert!(
        s.starts_with("Ln 2, Col 3   3 lines   15 B   UTF-8   CRLF"),
        "{s}"
    );
    e.insert_str("x");
    assert!(status_line(&e.status(), false).ends_with("* modified"));
    assert!(status_line(&e.status(), true).ends_with("READ ONLY"));
    let b = status_bar(&Editor::from_bytes(b"x").status());
    assert_eq!(b.facts[3], "1 line");
    let big = status_bar(&Editor::from_bytes(&alloc::vec![b'x'; 1536]).status());
    assert_eq!(big.facts[2], "1.5 KiB");
    let mut e = Editor::from_bytes(b"ab");
    e.select_range(0, 2);
    assert!(status_bar(&e.status()).position.ends_with("(2 selected)"));
    assert_eq!(CloseAsk::label(CloseChoice::Discard), "Discard");
    assert_eq!(CloseAsk::label(CloseChoice::Cancel), "Cancel");
}

#[test]
fn close_labels_in_portuguese() {
    let _g = LangGuard::new(Lang::Pt);
    assert_eq!(CloseAsk::label(CloseChoice::Save), "Salvar");
    assert_eq!(CloseAsk::label(CloseChoice::Discard), "Descartar");
    assert_eq!(CloseAsk::label(CloseChoice::Cancel), "Cancelar");
}

#[test]
fn the_starting_name_is_fresh_until_a_key_touches_it() {
    let mut p = Picker::new(PickMode::SaveAs, "/", "nota.txt");
    assert!(p.field_fresh());
    p.key(k(KeyCode::Char('x')));
    assert!(!p.field_fresh());
    assert_eq!(p.field().0, "x");
    assert!(!Picker::new(PickMode::Open, "/", "").field_fresh());
}
