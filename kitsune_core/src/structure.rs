//! Keeps the module groups honest: scans the sources for `crate::<group>::...` paths and fails
//! when a group depends on one it is not allowed to (the graph is in
//! `docs/design/code-structure.md`). Test-only modules are skipped, and so are comments, so a
//! doc link never counts as a dependency.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::string::String;
use std::vec::Vec;

use crate::testutil::test_modules;

/// Every group and the groups it may use (besides itself). Keep in step with the table in
/// `docs/design/code-structure.md`.
const ALLOWED: &[(&str, &[&str])] = &[
    ("format", &[]),
    ("hw", &["network", "storage"]),
    ("i18n", &["hw"]),
    ("storage", &[]),
    ("network", &["format"]),
    ("browsing", &["format", "i18n", "network", "ui"]),
    ("platform", &["browsing", "format", "i18n", "storage"]),
    ("ui", &["format", "i18n", "windowing"]),
    ("windowing", &["format", "i18n", "ui"]),
    ("system", &["format", "hw", "i18n", "ui", "windowing"]),
    (
        "apps",
        &[
            "format",
            "i18n",
            "platform",
            "storage",
            "system",
            "ui",
            "windowing",
        ],
    ),
];

fn src_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut items: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    items.sort();
    for p in items {
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// Group names and the modules each owns: `{"ui": {"gfx", ...}, ...}`.
fn layout() -> BTreeMap<String, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    let root = src_root();
    for e in fs::read_dir(&root).unwrap().flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let group = p.file_name().unwrap().to_string_lossy().into_owned();
        let mut mods = BTreeSet::new();
        for m in fs::read_dir(&p).unwrap().flatten() {
            let q = m.path();
            let name = q.file_stem().unwrap().to_string_lossy().into_owned();
            if name != "mod" {
                mods.insert(name);
            }
        }
        out.insert(group, mods);
    }
    out
}

/// `(from group, to group, file, line)` of every production-code dependency, plus the
/// flat-facade paths (`crate::gfx`) found inside the crate.
/// `(from group, to group, file, line)`.
type Dep = (String, String, String, usize);

fn scan() -> (Vec<Dep>, Vec<String>) {
    let lay = layout();
    let root = src_root();
    let mut files = Vec::new();
    rs_files(&root, &mut files);
    let mut skip = BTreeSet::new();
    let mut srcs = Vec::new();
    for f in &files {
        let s = fs::read_to_string(f).unwrap();
        test_modules(f, &s, &mut skip);
        srcs.push((f.clone(), s));
    }
    let owner: BTreeMap<&str, &str> = lay
        .iter()
        .flat_map(|(g, ms)| ms.iter().map(move |m| (m.as_str(), g.as_str())))
        .collect();
    let mut deps = Vec::new();
    let mut flat = Vec::new();
    for (f, s) in &srcs {
        if f.ancestors().any(|a| skip.contains(a)) || f.ends_with("structure.rs") {
            continue;
        }
        let rel = f.strip_prefix(&root).unwrap();
        let first = rel
            .components()
            .next()
            .unwrap()
            .as_os_str()
            .to_string_lossy();
        let from = if lay.contains_key(first.as_ref()) {
            first.into_owned()
        } else {
            rel.file_stem().unwrap().to_string_lossy().into_owned()
        };
        if from == "lib" || from == "testutil" {
            continue;
        }
        // Inline `#[cfg(test)] mod tests { .. }` blocks run to the end of the file.
        let body = match s.find("\n#[cfg(test)]") {
            Some(i) => &s[..i],
            None => s.as_str(),
        };
        for (n, line) in body.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            let mut rest = line;
            while let Some(i) = rest.find("crate::") {
                let after = &rest[i + 7..];
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                rest = after;
                if name.is_empty() {
                    continue;
                }
                let to = if lay.contains_key(&name) || name == "hw" || name == "i18n" {
                    Some(name.clone())
                } else if let Some(g) = owner.get(name.as_str()) {
                    flat.push(std::format!(
                        "{}:{}: `crate::{name}` should be `crate::{g}::{name}`",
                        rel.display(),
                        n + 1
                    ));
                    Some((*g).to_string())
                } else {
                    None
                };
                if let Some(to) = to
                    && to != from
                {
                    deps.push((from.clone(), to, rel.display().to_string(), n + 1));
                }
            }
        }
    }
    (deps, flat)
}

#[test]
fn groups_only_use_the_groups_they_may() {
    let allowed: BTreeMap<&str, &[&str]> = ALLOWED.iter().copied().collect();
    let (deps, _) = scan();
    let mut bad = Vec::new();
    for (from, to, file, line) in &deps {
        match allowed.get(from.as_str()) {
            Some(ok) if ok.contains(&to.as_str()) => {}
            Some(_) => bad.push(std::format!("{file}:{line}: `{from}` must not use `{to}`")),
            None => bad.push(std::format!("{file}:{line}: `{from}` is not a known group")),
        }
    }
    assert!(
        bad.is_empty(),
        "forbidden dependencies:\n{}",
        bad.join("\n")
    );
}

#[test]
fn every_allowed_edge_is_used_or_removed_from_the_table() {
    let (deps, _) = scan();
    let used: BTreeSet<(&str, &str)> = deps
        .iter()
        .map(|(f, t, _, _)| (f.as_str(), t.as_str()))
        .collect();
    let mut stale = Vec::new();
    for (from, tos) in ALLOWED {
        for to in *tos {
            if !used.contains(&(*from, *to)) {
                stale.push(std::format!("{from} -> {to}"));
            }
        }
    }
    assert!(
        stale.is_empty(),
        "allowed but unused (tighten the table and the docs): {stale:?}"
    );
}

#[test]
fn inside_the_crate_paths_name_their_group() {
    let (_, flat) = scan();
    assert!(
        flat.is_empty(),
        "use the grouped path inside the crate:\n{}",
        flat.join("\n")
    );
}

#[test]
fn the_leaf_groups_depend_on_nothing_they_should_not() {
    // The rules the owner of the tree cares most about, spelled out so a failure reads well.
    let (deps, _) = scan();
    let forbid: &[(&str, &str)] = &[
        ("storage", "i18n"),
        ("storage", "ui"),
        ("storage", "apps"),
        ("storage", "browsing"),
        ("storage", "network"),
        ("network", "ui"),
        ("network", "apps"),
        ("network", "browsing"),
        ("format", "ui"),
        ("format", "storage"),
        ("ui", "apps"),
        ("ui", "storage"),
        ("windowing", "apps"),
        ("platform", "apps"),
        ("browsing", "apps"),
    ];
    for (f, t, file, line) in &deps {
        assert!(
            !forbid.contains(&(f.as_str(), t.as_str())),
            "{file}:{line}: `{f}` must never depend on `{t}`"
        );
    }
}
