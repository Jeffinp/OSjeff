use super::*;

#[test]
fn plain_key_conversion() {
    assert_eq!(KeyEvent::from(Key::Enter), KeyEvent::plain(KeyCode::Enter));
    assert_eq!(KeyEvent::from(Key::Char(b'x')), KeyEvent::ch('x'));
}

#[test]
fn ctrl_lowercases_letters() {
    let e = KeyEvent::from_key(Key::Char(b'A'), Mods::CTRL);
    assert_eq!(e, KeyEvent::ctrl('a'));
}

#[test]
fn shifted_keeps_other_mods() {
    let e = KeyEvent::ctrl('z').shifted();
    assert_eq!(e.mods, Mods::CTRL_SHIFT);
    assert_eq!(KeyEvent::plain(KeyCode::Left).with_ctrl().mods, Mods::CTRL);
}

#[test]
fn mods_is_none() {
    assert!(Mods::NONE.is_none());
    assert!(!Mods::ALT.is_none());
}

#[test]
fn every_legacy_key_maps() {
    let keys = [
        Key::Tab,
        Key::Esc,
        Key::Delete,
        Key::Left,
        Key::Right,
        Key::Up,
        Key::Down,
        Key::Home,
        Key::End,
        Key::Backspace,
    ];
    for k in keys {
        assert!(KeyEvent::from(k).mods.is_none());
    }
}
