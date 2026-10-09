//! The Apps launcher's logic: the categories of its left rail, which category an app belongs to,
//! filtering and ranking by category and search text, and the short *Recentes* list.
//!
//! Categories come from a built-in table (the app manifest has no category key yet): system apps
//! by their process name, installed apps by their id with *Utilitários* as the default. Pure and
//! host tested; the kernel only draws.

use crate::format::search;
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
    /// Catalog key of the category's name.
    pub const fn key(self) -> &'static str {
        match self {
            Category::All => crate::tk!("launcher.cat.all"),
            Category::System => crate::tk!("launcher.cat.system"),
            Category::Internet => crate::tk!("launcher.cat.internet"),
            Category::Media => crate::tk!("launcher.cat.media"),
            Category::Utilities => crate::tk!("launcher.cat.utilities"),
        }
    }

    /// The name in the language in effect.
    pub fn label(self) -> &'static str {
        crate::i18n::tr(self.key())
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
mod tests;
