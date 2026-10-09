//! Message catalogs: plain `key = value` text compiled into a sorted static table.
//!
//! The source of a language is one text file (`assets/i18n/<lang>.txt`) pulled in with
//! `include_str!`. A `const fn` parses it **at compile time** into a sorted array of
//! `(key, value)` pairs, both `&'static str` slices of the file itself. So at run time
//! there is no parsing, no boot cost, no heap and no global state: a lookup is a binary
//! search over read-only data and returns a `&'static str`.
//!
//! File grammar (one entry per line, no escapes, no multi-line values):
//!
//! ```text
//! # comment (only at the start of a line)
//! files.sidebar.favorites = Favoritos
//! ```
//!
//! * Blank lines and lines whose first non-blank character is `#` are skipped.
//! * The key is everything before the first `=`, the value everything after it; both are
//!   trimmed (spaces, tabs, `\r`). A value may contain `=` and `#`.
//! * A line without `=`, an empty key or a **duplicate key** is a compile error (a `const`
//!   panic), so a broken catalog never reaches a boot image.
//! * Entries may be in any order; the build sorts them (byte order of the key).

/// One language's messages: `(key, value)` pairs sorted by key.
pub struct Catalog {
    entries: &'static [(&'static str, &'static str)],
}

impl Catalog {
    /// Wrap an already sorted, duplicate-free table (what [`build`] returns).
    pub const fn new(entries: &'static [(&'static str, &'static str)]) -> Catalog {
        Catalog { entries }
    }

    /// The text for `key`, if this catalog defines it. Binary search, no allocation.
    pub fn get(&self, key: &str) -> Option<&'static str> {
        self.entries
            .binary_search_by(|(k, _)| (*k).cmp(key))
            .ok()
            .map(|i| self.entries[i].1)
    }

    /// Every entry, sorted by key.
    pub fn entries(&self) -> &'static [(&'static str, &'static str)] {
        self.entries
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when the catalog has no entry.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Byte slice `s[a..b]` as a `str` (the caller cuts at ASCII bytes, so always valid).
const fn sub(s: &'static str, a: usize, b: usize) -> &'static str {
    let (head, _) = s.as_bytes().split_at(b);
    let (_, mid) = head.split_at(a);
    match core::str::from_utf8(mid) {
        Ok(t) => t,
        Err(_) => panic!("catalog: cut inside a UTF-8 sequence"),
    }
}

const fn is_blank(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r')
}

/// `s[a..b]` without surrounding blanks.
const fn trim(s: &'static str, mut a: usize, mut b: usize) -> &'static str {
    let bytes = s.as_bytes();
    while a < b && is_blank(bytes[a]) {
        a += 1;
    }
    while b > a && is_blank(bytes[b - 1]) {
        b -= 1;
    }
    sub(s, a, b)
}

/// The next entry at or after byte `from`: `(key, value, next_from)`, or `None` at the end.
const fn next_entry(
    src: &'static str,
    mut from: usize,
) -> Option<(&'static str, &'static str, usize)> {
    let bytes = src.as_bytes();
    let n = bytes.len();
    while from < n {
        let start = from;
        let mut end = start;
        while end < n && bytes[end] != b'\n' {
            end += 1;
        }
        from = end + 1;
        let mut i = start;
        while i < end && is_blank(bytes[i]) {
            i += 1;
        }
        if i == end || bytes[i] == b'#' {
            continue;
        }
        let mut eq = i;
        while eq < end && bytes[eq] != b'=' {
            eq += 1;
        }
        if eq == end {
            panic!("catalog: a line without '='");
        }
        let key = trim(src, i, eq);
        if key.is_empty() {
            panic!("catalog: an empty key");
        }
        return Some((key, trim(src, eq + 1, end), from));
    }
    None
}

/// Number of entries in `src` (to size the table).
pub const fn count(src: &'static str) -> usize {
    let mut n = 0;
    let mut from = 0;
    while let Some((_, _, next)) = next_entry(src, from) {
        n += 1;
        from = next;
    }
    n
}

/// `a < b` in byte order (what `str::cmp` does).
const fn less(a: &str, b: &str) -> bool {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    let m = if x.len() < y.len() { x.len() } else { y.len() };
    let mut i = 0;
    while i < m {
        if x[i] != y[i] {
            return x[i] < y[i];
        }
        i += 1;
    }
    x.len() < y.len()
}

const fn same(a: &str, b: &str) -> bool {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    if x.len() != y.len() {
        return false;
    }
    let mut i = 0;
    while i < x.len() {
        if x[i] != y[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Parse `src` into its `N` entries (`N` = [`count`]`(src)`), sorted by key. Panics (at
/// compile time, when evaluated in a `const`) on a malformed line or a duplicate key.
pub const fn build<const N: usize>(src: &'static str) -> [(&'static str, &'static str); N] {
    let mut t = [("", ""); N];
    let mut k = 0;
    let mut from = 0;
    while let Some((key, val, next)) = next_entry(src, from) {
        if k == N {
            panic!("catalog: N is smaller than the entry count");
        }
        t[k] = (key, val);
        k += 1;
        from = next;
    }
    if k != N {
        panic!("catalog: N is larger than the entry count");
    }
    // Shell sort (Ciura gaps): n log n-ish, cheap enough for the const evaluator.
    let gaps = [701usize, 301, 132, 57, 23, 10, 4, 1];
    let mut g = 0;
    while g < gaps.len() {
        let gap = gaps[g];
        let mut i = gap;
        while i < N {
            let item = t[i];
            let mut j = i;
            while j >= gap && less(item.0, t[j - gap].0) {
                t[j] = t[j - gap];
                j -= gap;
            }
            t[j] = item;
            i += 1;
        }
        g += 1;
    }
    let mut i = 1;
    while i < N {
        if same(t[i - 1].0, t[i].0) {
            panic!("catalog: duplicate key");
        }
        i += 1;
    }
    t
}

/// Define `static $name: Catalog` from the text `$src` (a `&'static str` constant expression).
macro_rules! catalog_src {
    ($name:ident, $src:expr) => {
        #[allow(long_running_const_eval)]
        static $name: $crate::i18n::catalog::Catalog = {
            const SRC: &str = $src;
            const N: usize = $crate::i18n::catalog::count(SRC);
            static TABLE: [(&str, &str); N] = $crate::i18n::catalog::build::<N>(SRC);
            $crate::i18n::catalog::Catalog::new(&TABLE)
        };
    };
}
pub(crate) use catalog_src;

/// Define `static $name: Catalog` from the text file `$file` (a path for `include_str!`).
macro_rules! catalog {
    ($name:ident, $file:literal) => {
        $crate::i18n::catalog::catalog_src!($name, include_str!($file));
    };
}
pub(crate) use catalog;

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str =
        "# header\n\n  b.key = Second value  \r\na.key=First=with equals # not a comment\n\tc = \n";
    const N: usize = count(SAMPLE);
    static T: [(&str, &str); N] = build::<N>(SAMPLE);

    #[test]
    fn parses_sorts_and_trims() {
        assert_eq!(N, 3);
        let c = Catalog::new(&T);
        assert_eq!(c.len(), 3);
        assert_eq!(c.get("a.key"), Some("First=with equals # not a comment"));
        assert_eq!(c.get("b.key"), Some("Second value"));
        assert_eq!(c.get("c"), Some(""));
        assert_eq!(c.get("d"), None);
        assert_eq!(c.get(""), None);
        let keys: std::vec::Vec<_> = c.entries().iter().map(|e| e.0).collect();
        assert_eq!(keys, ["a.key", "b.key", "c"]);
    }

    #[test]
    fn empty_source_is_an_empty_catalog() {
        const E: &str = "# nothing\n\n";
        const EN: usize = count(E);
        static ET: [(&str, &str); EN] = build::<EN>(E);
        assert_eq!(EN, 0);
        let c = Catalog::new(&ET);
        assert!(c.is_empty());
        assert_eq!(c.get("x"), None);
    }

    #[test]
    fn utf8_values_survive() {
        const U: &str = "k = Configurações – “ok” …\nz = ç\n";
        const UN: usize = count(U);
        static UT: [(&str, &str); UN] = build::<UN>(U);
        let c = Catalog::new(&UT);
        assert_eq!(c.get("k"), Some("Configurações – “ok” …"));
        assert_eq!(c.get("z"), Some("ç"));
    }

    #[test]
    fn last_line_without_newline_and_crlf() {
        const S: &str = "a = 1\r\nb = 2";
        const SN: usize = count(S);
        static ST: [(&str, &str); SN] = build::<SN>(S);
        let c = Catalog::new(&ST);
        assert_eq!((c.get("a"), c.get("b")), (Some("1"), Some("2")));
    }

    #[test]
    fn sort_is_byte_order_and_lookup_finds_every_key() {
        const S: &str = "b = 1\nB = 2\na.b = 3\na = 4\na_ = 5\na.a = 6\nz = 7\n";
        const SN: usize = count(S);
        static ST: [(&str, &str); SN] = build::<SN>(S);
        let c = Catalog::new(&ST);
        for w in c.entries().windows(2) {
            assert!(w[0].0 < w[1].0);
        }
        for (k, v) in c.entries() {
            assert_eq!(c.get(k), Some(*v));
        }
    }

    #[test]
    fn shell_sort_sorts_a_large_reversed_input() {
        // Built at run time here (the const path is the same code) over > 701 entries so
        // every gap of the sequence is exercised.
        let mut src = std::string::String::new();
        for i in (0..900).rev() {
            src.push_str(&std::format!("k.{i:04} = v{i}\n"));
        }
        let src: &'static str = std::boxed::Box::leak(src.into_boxed_str());
        let t = build::<900>(src);
        for (i, e) in t.iter().enumerate() {
            assert_eq!(e.0, std::format!("k.{i:04}"));
        }
    }
}
