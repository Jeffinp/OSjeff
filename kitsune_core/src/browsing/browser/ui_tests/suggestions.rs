use super::*;

#[test]
fn empty_query_suggests_nothing() {
    assert!(suggest("", &[bm("http://a/", "A")], &["http://b/".into()]).is_empty());
    assert!(suggest("   ", &[], &[]).is_empty());
}

#[test]
fn prefix_ignores_scheme_and_www() {
    let h = vec!["https://www.example.com/page".to_string()];
    for q in [
        "exa",
        "www.exa",
        "https://exa",
        "http://www.example",
        "EXAMPLE.COM",
    ] {
        let s = suggest(q, &[], &h);
        assert_eq!(s.len(), 1, "{q}");
        assert!(!s[0].bookmark);
    }
}

#[test]
fn substring_matches_after_prefix_matches() {
    let h = vec![
        "http://site.test/wiki".to_string(),
        "http://wiki.test/".to_string(),
    ];
    let s = suggest("wiki", &[], &h);
    assert_eq!(s[0].url, "http://wiki.test/");
    assert_eq!(s[1].url, "http://site.test/wiki");
}

#[test]
fn favourites_come_before_history_and_match_titles() {
    let b = [bm("http://fav.test/", "Receitas de bolo")];
    let h = vec!["http://hist.test/bolo".to_string()];
    let s = suggest("bolo", &b, &h);
    assert_eq!(s.len(), 2);
    assert!(s[0].bookmark);
    assert_eq!(s[0].label, "Receitas de bolo");
    assert!(!s[1].bookmark);
}

#[test]
fn duplicates_are_listed_once() {
    let b = [bm("http://a.test/", "A")];
    let h = vec!["http://a.test/".to_string(), "http://a.test/x".to_string()];
    let s = suggest("a.test", &b, &h);
    assert_eq!(s.len(), 2);
    assert_eq!(s.iter().filter(|x| x.url == "http://a.test/").count(), 1);
}

#[test]
fn at_most_six_suggestions() {
    let h: Vec<String> = (0..20)
        .map(|i| alloc::format!("http://x.test/{i}"))
        .collect();
    assert_eq!(suggest("x.test", &[], &h).len(), MAX_SUGGESTIONS);
}

#[test]
fn the_exact_address_alone_is_not_suggested() {
    let h = vec!["http://a.test/".to_string()];
    assert!(suggest("http://a.test/", &[], &h).is_empty());
    assert!(suggest("a.test/", &[], &h).is_empty());
}

#[test]
fn bar_suggests_while_typing_from_history() {
    let mut b = browser_with_history(&["http://alpha.test/", "http://beta.test/"]);
    clear_bar(&mut b);
    type_str(&mut b, "alp");
    let s = b.suggestions();
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].url, "http://alpha.test/");
}

#[test]
fn arrows_move_the_highlight_and_enter_opens_it() {
    let mut b = browser_with_history(&["http://alpha.test/", "http://alps.test/"]);
    clear_bar(&mut b);
    type_str(&mut b, "alp");
    assert_eq!(b.suggestion_selected(), None);
    assert!(b.on_key(Key::Down));
    assert_eq!(b.suggestion_selected(), Some(0));
    b.on_key(Key::Down);
    assert_eq!(b.suggestion_selected(), Some(1));
    b.on_key(Key::Down);
    assert_eq!(b.suggestion_selected(), Some(1), "stops at the last");
    b.on_key(Key::Up);
    assert_eq!(b.suggestion_selected(), Some(0));
    b.on_key(Key::Up);
    assert_eq!(b.suggestion_selected(), None);
    b.on_key(Key::Down);
    let want = b.suggestions()[0].url.clone();
    b.on_key(Key::Enter);
    assert_eq!(b.take_request(), Some(want.as_bytes()));
}

#[test]
fn enter_without_a_highlight_submits_the_typed_text() {
    let mut b = browser_with_history(&["http://alpha.test/"]);
    clear_bar(&mut b);
    type_str(&mut b, "alp");
    b.on_key(Key::Enter);
    let n = b.take_request().unwrap().to_vec();
    assert!(
        n.starts_with(b"https://www.bing.com/search?q=alp"),
        "{:?}",
        String::from_utf8_lossy(&n)
    );
}

#[test]
fn typing_resets_the_highlight() {
    let mut b = browser_with_history(&["http://alpha.test/"]);
    clear_bar(&mut b);
    type_str(&mut b, "al");
    b.on_key(Key::Down);
    assert_eq!(b.suggestion_selected(), Some(0));
    type_str(&mut b, "p");
    assert_eq!(b.suggestion_selected(), None);
}

#[test]
fn esc_closes_the_list_until_the_text_changes() {
    let mut b = browser_with_history(&["http://alpha.test/"]);
    clear_bar(&mut b);
    type_str(&mut b, "al");
    assert!(!b.suggestions().is_empty());
    assert!(b.on_key(Key::Esc));
    assert!(b.suggestions().is_empty());
    type_str(&mut b, "p");
    assert!(!b.suggestions().is_empty());
}

#[test]
fn no_suggestions_when_the_page_has_the_focus() {
    let mut b = browser_with_history(&["http://alpha.test/"]);
    clear_bar(&mut b);
    type_str(&mut b, "al");
    b.set_bar_focus(false);
    assert!(b.suggestions().is_empty());
    assert!(!b.bar_focus());
    b.set_bar_focus(true);
    assert!(!b.suggestions().is_empty());
}

#[test]
fn picking_a_suggestion_with_the_mouse_opens_it() {
    let mut b = browser_with_history(&["http://alpha.test/"]);
    clear_bar(&mut b);
    type_str(&mut b, "al");
    assert!(b.pick_suggestion(0));
    assert_eq!(b.take_request(), Some(&b"http://alpha.test/"[..]));
    assert!(!b.pick_suggestion(5));
}

#[test]
fn up_and_down_without_suggestions_are_not_consumed() {
    let mut b = Browser::new();
    assert!(!b.on_key(Key::Down));
    assert!(!b.on_key(Key::Up));
}
