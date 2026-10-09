//! Plural categories. A language names its rule in its catalog (`fmt.plural`), so adding a
//! language that shares a rule with an existing one is data only.
//!
//! Only the two categories the shipped languages need exist (`one`, `other`); the rule
//! table is where a language with more (few, many...) would be added, together with the
//! suffix its messages use.

/// The plural category of a count.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    One,
    Other,
}

impl Category {
    /// The key suffix: `files.items.one` / `files.items.other`.
    pub const fn suffix(self) -> &'static str {
        match self {
            Category::One => "one",
            Category::Other => "other",
        }
    }
}

/// A plural rule, by the name used in the catalogs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rule {
    /// `one` for exactly 1 (English, German, Spanish, Italian, Dutch...).
    OneIsSingular,
    /// `one` for 0 and 1 (Brazilian Portuguese, French): CLDR `i = 0..1`.
    ZeroAndOneSingular,
    /// Always `other` (Japanese, Chinese, Korean...).
    Invariant,
}

impl Rule {
    /// The rule named `name` in a catalog; unknown names read as [`Rule::OneIsSingular`].
    pub fn from_name(name: &str) -> Rule {
        match name.trim() {
            "zero-one" => Rule::ZeroAndOneSingular,
            "invariant" => Rule::Invariant,
            _ => Rule::OneIsSingular,
        }
    }

    /// The category of the integer `n`.
    pub const fn category(self, n: u64) -> Category {
        match self {
            Rule::OneIsSingular if n == 1 => Category::One,
            Rule::ZeroAndOneSingular if n <= 1 => Category::One,
            _ => Category::Other,
        }
    }
}

#[cfg(test)]
mod tests;
