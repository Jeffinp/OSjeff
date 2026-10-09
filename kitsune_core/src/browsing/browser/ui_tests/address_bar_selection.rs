use super::*;

#[test]
fn select_all_then_typing_replaces_the_address() {
    let mut b = browser_with_history(&["http://a.test/"]);
    b.select_bar();
    assert!(b.bar_selected());
    type_str(&mut b, "x.test");
    assert_eq!(b.url(), b"x.test");
    assert!(!b.bar_selected());
}

#[test]
fn select_all_then_backspace_clears() {
    let mut b = browser_with_history(&["http://a.test/"]);
    b.select_bar();
    b.on_key(Key::Backspace);
    assert_eq!(b.url(), b"");
    b.select_bar();
    assert!(!b.bar_selected(), "nothing to select in an empty bar");
}

#[test]
fn moving_the_caret_drops_the_selection() {
    let mut b = browser_with_history(&["http://a.test/"]);
    b.select_bar();
    b.on_key(Key::End);
    assert!(!b.bar_selected());
    type_str(&mut b, "z");
    assert_eq!(b.url(), b"http://a.test/z");
}

#[test]
fn select_all_gives_the_bar_the_focus_and_hides_suggestions() {
    let mut b = browser_with_history(&["http://a.test/"]);
    b.set_bar_focus(false);
    b.select_bar();
    assert!(b.bar_focus());
    assert!(b.suggestions().is_empty());
}

/// Regression (fuzz `html_img_form`): the 80-byte cut of a page title landed inside a
/// multi-byte character and `&title[..80]` panicked.
#[test]
fn page_title_cut_inside_a_multibyte_character_does_not_panic() {
    let mut b = Browser::new();
    // 78 ASCII bytes then a 3-byte character spanning bytes 78..81.
    let title = alloc::format!("{}\u{20ac}tail", "a".repeat(78));
    b.set_page_title(&title);
    assert_eq!(b.page_title(), "a".repeat(78));
    // Every cut position of a title made of 3-byte characters.
    for n in 0..120 {
        b.set_page_title(&"\u{20ac}".repeat(n));
        assert!(b.page_title().len() <= 80);
        assert!(b.page_title().chars().all(|c| c == '\u{20ac}'));
    }
}
