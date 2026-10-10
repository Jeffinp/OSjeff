//! removal (split out of `ops.rs`).

use super::*;

impl<D: BlockDevice> Fs3<D> {
    /// Free an inode and all of its blocks.
    pub(super) fn release_inode(&mut self, ino: Ino) -> Result<(), FsError> {
        let n = self.read_child(ino)?;
        if n.kind == Kind::Dir && !self.dir_list(ino)?.is_empty() {
            return Err(FsError::NotEmpty);
        }
        self.release_blocks(ino, &n)?;
        self.free_inode(ino)
    }

    /// Drop one entry and free what it pointed at (which must be childless).
    pub(super) fn remove_entry(&mut self, pdir: Ino, name: &[u8], ino: Ino) -> Result<(), FsError> {
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
    pub(super) fn purge_tree(
        &mut self,
        pdir: Ino,
        name: &[u8],
        ino: Ino,
        kind: Kind,
    ) -> Result<(), FsError> {
        if kind == Kind::File {
            return self.txn(|fs| fs.remove_entry(pdir, name, ino));
        }
        // Explicit stack: depth must not be bounded by the call stack, and a
        // corrupt (cyclic) tree must terminate.
        let mut stack: Vec<(Ino, Ino, Vec<u8>)> = alloc::vec![(ino, pdir, name.to_vec())];
        let mut budget = (self.geo.inode_count as u64) * 4 + 16;
        while let Some((dir, parent, dname)) = stack.last().cloned() {
            if budget == 0 {
                return Err(FsError::Corrupt("directory tree never ends"));
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
}
