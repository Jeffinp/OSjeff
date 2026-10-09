//! The filesystem the shell talks to, as a trait, plus path helpers and an
//! in-memory implementation ([`MemFs`]) used by tests and fuzzing.
//!
//! The kernel implements [`ShellFs`] on top of OJFS v3 (absolute and relative
//! `/a/b/c` paths with `.` and `..`). The shell never touches
//! `kitsune_core::fs` directly, so the format can change without touching it.

use crate::tk;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Why a filesystem call failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FsErr {
    NotFound,
    NotADirectory,
    IsADirectory,
    AlreadyExists,
    NotEmpty,
    NoSpace,
    TooBig,
    NameTooLong,
    InvalidPath,
    ReadOnly,
    Io,
}

impl FsErr {
    /// The catalog key of the text for this error.
    pub const fn key(self) -> &'static str {
        match self {
            FsErr::NotFound => tk!("sh.fs.not_found"),
            FsErr::NotADirectory => tk!("sh.fs.not_dir"),
            FsErr::IsADirectory => tk!("sh.fs.is_dir"),
            FsErr::AlreadyExists => tk!("sh.fs.exists"),
            FsErr::NotEmpty => tk!("sh.fs.not_empty"),
            FsErr::NoSpace => tk!("sh.fs.no_space"),
            FsErr::TooBig => tk!("sh.fs.too_big"),
            FsErr::NameTooLong => tk!("sh.fs.name_long"),
            FsErr::InvalidPath => tk!("sh.fs.bad_path"),
            FsErr::ReadOnly => tk!("sh.fs.read_only"),
            FsErr::Io => tk!("sh.fs.io"),
        }
    }

    /// Short human-readable text (what `ls`/`cat` print after the file name), in the
    /// language in effect.
    pub fn message(self) -> &'static str {
        crate::i18n::tr(self.key())
    }
}

/// File or directory.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    File,
    Dir,
}

/// Result of [`ShellFs::stat`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stat {
    pub kind: Kind,
    pub size: u64,
}

/// One directory entry from [`ShellFs::list`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DirEntry {
    pub name: String,
    pub kind: Kind,
    pub size: u64,
}

/// Space accounting for `df`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FsUsage {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub files: u64,
    pub dirs: u64,
}

/// Filesystem operations the shell needs.
///
/// Paths may be absolute (`/a/b`) or relative to [`ShellFs::cwd`], and may
/// contain `.`, `..` and repeated slashes; implementations resolve them with
/// [`normalize`] (the default [`ShellFs::resolve`]). `..` above `/` stays at
/// `/`. Methods that only look take `&self` so tab completion can run while the
/// shell is idle.
pub trait ShellFs {
    /// Current working directory, absolute and normalized.
    fn cwd(&self) -> String;

    /// Change the working directory; the target must be a directory.
    fn set_cwd(&mut self, path: &str) -> Result<(), FsErr>;

    /// Absolute, normalized form of `path` (no existence check).
    fn resolve(&self, path: &str) -> String {
        normalize(&self.cwd(), path)
    }

    /// Kind and size of `path`.
    fn stat(&self, path: &str) -> Result<Stat, FsErr>;

    /// Whole content of a file.
    fn read(&mut self, path: &str) -> Result<Vec<u8>, FsErr>;

    /// Up to `len` bytes starting at `offset`. The default reads the whole
    /// file; implement it natively for large files so `head`/`tail` are cheap.
    fn read_at(&mut self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>, FsErr> {
        let all = self.read(path)?;
        let start = (offset as usize).min(all.len());
        let end = start.saturating_add(len).min(all.len());
        Ok(all[start..end].to_vec())
    }

    /// Create the file or truncate it, then write `data`. The parent must be an
    /// existing directory.
    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), FsErr>;

    /// Append to a file, creating it if needed.
    fn append(&mut self, path: &str, data: &[u8]) -> Result<(), FsErr>;

    /// Entries of a directory (any order; the shell sorts).
    fn list(&self, path: &str) -> Result<Vec<DirEntry>, FsErr>;

    /// Create one directory (the parent must exist).
    fn mkdir(&mut self, path: &str) -> Result<(), FsErr>;

    /// Remove a file or an empty directory.
    fn remove(&mut self, path: &str) -> Result<(), FsErr>;

    /// Rename/move. An existing destination file is replaced.
    fn rename(&mut self, from: &str, to: &str) -> Result<(), FsErr>;

    /// Space usage for `df` (defaults to zeros).
    fn usage(&self) -> FsUsage {
        FsUsage::default()
    }
}

/// Resolve `path` against `cwd` into an absolute path without `.`, `..` or
/// duplicate slashes. `..` never goes above the root.
pub fn normalize(cwd: &str, path: &str) -> String {
    fn walk(s: &str, stack: &mut Vec<String>) {
        for c in s.split('/') {
            match c {
                "" | "." => {}
                ".." => {
                    stack.pop();
                }
                x => stack.push(x.to_string()),
            }
        }
    }
    let mut stack: Vec<String> = Vec::new();
    if !path.starts_with('/') {
        walk(cwd, &mut stack);
    }
    walk(path, &mut stack);
    if stack.is_empty() {
        return "/".to_string();
    }
    let mut out = String::new();
    for s in &stack {
        out.push('/');
        out.push_str(s);
    }
    out
}

/// Directory part of an absolute normalized path (`/` for top-level items).
pub fn parent(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) | None => "/",
        Some(i) => &path[..i],
    }
}

/// Last component of a path (`""` for `/`).
pub fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or("")
}

/// Join a directory and a name.
pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        alloc::format!("{dir}{name}")
    } else {
        alloc::format!("{dir}/{name}")
    }
}

#[derive(Clone)]
enum Node {
    Dir,
    File(Vec<u8>),
}

/// An in-memory [`ShellFs`] with configurable limits, for tests, fuzzing and
/// host tools.
#[derive(Clone)]
pub struct MemFs {
    nodes: BTreeMap<String, Node>,
    cwd: String,
    /// Maximum number of files plus directories (excluding `/`).
    pub max_nodes: usize,
    /// Maximum bytes in one file.
    pub max_file: usize,
    /// Maximum bytes in all files together.
    pub max_total: usize,
    /// Maximum length of one path component.
    pub max_name: usize,
}

impl Default for MemFs {
    fn default() -> Self {
        Self::new()
    }
}

impl MemFs {
    pub fn new() -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert("/".to_string(), Node::Dir);
        Self {
            nodes,
            cwd: "/".to_string(),
            max_nodes: 4096,
            max_file: 16 * 1024 * 1024,
            max_total: usize::MAX,
            max_name: 255,
        }
    }

    /// A filesystem with tight limits (used by the fuzz targets).
    pub fn small() -> Self {
        let mut f = Self::new();
        f.max_nodes = 64;
        f.max_file = 4096;
        f.max_total = 32 * 1024;
        f.max_name = 24;
        f
    }

    /// Builder helper: create a file (and nothing else).
    pub fn with_file(mut self, path: &str, data: &[u8]) -> Self {
        let _ = self.write(path, data);
        self
    }

    /// Builder helper: create a directory.
    pub fn with_dir(mut self, path: &str) -> Self {
        let _ = self.mkdir(path);
        self
    }

    fn total_bytes(&self) -> usize {
        self.nodes
            .values()
            .map(|n| match n {
                Node::File(d) => d.len(),
                Node::Dir => 0,
            })
            .sum()
    }

    fn check_name(&self, abs: &str) -> Result<(), FsErr> {
        if abs.split('/').any(|c| c.len() > self.max_name) {
            return Err(FsErr::NameTooLong);
        }
        Ok(())
    }

    fn check_parent(&self, abs: &str) -> Result<(), FsErr> {
        if abs == "/" {
            return Err(FsErr::InvalidPath);
        }
        match self.nodes.get(parent(abs)) {
            Some(Node::Dir) => Ok(()),
            Some(Node::File(_)) => Err(FsErr::NotADirectory),
            None => Err(FsErr::NotFound),
        }
    }

    fn put_file(&mut self, abs: String, data: Vec<u8>, append: bool) -> Result<(), FsErr> {
        self.check_name(&abs)?;
        self.check_parent(&abs)?;
        let existing = match self.nodes.get(&abs) {
            Some(Node::Dir) => return Err(FsErr::IsADirectory),
            Some(Node::File(d)) => Some(d.len()),
            None => None,
        };
        if existing.is_none() && self.nodes.len() > self.max_nodes {
            return Err(FsErr::NoSpace);
        }
        let old = existing.unwrap_or(0);
        let new_len = if append { old + data.len() } else { data.len() };
        if new_len > self.max_file {
            return Err(FsErr::TooBig);
        }
        let others = self.total_bytes() - old;
        if others + new_len > self.max_total {
            return Err(FsErr::NoSpace);
        }
        match self.nodes.get_mut(&abs) {
            Some(Node::File(d)) if append => d.extend_from_slice(&data),
            _ => {
                self.nodes.insert(abs, Node::File(data));
            }
        }
        Ok(())
    }
}

impl ShellFs for MemFs {
    fn cwd(&self) -> String {
        self.cwd.clone()
    }

    fn set_cwd(&mut self, path: &str) -> Result<(), FsErr> {
        let abs = self.resolve(path);
        match self.nodes.get(&abs) {
            Some(Node::Dir) => {
                self.cwd = abs;
                Ok(())
            }
            Some(Node::File(_)) => Err(FsErr::NotADirectory),
            None => Err(FsErr::NotFound),
        }
    }

    fn stat(&self, path: &str) -> Result<Stat, FsErr> {
        match self.nodes.get(&self.resolve(path)) {
            Some(Node::Dir) => Ok(Stat {
                kind: Kind::Dir,
                size: 0,
            }),
            Some(Node::File(d)) => Ok(Stat {
                kind: Kind::File,
                size: d.len() as u64,
            }),
            None => Err(FsErr::NotFound),
        }
    }

    fn read(&mut self, path: &str) -> Result<Vec<u8>, FsErr> {
        match self.nodes.get(&self.resolve(path)) {
            Some(Node::File(d)) => Ok(d.clone()),
            Some(Node::Dir) => Err(FsErr::IsADirectory),
            None => Err(FsErr::NotFound),
        }
    }

    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), FsErr> {
        let abs = self.resolve(path);
        self.put_file(abs, data.to_vec(), false)
    }

    fn append(&mut self, path: &str, data: &[u8]) -> Result<(), FsErr> {
        let abs = self.resolve(path);
        self.put_file(abs, data.to_vec(), true)
    }

    fn list(&self, path: &str) -> Result<Vec<DirEntry>, FsErr> {
        let abs = self.resolve(path);
        match self.nodes.get(&abs) {
            Some(Node::Dir) => {}
            Some(Node::File(_)) => return Err(FsErr::NotADirectory),
            None => return Err(FsErr::NotFound),
        }
        let prefix = if abs == "/" {
            "/".to_string()
        } else {
            alloc::format!("{abs}/")
        };
        let mut out = Vec::new();
        for (k, v) in self.nodes.range(prefix.clone()..) {
            if !k.starts_with(&prefix) {
                break;
            }
            let rest = &k[prefix.len()..];
            if rest.is_empty() || rest.contains('/') {
                continue;
            }
            out.push(match v {
                Node::Dir => DirEntry {
                    name: rest.to_string(),
                    kind: Kind::Dir,
                    size: 0,
                },
                Node::File(d) => DirEntry {
                    name: rest.to_string(),
                    kind: Kind::File,
                    size: d.len() as u64,
                },
            });
        }
        Ok(out)
    }

    fn mkdir(&mut self, path: &str) -> Result<(), FsErr> {
        let abs = self.resolve(path);
        self.check_name(&abs)?;
        if self.nodes.contains_key(&abs) {
            return Err(FsErr::AlreadyExists);
        }
        self.check_parent(&abs)?;
        if self.nodes.len() > self.max_nodes {
            return Err(FsErr::NoSpace);
        }
        self.nodes.insert(abs, Node::Dir);
        Ok(())
    }

    fn remove(&mut self, path: &str) -> Result<(), FsErr> {
        let abs = self.resolve(path);
        if abs == "/" {
            return Err(FsErr::InvalidPath);
        }
        match self.nodes.get(&abs) {
            None => return Err(FsErr::NotFound),
            Some(Node::Dir) => {
                if !self.list(&abs)?.is_empty() {
                    return Err(FsErr::NotEmpty);
                }
                // Removing the directory we stand in would leave a dangling cwd.
                if self.cwd == abs || self.cwd.starts_with(&alloc::format!("{abs}/")) {
                    return Err(FsErr::InvalidPath);
                }
            }
            Some(Node::File(_)) => {}
        }
        self.nodes.remove(&abs);
        Ok(())
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), FsErr> {
        let a = self.resolve(from);
        let b = self.resolve(to);
        if a == "/" || b == "/" {
            return Err(FsErr::InvalidPath);
        }
        let src_is_dir = match self.nodes.get(&a) {
            None => return Err(FsErr::NotFound),
            Some(Node::Dir) => true,
            Some(Node::File(_)) => false,
        };
        if a == b {
            return Ok(());
        }
        self.check_name(&b)?;
        self.check_parent(&b)?;
        if src_is_dir && b.starts_with(&alloc::format!("{a}/")) {
            return Err(FsErr::InvalidPath);
        }
        match self.nodes.get(&b) {
            Some(Node::Dir) => return Err(FsErr::AlreadyExists),
            Some(Node::File(_)) if src_is_dir => return Err(FsErr::NotADirectory),
            _ => {}
        }
        let moved: Vec<String> = self
            .nodes
            .range(a.clone()..)
            .map(|(k, _)| k.clone())
            .take_while(|k| *k == a || k.starts_with(&alloc::format!("{a}/")))
            .collect();
        for k in moved {
            if let Some(v) = self.nodes.remove(&k) {
                let nk = alloc::format!("{b}{}", &k[a.len()..]);
                self.nodes.insert(nk, v);
            }
        }
        if self.cwd == a || self.cwd.starts_with(&alloc::format!("{a}/")) {
            self.cwd = alloc::format!("{b}{}", &self.cwd[a.len()..]);
        }
        Ok(())
    }

    fn usage(&self) -> FsUsage {
        let mut u = FsUsage {
            total_bytes: self.max_total.min(u64::MAX as usize) as u64,
            ..FsUsage::default()
        };
        for n in self.nodes.values() {
            match n {
                Node::Dir => u.dirs += 1,
                Node::File(d) => {
                    u.files += 1;
                    u.used_bytes += d.len() as u64;
                }
            }
        }
        u
    }
}

#[cfg(test)]
mod tests;
