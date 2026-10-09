//! Tests for the browser UI model: favourites, suggestions, internal pages,
//! focus, history interplay.

use super::*;

fn browser_with_history(urls: &[&str]) -> Browser {
    let mut b = Browser::new();
    for u in urls {
        b.open(u.as_bytes());
        b.take_request();
        b.loaded_with(Conn::Plain, false);
    }
    b
}

fn type_str(b: &mut Browser, s: &str) {
    for c in s.bytes() {
        b.on_key(Key::Char(c));
    }
}

fn clear_bar(b: &mut Browser) {
    while b.url_len > 0 {
        b.on_key(Key::Backspace);
    }
}

// ---- favourites ----

#[test]
fn start_page_cannot_be_bookmarked() {
    let mut b = Browser::new();
    assert_eq!(b.toggle_bookmark(), None);
    assert!(!b.is_bookmarked());
}

#[test]
fn ctrl_d_toggles_the_current_page() {
    let mut b = browser_with_history(&["http://a.test/x"]);
    assert!(!b.is_bookmarked());
    assert_eq!(b.toggle_bookmark(), Some(true));
    assert!(b.is_bookmarked());
    assert_eq!(b.bookmarks().len(), 1);
    assert_eq!(b.toggle_bookmark(), Some(false));
    assert!(!b.is_bookmarked());
    assert!(b.bookmarks().is_empty());
}

#[test]
fn bookmark_uses_the_page_title_or_the_url() {
    let mut b = browser_with_history(&["http://a.test/x"]);
    b.set_page_title("Página A");
    b.toggle_bookmark();
    assert_eq!(b.bookmarks()[0].title, "Página A");
    b.toggle_bookmark();
    b.set_page_title("");
    b.toggle_bookmark();
    assert_eq!(b.bookmarks()[0].title, "http://a.test/x");
}

#[test]
fn memory_store_rules() {
    let mut s = MemoryBookmarks::default();
    let bm = |u: &str| Bookmark {
        url: u.into(),
        title: String::new(),
    };
    assert!(s.add(bm("http://a/")));
    assert!(!s.add(bm("http://a/")), "no duplicates");
    assert!(s.contains("http://a/"));
    assert!(s.remove("http://a/"));
    assert!(!s.remove("http://a/"));
    for i in 0..MAX_BOOKMARKS {
        assert!(s.add(bm(&alloc::format!("http://h/{i}"))));
    }
    assert!(!s.add(bm("http://one-too-many/")));
    assert_eq!(s.all().len(), MAX_BOOKMARKS);
}

#[test]
fn a_custom_store_is_used() {
    use alloc::rc::Rc;
    use core::cell::RefCell;
    struct Shared(Rc<RefCell<Vec<Bookmark>>>);
    impl BookmarkStore for Shared {
        fn all(&self) -> Vec<Bookmark> {
            self.0.borrow().clone()
        }
        fn add(&mut self, b: Bookmark) -> bool {
            self.0.borrow_mut().push(b);
            true
        }
        fn remove(&mut self, url: &str) -> bool {
            self.0.borrow_mut().retain(|b| b.url != url);
            true
        }
    }
    let backing = Rc::new(RefCell::new(Vec::new()));
    let mut b = Browser::with_store(Box::new(Shared(backing.clone())));
    b.open(b"http://z.test/");
    b.take_request();
    b.loaded_with(Conn::Plain, false);
    b.toggle_bookmark();
    assert_eq!(backing.borrow().len(), 1);
    assert!(b.is_bookmarked());
}

// ---- suggestions ----

fn bm(url: &str, title: &str) -> Bookmark {
    Bookmark {
        url: url.into(),
        title: title.into(),
    }
}

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

// ---- internal pages ----

fn open_internal(b: &mut Browser, url: &str) -> String {
    b.open(url.as_bytes());
    String::from_utf8(b.take_internal().expect("internal html")).unwrap()
}

#[test]
fn internal_pages_load_without_the_network() {
    let mut b = Browser::new();
    let html = open_internal(&mut b, "osjeff://sobre");
    assert!(html.contains("Sobre o Navegador"));
    assert!(b.take_request().is_none(), "nothing for the fetcher");
    assert_eq!(b.status(), Status::Done);
    assert!(b.is_internal());
    assert!(!b.is_home());
    assert_eq!(b.security(), Security::None);
    assert_eq!(b.url(), b"osjeff://sobre");
    assert!(b.take_internal().is_none(), "delivered once");
}

#[test]
fn typed_osjeff_url_is_not_a_search() {
    let mut b = Browser::new();
    type_str(&mut b, "osjeff://favoritos");
    b.on_key(Key::Enter);
    assert!(b.take_request().is_none());
    assert!(b.take_internal().is_some());
}

#[test]
fn inicio_is_the_start_page() {
    let mut b = browser_with_history(&["http://a.test/"]);
    b.open(b"osjeff://inicio");
    assert!(b.is_home());
    assert!(!b.is_internal());
    assert!(b.take_internal().is_none());
}

#[test]
fn favourites_page_lists_and_removes() {
    let mut b = browser_with_history(&["http://a.test/x?a=1&b=2"]);
    b.set_page_title("A <b>& B");
    b.toggle_bookmark();
    let html = open_internal(&mut b, "osjeff://favoritos");
    assert!(html.contains("A &lt;b&gt;&amp; B"), "{html}");
    assert!(html.contains("http://a.test/x?a=1&amp;b=2"));
    assert!(html.contains("osjeff://favoritos?rm=0"));
    // The remove link.
    let html = open_internal(&mut b, "osjeff://favoritos?rm=0");
    assert!(html.contains("Nenhum favorito"));
    assert!(b.bookmarks().is_empty());
    assert_eq!(b.url(), b"osjeff://favoritos");
}

#[test]
fn removing_a_missing_favourite_is_harmless() {
    let mut b = Browser::new();
    let html = open_internal(&mut b, "osjeff://favoritos?rm=7");
    assert!(html.contains("Nenhum favorito"));
    let html = open_internal(&mut b, "osjeff://favoritos?rm=abc");
    assert!(html.contains("Nenhum favorito"));
}

#[test]
fn history_page_lists_newest_first_and_skips_internal_pages() {
    let mut b = browser_with_history(&["http://one.test/", "http://two.test/"]);
    let html = open_internal(&mut b, "osjeff://historico");
    let one = html.find("http://one.test/").unwrap();
    let two = html.find("http://two.test/").unwrap();
    assert!(two < one);
    assert!(!html.contains(">osjeff://historico<"));
}

#[test]
fn history_page_is_empty_message() {
    let mut b = Browser::new();
    let html = open_internal(&mut b, "osjeff://historico");
    assert!(html.contains("Nada visitado"));
}

#[test]
fn unknown_internal_name_shows_the_about_page() {
    let mut b = Browser::new();
    let html = open_internal(&mut b, "osjeff://nada");
    assert!(html.contains("osjeff://favoritos"));
}

#[test]
fn internal_scheme_is_case_insensitive() {
    let mut b = Browser::new();
    assert!(open_internal(&mut b, "OSJEFF://SOBRE").contains("Navegador"));
}

#[test]
fn internal_pages_enter_the_history_and_back_replays_them() {
    let mut b = browser_with_history(&["http://a.test/"]);
    open_internal(&mut b, "osjeff://sobre");
    assert_eq!(b.history_len(), 2);
    assert!(b.can_back());
    b.back();
    // Back goes to the real page: a network request, history untouched.
    assert_eq!(b.take_request(), Some(&b"http://a.test/"[..]));
    b.loaded_with(Conn::Plain, false);
    assert_eq!(b.history_len(), 2);
    assert!(b.can_forward());
    b.forward();
    assert!(
        b.take_internal().is_some(),
        "forward replays the internal page"
    );
    assert_eq!(b.history_len(), 2, "replayed, not recorded again");
    assert_eq!(b.url(), b"osjeff://sobre");
}

#[test]
fn links_inside_internal_pages_navigate() {
    let mut b = browser_with_history(&["http://a.test/"]);
    open_internal(&mut b, "osjeff://sobre");
    assert!(b.open_link(b"osjeff://favoritos"));
    assert!(b.take_internal().is_some());
    // An absolute http link from an internal page is not an https downgrade.
    assert!(b.open_link(b"http://a.test/x"));
    assert_eq!(b.take_request(), Some(&b"http://a.test/x"[..]));
    assert!(!b.is_internal());
}

#[test]
fn reload_of_an_internal_page_regenerates_it() {
    let mut b = Browser::new();
    open_internal(&mut b, "osjeff://historico");
    b.reload();
    assert!(b.take_internal().is_some());
    assert!(b.take_request().is_none());
}

#[test]
fn network_navigation_clears_the_internal_flag() {
    let mut b = Browser::new();
    open_internal(&mut b, "osjeff://sobre");
    b.open(b"http://a.test/");
    assert!(!b.is_internal());
    assert!(b.take_request().is_some());
}

#[test]
fn internal_html_is_escaped() {
    let mut b = browser_with_history(&["http://a.test/\"onmouseover=\"x"]);
    let html = open_internal(&mut b, "osjeff://historico");
    assert!(!html.contains("\"onmouseover"), "{html}");
}

#[test]
fn html_escape_covers_the_special_characters() {
    assert_eq!(
        html_escape("<a href=\"x\">&"),
        "&lt;a href=&quot;x&quot;&gt;&amp;"
    );
    assert_eq!(html_escape("plain"), "plain");
}

// ---- title / URL cap ----

#[test]
fn page_title_is_capped_and_kept_on_char_boundaries() {
    let mut b = Browser::new();
    b.set_page_title(&"\u{e9}".repeat(100));
    assert!(b.page_title().len() <= 80);
    assert!(b.page_title().chars().all(|c| c == '\u{e9}'));
    b.set_page_title("");
    assert_eq!(b.page_title(), "");
}

#[test]
fn a_480_byte_url_is_kept_whole() {
    let mut b = Browser::new();
    let url = alloc::format!("http://h.test/{}", "a".repeat(URL_CAP - 14));
    b.open(url.as_bytes());
    assert_eq!(b.take_request(), Some(url.as_bytes()));
}

#[test]
fn a_form_target_resolves_against_the_page() {
    let mut b = browser_with_history(&["http://h.test/dir/page.html"]);
    assert!(b.open_link(b"/busca?q=a%C3%A7%C3%A3o&x=1"));
    assert_eq!(
        b.take_request(),
        Some(&b"http://h.test/busca?q=a%C3%A7%C3%A3o&x=1"[..])
    );
    b.loaded_with(Conn::Plain, false);
    assert!(b.open_link(b"?q=novo"));
    assert_eq!(b.take_request(), Some(&b"http://h.test/busca?q=novo"[..]));
}

#[test]
fn history_urls_iterate_oldest_first() {
    let b = browser_with_history(&["http://a.test/", "http://b.test/"]);
    let v: Vec<&[u8]> = b.history_urls().collect();
    assert_eq!(v, [&b"http://a.test/"[..], &b"http://b.test/"[..]]);
}

// ---- address bar selection ----

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

// ---- tabs share the window's stores ----

#[test]
fn a_sibling_shares_favourites_and_allowed_hosts_but_not_history() {
    let mut a = browser_with_history(&["http://a.test/x"]);
    a.toggle_bookmark();
    let mut b = a.sibling();
    assert!(b.is_home(), "a new tab starts on the start page");
    assert_eq!(b.bookmarks().len(), 1, "favourites are the window's");
    assert_eq!(b.history_len(), 0, "history is the tab's");
    // A favourite added in one tab shows in the other.
    b.open(b"http://b.test/y");
    b.take_request();
    b.loaded_with(Conn::Plain, false);
    b.toggle_bookmark();
    assert_eq!(a.bookmarks().len(), 2);
    // So does the one-site-one-session certificate override.
    a.open(b"https://expired.example.com/");
    a.take_request();
    a.fail_with(FailReason::Cert(crate::tlsverify::CertError::Expired));
    a.continue_insecure();
    b.open(b"https://expired.example.com/");
    assert_eq!(
        b.insecure_host().as_deref(),
        Some(&b"expired.example.com"[..])
    );
}

#[test]
fn stop_drops_a_pending_load_and_keeps_the_page() {
    let mut b = browser_with_history(&["http://a.test/x"]);
    b.open(b"http://b.test/y");
    assert!(b.is_loading());
    b.stop();
    assert!(!b.is_loading());
    assert!(b.take_request().is_none(), "nothing left to fetch");
    assert_eq!(b.history_len(), 1);
    b.stop(); // nothing to stop: harmless
}

#[test]
fn recent_lists_each_address_once_newest_first_without_internal_pages() {
    let mut b = browser_with_history(&["http://a.test/", "http://b.test/", "http://a.test/"]);
    b.open(b"osjeff://sobre");
    let r = b.recent(10);
    assert_eq!(r, ["http://a.test/", "http://b.test/"]);
    assert_eq!(b.recent(1), ["http://a.test/"]);
    assert!(Browser::new().recent(5).is_empty());
}

#[test]
fn reload_refetches_the_navigated_address_not_the_typed_text() {
    let mut b = Browser::new();
    b.open(b"https://example.com/a");
    assert!(b.take_request().is_some());
    type_str(&mut b, "typed junk");
    b.reload();
    assert_eq!(b.take_request(), Some(&b"https://example.com/a"[..]));
}

#[test]
fn the_request_asks_for_the_language_of_the_interface() {
    let ask = |l: Lang, host: &str, port: u16, tls: bool| {
        let mut r = Vec::new();
        build_get_request(&mut r, l, host, "/a?b=1", port, tls);
        String::from_utf8(r).unwrap()
    };
    let pt = ask(Lang::Pt, "example.com", 443, true);
    assert!(pt.starts_with("GET /a?b=1 HTTP/1.1\r\nHost: example.com\r\n"));
    assert!(pt.contains("\r\nAccept-Language: pt-BR,pt;q=0.9,en;q=0.8\r\n"));
    assert!(pt.ends_with("Connection: close\r\n\r\n"));
    let en = ask(Lang::En, "example.com", 80, false);
    assert!(en.contains("\r\nAccept-Language: en;q=1\r\n"));
    assert!(!en.contains("pt-BR"));
    // The port shows only when it is not the default of the scheme.
    assert!(ask(Lang::En, "h.test", 8080, false).contains("Host: h.test:8080\r\n"));
    assert!(ask(Lang::En, "h.test", 8080, true).contains("Host: h.test:8080\r\n"));
    assert!(ask(Lang::En, "h.test", 443, false).contains("Host: h.test:443\r\n"));
    assert!(ask(Lang::En, "h.test", 443, true).contains("Host: h.test\r\n"));
    // One header line each, in a fixed place.
    for l in Lang::ALL {
        let r = ask(l, "x.test", 80, false);
        assert_eq!(r.matches("Accept-Language:").count(), 1);
        assert_eq!(r.matches("\r\n").count(), 8);
    }
}

#[test]
fn labels_and_banners_have_both_languages() {
    for s in [
        Security::Http,
        Security::HttpsVerified,
        Security::HttpsInvalid,
    ] {
        let k = s.label_key().unwrap();
        assert_ne!(i18n::tr_in(Lang::Pt, k), i18n::tr_in(Lang::En, k));
    }
    for n in [
        PageNote::Truncated,
        PageNote::Incomplete,
        PageNote::Damaged,
        PageNote::BadChecksum,
    ] {
        for l in Lang::ALL {
            let t = i18n::tr_in(l, n.label_key());
            assert!(!t.is_empty() && !t.starts_with("web."), "{l:?} {n:?}");
        }
    }
    for (k, _) in QUICK_LINKS {
        assert!(!i18n::tr_in(Lang::En, k).starts_with("web."), "{k}");
    }
    assert_eq!(i18n::tr_in(Lang::Pt, QUICK_LINKS[3].0), "Exemplo");
    assert_eq!(i18n::tr_in(Lang::En, QUICK_LINKS[3].0), "Example");
}

#[test]
fn an_internal_page_can_be_built_again_in_another_language() {
    let mut b = Browser::new();
    assert!(b.internal_html_in(Lang::En).is_none());
    b.open(b"osjeff://sobre");
    let first = b.take_internal().unwrap();
    assert!(b.take_internal().is_none(), "delivered once");
    let en = String::from_utf8(b.internal_html_in(Lang::En).unwrap()).unwrap();
    let pt = String::from_utf8(b.internal_html_in(Lang::Pt).unwrap()).unwrap();
    assert!(en.contains("<html lang=\"en\">") && en.contains("Shortcuts"));
    assert!(pt.contains("<html lang=\"pt-BR\">") && pt.contains("Atalhos"));
    assert_eq!(first, pt.into_bytes(), "the default language is Portuguese");
}
