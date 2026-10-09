use super::*;

fn text(b: Vec<u8>) -> String {
    String::from_utf8(b).unwrap()
}

#[test]
fn about_names_neither_language_nor_toolchain() {
    for l in Lang::ALL {
        let h = text(about_in(l)).to_lowercase();
        for w in ["rust", "cargo", "llvm", "nightly", "crate", "no_std"] {
            assert!(!h.contains(w), "{w}");
        }
    }
    assert!(text(about_in(Lang::Pt)).contains("Atalhos"));
    assert!(text(about_in(Lang::En)).contains("Shortcuts"));
}

#[test]
fn pages_say_their_language_and_use_it_everywhere() {
    let pt = text(about_in(Lang::Pt));
    let en = text(about_in(Lang::En));
    assert!(pt.starts_with("<html lang=\"pt-BR\">"), "{pt}");
    assert!(en.starts_with("<html lang=\"en\">"), "{en}");
    // Portuguese has its accents, English has no Portuguese in it.
    for w in ["Início", "Histórico", "Próxima aba", "Endereço"] {
        assert!(pt.contains(w), "{w}");
    }
    assert!(en.contains("Next tab") && en.contains("Ctrl+1 to 9"));
    assert!(!en.contains("Próxima") && !en.contains("Atalhos"));
    assert!(pt.contains("Ctrl+1 a 9") && pt.contains("Espaço, PgDn"));
    // No missing key shows up as raw text.
    assert!(!pt.contains("web.") && !en.contains("web."));
}

#[test]
fn bookmark_and_history_pages_count_with_the_plural_of_the_language() {
    let one = [Bookmark {
        url: String::from("http://a.test/"),
        title: String::new(),
    }];
    let two = [
        one[0].clone(),
        Bookmark {
            url: String::from("http://b.test/"),
            title: String::from("B"),
        },
    ];
    assert!(text(bookmarks_in(Lang::Pt, &one)).contains("1 favorito<"));
    assert!(text(bookmarks_in(Lang::Pt, &two)).contains("2 favoritos<"));
    assert!(text(bookmarks_in(Lang::En, &one)).contains("1 bookmark<"));
    assert!(text(bookmarks_in(Lang::En, &two)).contains("2 bookmarks<"));
    assert!(text(bookmarks_in(Lang::En, &[])).contains("No bookmarks yet"));
    assert!(text(bookmarks_in(Lang::Pt, &[])).contains("Nenhum favorito ainda"));
    assert!(text(bookmarks_in(Lang::En, &one)).contains(">Remove<"));
    assert!(text(bookmarks_in(Lang::Pt, &one)).contains(">Remover<"));
    let urls: Vec<Vec<u8>> = ["http://one.test/", "http://two.test/"]
        .iter()
        .map(|s| s.as_bytes().to_vec())
        .collect();
    assert!(text(history_in(Lang::En, &urls)).contains("2 pages<"));
    assert!(text(history_in(Lang::Pt, &urls)).contains("2 páginas<"));
    assert!(text(history_in(Lang::En, &[])).contains("Nothing visited yet."));
    assert!(text(history_in(Lang::Pt, &[])).contains("Nada visitado ainda."));
    assert!(text(history_in(Lang::En, &urls)).contains("<title>History</title>"));
    assert!(text(history_in(Lang::Pt, &urls)).contains("<title>Histórico</title>"));
}

#[test]
fn pages_escape_what_they_show() {
    let all = [Bookmark {
        url: String::from("http://a.test/?x=1&y=<2>"),
        title: String::from("A <b>"),
    }];
    let h = text(bookmarks(&all));
    assert!(h.contains("A &lt;b&gt;"));
    assert!(h.contains("x=1&amp;y=&lt;2&gt;"));
    assert!(!h.contains("<b>"));
}

#[test]
fn history_is_newest_first_without_repeats_or_internal_pages() {
    let urls: Vec<Vec<u8>> = [
        "http://one.test/",
        "kitsune://sobre",
        "http://two.test/",
        "http://one.test/",
    ]
    .iter()
    .map(|s| s.as_bytes().to_vec())
    .collect();
    let h = text(history(&urls));
    let one = h.find(">http://one.test/<").unwrap();
    let two = h.find(">http://two.test/<").unwrap();
    assert!(one < two);
    assert_eq!(h.matches(">http://one.test/<").count(), 1);
    assert!(!h.contains(">kitsune://sobre<"));
}

#[test]
fn history_rows_are_capped() {
    let urls: Vec<Vec<u8>> = (0..HISTORY_ROWS + 50)
        .map(|i| alloc::format!("http://h{i}.test/").into_bytes())
        .collect();
    let h = text(history(&urls));
    assert_eq!(h.matches("<tr>").count(), HISTORY_ROWS);
}
