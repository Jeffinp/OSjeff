//! Helpers shared by the unit tests (`cfg(test)` only): hex decoding for codec vectors and the
//! finder of test-only modules that `structure.rs` and the i18n audit use to skip fixtures.

use alloc::vec::Vec;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Decodes a lowercase/uppercase hex string.
pub(crate) fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

/// Paths of the modules declared `#[cfg(test)] mod name;` next to `file`.
pub(crate) fn test_modules(file: &Path, src: &str, skip: &mut BTreeSet<PathBuf>) {
    let dir = match file.file_name().and_then(|n| n.to_str()) {
        Some("mod.rs" | "lib.rs") => file.parent().unwrap().to_path_buf(),
        _ => file.with_extension(""),
    };
    let lines: Vec<&str> = src.lines().collect();
    for (i, l) in lines.iter().enumerate() {
        if l.trim() != "#[cfg(test)]" {
            continue;
        }
        let Some(next) = lines.get(i + 1) else {
            continue;
        };
        let t = next.trim();
        let t = t
            .strip_prefix("pub(crate) ")
            .or_else(|| t.strip_prefix("pub "))
            .unwrap_or(t);
        if let Some(name) = t.strip_prefix("mod ").and_then(|r| r.strip_suffix(';')) {
            skip.insert(dir.join(format!("{name}.rs")));
            skip.insert(dir.join(name));
        }
    }
}
