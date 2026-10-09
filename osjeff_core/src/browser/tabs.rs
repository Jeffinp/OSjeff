//! The tabs of a browser window: which one is active, opening, closing and switching,
//! and the words and letter a tab shows. Pure: the kernel keeps the page state of each
//! tab in the `T` it stores here.

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
/// starts at 0. Shown highlighted while the rest is dimmed.
pub fn host_range(url: &str) -> (usize, usize) {
    let start = url.find("://").map_or(0, |i| i + 3);
    let end = url[start..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |i| start + i);
    (start, end)
}

/// What a tab says: the page title, else the address's host, else "Nova aba".
pub fn tab_title(title: &str, url: &str) -> String {
    let t = title.trim();
    if !t.is_empty() {
        return String::from(t);
    }
    let u = url.trim();
    if u.starts_with("osjeff://") {
        return match u.trim_start_matches("osjeff://").trim_end_matches('/') {
            "favoritos" => String::from("Favoritos"),
            "historico" => String::from("Histórico"),
            "sobre" => String::from("Sobre o Navegador"),
            _ => String::from("Nova aba"),
        };
    }
    let h = host_of(u);
    if h.is_empty() {
        String::from("Nova aba")
    } else {
        String::from(h)
    }
}

/// The letter on a tab's badge (there is no favicon): the first letter of the host, or of the
/// title when the host has none (an IP address), upper case; `•` when there is none.
pub fn tab_badge(title: &str, url: &str) -> char {
    let host = host_of(url.trim());
    let from = |s: &str| {
        s.chars()
            .find(|c| c.is_alphabetic())
            .map(|c| c.to_uppercase().next().unwrap_or(c))
    };
    from(host).or_else(|| from(title)).unwrap_or('\u{2022}')
}

#[cfg(test)]
mod tests {
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
        assert_eq!(tab_title("  Olá  ", "http://a.test/x"), "Olá");
        assert_eq!(
            tab_title("", "https://www.exemplo.com.br/a?b#c"),
            "exemplo.com.br"
        );
        assert_eq!(tab_title("", "203.0.113.5:8079/x"), "203.0.113.5:8079");
        assert_eq!(tab_title("", ""), "Nova aba");
        assert_eq!(tab_title("", "osjeff://favoritos"), "Favoritos");
        assert_eq!(tab_title("", "osjeff://historico/"), "Histórico");
        assert_eq!(tab_title("", "osjeff://inicio"), "Nova aba");
    }

    #[test]
    fn badges_use_the_host_letter() {
        assert_eq!(tab_badge("", "https://www.exemplo.com"), 'E');
        assert_eq!(tab_badge("x", "http://123.test/"), 'T');
        assert_eq!(tab_badge("Tipografia", "http://203.0.113.5:8079/"), 'T');
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
        let u = "osjeff://favoritos";
        assert_eq!(&u[host_range(u).0..host_range(u).1], "favoritos");
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
            let _ = tab_badge("", u);
        }
    }
}
