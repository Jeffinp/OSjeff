//! The tabs of a browser window: which one is active, opening, closing and switching,
//! and the words and letter a tab shows. Pure: the kernel keeps the page state of each
//! tab in the `T` it stores here.

use crate::i18n::{self, Lang};
use crate::tk;
use alloc::string::String;
use alloc::vec::Vec;

/// Most tabs one window holds.
pub const MAX_TABS: usize = 8;

/// An ordered list of tabs with one active (never empty).
#[derive(Debug)]
pub struct TabList<T> {
    items: Vec<T>,
    active: usize,
}

impl<T> TabList<T> {
    /// A list with its first tab, active.
    pub fn new(first: T) -> Self {
        TabList {
            items: alloc::vec![first],
            active: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Never true: a window always has a tab.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active(&self) -> &T {
        &self.items[self.active]
    }

    pub fn active_mut(&mut self) -> &mut T {
        &mut self.items[self.active]
    }

    pub fn get(&self, i: usize) -> Option<&T> {
        self.items.get(i)
    }

    pub fn get_mut(&mut self, i: usize) -> Option<&mut T> {
        self.items.get_mut(i)
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.items.iter_mut()
    }

    /// Room for one more?
    pub fn can_open(&self) -> bool {
        self.items.len() < MAX_TABS
    }

    /// Open `t` right after the active tab and make it active. Returns its index, or `None`
    /// (handing nothing back) when the window is full.
    pub fn open(&mut self, t: T) -> Option<usize> {
        if !self.can_open() {
            return None;
        }
        let at = self.active + 1;
        self.items.insert(at, t);
        self.active = at;
        Some(at)
    }

    /// Close tab `i`. The tab to its right becomes active (the one to its left when it was the
    /// last); closing a tab before the active one keeps the active tab. The last tab cannot be
    /// closed here: the caller closes the window instead (returns `None`).
    pub fn close(&mut self, i: usize) -> Option<T> {
        if self.items.len() <= 1 || i >= self.items.len() {
            return None;
        }
        let t = self.items.remove(i);
        if i < self.active {
            self.active -= 1;
        } else if self.active >= self.items.len() {
            self.active = self.items.len() - 1;
        }
        Some(t)
    }

    /// Make tab `i` active.
    pub fn select(&mut self, i: usize) -> bool {
        if i < self.items.len() {
            self.active = i;
            true
        } else {
            false
        }
    }

    /// The next tab (wraps).
    pub fn next(&mut self) {
        self.active = (self.active + 1) % self.items.len();
    }

    /// The previous tab (wraps).
    pub fn prev(&mut self) {
        self.active = (self.active + self.items.len() - 1) % self.items.len();
    }

    /// Ctrl+1 .. Ctrl+9: tab number `n` (1-based); 9 is always the last tab.
    pub fn select_number(&mut self, n: usize) -> bool {
        match n {
            0 => false,
            9 => {
                self.active = self.items.len() - 1;
                true
            }
            n => self.select(n - 1),
        }
    }

    /// Move the active tab one place left or right (the strip order); returns whether it moved.
    pub fn move_active(&mut self, right: bool) -> bool {
        let to = if right {
            self.active + 1
        } else {
            match self.active.checked_sub(1) {
                Some(v) => v,
                None => return false,
            }
        };
        if to >= self.items.len() {
            return false;
        }
        self.items.swap(self.active, to);
        self.active = to;
        true
    }
}

/// The host part of an address: scheme, `www.` and the path are cut.
pub fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    host.strip_prefix("www.").unwrap_or(host)
}

/// Byte range of the host in an address typed or shown in the omnibox: after a scheme
/// (`https://`, `http://`) up to the first `/`, `?` or `#`. When there is no scheme the range
/// starts at 0. Shown highlighted while the rest is dimmed. The browser's own `kitsune://` pages
/// keep their scheme in the highlighted part.
pub fn host_range(url: &str) -> (usize, usize) {
    let own = super::internal_prefix_len(url);
    let start = match own {
        Some(_) => 0,
        None => url.find("://").map_or(0, |i| i + 3),
    };
    let skip = own.unwrap_or(0);
    let end = url[start + skip..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |i| start + skip + i);
    (start, end)
}

/// What a tab says: the page title, else the address's host, else "Nova aba" (in the language
/// in effect: ask again at every frame).
pub fn tab_title(title: &str, url: &str) -> String {
    tab_title_in(i18n::lang(), title, url)
}

/// [`tab_title`] in `lang`.
pub fn tab_title_in(lang: Lang, title: &str, url: &str) -> String {
    let t = title.trim();
    if !t.is_empty() {
        return String::from(t);
    }
    let u = url.trim();
    if let Some(n) = super::internal_prefix_len(u) {
        return match u[n..].trim_end_matches('/') {
            "favoritos" => String::from(i18n::tr_in(lang, tk!("web.tab.bookmarks"))),
            "historico" => String::from(i18n::tr_in(lang, tk!("web.tab.history"))),
            "sobre" => String::from(i18n::tr_in(lang, tk!("web.tab.about"))),
            _ => String::from(i18n::tr_in(lang, tk!("web.tab.new"))),
        };
    }
    let h = host_of(u);
    if h.is_empty() {
        String::from(i18n::tr_in(lang, tk!("web.tab.new")))
    } else {
        String::from(h)
    }
}

/// The badge of the browser's own pages (`kitsune://...`): not a letter but the mark of the
/// system. A private-use code point, so it can travel in the same `char` as the letters; the
/// kernel draws the fox head for it.
pub const BRAND_BADGE: char = '\u{E000}';

/// The letter on a tab's badge (there is no favicon): the first letter of the host, or of the
/// title when the host has none (an IP address), else its first digit; upper case; `•` when
/// there is nothing. The browser's own pages get [`BRAND_BADGE`].
pub fn tab_badge(title: &str, url: &str) -> char {
    if super::is_internal_url(url.trim()) {
        return BRAND_BADGE;
    }
    let host = host_of(url.trim());
    let from = |s: &str, f: fn(&char) -> bool| {
        s.chars()
            .find(f)
            .map(|c| c.to_uppercase().next().unwrap_or(c))
    };
    from(host, |c| c.is_alphabetic())
        .or_else(|| from(title, |c| c.is_alphabetic()))
        .or_else(|| from(host, |c| c.is_alphanumeric()))
        .unwrap_or('\u{2022}')
}

#[cfg(test)]
mod tests;
