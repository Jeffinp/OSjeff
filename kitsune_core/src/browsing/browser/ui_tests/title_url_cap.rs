use super::*;

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
