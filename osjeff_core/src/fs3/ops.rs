//! The public namespace and data operations of [`Fs3`].

use super::dir::DIR_HDR;
use super::extent::{cut_tail, map_block, remap};
use super::inode::{FLAG_TRASHED, Inode, Kind, MAX_NAME};
use super::layout::{ROOT_INO, TRASH_INO, TRASH_NAME};
use super::{DirEntry, Fs3, FsError, Ino, MAX_FILE_BYTES, Stat, TrashEntry};
use crate::blockcache::{BLOCK_SIZE, Block};
use crate::blockdev::BlockDevice;
use alloc::vec::Vec;

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

    /// Free an inode and all of its blocks.
    fn release_inode(&mut self, ino: Ino) -> Result<(), FsError> {
        let n = self.read_child(ino)?;
        if n.kind == Kind::Dir && !self.dir_list(ino)?.is_empty() {
            return Err(FsError::NotEmpty);
        }
        self.release_blocks(ino, &n)?;
        self.free_inode(ino)
    }

    /// Drop one entry and free what it pointed at (which must be childless).
    fn remove_entry(&mut self, pdir: Ino, name: &[u8], ino: Ino) -> Result<(), FsError> {
        let (got, _) = self.dir_remove(pdir, name)?;
        if got != ino {
            return Err(FsError::Corrupt("directory entry changed under us"));
        }
        self.release_inode(ino)
    }

    /// Permanently delete a regular file.
    pub fn remove<P: AsRef<[u8]> + ?Sized>(&mut self, path: &P) -> Result<(), FsError> {
        self.ready()?;
        let p = self.resolve_parent(path.as_ref())?;
        p.check_mutable()?;
        self.txn(|fs| {
            let (ino, kind) = fs.entry(p.dir, &p.name)?.ok_or(FsError::NotFound)?;
            if kind == Kind::Dir {
                return Err(FsError::IsDir);
            }
            fs.remove_entry(p.dir, &p.name, ino)
        })
    }

    /// Permanently delete an empty directory.
    pub fn rmdir<P: AsRef<[u8]> + ?Sized>(&mut self, path: &P) -> Result<(), FsError> {
        self.ready()?;
        let p = self.resolve_parent(path.as_ref())?;
        p.check_mutable()?;
        self.txn(|fs| {
            let (ino, kind) = fs.entry(p.dir, &p.name)?.ok_or(FsError::NotFound)?;
            if kind != Kind::Dir {
                return Err(FsError::NotDir);
            }
            fs.remove_entry(p.dir, &p.name, ino)
        })
    }

    /// Permanently delete a file or a whole directory tree. Leaves first, one
    /// transaction per item, so a power cut leaves a valid (smaller) tree.
    pub fn remove_all<P: AsRef<[u8]> + ?Sized>(&mut self, path: &P) -> Result<(), FsError> {
        self.ready()?;
        let p = self.resolve_parent(path.as_ref())?;
        p.check_mutable()?;
        let (ino, kind) = self.entry(p.dir, &p.name)?.ok_or(FsError::NotFound)?;
        self.purge_tree(p.dir, &p.name, ino, kind)
    }

    /// Delete `name` (inode `ino`) from `pdir` and, if it is a directory,
    /// everything below it, deepest items first.
    fn purge_tree(&mut self, pdir: Ino, name: &[u8], ino: Ino, kind: Kind) -> Result<(), FsError> {
        if kind == Kind::File {
            return self.txn(|fs| fs.remove_entry(pdir, name, ino));
        }
        // Explicit stack: depth must not be bounded by the call stack, and a
        // corrupt (cyclic) tree must terminate.
        let mut stack: Vec<(Ino, Ino, Vec<u8>)> = alloc::vec![(ino, pdir, name.to_vec())];
        let mut budget = (self.geo.inode_count as u64) * 4 + 16;
        while let Some((dir, parent, dname)) = stack.last().cloned() {
            if budget == 0 {
                return Err(FsError::Corrupt("directory tree does not terminate"));
            }
            budget -= 1;
            let entries = self.dir_list(dir)?;
            if entries.is_empty() {
                self.txn(|fs| fs.remove_entry(parent, &dname, dir))?;
                stack.pop();
                continue;
            }
            let mut pushed = false;
            for (n, i, k) in entries {
                if k == Kind::File {
                    self.txn(|fs| fs.remove_entry(dir, &n, i))?;
                } else if !pushed {
                    stack.push((i, dir, n));
                    pushed = true;
                }
            }
        }
        Ok(())
    }

    /// Move or rename. The destination must not exist (`Exists`); a directory
    /// cannot go inside itself (`InvalidMove`); `from == to` is a no-op.
    pub fn rename<P: AsRef<[u8]> + ?Sized, Q: AsRef<[u8]> + ?Sized>(
        &mut self,
        from: &P,
        to: &Q,
        now: u64,
    ) -> Result<(), FsError> {
        self.ready()?;
        let s = self.resolve_parent(from.as_ref())?;
        let d = self.resolve_parent(to.as_ref())?;
        check_name(&d.name)?;
        s.check_mutable()?;
        d.check_mutable()?;
        self.txn(|fs| {
            let (ino, kind) = fs.entry(s.dir, &s.name)?.ok_or(FsError::NotFound)?;
            if s.dir == d.dir && s.name == d.name {
                return Ok(());
            }
            let dd = fs.read_inode(d.dir)?;
            if dd.kind != Kind::Dir {
                return Err(FsError::NotDir);
            }
            if fs.dir_find(d.dir, &dd, &d.name)?.is_some() {
                return Err(FsError::Exists);
            }
            if kind == Kind::Dir {
                let mut cur = d.dir;
                let mut steps = 0u32;
                while cur != ROOT_INO {
                    if cur == ino {
                        return Err(FsError::InvalidMove);
                    }
                    steps += 1;
                    if steps > fs.geo.inode_count {
                        return Err(FsError::Corrupt("parent chain loops"));
                    }
                    cur = fs.read_child(cur)?.parent;
                }
                if ino == ROOT_INO {
                    return Err(FsError::InvalidMove);
                }
            }
            fs.dir_remove(s.dir, &s.name)?;
            fs.dir_insert(d.dir, &d.name, ino, kind, now)?;
            let mut n = fs.read_child(ino)?;
            n.parent = d.dir;
            fs.write_inode(ino, &n)
        })
    }

    // -----------------------------------------------------------------------
    // File data
    // -----------------------------------------------------------------------

    /// Read up to `buf.len()` bytes at `off`; returns the count (0 at/after EOF).
    /// Holes read as zeros.
    pub fn read_at(&mut self, ino: Ino, off: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        self.ready()?;
        let node = self.read_inode(ino)?;
        if node.kind != Kind::File {
            return Err(FsError::IsDir);
        }
        if off >= node.size || buf.is_empty() {
            return Ok(0);
        }
        let n = (buf.len() as u64).min(node.size - off) as usize;
        let buf = &mut buf[..n];
        buf.fill(0);
        let end = off + n as u64;
        let (first, last) = (off / BS, (end - 1) / BS);
        let exts = self.load_extents(ino, &node)?;
        let mut scratch: Vec<u8> = Vec::new();
        for e in &exts {
            if e.lend() <= first {
                continue;
            }
            if e.lblk as u64 > last {
                break;
            }
            let a = (e.lblk as u64).max(first);
            let b = e.lend().min(last + 1);
            let mut lb = a;
            while lb < b {
                let c = (b - lb).min(WRITE_CHUNK as u64);
                scratch.resize(c as usize * BLOCK_SIZE, 0);
                let pb = e.pblk as u64 + (lb - e.lblk as u64);
                self.cache.read_many(pb, &mut scratch)?;
                // Copy the overlap of [lb*BS, (lb+c)*BS) with [off, end).
                let s = (lb * BS).max(off);
                let t = ((lb + c) * BS).min(end);
                let src = &scratch[(s - lb * BS) as usize..(t - lb * BS) as usize];
                buf[(s - off) as usize..(t - off) as usize].copy_from_slice(src);
                lb += c;
            }
        }
        Ok(n)
    }

    /// Read a whole file by path.
    pub fn read_file<P: AsRef<[u8]> + ?Sized>(&mut self, path: &P) -> Result<Vec<u8>, FsError> {
        let ino = self.open(path)?;
        let size = self.read_child(ino)?.size;
        if size > MAX_READ_FILE {
            return Err(FsError::TooBig);
        }
        let mut v = alloc::vec![0u8; size as usize];
        let n = self.read_at(ino, 0, &mut v)?;
        v.truncate(n);
        Ok(v)
    }

    /// Write `data` at `off` (extending the file; a gap becomes a hole of
    /// zeros). Copy-on-write: atomic, the old content survives a power cut.
    pub fn write_at(&mut self, ino: Ino, off: u64, data: &[u8], now: u64) -> Result<(), FsError> {
        self.ready()?;
        self.txn(|fs| fs.write_inner(ino, off, data, now))
    }

    /// Append `data` at the current end of the file.
    pub fn append(&mut self, ino: Ino, data: &[u8], now: u64) -> Result<(), FsError> {
        self.ready()?;
        self.txn(|fs| {
            let size = fs.read_inode(ino)?.size;
            fs.write_inner(ino, size, data, now)
        })
    }

    /// Set the file size: shrinking frees blocks, growing leaves a hole.
    pub fn truncate(&mut self, ino: Ino, size: u64, now: u64) -> Result<(), FsError> {
        self.ready()?;
        self.txn(|fs| fs.truncate_inner(ino, size, now))
    }

    /// Create the file or replace its whole content, atomically.
    pub fn write_file<P: AsRef<[u8]> + ?Sized>(
        &mut self,
        path: &P,
        data: &[u8],
        now: u64,
    ) -> Result<(), FsError> {
        self.ready()?;
        let p = self.resolve_parent(path.as_ref())?;
        check_name(&p.name)?;
        p.check_mutable()?;
        self.txn(|fs| {
            let ino = match fs.entry(p.dir, &p.name)? {
                Some((i, Kind::File)) => {
                    fs.truncate_inner(i, 0, now)?;
                    i
                }
                Some((_, Kind::Dir)) => return Err(FsError::IsDir),
                None => fs.new_node(p.dir, &p.name, Kind::File, now)?,
            };
            fs.write_inner(ino, 0, data, now)
        })
    }

    pub(super) fn write_inner(
        &mut self,
        ino: Ino,
        off: u64,
        data: &[u8],
        now: u64,
    ) -> Result<(), FsError> {
        let mut node = self.read_inode(ino)?;
        if node.kind != Kind::File {
            return Err(FsError::IsDir);
        }
        if data.is_empty() {
            return Ok(());
        }
        let end = off.checked_add(data.len() as u64).ok_or(FsError::TooBig)?;
        if end > MAX_FILE_BYTES {
            return Err(FsError::TooBig);
        }
        let first_lb = (off / BS) as u32;
        let last_lb = ((end - 1) / BS) as u32;
        let nlb = last_lb - first_lb + 1;
        let mut exts = self.load_extents(ino, &node)?;
        let hint = map_block(&exts, first_lb)
            .or_else(|| exts.last().map(|e| (e.pblk + e.len).min(u32::MAX - 1)))
            .unwrap_or(self.alloc_hint);
        let runs = self.alloc_blocks(nlb, hint)?;
        self.tx.data.extend_from_slice(&runs);
        let mut lb = first_lb;
        for &(pstart, plen) in &runs {
            let mut done = 0u32;
            while done < plen {
                let c = (plen - done).min(WRITE_CHUNK);
                let mut buf = alloc::vec![0u8; c as usize * BLOCK_SIZE];
                for k in 0..c {
                    let cur = lb + k;
                    let bstart = cur as u64 * BS;
                    let bend = bstart + BS;
                    let (s, e) = (bstart.max(off), bend.min(end));
                    let dst = &mut buf[k as usize * BLOCK_SIZE..(k as usize + 1) * BLOCK_SIZE];
                    if s > bstart || e < bend {
                        // Partial block: keep the old bytes around the new ones.
                        if let Some(old) = map_block(&exts, cur) {
                            let mut tmp: Block = [0u8; BLOCK_SIZE];
                            self.cache.read(old as u64, &mut tmp)?;
                            dst.copy_from_slice(&tmp);
                        }
                    }
                    dst[(s - bstart) as usize..(e - bstart) as usize]
                        .copy_from_slice(&data[(s - off) as usize..(e - off) as usize]);
                }
                self.cache.write_many((pstart + done) as u64, &buf)?;
                lb += c;
                done += c;
            }
        }
        let mut freed = Vec::new();
        remap(&mut exts, first_lb, nlb, &runs, &mut freed);
        self.tx.to_free.extend(freed);
        node.size = node.size.max(end);
        node.mtime = now;
        self.store_extents(ino, &mut node, &exts)?;
        self.write_inode(ino, &node)
    }

    fn truncate_inner(&mut self, ino: Ino, size: u64, now: u64) -> Result<(), FsError> {
        let mut node = self.read_inode(ino)?;
        if node.kind != Kind::File {
            return Err(FsError::IsDir);
        }
        if size > MAX_FILE_BYTES {
            return Err(FsError::TooBig);
        }
        if size == node.size {
            return Ok(());
        }
        if size > node.size {
            node.size = size;
            node.mtime = now;
            return self.write_inode(ino, &node);
        }
        let keep = size.div_ceil(BS) as u32;
        let mut exts = self.load_extents(ino, &node)?;
        let mut freed = Vec::new();
        cut_tail(&mut exts, keep, &mut freed);
        if !size.is_multiple_of(BS) {
            // The last kept block must read as zero past the new end, or a later
            // extension would resurrect old bytes: rewrite it copy-on-write.
            let lb = (size / BS) as u32;
            if let Some(old) = map_block(&exts, lb) {
                let r = self.alloc_blocks(1, old)?;
                let np = r[0].0;
                self.tx.data.push((np, 1));
                let mut tmp: Block = [0u8; BLOCK_SIZE];
                self.cache.read(old as u64, &mut tmp)?;
                tmp[(size % BS) as usize..].fill(0);
                self.cache.write(np as u64, &tmp)?;
                remap(&mut exts, lb, 1, &[(np, 1)], &mut freed);
            }
        }
        self.tx.to_free.extend(freed);
        node.size = size;
        node.mtime = now;
        self.store_extents(ino, &mut node, &exts)?;
        self.write_inode(ino, &node)
    }

    // -----------------------------------------------------------------------
    // Trash
    // -----------------------------------------------------------------------

    /// A name for `name` that is free inside `/.trash`: the name itself, else
    /// `name~2`, `name~3`, ... (truncated to stay within 255 bytes).
    fn unique_trash_name(&mut self, name: &[u8]) -> Result<Vec<u8>, FsError> {
        if self.entry(TRASH_INO, name)?.is_none() {
            return Ok(name.to_vec());
        }
        for n in 2u32..=100_000 {
            let suffix = alloc::format!("~{n}");
            let keep = MAX_NAME - suffix.len();
            let mut cand = name[..name.len().min(keep)].to_vec();
            cand.extend_from_slice(suffix.as_bytes());
            if self.entry(TRASH_INO, &cand)?.is_none() {
                return Ok(cand);
            }
        }
        Err(FsError::Exists)
    }

    /// Move a file or directory (with everything inside) to `/.trash`,
    /// remembering where it came from. O(1) regardless of size.
    pub fn trash<P: AsRef<[u8]> + ?Sized>(&mut self, path: &P, now: u64) -> Result<(), FsError> {
        self.ready()?;
        let p = self.resolve_parent(path.as_ref())?;
        p.check_mutable()?;
        self.txn(|fs| {
            let (ino, kind) = fs.entry(p.dir, &p.name)?.ok_or(FsError::NotFound)?;
            let tname = fs.unique_trash_name(&p.name)?;
            fs.dir_remove(p.dir, &p.name)?;
            fs.dir_insert(TRASH_INO, &tname, ino, kind, now)?;
            let mut n = fs.read_child(ino)?;
            n.flags |= FLAG_TRASHED;
            n.trash_parent = p.dir;
            n.trash_time = now;
            n.trash_pctime = fs.read_inode(p.dir)?.ctime as u32;
            n.trash_name = p.name.clone();
            n.parent = TRASH_INO;
            fs.write_inode(ino, &n)
        })
    }

    /// Everything currently in the trash.
    pub fn trash_list(&mut self) -> Result<Vec<TrashEntry>, FsError> {
        self.ready()?;
        let mut out = Vec::new();
        for (name, ino, kind) in self.dir_list(TRASH_INO)? {
            let n = self.read_child(ino)?;
            let trashed = n.flags & FLAG_TRASHED != 0 && !n.trash_name.is_empty();
            out.push(TrashEntry {
                orig_name: if trashed {
                    n.trash_name.clone()
                } else {
                    name.clone()
                },
                orig_parent: if trashed { n.trash_parent } else { ROOT_INO },
                trash_name: name,
                ino,
                kind,
                size: n.size,
                deleted_at: n.trash_time,
            });
        }
        Ok(out)
    }

    /// Put a trashed item back where it was deleted from (or at the root if
    /// that directory is gone or itself trashed). `Exists` if the name is taken
    /// there. Returns the restored absolute path.
    pub fn trash_restore(&mut self, trash_name: &[u8], now: u64) -> Result<Vec<u8>, FsError> {
        self.ready()?;
        self.txn(|fs| {
            let (ino, kind) = fs.entry(TRASH_INO, trash_name)?.ok_or(FsError::NotFound)?;
            let mut n = fs.read_child(ino)?;
            let trashed = n.flags & FLAG_TRASHED != 0 && !n.trash_name.is_empty();
            let (dest, name) = if trashed {
                let d = if fs.is_live_dir(n.trash_parent)
                    && fs.read_inode(n.trash_parent)?.ctime as u32 == n.trash_pctime
                {
                    n.trash_parent
                } else {
                    ROOT_INO
                };
                (d, n.trash_name.clone())
            } else {
                (ROOT_INO, trash_name.to_vec())
            };
            if dest == ROOT_INO && name == TRASH_NAME {
                return Err(FsError::Reserved);
            }
            if fs.entry(dest, &name)?.is_some() {
                return Err(FsError::Exists);
            }
            fs.dir_remove(TRASH_INO, trash_name)?;
            fs.dir_insert(dest, &name, ino, kind, now)?;
            n.flags &= !FLAG_TRASHED;
            n.parent = dest;
            n.trash_parent = 0;
            n.trash_time = 0;
            n.trash_pctime = 0;
            n.trash_name = Vec::new();
            fs.write_inode(ino, &n)?;
            // Computed inside the transaction so that a failing read cannot
            // report an error for a restore that already took effect.
            fs.path_of(ino)
        })
    }

    /// Permanently delete one item from the trash.
    pub fn trash_purge(&mut self, trash_name: &[u8]) -> Result<(), FsError> {
        self.ready()?;
        let (ino, kind) = self
            .entry(TRASH_INO, trash_name)?
            .ok_or(FsError::NotFound)?;
        self.purge_tree(TRASH_INO, trash_name, ino, kind)
    }

    /// Permanently delete everything in the trash.
    pub fn empty_trash(&mut self) -> Result<(), FsError> {
        self.ready()?;
        for (name, ino, kind) in self.dir_list(TRASH_INO)? {
            self.purge_tree(TRASH_INO, &name, ino, kind)?;
        }
        Ok(())
    }
}

#[allow(dead_code)]
const _: usize = DIR_HDR;
