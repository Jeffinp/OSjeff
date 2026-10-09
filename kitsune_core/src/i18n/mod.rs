//! Internationalisation: a message catalog per language, looked up by stable string ids.
//!
//! Two languages ship: Brazilian Portuguese ([`Lang::Pt`], the default) and English
//! ([`Lang::En`]). Design and rules: `docs/design/i18n.md`.
//!
//! * **Catalogs** are plain `key = value` files (`assets/i18n/pt.txt`, `en.txt`) compiled
//!   in with `include_str!` and parsed by a `const fn` into sorted static tables
//!   ([`catalog`]): no boot cost, no heap, no global mutable state besides the current
//!   language (one atomic).
//! * **Lookup** ([`tr`], [`tr_in`]): the current language, then English, then the key
//!   itself. It never panics and never allocates; a miss only bumps a counter
//!   ([`missing_count`]).
//! * **Placeholders** (`{name}`, `{0}`) take typed [`Arg`]s that format numbers, sizes and
//!   dates for the language ([`template`], [`locale`]); **plurals** are separate keys
//!   (`key.one` / `key.other`) chosen by the language's rule ([`plural`]).
//! * **Macros** for call sites: `t!("key")` is a `&str`; `t!("key", n = 3)` a `String`;
//!   `tp!("key", count)` picks the plural; `tk!("key")` marks a key in a table without
//!   looking it up. The host tests (`cargo test -p kitsune_core i18n`) scan the sources for
//!   these literals and fail on a key that is missing, unused, or that differs between
//!   the catalogs.
//!
//! A third language is data only: a new `assets/i18n/<code>.txt` plus one line in [`Lang`]
//! (its variant, its place in [`Lang::ALL`] and its catalog).

pub mod catalog;
pub mod locale;
pub mod plural;
pub mod template;

#[cfg(test)]
mod audit;
#[cfg(test)]
pub(crate) mod testlang;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use catalog::Catalog;
pub use locale::{Civil, DateFmt, DateStyle, TimeFmt};
pub use template::{Arg, Args, Rendered, bytes, dec, num, placeholders, render};

catalog::catalog!(PT, "../../../assets/i18n/pt.txt");
catalog::catalog!(EN, "../../../assets/i18n/en.txt");

/// A supported language.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum Lang {
    /// Brazilian Portuguese.
    Pt = 0,
    /// English.
    En = 1,
}

/// The language whose catalog fills in what another one lacks.
pub const FALLBACK: Lang = Lang::En;

impl Lang {
    /// Every language, in the order the picker lists them.
    pub const ALL: [Lang; 2] = [Lang::Pt, Lang::En];
    /// The language of a fresh install.
    pub const DEFAULT: Lang = Lang::Pt;

    /// The catalog of this language.
    pub fn catalog(self) -> &'static catalog::Catalog {
        match self {
            Lang::Pt => &PT,
            Lang::En => &EN,
        }
    }

    /// The language tag stored in the settings (`pt-BR`, `en`): the `meta.code` entry of its
    /// own catalog.
    pub fn code(self) -> &'static str {
        self.catalog().get("meta.code").unwrap_or("en")
    }

    /// The name of the language in that language (`Português (Brasil)`, `English`), for the
    /// picker.
    pub fn native_name(self) -> &'static str {
        self.catalog().get("meta.name").unwrap_or("?")
    }

    /// The language a tag names, ignoring case and `_` versus `-`: the full tag (`pt-BR`)
    /// or its primary subtag (`pt`, `en-US` reads as `en`). `None` for anything else.
    pub fn from_code(code: &[u8]) -> Option<Lang> {
        let mut buf = [0u8; 16];
        if code.is_empty() || code.len() > buf.len() {
            return None;
        }
        for (d, &s) in buf.iter_mut().zip(code) {
            *d = if s == b'_' {
                b'-'
            } else {
                s.to_ascii_lowercase()
            };
        }
        let want = &buf[..code.len()];
        let primary = want.split(|&b| b == b'-').next().unwrap_or(want);
        Lang::ALL.into_iter().find(|l| {
            let c = l.code().as_bytes();
            let eq = |a: &[u8]| {
                a.len() == c.len() && a.iter().zip(c).all(|(x, y)| *x == y.to_ascii_lowercase())
            };
            let cp = c.split(|&b| b == b'-').next().unwrap_or(c);
            eq(want)
                || (primary.len() == cp.len()
                    && primary
                        .iter()
                        .zip(cp)
                        .all(|(x, y)| *x == y.to_ascii_lowercase()))
        })
    }

    /// The language stored as `i` (the default for anything out of range).
    pub fn from_index(i: u8) -> Lang {
        Lang::ALL.get(i as usize).copied().unwrap_or(Lang::DEFAULT)
    }

    /// Position in [`Lang::ALL`].
    pub const fn index(self) -> u8 {
        self as u8
    }

    /// The plural rule of this language.
    pub fn plural_rule(self) -> plural::Rule {
        plural::Rule::from_name(tr_in(self, "fmt.plural"))
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(Lang::DEFAULT as u8);
static GENERATION: AtomicU32 = AtomicU32::new(0);
static MISSING: AtomicU32 = AtomicU32::new(0);

/// The language in effect.
pub fn lang() -> Lang {
    #[cfg(test)]
    if let Some(l) = testlang::current() {
        return l;
    }
    Lang::from_index(CURRENT.load(Ordering::Relaxed))
}

/// Make `l` the language in effect. Anything that cached text built with the old language
/// must notice: compare [`generation`] to the value seen when the cache was filled.
pub fn set_lang(l: Lang) {
    if CURRENT.swap(l as u8, Ordering::Relaxed) != l as u8 {
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
}

/// Counts language changes (starts at 0): a cache of rendered text is stale when this differs
/// from the value read when it was filled.
pub fn generation() -> u32 {
    GENERATION.load(Ordering::Relaxed)
}

/// How many lookups found no text in any language (the key was shown instead).
pub fn missing_count() -> u32 {
    MISSING.load(Ordering::Relaxed)
}

/// The first catalog of `chain` that defines `key`.
fn lookup_chain(chain: &[&Catalog], key: &str) -> Option<&'static str> {
    chain.iter().find_map(|c| c.get(key))
}

/// The catalogs consulted for `l`: its own, then the fallback language's.
fn chain(l: Lang) -> [&'static Catalog; 2] {
    [l.catalog(), FALLBACK.catalog()]
}

/// The text of `key` in `l`; else in English; else `key` itself. Never allocates or panics.
pub fn tr_in(l: Lang, key: &str) -> &str {
    match lookup_chain(&chain(l), key) {
        Some(v) => v,
        None => {
            MISSING.fetch_add(1, Ordering::Relaxed);
            key
        }
    }
}

/// The text of `key` in the language in effect (see [`tr_in`]).
pub fn tr(key: &str) -> &str {
    tr_in(lang(), key)
}

/// `key` filled with `args`, in `l`.
pub fn tr_fmt_in(l: Lang, key: &str, args: Args<'_>) -> String {
    let mut out = String::new();
    let _ = render(&mut out, l, tr_in(l, key), args);
    out
}

/// `key` filled with `args`, in the language in effect.
pub fn tr_fmt(key: &str, args: Args<'_>) -> String {
    tr_fmt_in(lang(), key, args)
}

/// Longest `key` accepted by the plural lookups (the suffix is built on the stack).
const MAX_PLURAL_KEY: usize = 96;

/// `key.one` / `key.other` (by `rule`) from the first catalog of `chain` that has it,
/// `key.other` when the one wanted is missing.
fn plural_chain(chain: &[&Catalog], rule: plural::Rule, key: &str, n: u64) -> Option<&'static str> {
    if key.len() > MAX_PLURAL_KEY {
        return None;
    }
    let mut buf = [0u8; MAX_PLURAL_KEY + 6];
    for suffix in [rule.category(n).suffix(), plural::Category::Other.suffix()] {
        let len = key.len() + 1 + suffix.len();
        buf[..key.len()].copy_from_slice(key.as_bytes());
        buf[key.len()] = b'.';
        buf[key.len() + 1..len].copy_from_slice(suffix.as_bytes());
        // The key and the suffix are UTF-8 and the dot is ASCII: always valid.
        if let Ok(k) = core::str::from_utf8(&buf[..len])
            && let Some(v) = lookup_chain(chain, k)
        {
            return Some(v);
        }
    }
    None
}

/// The template of `key` for the count `n` in `l`: `key.one` or `key.other` by the language's
/// rule, `key.other` when the one wanted is missing, else `key`.
pub fn plural_in(l: Lang, key: &str, n: u64) -> &str {
    match plural_chain(&chain(l), l.plural_rule(), key, n) {
        Some(v) => v,
        None => {
            MISSING.fetch_add(1, Ordering::Relaxed);
            key
        }
    }
}

/// [`plural_in`] for the language in effect.
pub fn plural(key: &str, n: u64) -> &str {
    plural_in(lang(), key, n)
}

/// The plural message for `n`, filled in: `{n}` is the count (grouped, e.g. `1.234`) and the
/// other placeholders come from `args` (which win over `n` if they use the same name).
pub fn plural_fmt_in(l: Lang, key: &str, n: u64, args: Args<'_>) -> String {
    let mut all: Vec<(&str, Arg<'_>)> = Vec::with_capacity(args.len() + 1);
    all.extend_from_slice(args);
    all.push(("n", Arg::Num(i64::try_from(n).unwrap_or(i64::MAX))));
    let mut out = String::new();
    let _ = render(&mut out, l, plural_in(l, key, n), &all);
    out
}

/// [`plural_fmt_in`] for the language in effect.
pub fn plural_fmt(key: &str, n: u64, args: Args<'_>) -> String {
    plural_fmt_in(lang(), key, n, args)
}

/// A date (and time) in the language in effect.
pub fn format_date(civil: Civil, style: DateStyle, clock24: bool) -> String {
    let mut s = String::new();
    let _ = write!(s, "{}", locale::date_now(civil, style, clock24));
    s
}

/// A time of day in the language in effect.
pub fn format_time(civil: Civil, clock24: bool, seconds: bool) -> String {
    let mut s = String::new();
    let _ = write!(s, "{}", locale::time_now(civil, clock24, seconds));
    s
}

/// A byte size (`1,5 KiB`) in the language in effect.
pub fn format_size(bytes: u64) -> String {
    let mut s = String::new();
    let _ = locale::write_size(&mut s, lang(), bytes);
    s
}

/// A quantity with thousands separators in the language in effect.
pub fn format_num(n: i64) -> String {
    let mut s = String::new();
    let _ = locale::write_int(&mut s, lang(), n, true);
    s
}

/// A key marker: expands to the literal itself, so a key kept in a `const` table or a field
/// is still seen by the catalog checks. Look it up later with [`tr`].
#[macro_export]
macro_rules! tk {
    ($key:literal) => {
        $key
    };
}

/// The text of a key in the language in effect: `t!("panel.apps")` is a `&str`;
/// `t!("files.copied", n = 3, name = file)` fills placeholders and is a `String`.
/// Arguments are anything `Arg: From` (`&str`, integers) or an `Arg` (`num(x)`, `bytes(x)`).
#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::i18n::tr($key)
    };
    ($key:literal, $($name:ident = $val:expr),+ $(,)?) => {
        $crate::i18n::tr_fmt(
            $key,
            &[$((stringify!($name), $crate::i18n::Arg::from($val))),+],
        )
    };
}

/// The plural message of a key for a count: `tp!("files.items", n)`; more placeholders as in
/// [`t!`]: `tp!("files.deleted", n, folder = name)`. A `String`; `{n}` is the count.
#[macro_export]
macro_rules! tp {
    ($key:literal, $n:expr) => {
        $crate::i18n::plural_fmt($key, ($n) as u64, &[])
    };
    ($key:literal, $n:expr, $($name:ident = $val:expr),+ $(,)?) => {
        $crate::i18n::plural_fmt(
            $key,
            ($n) as u64,
            &[$((stringify!($name), $crate::i18n::Arg::from($val))),+],
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that change the global language take this lock (the test harness is parallel).
    pub(crate) static LANG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn lookup_in_each_language_and_fallback_chain() {
        assert_eq!(tr_in(Lang::Pt, "meta.name"), "Português (Brasil)");
        assert_eq!(tr_in(Lang::En, "meta.name"), "English");
        // Unknown keys come back as themselves, in any language.
        assert_eq!(tr_in(Lang::Pt, "no.such.key"), "no.such.key");
        assert_eq!(tr_in(Lang::En, ""), "");
    }

    #[test]
    fn missing_lookups_are_counted() {
        let before = missing_count();
        let _ = tr_in(Lang::Pt, "still.no.such.key");
        assert!(missing_count() > before);
        let b2 = missing_count();
        let _ = tr_in(Lang::Pt, "meta.code");
        assert_eq!(missing_count(), b2);
    }

    #[test]
    fn current_language_switches_and_bumps_the_generation() {
        let _g = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_lang(Lang::Pt);
        let g0 = generation();
        assert_eq!(tr("meta.name"), "Português (Brasil)");
        set_lang(Lang::Pt);
        assert_eq!(generation(), g0, "same language: no new generation");
        set_lang(Lang::En);
        assert_eq!(generation(), g0 + 1);
        assert_eq!(tr("meta.name"), "English");
        assert_eq!(lang(), Lang::En);
        set_lang(Lang::Pt);
    }

    #[test]
    fn language_tags() {
        assert_eq!(Lang::Pt.code(), "pt-BR");
        assert_eq!(Lang::En.code(), "en");
        for (tag, l) in [
            ("pt", Some(Lang::Pt)),
            ("pt-BR", Some(Lang::Pt)),
            ("PT_br", Some(Lang::Pt)),
            ("pt-PT", Some(Lang::Pt)),
            ("en", Some(Lang::En)),
            ("en-US", Some(Lang::En)),
            ("EN_gb", Some(Lang::En)),
            ("fr", None),
            ("", None),
            ("p", None),
            ("português", None),
            ("pt-BR-extra-way-too-long", None),
        ] {
            assert_eq!(Lang::from_code(tag.as_bytes()), l, "{tag}");
        }
        for l in Lang::ALL {
            assert_eq!(Lang::from_code(l.code().as_bytes()), Some(l));
            assert_eq!(Lang::from_index(l.index()), l);
        }
        assert_eq!(Lang::from_index(200), Lang::DEFAULT);
        assert_eq!(Lang::Pt.native_name(), "Português (Brasil)");
        assert_eq!(Lang::En.native_name(), "English");
    }

    catalog::catalog_src!(
        FIX_PT,
        "fix.items.one = {n} item\nfix.items.other = {n} itens\nfix.only_other.other = so {n}\n"
    );
    catalog::catalog_src!(
        FIX_EN,
        "fix.items.one = {n} item\nfix.items.other = {n} items\nfix.en_only.other = en {n}\n"
    );

    #[test]
    fn plural_keys_use_each_languages_rule() {
        assert_eq!(Lang::Pt.plural_rule(), plural::Rule::ZeroAndOneSingular);
        assert_eq!(Lang::En.plural_rule(), plural::Rule::OneIsSingular);
        let pt = [&FIX_PT, &FIX_EN];
        let en = [&FIX_EN, &FIX_PT];
        let zo = plural::Rule::ZeroAndOneSingular;
        let one = plural::Rule::OneIsSingular;
        assert_eq!(plural_chain(&pt, zo, "fix.items", 0), Some("{n} item"));
        assert_eq!(plural_chain(&pt, zo, "fix.items", 1), Some("{n} item"));
        assert_eq!(plural_chain(&pt, zo, "fix.items", 2), Some("{n} itens"));
        assert_eq!(plural_chain(&en, one, "fix.items", 0), Some("{n} items"));
        assert_eq!(plural_chain(&en, one, "fix.items", 1), Some("{n} item"));
        // `.one` missing: `.other` stands in; missing in the language: the next catalog.
        assert_eq!(plural_chain(&pt, zo, "fix.only_other", 1), Some("so {n}"));
        assert_eq!(plural_chain(&pt, zo, "fix.en_only", 5), Some("en {n}"));
        assert_eq!(plural_chain(&pt, zo, "nope", 1), None);
        let long = "k".repeat(500);
        assert_eq!(plural_chain(&pt, zo, &long, 1), None);
        assert_eq!(plural_in(Lang::En, &long, 1), long);
        assert_eq!(plural_in(Lang::En, "nope", 1), "nope");
    }

    #[test]
    fn plural_fmt_binds_n() {
        let chain = [&FIX_EN];
        let t = plural_chain(&chain, plural::Rule::OneIsSingular, "fix.items", 1234).unwrap();
        let mut o = String::new();
        render(&mut o, Lang::En, t, &[("n", Arg::Num(1234))]).unwrap();
        assert_eq!(o, "1,234 items");
        // Through the public API: a key that is in no catalog formats as itself.
        assert_eq!(
            plural_fmt_in(Lang::En, "no.such.family", 2, &[]),
            "no.such.family"
        );
        assert_eq!(
            plural_fmt_in(Lang::Pt, "x.y {n}", 3, &[]),
            "x.y {n}".replace("{n}", "3")
        );
    }

    #[test]
    fn formatted_lookup() {
        let a: Args = &[("n", Arg::Int(3)), ("size", bytes(1536))];
        assert_eq!(
            tr_fmt_in(
                Lang::Pt,
                "fmt.time.24",
                &[("hour", Arg::Pad(7, 2)), ("minute", Arg::Pad(5, 2))]
            ),
            "07:05"
        );
        // A missing key formats the key itself: placeholders in it still work.
        assert_eq!(tr_fmt_in(Lang::En, "x {n} {size}", a), "x 3 1.5 KiB");
        assert_eq!(tr_fmt_in(Lang::Pt, "x {n} {size}", a), "x 3 1,5 KiB");
    }

    #[test]
    fn macros() {
        let _g = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_lang(Lang::En);
        assert_eq!(crate::t!("meta.name"), "English");
        assert_eq!(crate::t!("fmt.am"), "AM");
        assert_eq!(crate::t!("no.key.here", n = 7), "no.key.here");
        assert_eq!(crate::tp!("no.family", 3u32), "no.family");
        assert_eq!(crate::tk!("a.b"), "a.b");
        set_lang(Lang::Pt);
        assert_eq!(crate::t!("meta.name"), "Português (Brasil)");
    }

    #[test]
    fn date_helpers_follow_the_current_language() {
        let _g = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let c = Civil {
            year: 2026,
            month: 10,
            day: 8,
            weekday: 4,
            hour: 23,
            minute: 49,
            second: 0,
        };
        set_lang(Lang::Pt);
        assert_eq!(
            format_date(c, DateStyle::Full, true),
            "qui, 8 out 2026 23:49"
        );
        assert_eq!(format_time(c, true, false), "23:49");
        assert_eq!(format_size(1536), "1,5 KiB");
        assert_eq!(format_num(1234567), "1.234.567");
        set_lang(Lang::En);
        assert_eq!(
            format_date(c, DateStyle::Full, false),
            "Thu, Oct 8 2026 11:49 PM"
        );
        assert_eq!(format_time(c, false, true), "11:49:00 PM");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_num(1234567), "1,234,567");
        set_lang(Lang::Pt);
    }
}
