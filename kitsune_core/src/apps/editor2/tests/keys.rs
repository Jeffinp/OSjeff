use super::*;

#[test]
fn key_events_from_legacy_keymap_work() {
    use crate::system::keymap::Key;
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
