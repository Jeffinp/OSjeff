use super::*;

#[test]
fn internal_pages_load_without_the_network() {
    let mut b = Browser::new();
    let html = open_internal(&mut b, "kitsune://sobre");
    assert!(html.contains("Sobre o Navegador"));
    assert!(b.take_request().is_none(), "nothing for the fetcher");
    assert_eq!(b.status(), Status::Done);
    assert!(b.is_internal());
    assert!(!b.is_home());
    assert_eq!(b.security(), Security::None);
    assert_eq!(b.url(), b"kitsune://sobre");
    assert!(b.take_internal().is_none(), "delivered once");
}

#[test]
fn typed_kitsune_url_is_not_a_search() {
    let mut b = Browser::new();
    type_str(&mut b, "kitsune://favoritos");
    b.on_key(Key::Enter);
    assert!(b.take_request().is_none());
    assert!(b.take_internal().is_some());
}

#[test]
fn inicio_is_the_start_page() {
    let mut b = browser_with_history(&["http://a.test/"]);
    b.open(b"kitsune://inicio");
    assert!(b.is_home());
    assert!(!b.is_internal());
    assert!(b.take_internal().is_none());
}

#[test]
fn favourites_page_lists_and_removes() {
    let mut b = browser_with_history(&["http://a.test/x?a=1&b=2"]);
    b.set_page_title("A <b>& B");
    b.toggle_bookmark();
    let html = open_internal(&mut b, "kitsune://favoritos");
    assert!(html.contains("A &lt;b&gt;&amp; B"), "{html}");
    assert!(html.contains("http://a.test/x?a=1&amp;b=2"));
    assert!(html.contains("kitsune://favoritos?rm=0"));
    // The remove link.
    let html = open_internal(&mut b, "kitsune://favoritos?rm=0");
    assert!(html.contains("Nenhum favorito"));
    assert!(b.bookmarks().is_empty());
    assert_eq!(b.url(), b"kitsune://favoritos");
}

#[test]
fn removing_a_missing_favourite_is_harmless() {
    let mut b = Browser::new();
    let html = open_internal(&mut b, "kitsune://favoritos?rm=7");
    assert!(html.contains("Nenhum favorito"));
    let html = open_internal(&mut b, "kitsune://favoritos?rm=abc");
    assert!(html.contains("Nenhum favorito"));
}

#[test]
fn history_page_lists_newest_first_and_skips_internal_pages() {
    let mut b = browser_with_history(&["http://one.test/", "http://two.test/"]);
    let html = open_internal(&mut b, "kitsune://historico");
    let one = html.find("http://one.test/").unwrap();
    let two = html.find("http://two.test/").unwrap();
    assert!(two < one);
    assert!(!html.contains(">kitsune://historico<"));
}

#[test]
fn history_page_is_empty_message() {
    let mut b = Browser::new();
    let html = open_internal(&mut b, "kitsune://historico");
    assert!(html.contains("Nada visitado"));
}

#[test]
fn unknown_internal_name_shows_the_about_page() {
    let mut b = Browser::new();
    let html = open_internal(&mut b, "kitsune://nada");
    assert!(html.contains("kitsune://favoritos"));
}

#[test]
fn internal_scheme_is_case_insensitive() {
    let mut b = Browser::new();
    assert!(open_internal(&mut b, "KITSUNE://SOBRE").contains("Navegador"));
}

#[test]
fn old_osjeff_scheme_is_redirected_to_kitsune() {
    let mut b = Browser::new();
    let html = open_internal(&mut b, "osjeff://sobre");
    assert!(html.contains("Sobre o Navegador"));
    assert_eq!(b.url(), b"kitsune://sobre", "shown with the new scheme");
    assert!(b.take_request().is_none(), "never goes to the network");
    assert!(open_internal(&mut b, "OSJEFF://FAVORITOS").contains("Favoritos"));
    assert_eq!(b.url(), b"kitsune://favoritos");
    // A link with the old scheme on one of the pages works too.
    assert!(b.open_link(b"osjeff://historico"));
    assert_eq!(b.url(), b"kitsune://historico");
}

#[test]
fn internal_pages_enter_the_history_and_back_replays_them() {
    let mut b = browser_with_history(&["http://a.test/"]);
    open_internal(&mut b, "kitsune://sobre");
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
    assert_eq!(b.url(), b"kitsune://sobre");
}

#[test]
fn links_inside_internal_pages_navigate() {
    let mut b = browser_with_history(&["http://a.test/"]);
    open_internal(&mut b, "kitsune://sobre");
    assert!(b.open_link(b"kitsune://favoritos"));
    assert!(b.take_internal().is_some());
    // An absolute http link from an internal page is not an https downgrade.
    assert!(b.open_link(b"http://a.test/x"));
    assert_eq!(b.take_request(), Some(&b"http://a.test/x"[..]));
    assert!(!b.is_internal());
}

#[test]
fn reload_of_an_internal_page_regenerates_it() {
    let mut b = Browser::new();
    open_internal(&mut b, "kitsune://historico");
    b.reload();
    assert!(b.take_internal().is_some());
    assert!(b.take_request().is_none());
}

#[test]
fn network_navigation_clears_the_internal_flag() {
    let mut b = Browser::new();
    open_internal(&mut b, "kitsune://sobre");
    b.open(b"http://a.test/");
    assert!(!b.is_internal());
    assert!(b.take_request().is_some());
}

#[test]
fn internal_html_is_escaped() {
    let mut b = browser_with_history(&["http://a.test/\"onmouseover=\"x"]);
    let html = open_internal(&mut b, "kitsune://historico");
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
