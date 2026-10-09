use super::*;
use crate::browsing::web::{FixedAdvance, render};

fn page() -> Page {
    render(
        b"<p>one two three</p><p>two again and two more</p><p>last line</p>",
        600,
    )
}

fn type_str(f: &mut FindBar, s: &str, p: &Page) {
    for b in s.bytes() {
        f.on_key(Key::Char(b), false, Some(p), &FixedAdvance);
    }
}

#[test]
fn closed_bar_ignores_keys() {
    let mut f = FindBar::new();
    let p = page();
    assert_eq!(
        f.on_key(Key::Char(b'a'), false, Some(&p), &FixedAdvance),
        FindOutcome::Ignored
    );
    assert!(!f.is_open());
    assert_eq!(f.count(), 0);
}

#[test]
fn typing_finds_matches_live() {
    let p = page();
    let mut f = FindBar::new();
    f.open(Some(&p), &FixedAdvance);
    type_str(&mut f, "tw", &p);
    assert_eq!(f.count(), 3);
    type_str(&mut f, "o", &p);
    assert_eq!(f.count(), 3);
    type_str(&mut f, "x", &p);
    assert_eq!(f.count(), 0);
    assert_eq!(f.position(), 0);
    f.on_key(Key::Backspace, false, Some(&p), &FixedAdvance);
    assert_eq!(f.count(), 3);
}

#[test]
fn enter_walks_forward_and_wraps_shift_enter_goes_back() {
    let p = page();
    let mut f = FindBar::new();
    f.open(Some(&p), &FixedAdvance);
    type_str(&mut f, "two", &p);
    assert_eq!(f.position(), 1);
    f.on_key(Key::Enter, false, Some(&p), &FixedAdvance);
    assert_eq!(f.position(), 2);
    f.on_key(Key::Enter, false, Some(&p), &FixedAdvance);
    f.on_key(Key::Enter, false, Some(&p), &FixedAdvance);
    assert_eq!(f.position(), 1, "wraps to the first");
    f.on_key(Key::Enter, true, Some(&p), &FixedAdvance);
    assert_eq!(f.position(), 3, "shift+enter wraps backwards");
}

#[test]
fn current_match_has_a_scroll_target_and_the_others_are_listed() {
    let p = page();
    let mut f = FindBar::new();
    f.open(Some(&p), &FixedAdvance);
    type_str(&mut f, "two", &p);
    let y1 = f.current_y().unwrap();
    f.next();
    f.next();
    assert!(f.current_y().unwrap() > y1);
    assert_eq!(f.other_spans().count(), 2);
    assert_eq!(f.current_spans().len(), 1);
}

#[test]
fn esc_closes_and_clears_matches_but_keeps_the_query() {
    let p = page();
    let mut f = FindBar::new();
    f.open(Some(&p), &FixedAdvance);
    type_str(&mut f, "two", &p);
    assert_eq!(
        f.on_key(Key::Esc, false, Some(&p), &FixedAdvance),
        FindOutcome::Closed
    );
    assert!(!f.is_open());
    assert_eq!(f.count(), 0);
    f.open(Some(&p), &FixedAdvance);
    assert_eq!(f.query(), "two");
    assert_eq!(f.count(), 3);
}

#[test]
fn query_length_is_capped() {
    let p = page();
    let mut f = FindBar::new();
    f.open(Some(&p), &FixedAdvance);
    type_str(&mut f, &"a".repeat(200), &p);
    assert_eq!(f.query().len(), MAX_QUERY);
}

#[test]
fn refresh_after_a_relayout_keeps_a_valid_index() {
    let p = page();
    let mut f = FindBar::new();
    f.open(Some(&p), &FixedAdvance);
    type_str(&mut f, "two", &p);
    f.next();
    f.next();
    let small = render(b"<p>two</p>", 600);
    f.refresh(Some(&small), &FixedAdvance);
    assert_eq!(f.count(), 1);
    assert_eq!(f.position(), 1);
    f.refresh(None, &FixedAdvance);
    assert_eq!(f.count(), 0);
}

#[test]
fn control_keys_are_ignored_and_empty_query_matches_nothing() {
    let p = page();
    let mut f = FindBar::new();
    f.open(Some(&p), &FixedAdvance);
    assert_eq!(f.count(), 0);
    assert_eq!(
        f.on_key(Key::Tab, false, Some(&p), &FixedAdvance),
        FindOutcome::Ignored
    );
    assert_eq!(
        f.on_key(Key::Char(0x01), false, Some(&p), &FixedAdvance),
        FindOutcome::Ignored
    );
    f.next();
    f.prev();
    assert_eq!(f.position(), 0);
    assert_eq!(f.current_y(), None);
}
