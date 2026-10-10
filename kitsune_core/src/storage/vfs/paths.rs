//! paths (split out of `vfs.rs`).

use super::*;

/// `dir` + `/` + `name` (`dir == "/"` gives `/name`).
pub fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(dir.len() + 1 + name.len());
    out.extend_from_slice(dir);
    if dir != b"/" {
        out.push(b'/');
    }
    out.extend_from_slice(name);
    out
}

/// The folder containing `path` (`/` for a top-level item and for `/` itself).
pub fn parent(path: &[u8]) -> Vec<u8> {
    match path.iter().rposition(|&b| b == b'/') {
        Some(0) | None => b"/".to_vec(),
        Some(i) => path[..i].to_vec(),
    }
}

/// The last component of `path` (empty for `/`).
pub fn base_name(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|&b| b == b'/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// True for `/`.
pub fn is_root(path: &[u8]) -> bool {
    path == b"/"
}

/// True if `path` is `ancestor` or lies below it (component-wise).
pub fn is_inside(path: &[u8], ancestor: &[u8]) -> bool {
    if is_root(ancestor) {
        return path.first() == Some(&b'/');
    }
    path == ancestor || (path.starts_with(ancestor) && path.get(ancestor.len()) == Some(&b'/'))
}

/// The components of an absolute path (`/a/b` gives `["a", "b"]`).
pub fn components(path: &[u8]) -> Vec<&[u8]> {
    path.split(|&b| b == b'/')
        .filter(|c| !c.is_empty())
        .collect()
}

/// Strip surrounding ASCII spaces from a typed name.
pub fn trim_name(raw: &[u8]) -> &[u8] {
    let mut s = raw;
    while let [b' ', rest @ ..] = s {
        s = rest;
    }
    while let [rest @ .., b' '] = s {
        s = rest;
    }
    s
}

/// Check a file or folder name: 1..=255 bytes of valid UTF-8, no `/`, NUL or control
/// characters, not `.` or `..`.
pub fn validate_name(name: &[u8]) -> Result<()> {
    if name.is_empty() || name == b"." || name == b".." {
        return Err(VfsError::InvalidName);
    }
    if name.len() > MAX_NAME {
        return Err(VfsError::NameTooLong);
    }
    if name.iter().any(|&b| b == b'/' || b < 0x20 || b == 0x7F)
        || core::str::from_utf8(name).is_err()
    {
        return Err(VfsError::InvalidName);
    }
    Ok(())
}

/// Split `name` into stem and extension (with the dot). A leading dot (`.profile`)
/// or a trailing one does not start an extension.
pub fn split_ext(name: &[u8]) -> (&[u8], &[u8]) {
    match name.iter().rposition(|&b| b == b'.') {
        Some(i) if i > 0 && i + 1 < name.len() => (&name[..i], &name[i..]),
        _ => (name, &[]),
    }
}

/// Remove a trailing `" (N)"` (N decimal) from a stem; returns the bare stem.
pub(super) fn strip_copy_suffix(stem: &[u8]) -> &[u8] {
    let Some((b')', body)) = stem.split_last() else {
        return stem;
    };
    let Some(open) = body.iter().rposition(|&b| b == b'(') else {
        return stem;
    };
    let digits = &body[open + 1..];
    if open >= 1
        && body[open - 1] == b' '
        && !digits.is_empty()
        && digits.iter().all(u8::is_ascii_digit)
    {
        return &body[..open - 1];
    }
    stem
}

/// A name that is free according to `taken`: `name` itself if it is, else
/// `stem (2).ext`, `stem (3).ext`, ... An existing `" (N)"` in the stem is
/// replaced, so copying `a (2).txt` yields `a (3).txt`, not `a (2) (2).txt`.
/// The result never exceeds 255 bytes (the stem is cut on a UTF-8 boundary).
pub fn unique_name(name: &[u8], mut taken: impl FnMut(&[u8]) -> bool) -> Vec<u8> {
    if !taken(name) {
        return name.to_vec();
    }
    let (stem, ext) = split_ext(name);
    let stem = strip_copy_suffix(stem);
    for n in 2u32..100_000 {
        let mut tail = Vec::new();
        tail.extend_from_slice(b" (");
        push_u32(&mut tail, n);
        tail.push(b')');
        tail.extend_from_slice(ext);
        let room = MAX_NAME.saturating_sub(tail.len());
        let mut cut = stem.len().min(room);
        // Back up to a UTF-8 character boundary (continuation bytes are 10xxxxxx).
        while cut > 0 && cut < stem.len() && stem[cut] & 0xC0 == 0x80 {
            cut -= 1;
        }
        let mut cand = stem[..cut].to_vec();
        cand.extend_from_slice(&tail);
        if !taken(&cand) {
            return cand;
        }
    }
    name.to_vec()
}

pub(super) fn push_u32(out: &mut Vec<u8>, mut v: u32) {
    let mut tmp = [0u8; 10];
    let mut i = tmp.len();
    loop {
        i -= 1;
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    out.extend_from_slice(&tmp[i..]);
}
