//! Path normalization for the app sandbox.
//!
//! A guest path is **never** concatenated as text. [`normalize`] turns the raw
//! bytes into a canonical absolute path (`/` or `/a/b`) made only of validated
//! components, resolving `.`, `..` and repeated slashes lexically. A `..` that
//! would climb above the guest's root is an **error** ([`PathError::Escapes`]),
//! not a silent clamp: an app that tries `../../etc/x` must learn it did wrong.
//!
//! Component grammar: 1..=48 bytes of printable ASCII, none of `\ : * ? " < > |`
//! (and no NUL/control/non-ASCII), not made only of dots, no leading space, no
//! trailing space or dot. Whole path <= 256 bytes, <= 8 components after
//! normalization. A component such as `%2e%2e` is just a (legal) file name: the
//! sandbox never decodes anything.

use alloc::string::String;
use alloc::vec::Vec;

pub const MAX_PATH: usize = 256;
pub const MAX_COMPONENT: usize = 48;
pub const MAX_DEPTH: usize = 8;

/// Why a path was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathError {
    Empty,
    TooLong,
    /// Invalid byte or component name.
    BadName,
    TooDeep,
    /// `..` above the guest's root.
    Escapes,
}

/// Is `c` allowed inside a component?
fn name_byte_ok(c: u8) -> bool {
    (0x20..=0x7E).contains(&c)
        && !matches!(c, b'\\' | b':' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|')
}

/// Validates one component (not `.` / `..`, which [`normalize`] handles).
pub fn valid_component(c: &[u8]) -> bool {
    if c.is_empty() || c.len() > MAX_COMPONENT {
        return false;
    }
    if !c.iter().copied().all(name_byte_ok) {
        return false;
    }
    if c.iter().all(|&b| b == b'.') {
        return false;
    }
    c[0] != b' ' && !matches!(c[c.len() - 1], b' ' | b'.')
}

/// Canonical absolute form of `raw` relative to the guest's root.
pub fn normalize(raw: &[u8]) -> Result<String, PathError> {
    if raw.is_empty() {
        return Err(PathError::Empty);
    }
    if raw.len() > MAX_PATH {
        return Err(PathError::TooLong);
    }
    if !raw.iter().all(|&c| c == b'/' || name_byte_ok(c)) {
        return Err(PathError::BadName);
    }
    let mut stack: Vec<&[u8]> = Vec::new();
    for comp in raw.split(|&c| c == b'/') {
        match comp {
            b"" | b"." => {}
            b".." => {
                if stack.pop().is_none() {
                    return Err(PathError::Escapes);
                }
            }
            c => {
                if !valid_component(c) {
                    return Err(PathError::BadName);
                }
                stack.push(c);
                if stack.len() > MAX_DEPTH {
                    return Err(PathError::TooDeep);
                }
            }
        }
    }
    let mut out = String::with_capacity(raw.len().min(MAX_PATH) + 1);
    if stack.is_empty() {
        out.push('/');
    }
    for c in stack {
        out.push('/');
        // every byte was validated as printable ASCII
        for &b in c {
            out.push(b as char);
        }
    }
    Ok(out)
}

/// `prefix` + canonical `rel` (both already canonical): `("/data/x", "/a")` ->
/// `/data/x/a`; `("/data/x", "/")` -> `/data/x`.
pub fn join(prefix: &str, rel: &str) -> String {
    if rel == "/" {
        return String::from(if prefix.is_empty() { "/" } else { prefix });
    }
    let mut s = String::with_capacity(prefix.len() + rel.len());
    s.push_str(prefix);
    s.push_str(rel);
    s
}

/// Splits a canonical path into `(parent, name)`; the root has no parent.
pub fn split(path: &str) -> Option<(&str, &str)> {
    if path == "/" {
        return None;
    }
    let i = path.rfind('/')?;
    let parent = if i == 0 { "/" } else { &path[..i] };
    Some((parent, &path[i + 1..]))
}

/// Is `path` equal to `dir` or inside it? Both canonical.
pub fn within(path: &str, dir: &str) -> bool {
    if dir == "/" {
        return path.starts_with('/');
    }
    path == dir || (path.starts_with(dir) && path.as_bytes().get(dir.len()) == Some(&b'/'))
}
