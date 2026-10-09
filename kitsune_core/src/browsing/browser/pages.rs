//! The browser's own pages (`kitsune://favoritos`, `historico`, `sobre`) as HTML + CSS that goes
//! through the same engine as any site. They are always light: the page area is never
//! dark-inverted, only the chrome follows the system appearance.
//!
//! Their words come from the catalog (`web.page.*`, `web.key.*`) in the language asked for
//! (`*_in`) and `<html lang>` says which; the kernel builds them again when the language
//! changes.

use alloc::string::String;
use alloc::vec::Vec;

use super::tabs::{host_of, tab_badge, tab_title_in};
use super::{Bookmark, html_escape};
use crate::i18n::{self, Lang, tr_in};
use crate::tk;

/// How many history rows the page shows.
const HISTORY_ROWS: usize = 200;

const CSS: &str = "\
body{margin:0;background:#ffffff;color:#1d1d1f;font-size:15px;line-height:1.5}\
.w{max-width:680px;margin:0 auto;padding:28px 24px 40px 24px}\
.nav{margin:0 0 20px 0}\
.nav a{color:#6e6e73;padding:5px 12px;border-radius:14px;margin:0 4px 0 0}\
.nav a.on{background:#eceaff;color:#4f46e5;font-weight:600}\
h1{font-size:28px;line-height:1.2;margin:8px 0 4px 0}\
h2{font-size:17px;margin:26px 0 8px 0}\
.sub{color:#6e6e73;margin:0 0 20px 0}\
a{color:#4f46e5}\
table.rows{width:100%;border-collapse:collapse}\
table.rows td{padding:10px 0;border-top:1px solid #e8e8ed;vertical-align:middle}\
td.ti{width:54px}\
div.tile{width:40px;margin-right:14px;min-height:40px;line-height:40px;text-align:center;font-weight:700;color:#ffffff;background:#6366f1;border-radius:10px}\
.t{font-weight:600}\
.u{color:#6e6e73;font-size:13px}\
.rm{color:#6e6e73;font-size:13px}\
.empty{color:#6e6e73;padding:24px 0}\
table.keys td{padding:5px 0;border-top:1px solid #f0f0f4}\
td.k{width:230px}\
code{background:#f0f0f5;border-radius:5px;padding:1px 6px;font-size:13px}\
.note{background:#f5f5fa;border-radius:12px;padding:12px 16px;margin:20px 0 0 0;color:#3a3a3f}";

/// Header and the row of links to the other pages; `on` marks the one shown.
fn open(lang: Lang, title: &str, on: &str, heading: &str, sub: &str) -> String {
    let mut h = alloc::format!(
        "<html lang=\"{}\"><head><meta charset=\"utf-8\"><title>",
        lang.code()
    );
    h.push_str(&html_escape(title));
    h.push_str("</title><style>");
    h.push_str(CSS);
    h.push_str("</style></head><body><div class=\"w\"><div class=\"nav\">");
    for (name, key) in [
        ("inicio", tk!("web.page.home")),
        ("favoritos", tk!("web.page.bookmarks")),
        ("historico", tk!("web.page.history")),
        ("sobre", tk!("web.page.about")),
    ] {
        let cls = if name == on { " class=\"on\"" } else { "" };
        h.push_str(&alloc::format!(
            "<a{cls} href=\"kitsune://{name}\">{}</a>",
            html_escape(tr_in(lang, key))
        ));
    }
    h.push_str("</div><h1>");
    h.push_str(&html_escape(heading));
    h.push_str("</h1>");
    if !sub.is_empty() {
        h.push_str("<p class=\"sub\">");
        h.push_str(&html_escape(sub));
        h.push_str("</p>");
    }
    h
}

fn close(mut h: String) -> Vec<u8> {
    h.push_str("</div></body></html>");
    h.into_bytes()
}

/// `kitsune://favoritos`: every favourite with a letter tile and a remove link.
pub fn bookmarks(all: &[Bookmark]) -> Vec<u8> {
    bookmarks_in(i18n::lang(), all)
}

/// [`bookmarks`] in `lang`.
pub fn bookmarks_in(lang: Lang, all: &[Bookmark]) -> Vec<u8> {
    let sub = if all.is_empty() {
        String::new()
    } else {
        i18n::plural_fmt_in(lang, "web.page.bookmarks_count", all.len() as u64, &[])
    };
    let title = tr_in(lang, tk!("web.page.bookmarks"));
    let mut h = open(lang, title, "favoritos", title, &sub);
    if all.is_empty() {
        h.push_str("<p class=\"empty\">");
        h.push_str(&html_escape(tr_in(lang, tk!("web.page.bookmarks_empty"))));
        h.push_str("</p>");
        return close(h);
    }
    h.push_str("<table class=\"rows\">");
    for (i, b) in all.iter().enumerate() {
        let title = tab_title_in(lang, &b.title, &b.url);
        h.push_str(&alloc::format!(
            "<tr><td class=\"ti\"><div class=\"tile\">{badge}</div></td>\
             <td><a class=\"t\" href=\"{u}\">{t}</a><br><span class=\"u\">{u}</span></td>\
             <td align=\"right\"><a class=\"rm\" href=\"kitsune://favoritos?rm={i}\">{rm}</a></td></tr>",
            rm = html_escape(tr_in(lang, tk!("web.page.remove"))),
            badge = tab_badge(&b.title, &b.url),
            u = html_escape(&b.url),
            t = html_escape(&title),
        ));
    }
    h.push_str("</table>");
    close(h)
}

/// `kitsune://historico`: addresses visited, newest first, each once, without the browser's
/// own pages. `urls` is in visiting order.
pub fn history(urls: &[Vec<u8>]) -> Vec<u8> {
    history_in(i18n::lang(), urls)
}

/// [`history`] in `lang`.
pub fn history_in(lang: Lang, urls: &[Vec<u8>]) -> Vec<u8> {
    let mut seen: Vec<String> = Vec::new();
    for u in urls.iter().rev() {
        let u = String::from_utf8_lossy(u).into_owned();
        if super::is_internal_url(&u) || seen.contains(&u) {
            continue;
        }
        seen.push(u);
        if seen.len() >= HISTORY_ROWS {
            break;
        }
    }
    let sub = if seen.is_empty() {
        String::new()
    } else {
        i18n::plural_fmt_in(lang, "web.page.history_count", seen.len() as u64, &[])
    };
    let title = tr_in(lang, tk!("web.page.history"));
    let mut h = open(lang, title, "historico", title, &sub);
    if seen.is_empty() {
        h.push_str("<p class=\"empty\">");
        h.push_str(&html_escape(tr_in(lang, tk!("web.page.history_empty"))));
        h.push_str("</p>");
        return close(h);
    }
    h.push_str("<table class=\"rows\">");
    for u in &seen {
        h.push_str(&alloc::format!(
            "<tr><td><a class=\"t\" href=\"{u}\">{host}</a><br><span class=\"u\">{u}</span></td></tr>",
            u = html_escape(u),
            host = html_escape(host_of(u)),
        ));
    }
    h.push_str("</table>");
    close(h)
}

/// `kitsune://sobre`: what the browser does and the shortcuts it answers to.
pub fn about() -> Vec<u8> {
    about_in(i18n::lang())
}

/// [`about`] in `lang`.
pub fn about_in(lang: Lang) -> Vec<u8> {
    let mut h = open(
        lang,
        tr_in(lang, tk!("web.tab.about")),
        "sobre",
        tr_in(lang, tk!("app.browser")),
        tr_in(lang, tk!("web.page.about_sub")),
    );
    h.push_str("<p>");
    h.push_str(&html_escape(tr_in(lang, tk!("web.page.about_body"))));
    h.push_str("</p><h2>");
    h.push_str(&html_escape(tr_in(lang, tk!("web.page.shortcuts"))));
    h.push_str("</h2><table class=\"rows keys\">");
    for (k, what) in [
        ("Ctrl+T", tk!("web.key.new_tab")),
        ("Ctrl+W", tk!("web.key.close_tab")),
        ("Ctrl+Tab", tk!("web.key.next_tab")),
        (tk!("web.key.goto_tab_keys"), tk!("web.key.goto_tab")),
        ("Ctrl+L", tk!("web.key.address")),
        ("Ctrl+D", tk!("web.key.bookmark")),
        ("Ctrl+F", tk!("web.key.find")),
        ("Ctrl++ / Ctrl+- / Ctrl+0", tk!("web.key.zoom")),
        ("Alt+\u{2190} / Alt+\u{2192}", tk!("web.key.history")),
        ("Ctrl+R", tk!("web.key.reload")),
        ("Esc", tk!("web.key.stop")),
        (tk!("web.key.scroll_keys"), tk!("web.key.scroll")),
    ] {
        // A key that is itself a catalog key (`web.key.*`) is a phrase; the rest are keystrokes.
        let k = if k.starts_with("web.") {
            tr_in(lang, k)
        } else {
            k
        };
        h.push_str(&alloc::format!(
            "<tr><td class=\"k\"><code>{}</code></td><td>{}</td></tr>",
            html_escape(k),
            html_escape(tr_in(lang, what))
        ));
    }
    h.push_str("</table>");
    h.push_str("<p class=\"note\">");
    h.push_str(&html_escape(tr_in(lang, tk!("web.page.note"))));
    h.push_str("</p>");
    close(h)
}

#[cfg(test)]
mod tests {
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
}
