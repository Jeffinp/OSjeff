//! `VolumeFs`: the [`AppFs`] of the real volume.
//!
//! One adapter over the desktop's [`Backend`](crate::vfs::Backend) (the object-safe
//! slice of OJFS v3 that `desktop/vfs.rs` hands out for the ATA disk **and** for the
//! RAM fallback volume), so `/apps/<id>.wasm`, `/data/<id>` and `/home` live in the
//! same file system as the user's files: they persist across reboots on a disk, they
//! are visible in the file manager, and when there is no v3 disk they share the RAM
//! volume the desktop already uses.
//!
//! The sandbox ([`Sandbox`](super::Sandbox)) is unchanged: it only ever sees
//! `&mut dyn AppFs`. This adapter adds the backend's own defense in depth ("backends
//! never trust their caller"): canonical paths only, and only the three trees the
//! apps platform owns ([`ROOTS`]) are reachable at all, so a sandbox bug could still
//! not touch `/etc/kitsune.conf`, `/var/log` or `/.trash`.
//!
//! Semantics match [`MemFs`](super::MemFs) (the test oracle: `volume_tests` runs the
//! same operations on both and compares), except the limits that come from the real
//! volume: space is the volume's, a single file is capped at [`MAX_FILE`], and a
//! folder listing skips names an app cannot address (non-ASCII or otherwise outside
//! the sandbox's path grammar; the user's own files in `/home` may have them).
//!
//! Every call is one short critical section of the backend: the caller holds the
//! volume lock for exactly one `AppFs` call, never across guest execution.

use super::path::{self, valid_component, within};
use super::{AppFs, DirEntry, ENTRY_OVERHEAD, FsError, Kind, Stat};
use crate::vfs::{self, Backend, EntryKind, VfsError};
use alloc::string::String;
use alloc::vec::Vec;

/// The top-level folders the apps platform owns. Nothing else is reachable.
pub const ROOTS: [&str; 3] = ["apps", "data", "home"];
/// Largest single file an app may grow (the volume may allow more; apps may not).
pub const MAX_FILE: u64 = 64 << 20;
/// Most entries one `tree_size` walk visits before it gives up.
const MAX_WALK: usize = 100_000;

/// An [`AppFs`] over a [`Backend`], stamping new data with `now`.
pub struct VolumeFs<'a> {
    be: &'a mut dyn Backend,
    now: u64,
    mutated: bool,
}

impl<'a> VolumeFs<'a> {
    /// Adapter over `be`; `now` is the Unix time written into the timestamps.
    pub fn new(be: &'a mut dyn Backend, now: u64) -> VolumeFs<'a> {
        VolumeFs {
            be,
            now,
            mutated: false,
        }
    }

    /// Whether any call so far may have changed the volume (a write, a resize, a
    /// create, a remove, a rename): the kernel bumps the desktop's "files changed"
    /// counter on it, so open file-manager windows can refresh.
    pub fn mutated(&self) -> bool {
        self.mutated
    }
}

/// How a desktop-VFS error looks to an app.
fn map(e: VfsError) -> FsError {
    match e {
        VfsError::NotFound => FsError::NotFound,
        VfsError::Exists => FsError::Exists,
        VfsError::NotDir => FsError::NotDir,
        VfsError::IsDir => FsError::IsDir,
        VfsError::NotEmpty => FsError::NotEmpty,
        VfsError::NoSpace | VfsError::NoInodes | VfsError::TooBig => FsError::NoSpace,
        VfsError::Reserved => FsError::Perm,
        VfsError::InvalidName
        | VfsError::NameTooLong
        | VfsError::InvalidPath
        | VfsError::InvalidMove => FsError::Invalid,
        VfsError::Busy
        | VfsError::Unavailable
        | VfsError::Io
        | VfsError::Corrupt
        | VfsError::Cancelled => FsError::Io,
    }
}

/// Re-validates `p` (canonical, absolute) and that it lies in one of the [`ROOTS`].
/// The volume root `/` itself is refused here: only `stat` accepts it.
fn check(p: &str) -> Result<&[u8], FsError> {
    match path::normalize(p.as_bytes()) {
        Ok(n) if n == p => {}
        _ => return Err(FsError::Invalid),
    }
    let first = p.split('/').nth(1).unwrap_or("");
    if ROOTS.contains(&first) {
        Ok(p.as_bytes())
    } else {
        Err(FsError::Perm)
    }
}

/// True for a top-level folder such as `/apps`: apps may not remove or move those.
fn is_root_dir(p: &str) -> bool {
    p.matches('/').count() == 1
}

fn kind_of(k: EntryKind) -> Kind {
    match k {
        EntryKind::File => Kind::File,
        EntryKind::Dir => Kind::Dir,
    }
}

impl AppFs for VolumeFs<'_> {
    fn stat(&mut self, p: &str) -> Result<Stat, FsError> {
        if p == "/" {
            return Ok(Stat {
                kind: Kind::Dir,
                size: 0,
            });
        }
        let info = self.be.stat(check(p)?).map_err(map)?;
        Ok(match info.kind {
            EntryKind::Dir => Stat {
                kind: Kind::Dir,
                size: 0,
            },
            EntryKind::File => Stat {
                kind: Kind::File,
                size: info.size,
            },
        })
    }

    fn read_at(&mut self, p: &str, off: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let p = check(p)?;
        self.be.read_at(p, off, buf).map_err(map)
    }

    fn write_at(&mut self, p: &str, off: u64, data: &[u8]) -> Result<usize, FsError> {
        let bp = check(p)?;
        let end = off.checked_add(data.len() as u64).ok_or(FsError::Invalid)?;
        if end > MAX_FILE {
            return Err(FsError::NoSpace);
        }
        if data.is_empty() {
            // Nothing to write, but a missing file or a folder is still an error.
            return match self.stat(p)?.kind {
                Kind::File => Ok(0),
                Kind::Dir => Err(FsError::IsDir),
            };
        }
        self.mutated = true;
        self.be.write_at(bp, off, data, self.now).map_err(map)?;
        Ok(data.len())
    }

    fn set_len(&mut self, p: &str, len: u64) -> Result<(), FsError> {
        let p = check(p)?;
        if len > MAX_FILE {
            return Err(FsError::NoSpace);
        }
        self.mutated = true;
        self.be.truncate(p, len, self.now).map_err(map)
    }

    fn create(&mut self, p: &str) -> Result<(), FsError> {
        let p = check(p)?;
        self.mutated = true;
        self.be.create(p, self.now).map_err(map)
    }

    fn mkdir(&mut self, p: &str) -> Result<(), FsError> {
        let p = check(p)?;
        self.mutated = true;
        self.be.mkdir(p, self.now).map_err(map)
    }

    fn remove(&mut self, p: &str) -> Result<(), FsError> {
        let bp = check(p)?;
        if is_root_dir(p) {
            return Err(FsError::Perm);
        }
        let info = self.be.stat(bp).map_err(map)?;
        if info.kind == EntryKind::Dir && !self.be.names(bp).map_err(map)?.is_empty() {
            return Err(FsError::NotEmpty);
        }
        // A file or an empty folder: `remove_all` deletes exactly that.
        self.mutated = true;
        self.be.remove_all(bp).map_err(map)
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), FsError> {
        let (f, t) = (check(from)?, check(to)?);
        if is_root_dir(from) || is_root_dir(to) {
            return Err(FsError::Perm);
        }
        self.be.stat(f).map_err(map)?;
        if from == to {
            return Err(FsError::Exists); // as `MemFs`: the destination is taken
        }
        if within(to, from) {
            return Err(FsError::Invalid); // into its own subtree
        }
        self.mutated = true;
        self.be.rename(f, t, self.now).map_err(map)
    }

    fn read_dir(&mut self, p: &str, index: usize) -> Result<Option<DirEntry>, FsError> {
        let p = check(p)?;
        let mut names: Vec<(String, Kind)> = self
            .be
            .names(p)
            .map_err(map)?
            .into_iter()
            .filter(|(n, _)| valid_component(n))
            // `valid_component` allows only printable ASCII, so this cannot fail.
            .filter_map(|(n, k)| String::from_utf8(n).ok().map(|n| (n, kind_of(k))))
            .collect();
        // Sorted, so `index` keeps meaning the same entry while other names come and go.
        names.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        Ok(names
            .into_iter()
            .nth(index)
            .map(|(name, kind)| DirEntry { name, kind }))
    }

    fn tree_size(&mut self, p: &str) -> Result<u64, FsError> {
        let bp = check(p)?;
        let info = self.be.stat(bp).map_err(map)?;
        if info.kind == EntryKind::File {
            return Ok(0);
        }
        let mut total = 0u64;
        let mut seen = 0usize;
        let mut stack: Vec<Vec<u8>> = alloc::vec![bp.to_vec()];
        while let Some(dir) = stack.pop() {
            for e in self.be.readdir(&dir).map_err(map)? {
                seen += 1;
                if seen > MAX_WALK {
                    return Err(FsError::Io);
                }
                total = total.saturating_add(ENTRY_OVERHEAD);
                match e.kind {
                    EntryKind::File => total = total.saturating_add(e.size),
                    EntryKind::Dir => stack.push(vfs::join(&dir, &e.name)),
                }
            }
        }
        Ok(total)
    }
}
