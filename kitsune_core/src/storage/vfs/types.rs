//! types (split out of `vfs.rs`).

use super::*;

/// File or folder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    File,
    Dir,
}

impl From<Kind> for EntryKind {
    fn from(k: Kind) -> Self {
        match k {
            Kind::File => EntryKind::File,
            Kind::Dir => EntryKind::Dir,
        }
    }
}

/// One item of a folder listing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    pub name: Vec<u8>,
    pub kind: EntryKind,
    pub size: u64,
    /// Seconds since the Unix epoch (UTC).
    pub mtime: u64,
}

impl From<DirEntry> for Entry {
    fn from(e: DirEntry) -> Self {
        Entry {
            name: e.name,
            kind: e.kind.into(),
            size: e.size,
            mtime: e.mtime,
        }
    }
}

/// `stat` of one path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Info {
    pub kind: EntryKind,
    pub size: u64,
    pub ctime: u64,
    pub mtime: u64,
    /// Allocated 4 KiB blocks.
    pub blocks: u32,
}

impl From<Stat> for Info {
    fn from(s: Stat) -> Self {
        Info {
            kind: s.kind.into(),
            size: s.size,
            ctime: s.ctime,
            mtime: s.mtime,
            blocks: s.blocks,
        }
    }
}

/// One item in the trash.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TrashItem {
    /// The key for [`Backend::trash_restore`] / [`Backend::trash_purge`].
    pub id: Vec<u8>,
    /// The name it had before it was deleted.
    pub name: Vec<u8>,
    pub kind: EntryKind,
    pub size: u64,
    pub deleted_at: u64,
}

impl From<TrashEntry> for TrashItem {
    fn from(t: TrashEntry) -> Self {
        TrashItem {
            id: t.trash_name,
            name: t.orig_name,
            kind: t.kind.into(),
            size: t.size,
            deleted_at: t.deleted_at,
        }
    }
}

/// Disk usage.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Usage {
    pub total: u64,
    pub free: u64,
    /// Files and folders the volume can hold, and how many slots are still free
    /// (both `0` when the backend does not know).
    pub inodes_total: u32,
    pub inodes_free: u32,
}

impl Usage {
    /// Used bytes.
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.free)
    }

    /// Files and folders in use (the root folder and the system folders included).
    pub fn inodes_used(&self) -> u32 {
        self.inodes_total.saturating_sub(self.inodes_free)
    }

    /// Used share in permille (0..=1000).
    pub fn used_permille(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.used() as u128 * 1000 / self.total as u128).min(1000) as u32
    }
}

impl From<StatFs> for Usage {
    fn from(s: StatFs) -> Self {
        Usage {
            total: s.data_bytes(),
            free: s.free_bytes(),
            inodes_total: s.total_inodes,
            inodes_free: s.free_inodes,
        }
    }
}
