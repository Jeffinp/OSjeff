//! Host checks over the catalogs and the sources (`cargo test -p osjeff_core i18n`).
//!
//! * the two catalogs define exactly the same keys with the same placeholders, and plural
//!   families are complete;
//! * every key used in the sources exists, and every key in the catalogs is used;
//! * Portuguese text has its accents (word list in `tools/i18n/accents.txt`), both in the
//!   catalog and in the string literals of the files already migrated;
//! * no text names the implementation language or toolchain;
//! * both fonts have a glyph for every character of both catalogs.
//!
//! `cargo test -p osjeff_core i18n_report -- --ignored --nocapture` prints, for the whole tree,
//! the literals that look like Portuguese without accents (the migration backlog).

use super::{Lang, placeholders};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::string::String;
use std::vec::Vec;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn entries(l: Lang) -> BTreeMap<&'static str, &'static str> {
    l.catalog().entries().iter().copied().collect()
}

// ----------------------------------------------------------------- source tokens

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Punct(char),
    Str(String),
}

/// Rust tokens good enough to find calls and string literals: comments are skipped, char
/// literals and lifetimes are told apart, raw strings are handled. Returns `(token, line)`.
fn tokenize(src: &str) -> Vec<(Tok, usize)> {
    let b: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut line) = (0, 1);
    while i < b.len() {
        let c = b[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '/' && b.get(i + 1) == Some(&'/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && b.get(i + 1) == Some(&'*') {
            let mut depth = 1;
            i += 2;
            while i < b.len() && depth > 0 {
                if b[i] == '\n' {
                    line += 1;
                }
                if b[i] == '/' && b.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if b[i] == '*' && b.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
        } else if c == '"' || (c == 'b' && b.get(i + 1) == Some(&'"')) {
            let start_line = line;
            // `b"..."` byte strings are data, not text: skip them without a token.
            let is_bytes = c == 'b';
            i += if is_bytes { 2 } else { 1 };
            let mut s = String::new();
            while i < b.len() && b[i] != '"' {
                if b[i] == '\n' {
                    line += 1;
                }
                if b[i] == '\\' && i + 1 < b.len() {
                    i += 1;
                    match b[i] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        '0' => s.push('\0'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        '\'' => s.push('\''),
                        'u' => {
                            // \u{XXXX}
                            let mut hex = String::new();
                            i += 1;
                            if b.get(i) == Some(&'{') {
                                i += 1;
                                while i < b.len() && b[i] != '}' {
                                    hex.push(b[i]);
                                    i += 1;
                                }
                            }
                            if let Some(ch) =
                                u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                            {
                                s.push(ch);
                            }
                        }
                        '\n' => {
                            // line continuation: skip the newline and the next indentation
                            line += 1;
                            while i + 1 < b.len() && b[i + 1].is_whitespace() {
                                if b[i + 1] == '\n' {
                                    line += 1;
                                }
                                i += 1;
                            }
                        }
                        'x' => {
                            i += 2;
                        }
                        other => s.push(other),
                    }
                    i += 1;
                } else {
                    s.push(b[i]);
                    i += 1;
                }
            }
            i += 1;
            if !is_bytes {
                out.push((Tok::Str(s), start_line));
            }
        } else if c == 'r' && matches!(b.get(i + 1), Some('"') | Some('#')) && raw_start(&b, i) {
            let start_line = line;
            let mut j = i + 1;
            let mut hashes = 0;
            while b.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            j += 1; // the quote
            let mut s = String::new();
            'raw: while j < b.len() {
                if b[j] == '"' {
                    let mut k = 0;
                    while k < hashes && b.get(j + 1 + k) == Some(&'#') {
                        k += 1;
                    }
                    if k == hashes {
                        j += 1 + hashes;
                        break 'raw;
                    }
                }
                if b[j] == '\n' {
                    line += 1;
                }
                s.push(b[j]);
                j += 1;
            }
            i = j;
            out.push((Tok::Str(s), start_line));
        } else if c == '\'' {
            // char literal or lifetime
            if b.get(i + 1) == Some(&'\\') {
                i += 2;
                while i < b.len() && b[i] != '\'' {
                    i += 1;
                }
                i += 1;
            } else if b.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1; // a lifetime tick
            }
        } else if c.is_alphabetic() || c == '_' {
            let mut s = String::new();
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_') {
                s.push(b[i]);
                i += 1;
            }
            out.push((Tok::Ident(s), line));
        } else if c.is_ascii_digit() {
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_' || b[i] == '.') {
                // stop before `..` ranges
                if b[i] == '.' && b.get(i + 1) == Some(&'.') {
                    break;
                }
                i += 1;
            }
        } else {
            out.push((Tok::Punct(c), line));
            i += 1;
        }
    }
    out
}

/// The tokens without the bodies of `#[cfg(test)] mod name { ... }` (their keys are fixtures).
fn without_test_modules(toks: Vec<(Tok, usize)>) -> Vec<(Tok, usize)> {
    let is = |i: usize, t: &Tok| toks.get(i).is_some_and(|x| &x.0 == t);
    let id = |n: &str| Tok::Ident(n.into());
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        // # [ cfg ( test ) ] [pub] mod name {
        if is(i, &Tok::Punct('#'))
            && is(i + 1, &Tok::Punct('['))
            && is(i + 2, &id("cfg"))
            && is(i + 3, &Tok::Punct('('))
            && is(i + 4, &id("test"))
            && is(i + 5, &Tok::Punct(')'))
            && is(i + 6, &Tok::Punct(']'))
        {
            let mut j = i + 7;
            if is(j, &id("pub")) {
                j += 1;
            }
            if is(j, &id("mod")) && matches!(toks.get(j + 2).map(|t| &t.0), Some(Tok::Punct('{'))) {
                let mut depth = 0;
                j += 2;
                while j < toks.len() {
                    match toks[j].0 {
                        Tok::Punct('{') => depth += 1,
                        Tok::Punct('}') => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                i = j + 1;
                continue;
            }
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

/// `r"`, `r#"`, `r##"`... at `i` (and not the tail of an identifier).
fn raw_start(b: &[char], i: usize) -> bool {
    if i > 0 && (b[i - 1].is_alphanumeric() || b[i - 1] == '_') {
        return false;
    }
    let mut j = i + 1;
    while b.get(j) == Some(&'#') {
        j += 1;
    }
    b.get(j) == Some(&'"')
}

/// Names that look a key up, and whether their key is a plural family (`.one`/`.other`).
const CALLS: &[(&str, bool, bool)] = &[
    // (name, is_macro, plural)
    ("t", true, false),
    ("tk", true, false),
    ("tp", true, true),
    ("tr", false, false),
    ("tr_in", false, false),
    ("tr_fmt", false, false),
    ("tr_fmt_in", false, false),
    ("plural", false, true),
    ("plural_in", false, true),
    ("plural_fmt", false, true),
    ("plural_fmt_in", false, true),
];

#[derive(Debug)]
struct KeyUse {
    key: String,
    plural: bool,
    file: String,
    line: usize,
}

/// The key literals passed to the lookup API in `src`.
fn key_uses(src: &str, file: &str) -> Vec<KeyUse> {
    let toks = without_test_modules(tokenize(src));
    let mut out = Vec::new();
    for (i, (t, line)) in toks.iter().enumerate() {
        let Tok::Ident(name) = t else { continue };
        let Some(&(_, is_macro, plural)) = CALLS.iter().find(|c| c.0 == name) else {
            continue;
        };
        // Not a method or a path tail of something else: `x.tr(` is not ours; `i18n::tr(` is.
        if i > 0 && matches!(toks[i - 1].0, Tok::Punct('.')) {
            continue;
        }
        if i > 0 && matches!(&toks[i - 1].0, Tok::Ident(k) if k == "fn") {
            continue;
        }
        let mut j = i + 1;
        if is_macro {
            if !matches!(toks.get(j).map(|t| &t.0), Some(Tok::Punct('!'))) {
                continue;
            }
            j += 1;
        }
        if !matches!(toks.get(j).map(|t| &t.0), Some(Tok::Punct('(' | '['))) {
            continue;
        }
        j += 1;
        // The key is the first argument, or the second for the `_in(lang, key)` forms.
        if name.ends_with("_in") {
            let mut depth = 0;
            while j < toks.len() {
                match &toks[j].0 {
                    Tok::Punct('(' | '[' | '{') => depth += 1,
                    Tok::Punct(')' | ']' | '}') => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    }
                    Tok::Punct(',') if depth == 0 => {
                        j += 1;
                        break;
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        if let Some((Tok::Str(k), _)) = toks.get(j) {
            out.push(KeyUse {
                key: k.clone(),
                plural,
                file: file.into(),
                line: *line,
            });
        }
    }
    out
}

fn is_valid_key(k: &str) -> bool {
    let segs: Vec<&str> = k.split('.').collect();
    segs.len() >= 2
        && k.len() <= 80
        && segs.iter().all(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

/// Rust files under `dir`, recursively (empty when the directory is missing).
fn rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(dir) else {
        return out;
    };
    let mut items: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    items.sort();
    for p in items {
        if p.is_dir() {
            out.extend(rs_files(&p));
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
    out
}

/// The sources the key checks read: the kernel and the core, minus this file (its
/// fixtures are made-up keys).
fn source_files() -> Vec<(String, String)> {
    let root = repo_root();
    let skip = ["audit.rs"];
    let mut out = Vec::new();
    for dir in ["kernel/src", "osjeff_core/src"] {
        for p in rs_files(&root.join(dir)) {
            let rel = p
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if rel.starts_with("osjeff_core/src/i18n/")
                && skip.contains(&p.file_name().unwrap().to_str().unwrap())
            {
                continue;
            }
            if let Ok(s) = fs::read_to_string(&p) {
                out.push((rel, s));
            }
        }
    }
    out
}

// -------------------------------------------------------------------- catalog checks

#[test]
fn catalogs_define_the_same_keys() {
    let (pt, en) = (entries(Lang::Pt), entries(Lang::En));
    let only_pt: Vec<_> = pt.keys().filter(|k| !en.contains_key(*k)).collect();
    let only_en: Vec<_> = en.keys().filter(|k| !pt.contains_key(*k)).collect();
    assert!(
        only_pt.is_empty() && only_en.is_empty(),
        "keys only in pt: {only_pt:?}\nkeys only in en: {only_en:?}"
    );
}

#[test]
fn catalogs_use_the_same_placeholders() {
    let (pt, en) = (entries(Lang::Pt), entries(Lang::En));
    let mut bad = Vec::new();
    for (k, pv) in &pt {
        let Some(ev) = en.get(k) else { continue };
        // `{Month_long}` and `{month_long}` name the same argument.
        fn set(v: &str) -> BTreeSet<String> {
            placeholders(v)
                .into_iter()
                .map(|p| {
                    let mut c = p.chars();
                    c.next()
                        .map(|f| f.to_lowercase().chain(c).collect())
                        .unwrap_or_default()
                })
                .collect()
        }
        if set(pv) != set(ev) {
            bad.push(std::format!("{k}: pt {:?} vs en {:?}", set(pv), set(ev)));
        }
    }
    assert!(bad.is_empty(), "placeholder mismatch:\n{}", bad.join("\n"));
}

#[test]
fn keys_and_values_are_well_formed() {
    let mut bad = Vec::new();
    for l in Lang::ALL {
        for (k, v) in l.catalog().entries() {
            if !is_valid_key(k) {
                bad.push(std::format!("{}: bad key syntax {k:?}", l.code()));
            }
            if v.chars().any(|c| c.is_control()) {
                bad.push(std::format!("{}: {k}: control character", l.code()));
            }
            // Braces must be escaped or form a complete {name}.
            let mut depth = 0i32;
            let mut it = v.chars().peekable();
            while let Some(c) = it.next() {
                match c {
                    '{' if it.peek() == Some(&'{') => {
                        it.next();
                    }
                    '}' if it.peek() == Some(&'}') => {
                        it.next();
                    }
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                if !(0..=1).contains(&depth) {
                    break;
                }
            }
            if depth != 0 {
                bad.push(std::format!(
                    "{}: {k}: unbalanced braces in {v:?}",
                    l.code()
                ));
            }
            if v.trim() != *v {
                bad.push(std::format!("{}: {k}: surrounding blanks", l.code()));
            }
            if v.is_empty() && !k.starts_with("meta.") {
                bad.push(std::format!("{}: {k}: empty value", l.code()));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn plural_families_are_complete() {
    for l in Lang::ALL {
        let e = entries(l);
        for k in e.keys() {
            if let Some(base) = k.strip_suffix(".one") {
                assert!(
                    e.contains_key(std::format!("{base}.other").as_str()),
                    "{}: {k} has no .other",
                    l.code()
                );
            }
        }
    }
}

#[test]
fn every_language_defines_the_format_keys() {
    for l in Lang::ALL {
        let e = entries(l);
        for k in [
            "meta.code",
            "meta.name",
            "fmt.decimal",
            "fmt.group",
            "fmt.plural",
            "fmt.am",
            "fmt.pm",
            "fmt.clock24_default",
        ] {
            assert!(e.contains_key(k), "{}: missing {k}", l.code());
        }
        assert_eq!(e["fmt.decimal"].chars().count(), 1);
        assert_eq!(e["fmt.group"].chars().count(), 1);
        assert_ne!(e["fmt.decimal"], e["fmt.group"]);
        assert!(matches!(e["fmt.clock24_default"], "0" | "1"));
        assert!(
            matches!(e["fmt.plural"], "one" | "zero-one" | "invariant"),
            "{}: unknown plural rule",
            l.code()
        );
    }
}

// ------------------------------------------------------------------------ source checks

/// Every key literal in the sources exists in both catalogs, every catalog key is used.
#[test]
fn source_keys_match_the_catalogs() {
    let files = source_files();
    if files.iter().all(|(f, _)| !f.starts_with("kernel/")) {
        return; // built from a package without the kernel: nothing to scan
    }
    let (pt, en) = (entries(Lang::Pt), entries(Lang::En));
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut missing = Vec::new();
    for (file, src) in &files {
        for u in key_uses(src, file) {
            if u.plural {
                for suffix in ["other"] {
                    let full = std::format!("{}.{suffix}", u.key);
                    used.insert(std::format!("{}.one", u.key));
                    used.insert(full.clone());
                    for (name, cat) in [("pt", &pt), ("en", &en)] {
                        if !cat.contains_key(full.as_str()) {
                            missing.push(std::format!(
                                "{}:{}: {name} lacks {full}",
                                u.file,
                                u.line
                            ));
                        }
                    }
                }
            } else {
                used.insert(u.key.clone());
                for (name, cat) in [("pt", &pt), ("en", &en)] {
                    if !cat.contains_key(u.key.as_str()) {
                        missing.push(std::format!(
                            "{}:{}: {name} lacks {:?}",
                            u.file,
                            u.line,
                            u.key
                        ));
                    }
                }
            }
        }
    }
    assert!(missing.is_empty(), "missing keys:\n{}", missing.join("\n"));
    let unused: Vec<_> = pt
        .keys()
        .filter(|k| !k.starts_with("meta.") && !used.contains(**k))
        .collect();
    assert!(
        unused.is_empty(),
        "keys in the catalogs that no source uses (remove them or use them):\n{unused:#?}"
    );
}

#[test]
fn key_scanner_finds_the_api_forms() {
    let src = r##"
        // t!("in.a.comment")
        let a = t!("a.b");
        let b = crate::i18n::tr("c.d");
        let c = tr_in(Lang::Pt, "e.f");
        let d = tp!("g.h", n, name = x);
        let e = [tk!("i.j"), tk!("k.l")];
        let f = plural_fmt_in(l, "m.n", 3, &[]);
        let g = x.tr("not.a.key");
        let h = "t!(\"in.a.string\")";
        let i = r#"tr("in.a.raw")"#;
        fn tr(k: &str) {}
        #[cfg(test)]
        mod tests {
            fn t() { let z = t!("fixture.key"); }
        }
        let after = tk!("after.tests");
        let j = tr_fmt("o.p", &[("q", Arg::Str("r.s"))]);
    "##;
    let keys: Vec<_> = key_uses(src, "x.rs")
        .into_iter()
        .map(|u| (u.key, u.plural))
        .collect();
    let want: Vec<(String, bool)> = [
        ("a.b", false),
        ("c.d", false),
        ("e.f", false),
        ("g.h", true),
        ("i.j", false),
        ("k.l", false),
        ("m.n", true),
        ("after.tests", false),
        ("o.p", false),
    ]
    .iter()
    .map(|(k, p)| (k.to_string(), *p))
    .collect();
    assert_eq!(keys, want);
}

#[test]
fn tokenizer_handles_chars_lifetimes_and_raw_strings() {
    let src = "fn f<'a>(x: &'a str) { let c = '\"'; let d = '\\''; let s = \"q\\\"uote\"; let r = r#\"a\"b\"#; }";
    let strs: Vec<_> = tokenize(src)
        .into_iter()
        .filter_map(|(t, _)| if let Tok::Str(s) = t { Some(s) } else { None })
        .collect();
    assert_eq!(strs, ["q\"uote", "a\"b"]);
}

// ------------------------------------------------------------------------- accents

struct Accents {
    exact: BTreeMap<String, String>,
    suffix: Vec<String>,
    allow: BTreeSet<String>,
}

fn accents() -> Accents {
    let mut a = Accents {
        exact: BTreeMap::new(),
        suffix: Vec::new(),
        allow: BTreeSet::new(),
    };
    for line in include_str!("../../../tools/i18n/accents.txt").lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(s) = line.strip_prefix('~') {
            a.suffix.push(s.into());
        } else if let Some(w) = line.strip_prefix('!') {
            a.allow.insert(w.into());
        } else if let Some((w, r)) = line.split_once('=') {
            a.exact.insert(w.into(), r.into());
        }
    }
    a
}

/// Words of `text` (runs of letters) that Portuguese writes with an accent but that appear
/// without: `(word, suggestion)`.
fn unaccented_words(a: &Accents, text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for w in text.split(|c: char| !c.is_alphabetic()) {
        if w.len() < 3 || !w.is_ascii() {
            continue;
        }
        let lw = w.to_ascii_lowercase();
        if a.allow.contains(&lw) {
            continue;
        }
        if let Some(r) = a.exact.get(&lw) {
            out.push((w.into(), r.clone()));
        } else if a
            .suffix
            .iter()
            .any(|s| lw.ends_with(s.as_str()) && lw.len() > s.len())
        {
            out.push((w.into(), "(accent)".into()));
        }
    }
    out
}

#[test]
fn the_word_list_is_big_and_sane() {
    let a = accents();
    assert!(a.exact.len() >= 150, "only {} stems", a.exact.len());
    for (w, r) in &a.exact {
        assert!(
            w.is_ascii() && !r.is_ascii(),
            "{w}={r}: the right form must have an accent"
        );
    }
}

#[test]
fn the_checker_catches_the_classics() {
    let a = accents();
    let bad = |t: &str| {
        unaccented_words(&a, t)
            .into_iter()
            .map(|p| p.0)
            .collect::<Vec<_>>()
    };
    assert_eq!(bad("Configuracoes"), ["Configuracoes"]);
    assert_eq!(bad("Nao tem pagina na area"), ["Nao", "pagina", "area"]);
    assert_eq!(bad("Ultima modificacao"), ["Ultima", "modificacao"]);
    assert_eq!(bad("nenhum arquivo, nenhuma pasta"), Vec::<String>::new());
    assert_eq!(bad("Configurações não há"), Vec::<String>::new());
    assert_eq!(bad("Mario"), Vec::<String>::new());
    assert_eq!(bad("selecionar versao"), ["versao"]);
}

#[test]
fn portuguese_catalog_text_has_its_accents() {
    let a = accents();
    let mut bad = Vec::new();
    for (k, v) in Lang::Pt.catalog().entries() {
        for (w, r) in unaccented_words(&a, v) {
            bad.push(std::format!("{k}: {w:?} should be {r}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn english_text_has_no_portuguese_leftovers() {
    // The accented Portuguese forms never occur in the English catalog.
    let a = accents();
    let right: BTreeSet<&str> = a.exact.values().map(String::as_str).collect();
    let mut bad = Vec::new();
    for (k, v) in Lang::En.catalog().entries() {
        for w in v.split(|c: char| !c.is_alphabetic()) {
            if right.contains(w.to_lowercase().as_str()) {
                bad.push(std::format!("{k}: {w:?}"));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn text_never_names_the_toolchain() {
    for l in Lang::ALL {
        for (k, v) in l.catalog().entries() {
            let lv = v.to_lowercase();
            for w in ["rust", "cargo", "rustc", "llvm", "no_std", "crate"] {
                assert!(
                    !lv.split(|c: char| !c.is_alphanumeric() && c != '_')
                        .any(|x| x == w),
                    "{}: {k} mentions {w}",
                    l.code()
                );
            }
        }
    }
}

/// Files whose user-visible literals are fully migrated: Portuguese text in them must carry
/// its accents. (The rest of the tree is the backlog printed by `i18n_report`.)
const ACCENT_STRICT: &[&str] = &[
    "kernel/src/desktop/panel.rs",
    "kernel/src/desktop/taskbar.rs",
    "kernel/src/desktop/overlays.rs",
    "kernel/src/desktop/shell.rs",
    "kernel/src/desktop/toasts_ui.rs",
    "kernel/src/desktop/chrome.rs",
    "osjeff_core/src/launcher.rs",
    // w30: Arquivos, Imagens, Editor.
    "kernel/src/desktop/files.rs",
    "kernel/src/desktop/files_ui.rs",
    "kernel/src/desktop/sysstore.rs",
    "kernel/src/desktop/vfs.rs",
    "osjeff_core/src/fileman.rs",
    "osjeff_core/src/fileman/apps.rs",
    "osjeff_core/src/fileman/ui.rs",
    "osjeff_core/src/vfs.rs",
];

fn unaccented_literals(file: &str, src: &str, a: &Accents) -> Vec<String> {
    let mut out = Vec::new();
    for (t, line) in tokenize(src) {
        if let Tok::Str(s) = t {
            // Skip identifiers-like strings (keys, paths, format-only text).
            if !s.contains(' ') && !s.chars().next().is_some_and(char::is_uppercase) {
                continue;
            }
            for (w, r) in unaccented_words(a, &s) {
                out.push(std::format!("{file}:{line}: {w:?} should be {r} in {s:?}"));
            }
        }
    }
    out
}

#[test]
fn migrated_sources_have_accents() {
    let a = accents();
    let files = source_files();
    let mut bad = Vec::new();
    let mut seen = 0;
    for (file, src) in &files {
        if ACCENT_STRICT.contains(&file.as_str()) {
            seen += 1;
            bad.extend(unaccented_literals(file, src, &a));
        }
    }
    if files.iter().any(|(f, _)| f.starts_with("kernel/")) {
        assert_eq!(seen, ACCENT_STRICT.len(), "a strict file is missing");
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The whole tree: literals that look like unaccented Portuguese. Not a pass/fail check.
#[test]
#[ignore = "prints the migration backlog: cargo test -p osjeff_core i18n_report -- --ignored --nocapture"]
fn i18n_report() {
    let a = accents();
    let mut n = 0;
    for (file, src) in source_files() {
        for l in unaccented_literals(&file, &src, &a) {
            std::println!("{l}");
            n += 1;
        }
    }
    std::println!("{n} literals without accents");
}

// ---------------------------------------------------------------------------- fonts

fn font_has(font: &crate::ttf::Font<'_>, c: char) -> bool {
    font.glyph_index(c) != 0
}

#[test]
fn fonts_cover_both_catalogs_and_the_portuguese_alphabet() {
    static FONTS: [(&str, &[u8]); 4] = [
        (
            "Inter Regular",
            include_bytes!("../../../assets/fonts/Inter-Regular.subset.ttf"),
        ),
        (
            "Inter Medium",
            include_bytes!("../../../assets/fonts/Inter-Medium.subset.ttf"),
        ),
        (
            "Inter SemiBold",
            include_bytes!("../../../assets/fonts/Inter-SemiBold.subset.ttf"),
        ),
        (
            "JetBrains Mono",
            include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.subset.ttf"),
        ),
    ];
    // Everything a Portuguese or English text can need, whatever the catalogs hold today.
    let required: String =
        "áàâãäéèêëíìîïóòôõöúùûüçñÁÀÂÃÄÉÈÊËÍÌÎÏÓÒÔÕÖÚÙÛÜÇÑ«»‘’“”„…–—−°ªº€£¥¿¡·•×÷±§¶©®™ß←→↑↓".into();
    let mut from_catalogs: BTreeSet<char> = BTreeSet::new();
    for l in Lang::ALL {
        for (k, v) in l.catalog().entries() {
            // Template syntax and keys are not drawn; the values are.
            let _ = k;
            from_catalogs.extend(v.chars());
        }
    }
    let mut missing = Vec::new();
    for (name, data) in FONTS {
        let font = crate::ttf::Font::parse(data).unwrap_or_else(|| panic!("{name}: no parse"));
        for c in required.chars() {
            // The mono face is the terminal and the editor; it only needs letters and
            // punctuation, not the arrows.
            if name == "JetBrains Mono" && "←→↑↓".contains(c) {
                continue;
            }
            if !font_has(&font, c) {
                missing.push(std::format!("{name}: U+{:04X} {c}", c as u32));
            }
        }
        if name != "JetBrains Mono" {
            for &c in &from_catalogs {
                if c != '\n' && !font_has(&font, c) {
                    missing.push(std::format!("{name}: catalog char U+{:04X} {c}", c as u32));
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "glyphs missing:\n{}",
        missing.join("\n")
    );
    // The check itself can fail: a snowman is in none of the subsets.
    let inter = crate::ttf::Font::parse(FONTS[0].1).unwrap();
    assert!(!font_has(&inter, '\u{2603}'));
}
