//! trash (split out of `ops.rs`).

use super::*;

impl<D: BlockDevice> Fs3<D> {
    /// A name for `name` that is free inside `/.trash`: the name itself, else
    /// `name~2`, `name~3`, ... (truncated to stay within 255 bytes).
    pub(super) fn unique_trash_name(&mut self, name: &[u8]) -> Result<Vec<u8>, FsError> {
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
