//! `Secured`: a VFS [`Backend`] that enforces ownership and permissions for one user.
//!
//! It wraps another backend (the real volume) and a [`Cred`]; every operation first checks the
//! rules in [`crate::security::perm`] and only then reaches the volume, so the file manager, the
//! terminal, the editor and the app sandbox all get the same enforcement by being handed a
//! `Secured` instead of the bare backend. New files and folders are created owned by the user,
//! with the mode asked for minus the `umask`.
//!
//! The rules, in short: every folder above the path must be searchable (`x`); reading a file or
//! listing a folder needs `r`; changing a file's contents needs `w`; creating, removing and
//! renaming need `w`+`x` on the folder (and, in a sticky folder, owning the item or the folder);
//! `chmod` is for the owner, `chown` for root (the owner may only pick one of its own groups).
//! The trash is shared storage, but each user sees and restores only what it deleted.
//!
//! What this is not: isolation from code running in ring 0. A bug in the kernel, or in any
//! component given the bare backend, ignores these checks (see `docs/SECURITY-MODEL.md`).

use super::vfs::{
    Backend, Entry, EntryKind, Info, Result, TrashItem, Usage, VfsError, base_name, join, parent,
};
use crate::security::perm::{self, Cred, Owner, R, W, X};
use alloc::vec::Vec;

/// Default mask: files come out `rw-r--r--`, folders `rwxr-xr-x`.
pub const DEFAULT_UMASK: u16 = 0o022;

/// A backend seen through one user's permissions.
pub struct Secured<'a, B: Backend + ?Sized> {
    inner: &'a mut B,
    cred: Cred,
    umask: u16,
}

fn owner_of(i: &Info) -> Owner {
    Owner::new(i.uid, i.gid, i.mode)
}

fn is_dir(i: &Info) -> bool {
    i.kind == EntryKind::Dir
}

const DENIED: VfsError = VfsError::PermissionDenied;

impl<'a, B: Backend + ?Sized> Secured<'a, B> {
    pub fn new(inner: &'a mut B, cred: Cred, umask: u16) -> Secured<'a, B> {
        Secured { inner, cred, umask }
    }

    pub fn cred(&self) -> &Cred {
        &self.cred
    }

    /// Every folder above `path` (the root included) must be searchable.
    fn check_search(&mut self, path: &[u8]) -> Result<()> {
        let comps = super::vfs::components(path);
        if comps.is_empty() {
            return Ok(());
        }
        let mut cur: Vec<u8> = alloc::vec![b'/'];
        for c in &comps[..comps.len() - 1] {
            let i = self.inner.stat(&cur)?;
            if !is_dir(&i) {
                return Err(VfsError::NotDir);
            }
            if !perm::allowed(&self.cred, &owner_of(&i), X, true) {
                return Err(DENIED);
            }
            cur = join(&cur, c);
        }
        let i = self.inner.stat(&cur)?;
        if !is_dir(&i) {
            return Err(VfsError::NotDir);
        }
        if !perm::allowed(&self.cred, &owner_of(&i), X, true) {
            return Err(DENIED);
        }
        Ok(())
    }

    /// Search the way down, stat the item and require `want` on it.
    fn need(&mut self, path: &[u8], want: u8) -> Result<Info> {
        self.check_search(path)?;
        let i = self.inner.stat(path)?;
        if !perm::allowed(&self.cred, &owner_of(&i), want, is_dir(&i)) {
            return Err(DENIED);
        }
        Ok(i)
    }

    /// The folder holding `path`, which must grant `want` (`W | X` to create or remove in it).
    fn need_parent(&mut self, path: &[u8], want: u8) -> Result<Info> {
        let dir = parent(path);
        let i = self.need(&dir, want)?;
        if !is_dir(&i) {
            return Err(VfsError::NotDir);
        }
        Ok(i)
    }

    /// Hand a freshly created item to the user.
    fn claim(&mut self, path: &[u8], base_mode: u16) -> Result<()> {
        let mode = perm::apply_umask(base_mode, self.umask);
        let r = self
            .inner
            .set_owner(path, Some(self.cred.uid), Some(self.cred.gid), Some(mode));
        if r.is_err() {
            // Do not leave an item the user cannot own behind.
            let _ = self.inner.remove_all(path);
        }
        r
    }

    /// May the user remove `path` (a file, or a folder and everything in it)?
    fn can_remove_tree(&mut self, path: &[u8]) -> Result<()> {
        self.check_search(path)?;
        if super::vfs::is_root(path) {
            return Err(VfsError::InvalidPath);
        }
        let child = self.inner.stat(path)?;
        let dir = self.inner.stat(&parent(path))?;
        if !perm::may_remove(&self.cred, &owner_of(&dir), &owner_of(&child)) {
            return Err(DENIED);
        }
        if is_dir(&child) {
            // Emptying a folder means removing each entry from it.
            let mut stack: Vec<(Vec<u8>, Info)> = alloc::vec![(path.to_vec(), child)];
            while let Some((p, info)) = stack.pop() {
                if !perm::allowed(&self.cred, &owner_of(&info), R | W | X, true) {
                    return Err(DENIED);
                }
                for (name, kind) in self.inner.names(&p)? {
                    let cp = join(&p, &name);
                    let ci = self.inner.stat(&cp)?;
                    if !perm::may_remove(&self.cred, &owner_of(&info), &owner_of(&ci)) {
                        return Err(DENIED);
                    }
                    if kind == EntryKind::Dir {
                        stack.push((cp, ci));
                    }
                }
            }
        }
        Ok(())
    }

    fn owns_trashed(&mut self, id: &[u8]) -> Result<bool> {
        if self.cred.is_root() {
            return Ok(true);
        }
        let i = self.inner.stat(&join(b"/.trash", id))?;
        Ok(i.uid == self.cred.uid)
    }
}

impl<B: Backend + ?Sized> Backend for Secured<'_, B> {
    fn stat(&mut self, path: &[u8]) -> Result<Info> {
        self.check_search(path)?;
        self.inner.stat(path)
    }

    fn readdir(&mut self, path: &[u8]) -> Result<Vec<Entry>> {
        self.need(path, R)?;
        self.inner.readdir(path)
    }

    fn read_file(&mut self, path: &[u8]) -> Result<Vec<u8>> {
        self.need(path, R)?;
        self.inner.read_file(path)
    }

    fn read_at(&mut self, path: &[u8], off: u64, buf: &mut [u8]) -> Result<usize> {
        self.need(path, R)?;
        self.inner.read_at(path, off, buf)
    }

    fn write_file(&mut self, path: &[u8], data: &[u8], now: u64) -> Result<()> {
        self.check_search(path)?;
        match self.inner.stat(path) {
            Ok(i) => {
                if is_dir(&i) {
                    return Err(VfsError::IsDir);
                }
                if !perm::allowed(&self.cred, &owner_of(&i), W, false) {
                    return Err(DENIED);
                }
                self.inner.write_file(path, data, now)
            }
            Err(VfsError::NotFound) => {
                self.need_parent(path, W | X)?;
                self.inner.write_file(path, data, now)?;
                self.claim(path, 0o666)
            }
            Err(e) => Err(e),
        }
    }

    fn create(&mut self, path: &[u8], now: u64) -> Result<()> {
        self.check_search(path)?;
        self.need_parent(path, W | X)?;
        self.inner.create(path, now)?;
        self.claim(path, 0o666)
    }

    fn write_at(&mut self, path: &[u8], off: u64, data: &[u8], now: u64) -> Result<()> {
        self.need(path, W)?;
        self.inner.write_at(path, off, data, now)
    }

    fn append(&mut self, path: &[u8], data: &[u8], now: u64) -> Result<()> {
        self.need(path, W)?;
        self.inner.append(path, data, now)
    }

    fn truncate(&mut self, path: &[u8], size: u64, now: u64) -> Result<()> {
        self.need(path, W)?;
        self.inner.truncate(path, size, now)
    }

    fn names(&mut self, path: &[u8]) -> Result<Vec<(Vec<u8>, EntryKind)>> {
        self.need(path, R)?;
        self.inner.names(path)
    }

    fn mkdir(&mut self, path: &[u8], now: u64) -> Result<()> {
        self.check_search(path)?;
        self.need_parent(path, W | X)?;
        self.inner.mkdir(path, now)?;
        self.claim(path, 0o777)
    }

    fn rename(&mut self, from: &[u8], to: &[u8], now: u64) -> Result<()> {
        self.check_search(from)?;
        self.check_search(to)?;
        let child = self.inner.stat(from)?;
        let src_dir = self.inner.stat(&parent(from))?;
        if !perm::may_remove(&self.cred, &owner_of(&src_dir), &owner_of(&child)) {
            return Err(DENIED);
        }
        self.need_parent(to, W | X)?;
        self.inner.rename(from, to, now)
    }

    fn remove_all(&mut self, path: &[u8]) -> Result<()> {
        self.can_remove_tree(path)?;
        self.inner.remove_all(path)
    }

    fn trash(&mut self, path: &[u8], now: u64) -> Result<()> {
        // Moving to the trash removes the item from its folder; what is inside moves with it.
        self.check_search(path)?;
        let child = self.inner.stat(path)?;
        let dir = self.inner.stat(&parent(path))?;
        if !perm::may_remove(&self.cred, &owner_of(&dir), &owner_of(&child)) {
            return Err(DENIED);
        }
        self.inner.trash(path, now)
    }

    fn trash_list(&mut self) -> Result<Vec<TrashItem>> {
        let all = self.inner.trash_list()?;
        if self.cred.is_root() {
            return Ok(all);
        }
        let mut mine = Vec::new();
        for t in all {
            if self.owns_trashed(&t.id)? {
                mine.push(t);
            }
        }
        Ok(mine)
    }

    fn trash_restore(&mut self, id: &[u8], now: u64) -> Result<Vec<u8>> {
        if !self.owns_trashed(id)? {
            return Err(DENIED);
        }
        self.inner.trash_restore(id, now)
    }

    fn trash_purge(&mut self, id: &[u8]) -> Result<()> {
        if !self.owns_trashed(id)? {
            return Err(DENIED);
        }
        self.inner.trash_purge(id)
    }

    fn empty_trash(&mut self) -> Result<()> {
        if self.cred.is_root() {
            return self.inner.empty_trash();
        }
        for t in self.trash_list()? {
            self.inner.trash_purge(&t.id)?;
        }
        Ok(())
    }

    fn usage(&mut self) -> Usage {
        self.inner.usage()
    }

    fn set_owner(
        &mut self,
        path: &[u8],
        uid: Option<u32>,
        gid: Option<u32>,
        mode: Option<u16>,
    ) -> Result<()> {
        self.check_search(path)?;
        let i = self.inner.stat(path)?;
        let o = owner_of(&i);
        if mode.is_some() && !perm::may_chmod(&self.cred, &o) {
            return Err(DENIED);
        }
        if (uid.is_some() || gid.is_some()) && !perm::may_chown(&self.cred, &o, uid, gid) {
            return Err(DENIED);
        }
        if base_name(path).is_empty() && !self.cred.is_root() {
            return Err(DENIED);
        }
        self.inner
            .set_owner(path, uid, gid, mode.map(|m| m & perm::MODE_MASK))
    }
}

#[cfg(test)]
mod tests;
