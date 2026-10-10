//! internal (split out of `browser.rs`).

use super::*;

/// Whether `input` names one of the browser's own pages (current or old scheme,
/// any letter case).
pub(super) fn is_internal_input(input: &[u8]) -> bool {
    starts_with_ci(input, INTERNAL_SCHEME) || starts_with_ci(input, LEGACY_INTERNAL_SCHEME)
}

/// Length of the `kitsune://` (or old `osjeff://`) prefix of `url`, if it has one.
pub fn internal_prefix_len(url: &str) -> Option<usize> {
    ["kitsune://", "osjeff://"]
        .into_iter()
        .find(|p| url.starts_with(p))
        .map(str::len)
}

/// Whether `url` is an address of the browser's own pages (current or old scheme).
pub fn is_internal_url(url: &str) -> bool {
    internal_prefix_len(url).is_some()
}

/// The text form of a favourites list: one `url<TAB>title` line each (tabs and
/// newlines inside a title become spaces).
pub fn bookmarks_to_text(items: &[Bookmark]) -> String {
    let mut out = String::new();
    for b in items {
        let clean = |s: &str| -> String {
            s.chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect()
        };
        out.push_str(&clean(&b.url));
        out.push('\t');
        out.push_str(&clean(&b.title));
        out.push('\n');
    }
    out
}

/// Parse [`bookmarks_to_text`] output. Total: bad UTF-8, lines without a URL and
/// duplicates are skipped, at most [`MAX_BOOKMARKS`] are kept.
pub fn bookmarks_from_text(text: &[u8]) -> Vec<Bookmark> {
    let mut items: Vec<Bookmark> = Vec::new();
    for line in text.split(|&b| b == b'\n') {
        let Ok(line) = core::str::from_utf8(line) else {
            continue;
        };
        let (url, title) = line.split_once('\t').unwrap_or((line, ""));
        let url = url.trim();
        if url.is_empty() || items.iter().any(|b| b.url == url) {
            continue;
        }
        if items.len() >= MAX_BOOKMARKS {
            break;
        }
        items.push(Bookmark {
            url: String::from(url),
            title: String::from(title.trim()),
        });
    }
    items
}

impl<F: FnMut(&[u8])> SavedBookmarks<F> {
    /// Start from the saved `text` (as read from the file, possibly empty or damaged).
    pub fn load(text: &[u8], save: F) -> Self {
        let mut inner = MemoryBookmarks::default();
        for b in bookmarks_from_text(text) {
            inner.add(b);
        }
        Self { inner, save }
    }

    pub(super) fn flush(&mut self) {
        let text = bookmarks_to_text(&self.inner.items);
        (self.save)(text.as_bytes());
    }
}

/// An address without its scheme and a leading `www.`, lower-cased, for matching.
pub(super) fn bare(url: &str) -> String {
    let l = url.trim().to_ascii_lowercase();
    let l = l
        .strip_prefix("https://")
        .or_else(|| l.strip_prefix("http://"))
        .unwrap_or(&l);
    l.strip_prefix("www.").unwrap_or(l).into()
}

/// Rank `url`/`title` against the query: 0 = prefix of the address, 1 = contained in the
/// address or the title, `None` = no match.
pub(super) fn rank(q: &str, url: &str, title: &str) -> Option<u8> {
    if bare(url).starts_with(q) {
        Some(0)
    } else if bare(url).contains(q) || title.to_ascii_lowercase().contains(q) {
        Some(1)
    } else {
        None
    }
}

/// Build the suggestion list for `query` from favourites and history (newest first).
pub fn suggest(query: &str, bookmarks: &[Bookmark], history: &[String]) -> Vec<Suggestion> {
    let q = bare(query);
    if q.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<Suggestion> = Vec::new();
    for want in [0u8, 1] {
        for b in bookmarks {
            if rank(&q, &b.url, &b.title) == Some(want) && !out.iter().any(|s| s.url == b.url) {
                out.push(Suggestion {
                    url: b.url.clone(),
                    label: if b.title.is_empty() {
                        b.url.clone()
                    } else {
                        b.title.clone()
                    },
                    bookmark: true,
                });
            }
        }
        for u in history {
            if rank(&q, u, "") == Some(want) && !out.iter().any(|s| s.url == *u) {
                out.push(Suggestion {
                    url: u.clone(),
                    label: u.clone(),
                    bookmark: false,
                });
            }
        }
    }
    // A single suggestion that is exactly what was typed adds nothing.
    if out.len() == 1 && bare(&out[0].url) == q {
        out.clear();
    }
    out.truncate(MAX_SUGGESTIONS);
    out
}

/// Escape `&`, `<`, `>`, `"` for HTML text and attribute values.
pub fn html_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}
