//! The browser's own pages (`osjeff://favoritos`, `historico`, `sobre`) as HTML + CSS that goes
//! through the same engine as any site. They are always light: the page area is never
//! dark-inverted, only the chrome follows the system appearance.

use alloc::string::String;
use alloc::vec::Vec;

use super::tabs::{host_of, tab_badge, tab_title};
use super::{Bookmark, html_escape};

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
fn open(title: &str, on: &str, heading: &str, sub: &str) -> String {
    let mut h = String::from("<html><head><meta charset=\"utf-8\"><title>");
    h.push_str(&html_escape(title));
    h.push_str("</title><style>");
    h.push_str(CSS);
    h.push_str("</style></head><body><div class=\"w\"><div class=\"nav\">");
    for (name, label) in [
        ("inicio", "Início"),
        ("favoritos", "Favoritos"),
        ("historico", "Histórico"),
        ("sobre", "Sobre"),
    ] {
        let cls = if name == on { " class=\"on\"" } else { "" };
        h.push_str(&alloc::format!(
            "<a{cls} href=\"osjeff://{name}\">{label}</a>"
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

/// `osjeff://favoritos`: every favourite with a letter tile and a remove link.
pub fn bookmarks(all: &[Bookmark]) -> Vec<u8> {
    let sub = match all.len() {
        0 => "",
        1 => "1 favorito",
        n => &alloc::format!("{n} favoritos"),
    };
    let mut h = open("Favoritos", "favoritos", "Favoritos", sub);
    if all.is_empty() {
        h.push_str("<p class=\"empty\">Nenhum favorito ainda. Em uma página, use Ctrl+D ou a estrela da barra.</p>");
        return close(h);
    }
    h.push_str("<table class=\"rows\">");
    for (i, b) in all.iter().enumerate() {
        let title = tab_title(&b.title, &b.url);
        h.push_str(&alloc::format!(
            "<tr><td class=\"ti\"><div class=\"tile\">{badge}</div></td>\
             <td><a class=\"t\" href=\"{u}\">{t}</a><br><span class=\"u\">{u}</span></td>\
             <td align=\"right\"><a class=\"rm\" href=\"osjeff://favoritos?rm={i}\">Remover</a></td></tr>",
            badge = tab_badge(&b.title, &b.url),
            u = html_escape(&b.url),
            t = html_escape(&title),
        ));
    }
    h.push_str("</table>");
    close(h)
}

/// `osjeff://historico`: addresses visited, newest first, each once, without the browser's
/// own pages. `urls` is in visiting order.
pub fn history(urls: &[Vec<u8>]) -> Vec<u8> {
    let mut seen: Vec<String> = Vec::new();
    for u in urls.iter().rev() {
        let u = String::from_utf8_lossy(u).into_owned();
        if u.starts_with("osjeff://") || seen.contains(&u) {
            continue;
        }
        seen.push(u);
        if seen.len() >= HISTORY_ROWS {
            break;
        }
    }
    let sub = match seen.len() {
        0 => String::new(),
        1 => String::from("1 página"),
        n => alloc::format!("{n} páginas"),
    };
    let mut h = open("Histórico", "historico", "Histórico", &sub);
    if seen.is_empty() {
        h.push_str("<p class=\"empty\">Nada visitado ainda.</p>");
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

/// `osjeff://sobre`: what the browser does and the shortcuts it answers to.
pub fn about() -> Vec<u8> {
    let mut h = open(
        "Sobre o Navegador",
        "sobre",
        "Navegador",
        "O navegador do OSjeff.",
    );
    h.push_str(
        "<p>Páginas HTML e CSS com tabelas, listas, imagens e formulários. Sem scripts. \
         HTTPS com TLS 1.3: o cadeado só aparece quando o certificado foi verificado.</p>",
    );
    h.push_str("<h2>Atalhos</h2><table class=\"rows keys\">");
    for (k, what) in [
        ("Ctrl+T", "Nova aba"),
        ("Ctrl+W", "Fechar aba"),
        ("Ctrl+Tab", "Próxima aba"),
        ("Ctrl+1 a 9", "Ir para a aba (9 é a última)"),
        ("Ctrl+L", "Endereço"),
        ("Ctrl+D", "Adicionar aos favoritos"),
        ("Ctrl+F", "Buscar na página"),
        ("Ctrl++ / Ctrl+- / Ctrl+0", "Zoom"),
        ("Alt+← / Alt+→", "Voltar e avançar"),
        ("Ctrl+R", "Recarregar"),
        ("Esc", "Parar o carregamento"),
        ("Espaço, PgDn, Home, End", "Rolar"),
    ] {
        h.push_str(&alloc::format!(
            "<tr><td class=\"k\"><code>{}</code></td><td>{}</td></tr>",
            html_escape(k),
            html_escape(what)
        ));
    }
    h.push_str("</table>");
    h.push_str(
        "<p class=\"note\">Fontes de outros alfabetos, emojis e escrita da direita para a \
         esquerda aparecem como quadros vazios.</p>",
    );
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
        let h = text(about()).to_lowercase();
        for w in ["rust", "cargo", "llvm", "nightly", "crate", "no_std"] {
            assert!(!h.contains(w), "{w}");
        }
        assert!(h.contains("atalhos"));
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
            "osjeff://sobre",
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
        assert!(!h.contains(">osjeff://sobre<"));
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
