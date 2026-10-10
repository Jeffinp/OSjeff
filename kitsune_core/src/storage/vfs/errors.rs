//! errors (split out of `vfs.rs`).

use super::*;

/// Why a desktop file operation failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VfsError {
    NotFound,
    Exists,
    NotDir,
    IsDir,
    NotEmpty,
    /// Empty, `.`/`..`, `/`, NUL, control characters or not UTF-8.
    InvalidName,
    NameTooLong,
    InvalidPath,
    /// `/.trash` is managed by the system.
    Reserved,
    /// A folder cannot go inside itself.
    InvalidMove,
    NoSpace,
    NoInodes,
    TooBig,
    /// The filesystem is in use (lock not available).
    Busy,
    /// Nothing is mounted.
    Unavailable,
    /// The disk reported an error.
    Io,
    /// The on-disk structures are damaged.
    Corrupt,
    /// The user cancelled a long operation.
    Cancelled,
}

impl VfsError {
    /// A short message in the language in effect, for the status bar or a dialog.
    pub fn message(self) -> &'static str {
        match self {
            VfsError::NotFound => crate::t!("files.err.not_found"),
            VfsError::Exists => crate::t!("files.err.exists"),
            VfsError::NotDir => crate::t!("files.err.not_dir"),
            VfsError::IsDir => crate::t!("files.err.is_dir"),
            VfsError::NotEmpty => crate::t!("files.err.not_empty"),
            VfsError::InvalidName => crate::t!("files.err.invalid_name"),
            VfsError::NameTooLong => crate::t!("files.err.name_too_long"),
            VfsError::InvalidPath => crate::t!("files.err.invalid_path"),
            VfsError::Reserved => crate::t!("files.err.reserved"),
            VfsError::InvalidMove => crate::t!("files.err.invalid_move"),
            VfsError::NoSpace => crate::t!("files.err.no_space"),
            VfsError::NoInodes => crate::t!("files.err.no_inodes"),
            VfsError::TooBig => crate::t!("files.err.too_big"),
            VfsError::Busy => crate::t!("files.err.busy"),
            VfsError::Unavailable => crate::t!("files.err.unavailable"),
            VfsError::Io => crate::t!("files.err.io"),
            VfsError::Corrupt => crate::t!("files.err.corrupt"),
            VfsError::Cancelled => crate::t!("files.err.cancelled"),
        }
    }
}

impl From<FsError> for VfsError {
    fn from(e: FsError) -> Self {
        match e {
            FsError::NotFound => VfsError::NotFound,
            FsError::Exists => VfsError::Exists,
            FsError::NotDir => VfsError::NotDir,
            FsError::IsDir => VfsError::IsDir,
            FsError::NotEmpty => VfsError::NotEmpty,
            FsError::InvalidName => VfsError::InvalidName,
            FsError::NameTooLong => VfsError::NameTooLong,
            FsError::InvalidPath => VfsError::InvalidPath,
            FsError::Reserved => VfsError::Reserved,
            FsError::InvalidMove => VfsError::InvalidMove,
            FsError::NoSpace => VfsError::NoSpace,
            FsError::NoInodes => VfsError::NoInodes,
            FsError::TooBig => VfsError::TooBig,
            FsError::TxTooLarge => VfsError::NoSpace,
            FsError::TooSmall => VfsError::NoSpace,
            FsError::BadSuperblock | FsError::Corrupt(_) => VfsError::Corrupt,
            FsError::Poisoned | FsError::Io(_) => VfsError::Io,
        }
    }
}

/// Result alias of this module.
pub type Result<T> = core::result::Result<T, VfsError>;
