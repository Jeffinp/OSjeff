//! The public namespace and data operations of [`Fs3`].

use super::dir::DIR_HDR;
use super::extent::{cut_tail, map_block, remap};
use super::inode::{FLAG_TRASHED, Inode, Kind, MAX_NAME};
use super::layout::{ROOT_INO, TRASH_INO, TRASH_NAME};
use super::{DirEntry, Fs3, FsError, Ino, MAX_FILE_BYTES, Stat, TrashEntry};
use crate::storage::blockcache::{BLOCK_SIZE, Block};
use crate::storage::blockdev::BlockDevice;
use alloc::vec::Vec;

mod data;
mod removal;
mod trash;

const BS: u64 = BLOCK_SIZE as u64;
/// Longest path accepted (bytes).
pub const MAX_PATH: usize = 4096;
/// Largest file `read_file` will load into memory.
pub const MAX_READ_FILE: u64 = 64 * 1024 * 1024;
/// Blocks assembled in memory at a time while writing.
const WRITE_CHUNK: u32 = 256;

/// Validate one name: 1..=255 bytes, no `/` or NUL, not `.` or `..`.
pub fn check_name(name: &[u8]) -> Result<(), FsError> {
    if name.is_empty() || name == b"." || name == b".." || name.iter().any(|&c| c == b'/' || c == 0)
    {
        return Err(FsError::InvalidName);
    }
    if name.len() > MAX_NAME {
        return Err(FsError::NameTooLong);
    }
    Ok(())
}

/// Split an absolute path into validated components (`"/"` is no component).
pub fn split_path(path: &[u8]) -> Result<Vec<&[u8]>, FsError> {
    if path.is_empty() || path.len() > MAX_PATH || path[0] != b'/' {
        return Err(FsError::InvalidPath);
    }
    if path == b"/" {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for c in path[1..].split(|&b| b == b'/') {
        if c.is_empty() || c == b"." || c == b".." {
            return Err(FsError::InvalidPath);
        }
        if c.contains(&0) {
            return Err(FsError::InvalidName);
        }
        if c.len() > MAX_NAME {
            return Err(FsError::NameTooLong);
        }
        out.push(c);
    }
    Ok(out)
}

/// Where a path leads, and whether it passes through `/.trash`.
struct Resolved {
    ino: Ino,
    in_trash: bool,
}

/// A path split into its parent directory and last component.
struct ParentRef {
    dir: Ino,
    name: Vec<u8>,
    in_trash: bool,
}

impl ParentRef {
    /// Mutating the trash (or creating/removing the trash itself) by path is refused.
    fn check_mutable(&self) -> Result<(), FsError> {
        if self.in_trash || (self.dir == ROOT_INO && self.name == TRASH_NAME) {
            Err(FsError::Reserved)
        } else {
            Ok(())
        }
    }
}

impl<D: BlockDevice> Fs3<D> {
    // -----------------------------------------------------------------------
    // Path resolution
    // -----------------------------------------------------------------------

    fn walk(&mut self, comps: &[&[u8]]) -> Result<Resolved, FsError> {
        let mut cur = ROOT_INO;
        let mut in_trash = false;
        for c in comps {
            let dir = self.read_inode(cur)?;
            let (next, _) = self.dir_find(cur, &dir, c)?.ok_or(FsError::NotFound)?;
            if next == TRASH_INO && cur == ROOT_INO {
                in_trash = true;
            }
            cur = next;
        }
        Ok(Resolved { ino: cur, in_trash })
    }

    fn resolve_parent(&mut self, path: &[u8]) -> Result<ParentRef, FsError> {
        let comps = split_path(path)?;
        let Some((last, dirs)) = comps.split_last() else {
            return Err(FsError::InvalidPath);
        };
        let r = self.walk(dirs)?;
        Ok(ParentRef {
            dir: r.ino,
            name: last.to_vec(),
            in_trash: r.in_trash,
        })
    }

    /// Find an entry named `name` directly inside `dir`.
    fn entry(&mut self, dir: Ino, name: &[u8]) -> Result<Option<(Ino, Kind)>, FsError> {
        let d = self.read_inode(dir)?;
        self.dir_find(dir, &d, name)
    }

    /// Follow a directory entry to its inode (a dangling entry is corruption).
    fn read_child(&mut self, ino: Ino) -> Result<Inode, FsError> {
        self.read_inode(ino).map_err(|e| match e {
            FsError::NotFound => FsError::Corrupt("directory entry points at a free inode"),
            other => other,
        })
    }

    /// Resolve an absolute path to its inode.
    pub fn lookup<P: AsRef<[u8]> + ?Sized>(&mut self, path: &P) -> Result<Ino, FsError> {
        self.ready()?;
        let comps = split_path(path.as_ref())?;
        Ok(self.walk(&comps)?.ino)
    }

    /// Resolve a path that must be a regular file.
    pub fn open<P: AsRef<[u8]> + ?Sized>(&mut self, path: &P) -> Result<Ino, FsError> {
        let ino = self.lookup(path)?;
        match self.read_child(ino)?.kind {
            Kind::File => Ok(ino),
            Kind::Dir => Err(FsError::IsDir),
        }
    }

    fn stat_of(ino: Ino, n: &Inode) -> Stat {
        Stat {
            ino,
            kind: n.kind,
            size: n.size,
            ctime: n.ctime,
            mtime: n.mtime,
            mode: n.mode,
            uid: n.uid,
            gid: n.gid,
            nlink: n.nlink,
            blocks: n.nblocks,
            parent: n.parent,
        }
    }

    /// Metadata of the item at `path`.
    pub fn stat<P: AsRef<[u8]> + ?Sized>(&mut self, path: &P) -> Result<Stat, FsError> {
        let ino = self.lookup(path)?;
        self.stat_ino(ino)
    }

    /// Metadata of inode `ino`.
    pub fn stat_ino(&mut self, ino: Ino) -> Result<Stat, FsError> {
        self.ready()?;
        let n = self.read_inode(ino)?;
        Ok(Self::stat_of(ino, &n))
    }

    /// Absolute path of inode `ino` (items inside the trash show as `/.trash/...`).
    pub fn path_of(&mut self, ino: Ino) -> Result<Vec<u8>, FsError> {
        self.ready()?;
        if ino == ROOT_INO {
            return Ok(b"/".to_vec());
        }
        let mut names: Vec<Vec<u8>> = Vec::new();
        let mut cur = ino;
        let mut steps = 0u32;
        while cur != ROOT_INO {
            steps += 1;
            if steps > self.geo.inode_count {
                return Err(FsError::Corrupt("parent chain loops"));
            }
            let node = self.read_inode(cur)?;
            let parent = node.parent;
            let found = self
                .dir_list(parent)?
                .into_iter()
                .find(|(_, i, _)| *i == cur)
                .ok_or(FsError::Corrupt("inode missing from its parent"))?;
            names.push(found.0);
            cur = parent;
        }
        let mut out = Vec::new();
        for n in names.iter().rev() {
            out.push(b'/');
            out.extend_from_slice(n);
        }
        Ok(out)
    }

    /// True if `ino` is a directory reachable from the root without going
    /// through `/.trash`.
    fn is_live_dir(&mut self, ino: Ino) -> bool {
        let mut cur = ino;
        let mut steps = 0u32;
        loop {
            let Ok(n) = self.read_inode(cur) else {
                return false;
            };
            if cur == ino && n.kind != Kind::Dir {
                return false;
            }
            if cur == ROOT_INO {
                return true;
            }
            if cur == TRASH_INO {
                return false;
            }
            steps += 1;
            if steps > self.geo.inode_count {
                return false;
            }
            cur = n.parent;
        }
    }

    // -----------------------------------------------------------------------
    // Namespace
    // -----------------------------------------------------------------------

    pub(super) fn new_node(
        &mut self,
        pdir: Ino,
        name: &[u8],
        kind: Kind,
        now: u64,
    ) -> Result<Ino, FsError> {
        let p = self.read_inode(pdir)?;
        if p.kind != Kind::Dir {
            return Err(FsError::NotDir);
        }
        let ino = self.alloc_inode()?;
        self.write_inode(ino, &Inode::new(kind, now, pdir))?;
        self.dir_insert(pdir, name, ino, kind, now)?;
        Ok(ino)
    }

    /// Create an empty regular file. `Exists` if the path is taken.
    pub fn create<P: AsRef<[u8]> + ?Sized>(&mut self, path: &P, now: u64) -> Result<Ino, FsError> {
        self.ready()?;
        let p = self.resolve_parent(path.as_ref())?;
        check_name(&p.name)?;
        p.check_mutable()?;
        self.txn(|fs| fs.new_node(p.dir, &p.name, Kind::File, now))
    }

    /// Create a directory (the parent must exist).
    pub fn mkdir<P: AsRef<[u8]> + ?Sized>(&mut self, path: &P, now: u64) -> Result<Ino, FsError> {
        self.ready()?;
        let p = self.resolve_parent(path.as_ref())?;
        check_name(&p.name)?;
        p.check_mutable()?;
        self.txn(|fs| fs.new_node(p.dir, &p.name, Kind::Dir, now))
    }

    /// Change the owner, the group and/or the permission bits (`mode & 0o1777`) of the item at
    /// `path`. The modification time is left alone. Who may do this is the caller's business
    /// (`security::perm`).
    pub fn set_owner<P: AsRef<[u8]> + ?Sized>(
        &mut self,
        path: &P,
        uid: Option<u32>,
        gid: Option<u32>,
        mode: Option<u16>,
    ) -> Result<(), FsError> {
        self.ready()?;
        let ino = self.lookup(path)?;
        self.txn(|fs| {
            let mut n = fs.read_inode(ino)?;
            if let Some(u) = uid {
                n.uid = u;
            }
            if let Some(g) = gid {
                n.gid = g;
            }
            if let Some(m) = mode {
                n.mode = m & 0o1777;
            }
            fs.write_inode(ino, &n)
        })
    }

    /// List a directory in storage order. `.trash` is hidden at the root.
    pub fn readdir<P: AsRef<[u8]> + ?Sized>(&mut self, path: &P) -> Result<Vec<DirEntry>, FsError> {
        self.ready()?;
        let comps = split_path(path.as_ref())?;
        let dir = self.walk(&comps)?.ino;
        let mut out = Vec::new();
        for (name, ino, kind) in self.dir_list(dir)? {
            if dir == ROOT_INO && ino == TRASH_INO {
                continue;
            }
            let n = self.read_child(ino)?;
            out.push(DirEntry {
                name,
                ino,
                kind,
                size: n.size,
                mtime: n.mtime,
            });
        }
        Ok(out)
    }

    /// Names and kinds of a directory's entries in storage order, without reading
    /// the entries' inodes (much cheaper than [`readdir`](Self::readdir) for a big
    /// folder). `.trash` is hidden at the root.
    pub fn readdir_names<P: AsRef<[u8]> + ?Sized>(
        &mut self,
        path: &P,
    ) -> Result<Vec<(Vec<u8>, Kind)>, FsError> {
        self.ready()?;
        let comps = split_path(path.as_ref())?;
        let dir = self.walk(&comps)?.ino;
        Ok(self
            .dir_list(dir)?
            .into_iter()
            .filter(|&(_, ino, _)| !(dir == ROOT_INO && ino == TRASH_INO))
            .map(|(name, _, kind)| (name, kind))
            .collect())
    }
}

#[allow(dead_code)]
const _: usize = DIR_HDR;
