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
mod tests {
    use super::*;

    #[test]
    fn english_is_one_only_for_one() {
        let r = Rule::OneIsSingular;
        assert_eq!(r.category(0), Category::Other);
        assert_eq!(r.category(1), Category::One);
        assert_eq!(r.category(2), Category::Other);
        assert_eq!(r.category(21), Category::Other);
        assert_eq!(r.category(u64::MAX), Category::Other);
    }

    #[test]
    fn portuguese_treats_zero_and_one_as_singular() {
        let r = Rule::ZeroAndOneSingular;
        assert_eq!(r.category(0), Category::One);
        assert_eq!(r.category(1), Category::One);
        assert_eq!(r.category(2), Category::Other);
        assert_eq!(r.category(100), Category::Other);
        assert_eq!(r.category(u64::MAX), Category::Other);
    }

    #[test]
    fn invariant_is_always_other() {
        for n in [0, 1, 2, 11, 1000] {
            assert_eq!(Rule::Invariant.category(n), Category::Other);
        }
    }

    #[test]
    fn rule_names() {
        assert_eq!(Rule::from_name("zero-one"), Rule::ZeroAndOneSingular);
        assert_eq!(Rule::from_name(" invariant "), Rule::Invariant);
        assert_eq!(Rule::from_name("one"), Rule::OneIsSingular);
        assert_eq!(Rule::from_name(""), Rule::OneIsSingular);
        assert_eq!(Category::One.suffix(), "one");
        assert_eq!(Category::Other.suffix(), "other");
    }
}
