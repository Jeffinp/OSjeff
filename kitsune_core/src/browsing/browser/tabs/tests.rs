use super::*;

fn list(n: usize) -> TabList<usize> {
    let mut l = TabList::new(0);
    for i in 1..n {
        l.open(i);
        l.select(l.len() - 1);
    }
    l.select(0);
    l
}

#[test]
fn a_new_list_has_one_active_tab() {
    let l = TabList::new("a");
    assert_eq!((l.len(), l.active_index(), *l.active()), (1, 0, "a"));
    assert!(!l.is_empty());
}

#[test]
fn open_inserts_after_the_active_tab_and_activates_it() {
    let mut l = TabList::new(0);
    assert_eq!(l.open(1), Some(1));
    assert_eq!(l.open(2), Some(2));
    l.select(0);
    assert_eq!(l.open(9), Some(1));
    let order: Vec<_> = l.iter().copied().collect();
    assert_eq!(order, [0, 9, 1, 2]);
    assert_eq!(*l.active(), 9);
}

#[test]
fn at_most_eight_tabs() {
    let mut l = list(MAX_TABS);
    assert_eq!(l.len(), MAX_TABS);
    assert!(!l.can_open());
    assert_eq!(l.open(99), None);
    assert_eq!(l.len(), MAX_TABS);
}

#[test]
fn closing_the_active_tab_activates_its_right_neighbour_or_the_left() {
    let mut l = list(4);
    l.select(1);
    assert_eq!(l.close(1), Some(1));
    assert_eq!(*l.active(), 2, "the tab that slid in");
    l.select(2);
    assert_eq!(l.close(2), Some(3));
    assert_eq!(*l.active(), 2, "the last tab closed: the new last");
    assert_eq!(l.len(), 2);
}

#[test]
fn closing_before_or_after_the_active_tab_keeps_it() {
    let mut l = list(4);
    l.select(2);
    l.close(0);
    assert_eq!(*l.active(), 2);
    assert_eq!(l.active_index(), 1);
    l.close(2);
    assert_eq!(*l.active(), 2);
}

#[test]
fn the_last_tab_cannot_be_closed_and_bad_indices_are_ignored() {
    let mut l = list(1);
    assert_eq!(l.close(0), None);
    let mut l = list(3);
    assert_eq!(l.close(7), None);
    assert_eq!(l.len(), 3);
}

#[test]
fn next_prev_wrap_and_numbers_pick_tabs() {
    let mut l = list(3);
    l.next();
    l.next();
    assert_eq!(l.active_index(), 2);
    l.next();
    assert_eq!(l.active_index(), 0);
    l.prev();
    assert_eq!(l.active_index(), 2);
    assert!(l.select_number(1));
    assert_eq!(l.active_index(), 0);
    assert!(l.select_number(3));
    assert_eq!(l.active_index(), 2);
    assert!(!l.select_number(5));
    assert!(!l.select_number(0));
    assert!(l.select_number(9), "9 is the last tab");
    assert_eq!(l.active_index(), 2);
}

#[test]
fn tabs_can_be_reordered() {
    let mut l = list(3);
    assert!(!l.move_active(false));
    assert!(l.move_active(true));
    let order: Vec<_> = l.iter().copied().collect();
    assert_eq!(order, [1, 0, 2]);
    assert_eq!(l.active_index(), 1);
    l.select(2);
    assert!(!l.move_active(true));
}

#[test]
fn titles_fall_back_to_the_host_and_then_to_new_tab() {
    let pt = |t: &str, u: &str| tab_title_in(Lang::Pt, t, u);
    let en = |t: &str, u: &str| tab_title_in(Lang::En, t, u);
    assert_eq!(pt("  Olá  ", "http://a.test/x"), "Olá");
    assert_eq!(pt("", "https://www.exemplo.com.br/a?b#c"), "exemplo.com.br");
    assert_eq!(pt("", "203.0.113.5:8079/x"), "203.0.113.5:8079");
    assert_eq!(pt("", ""), "Nova aba");
    assert_eq!(pt("", "kitsune://favoritos"), "Favoritos");
    assert_eq!(pt("", "kitsune://historico/"), "Histórico");
    assert_eq!(pt("", "kitsune://sobre"), "Sobre o Navegador");
    assert_eq!(pt("", "kitsune://inicio"), "Nova aba");
    assert_eq!(pt("", "osjeff://historico/"), "Histórico");
    // The same tabs in English; a page's own title and a host never change.
    assert_eq!(en("", ""), "New tab");
    assert_eq!(en("", "kitsune://favoritos"), "Bookmarks");
    assert_eq!(en("", "kitsune://historico/"), "History");
    assert_eq!(en("", "kitsune://sobre"), "About the Browser");
    assert_eq!(en("", "kitsune://inicio"), "New tab");
    assert_eq!(en("  Olá  ", "http://a.test/x"), "Olá");
    assert_eq!(en("", "https://www.exemplo.com.br/a"), "exemplo.com.br");
}

#[test]
fn badges_use_the_host_letter() {
    assert_eq!(tab_badge("", "https://www.exemplo.com"), 'E');
    assert_eq!(tab_badge("x", "http://123.test/"), 'T');
    assert_eq!(tab_badge("Tipografia", "http://203.0.113.5:8079/"), 'T');
    assert_eq!(tab_badge("", "http://203.0.113.5:8079/"), '2');
    assert_eq!(tab_badge("Título", ""), 'T');
    assert_eq!(tab_badge("", ""), '\u{2022}');
    assert_eq!(tab_badge("", "https://émile.test/"), 'É');
    assert_eq!(tab_badge("   ", "://"), '\u{2022}');
}

#[test]
fn host_range_marks_the_host_of_an_address() {
    let u = "https://www.exemplo.com/a/b?c=1";
    let (a, b) = host_range(u);
    assert_eq!(&u[a..b], "www.exemplo.com");
    let u = "203.0.113.5:8079/x";
    let (a, b) = host_range(u);
    assert_eq!(&u[a..b], "203.0.113.5:8079");
    let u = "http://a.test";
    let (a, b) = host_range(u);
    assert_eq!((&u[a..b], b), ("a.test", u.len()));
    assert_eq!(host_range(""), (0, 0));
    let u = "kitsune://favoritos";
    assert_eq!(&u[host_range(u).0..host_range(u).1], "kitsune://favoritos");
    let u = "osjeff://favoritos?rm=1";
    assert_eq!(&u[host_range(u).0..host_range(u).1], "osjeff://favoritos");
}

#[test]
fn own_pages_wear_the_brand_badge() {
    assert_eq!(tab_badge("", "kitsune://sobre"), BRAND_BADGE);
    assert_eq!(tab_badge("x", " osjeff://favoritos "), BRAND_BADGE);
    assert_eq!(tab_badge("", "https://example.com"), 'E');
}

#[test]
fn host_extraction_is_total() {
    for u in [
        "",
        "://",
        "http://",
        "//",
        "a",
        "http://a b/c",
        "\u{fffd}://\u{fffd}",
    ] {
        let (a, b) = host_range(u);
        assert!(a <= b && b <= u.len() && u.is_char_boundary(a) && u.is_char_boundary(b));
        let _ = host_of(u);
        let _ = tab_title("", u);
        let _ = tab_title_in(Lang::En, "", u);
        let _ = tab_badge("", u);
    }
}
