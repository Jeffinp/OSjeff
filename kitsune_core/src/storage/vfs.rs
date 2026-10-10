//! Desktop VFS: the file operations the file manager, the terminal and the
//! editor share, on top of an OJFS v3 volume.
//!
//! The kernel's `desktop/vfs.rs` is only glue (which disk, the RAM fallback, the
//! lock, the clock); every decision lives here so it runs on the host against a
//! [`RamDisk`](crate::storage::blockdev::RamDisk):
//!
//! * [`Backend`]: the object-safe slice of [`Fs3`] the desktop needs (implemented
//!   for every `Fs3<D>`), so one code path serves the ATA disk and the RAM disk.
//! * [`VfsError`]: typed errors with a Portuguese [`message`](VfsError::message)
//!   ready to show (ASCII only: the bitmap font has no accents).
//! * Path and name helpers: [`join`], [`parent`], [`base_name`], [`validate_name`],
//!   [`unique_name`] (`"a (2).txt"`).
//! * Whole operations: [`move_to`] (rename, instant) and [`CopyJob`], a copy that
//!   runs in small steps so a multi-megabyte copy never freezes the compositor,
//!   can be cancelled, and cleans up the half-written file.
//! * [`seed_welcome`]: the three welcome files of a fresh disk (the only copy).
//!
//! Paths are absolute (`/a/b`), as `Fs3` wants them; `/` is the root and
//! `/.trash` is reserved (the trash lives there and is reached through
//! [`Backend::trash_list`] and friends).

use crate::storage::blockdev::BlockDevice;
use crate::storage::fs3::{self, DirEntry, Fs3, FsError, Kind, Stat, StatFs, TrashEntry};
use alloc::vec::Vec;

mod backend;
mod copyjob;
mod errors;
mod ops;
mod paths;
mod types;
pub use backend::*;
pub use copyjob::*;
pub use errors::*;
pub use ops::*;
pub use paths::*;
pub use types::*;

/// Longest name in bytes.
pub const MAX_NAME: usize = fs3::MAX_NAME;
/// Largest number of items one copy may plan.
pub const MAX_COPY_ITEMS: usize = 200_000;
/// Bytes copied per [`CopyJob::step`] by default.
pub const COPY_CHUNK: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Backend
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Paths and names
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Whole operations
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Copy job
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
