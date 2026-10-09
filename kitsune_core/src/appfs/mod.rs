//! File access for apps: the [`AppFs`] backend trait, a per-app [`Sandbox`]
//! (root, permissions, descriptors, disk quota) and an in-memory [`MemFs`].
//!
//! The backend is a plain, stateless, path-addressed trait over **canonical
//! absolute paths** (see [`path`]). Everything that makes it safe for untrusted
//! code lives in [`Sandbox`], which is backend-agnostic: the kernel plugs in
//! [`VolumeFs`] (OJFS v3 on the disk, or the desktop's RAM volume), [`MemFs`] is
//! the in-memory implementation used by tests. See `docs/design/apps.md` §5.

pub mod memfs;
pub mod path;
pub mod sandbox;
pub mod volume;

pub use memfs::MemFs;
pub use path::PathError;
pub use sandbox::Sandbox;
pub use volume::VolumeFs;

use alloc::string::String;
use core::fmt;

/// Bytes charged against an app's disk quota for every entry (file or folder),
/// on top of the file's size, so millions of empty files cost something.
pub const ENTRY_OVERHEAD: u64 = 256;

/// Why a file operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsError {
    NotFound,
    Exists,
    NotDir,
    IsDir,
    NotEmpty,
    NoSpace,
    Invalid,
    /// Permission denied, or the path tried to leave the sandbox.
    Perm,
    BadFd,
    TooManyFds,
    Io,
}

impl FsError {
    /// The ABI error code (`appabi::ERR_*`).
    pub fn code(self) -> i32 {
        use crate::appabi::*;
        match self {
            FsError::NotFound => ERR_NOENT,
            FsError::Exists => ERR_EXIST,
            FsError::NotDir => ERR_NOTDIR,
            FsError::IsDir => ERR_ISDIR,
            FsError::NotEmpty => ERR_NOTEMPTY,
            FsError::NoSpace => ERR_NOSPC,
            FsError::Invalid | FsError::Io => ERR_INVAL,
            FsError::Perm => ERR_PERM,
            FsError::BadFd => ERR_BADF,
            FsError::TooManyFds => ERR_MFILE,
        }
    }
}

impl From<PathError> for FsError {
    fn from(e: PathError) -> FsError {
        match e {
            PathError::Escapes => FsError::Perm,
            _ => FsError::Invalid,
        }
    }
}

impl FsError {
    /// Catalog key of the reason, in words for the person.
    pub fn key(self) -> &'static str {
        match self {
            FsError::NotFound => crate::tk!("apps.fs.not_found"),
            FsError::Exists => crate::tk!("apps.fs.exists"),
            FsError::NotDir => crate::tk!("apps.fs.not_dir"),
            FsError::IsDir => crate::tk!("apps.fs.is_dir"),
            FsError::NotEmpty => crate::tk!("apps.fs.not_empty"),
            FsError::NoSpace => crate::tk!("apps.fs.no_space"),
            FsError::Invalid => crate::tk!("apps.fs.invalid"),
            FsError::Perm => crate::tk!("apps.fs.denied"),
            FsError::BadFd => crate::tk!("apps.fs.bad_fd"),
            FsError::TooManyFds => crate::tk!("apps.fs.too_many"),
            FsError::Io => crate::tk!("apps.fs.io"),
        }
    }
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FsError::NotFound => "not found",
            FsError::Exists => "already exists",
            FsError::NotDir => "not a directory",
            FsError::IsDir => "is a directory",
            FsError::NotEmpty => "directory not empty",
            FsError::NoSpace => "no space left (quota)",
            FsError::Invalid => "invalid argument",
            FsError::Perm => "permission denied",
            FsError::BadFd => "bad file descriptor",
            FsError::TooManyFds => "too many open files",
            FsError::Io => "i/o error",
        })
    }
}

/// Kind of an entry (the values are the ABI's: 1 file, 2 folder).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File = 1,
    Dir = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stat {
    pub kind: Kind,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub kind: Kind,
}

/// A filesystem an app's sandbox can sit on. Paths are canonical absolute
/// (`/`, `/a/b`); an implementation must still reject anything else.
pub trait AppFs {
    fn stat(&mut self, path: &str) -> Result<Stat, FsError>;
    /// Reads at `off`; `Ok(0)` at or past the end.
    fn read_at(&mut self, path: &str, off: u64, buf: &mut [u8]) -> Result<usize, FsError>;
    /// Writes at `off`, zero-filling any gap; extends the file.
    fn write_at(&mut self, path: &str, off: u64, data: &[u8]) -> Result<usize, FsError>;
    fn set_len(&mut self, path: &str, len: u64) -> Result<(), FsError>;
    /// Creates an empty file; the parent must be a folder; `Exists` if present.
    fn create(&mut self, path: &str) -> Result<(), FsError>;
    fn mkdir(&mut self, path: &str) -> Result<(), FsError>;
    /// Removes a file or an empty folder.
    fn remove(&mut self, path: &str) -> Result<(), FsError>;
    /// Moves an entry (and its subtree); never overwrites (`Exists`).
    fn rename(&mut self, from: &str, to: &str) -> Result<(), FsError>;
    /// The `index`-th entry of a folder (stable order), `None` past the end.
    fn read_dir(&mut self, path: &str, index: usize) -> Result<Option<DirEntry>, FsError>;
    /// Sum of `ENTRY_OVERHEAD + size` over every entry strictly inside `path`.
    fn tree_size(&mut self, path: &str) -> Result<u64, FsError>;

    /// `mkdir -p`: creates `path` and its missing parents; existing is fine.
    fn mkdir_all(&mut self, path: &str) -> Result<(), FsError> {
        let mut cur = String::new();
        for comp in path.split('/').filter(|c| !c.is_empty()) {
            cur.push('/');
            cur.push_str(comp);
            match self.mkdir(&cur) {
                Ok(()) | Err(FsError::Exists) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod volume_tests;
