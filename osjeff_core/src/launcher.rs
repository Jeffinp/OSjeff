//! The Apps launcher's logic: the categories of its left rail, which category an app belongs to,
//! filtering and ranking by category and search text, and the short *Recentes* list.
//!
//! Categories come from a built-in table (the app manifest has no category key yet): system apps
//! by their process name, installed apps by their id with *Utilitários* as the default. Pure and
//! host tested; the kernel only draws.

use crate::search;
use alloc::string::String;
use alloc::vec::Vec;

/// A category of the launcher's rail.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    /// Every app (not a category an app can have).
    All,
    System,
    Internet,
    Media,
    Utilities,
}

/// The rail, top to bottom.
pub const CATEGORIES: [Category; 5] = [
    Category::All,
    Category::System,
    Category::Internet,
    Category::Media,
    Category::Utilities,
];

impl Category {
    pub const fn label(self) -> &'static str {
        match self {
            Category::All => "Todos",
            Category::System => "Sistema",
            Category::Internet => "Internet",
            Category::Media => "Mídia",
            Category::Utilities => "Utilitários",
        }
    }

    /// Position in [`CATEGORIES`].
    pub const fn index(self) -> usize {
        match self {
            Category::All => 0,
            Category::System => 1,
            Category::Internet => 2,
            Category::Media => 3,
            Category::Utilities => 4,
        }
    }
}

/// The category of a built-in app by its process name (`shell`, `files`, ...).
pub fn system_category(proc_name: &str) -> Category {
    match proc_name {
        "shell" | "files" | "taskmgr" | "monitor" | "settings" | "syslog" | "gallery" => {
            Category::System
        }
        "browser" => Category::Internet,
        "viewer" => Category::Media,
        _ => Category::Utilities,
    }
}

/// The category of an installed app by its id.
pub fn app_category(id: &str) -> Category {
    match id {
        "paint" | "plasma" | "snake" | "doom" => Category::Media,
        "nettest" => Category::Internet,
        "cdemo" => Category::System,
        _ => Category::Utilities,
    }
}

/// Indices of the items to show for `cat` and `query`. A search looks at every app whatever the
/// category (best match first, ties in the original order); without one the items of the
/// category appear in their original order.
pub fn filter<'a>(
    items: impl Iterator<Item = (&'a str, Category)>,
    cat: Category,
    query: &str,
) -> Vec<usize> {
    if query.trim().is_empty() {
        return items
            .enumerate()
            .filter(|(_, (_, c))| cat == Category::All || *c == cat)
            .map(|(i, _)| i)
            .collect();
    }
    let mut hits: Vec<(u8, usize)> = items
        .enumerate()
        .filter_map(|(i, (label, _))| search::rank(query, label).map(|r| (r, i)))
        .collect();
    hits.sort_by_key(|&(r, i)| (r, i));
    hits.into_iter().map(|(_, i)| i).collect()
}

/// How many recent apps the launcher remembers.
pub const RECENTS: usize = 5;

/// The most recently launched apps, newest first, without repeats.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recents {
    keys: Vec<String>,
}

impl Recents {
    pub const fn new() -> Self {
        Self { keys: Vec::new() }
    }

    /// `key` was just launched.
    pub fn note(&mut self, key: &str) {
        self.keys.retain(|k| k != key);
        self.keys.insert(0, String::from(key));
        self.keys.truncate(RECENTS);
    }

    /// Forget `key` (the app was uninstalled).
    pub fn forget(&mut self, key: &str) {
        self.keys.retain(|k| k != key);
    }

    pub fn list(&self) -> &[String] {
        &self.keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITEMS: [(&str, Category); 6] = [
        ("Terminal", Category::System),
        ("Editor", Category::Utilities),
        ("Navegador", Category::Internet),
        ("Imagens", Category::Media),
        ("Calculadora", Category::Utilities),
        ("Notas", Category::Utilities),
    ];

    #[test]
    fn the_rail_has_five_stable_entries() {
        let labels: Vec<&str> = CATEGORIES.iter().map(|c| c.label()).collect();
        assert_eq!(
            labels,
            ["Todos", "Sistema", "Internet", "Mídia", "Utilitários"]
        );
        for (i, c) in CATEGORIES.iter().enumerate() {
            assert_eq!(c.index(), i);
        }
    }

    #[test]
    fn the_built_in_tables_cover_every_app() {
        for n in ["shell", "files", "taskmgr", "monitor", "settings", "syslog"] {
            assert_eq!(system_category(n), Category::System, "{n}");
        }
        assert_eq!(system_category("browser"), Category::Internet);
        assert_eq!(system_category("viewer"), Category::Media);
        for n in ["editor", "calc"] {
            assert_eq!(system_category(n), Category::Utilities, "{n}");
        }
        assert_eq!(app_category("paint"), Category::Media);
        assert_eq!(app_category("snake"), Category::Media);
        // Unknown ids land in Utilitários; no app is ever in All.
        assert_eq!(app_category("anything-else"), Category::Utilities);
        assert_ne!(system_category("zzz"), Category::All);
    }

    #[test]
    fn a_category_keeps_the_original_order_and_all_shows_everything() {
        let all = filter(ITEMS.iter().copied(), Category::All, "");
        assert_eq!(all, [0, 1, 2, 3, 4, 5]);
        assert_eq!(
            filter(ITEMS.iter().copied(), Category::Utilities, ""),
            [1, 4, 5]
        );
        assert_eq!(filter(ITEMS.iter().copied(), Category::Media, ""), [3]);
        assert_eq!(
            filter(ITEMS.iter().copied(), Category::Internet, "   "),
            [2]
        );
    }

    #[test]
    fn a_search_looks_everywhere_and_ranks_prefixes_first() {
        // "n" is a prefix of Navegador and Notas, inside Terminal's word? no: only contained.
        let hits = filter(ITEMS.iter().copied(), Category::Media, "n");
        assert_eq!(hits[0], 2); // Navegador (prefix), whatever the category
        assert!(hits.contains(&5) && hits.contains(&0));
        // Accents and case do not matter.
        assert_eq!(filter(ITEMS.iter().copied(), Category::All, "CALC"), [4]);
        assert!(filter(ITEMS.iter().copied(), Category::All, "zzz").is_empty());
        // Ties keep the original order.
        let tie = filter(ITEMS.iter().copied(), Category::All, "e");
        let mut sorted = tie.clone();
        sorted.sort_by_key(|&i| (search::rank("e", ITEMS[i].0), i));
        assert_eq!(tie, sorted);
    }

    #[test]
    fn recents_are_newest_first_unique_and_bounded() {
        let mut r = Recents::new();
        assert!(r.list().is_empty());
        for k in ["a", "b", "c", "a", "d", "e", "f", "g"] {
            r.note(k);
        }
        assert_eq!(r.list().len(), RECENTS);
        // "a" was launched again before d, e, f, g, so it is the oldest that is left; b and c fell off.
        assert_eq!(r.list(), ["g", "f", "e", "d", "a"]);
        r.note("e");
        assert_eq!(r.list()[0], "e");
        assert_eq!(r.list().iter().filter(|k| *k == "e").count(), 1);
        r.forget("e");
        assert!(!r.list().iter().any(|k| k == "e"));
        r.forget("never-launched");
    }
}
