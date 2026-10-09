//! The per-app view of the filesystem.
//!
//! A [`Sandbox`] belongs to one running app. It maps the guest's `/` to a fixed
//! backend prefix (`/data/<id>` for `fs=own`, `/home` for `fs=home`), refuses
//! everything for `fs=none`, owns the descriptor table (`max_fds`) and enforces
//! the disk quota. The backend is passed to each call (the kernel fetches it per
//! call), so the sandbox holds no reference and is trivially droppable: dropping
//! it closes every descriptor.
//!
//! Quota: for `own`, `used` starts at the size of what is already in
//! `/data/<id>` (`ENTRY_OVERHEAD + size` per entry); for `home` (a shared tree
//! that is not the app's to meter) it counts only what **this run** added.
//! A write past the quota is partial up to the limit, then `NoSpace`.

use super::path::{self, within};
use super::{AppFs, DirEntry, ENTRY_OVERHEAD, FsError, Kind, Stat};
use crate::platform::appabi::{
    MAX_IO, O_ALL, O_APPEND, O_CREATE, O_READ, O_TRUNC, O_WRITE, SEEK_CUR, SEEK_END, SEEK_SET,
};
use crate::platform::appmanifest::{self, FsPerm};
use alloc::string::String;
use alloc::vec::Vec;

/// Largest file offset a descriptor may reach.
pub const MAX_OFFSET: u64 = 1 << 30;

struct OpenFile {
    /// Real (backend) path.
    path: String,
    flags: u32,
    pos: u64,
}

pub struct Sandbox {
    perm: FsPerm,
    /// Backend prefix of the guest's `/` (empty for `none`).
    prefix: String,
    quota: u64,
    used: u64,
    rooted: bool,
    fds: Vec<Option<OpenFile>>,
    /// Count of path-escape attempts (for the log).
    pub escapes: u32,
}

impl Sandbox {
    /// A sandbox for app `id`. `None` when `id` is not a valid app id.
    pub fn new(perm: FsPerm, id: &str, quota_bytes: u64, max_fds: usize) -> Option<Sandbox> {
        if !appmanifest::valid_id(id) {
            return None;
        }
        let prefix = match perm {
            FsPerm::None => String::new(),
            FsPerm::Own => {
                let mut s = String::from("/data/");
                s.push_str(id);
                s
            }
            FsPerm::Home => String::from("/home"),
        };
        let mut fds = Vec::new();
        fds.resize_with(max_fds.clamp(1, 64), || None);
        Some(Sandbox {
            perm,
            prefix,
            quota: quota_bytes,
            used: 0,
            rooted: false,
            fds,
            escapes: 0,
        })
    }

    pub fn perm(&self) -> FsPerm {
        self.perm
    }

    /// Bytes charged so far.
    pub fn used(&self) -> u64 {
        self.used
    }

    pub fn quota(&self) -> u64 {
        self.quota
    }

    /// Descriptors currently open.
    pub fn open_count(&self) -> usize {
        self.fds.iter().filter(|f| f.is_some()).count()
    }

    /// Closes everything (app exit).
    pub fn close_all(&mut self) {
        for f in &mut self.fds {
            *f = None;
        }
    }

    /// Canonical backend path for a guest path.
    fn resolve(&mut self, raw: &[u8]) -> Result<String, FsError> {
        if self.perm == FsPerm::None {
            return Err(FsError::Perm);
        }
        match path::normalize(raw) {
            Ok(rel) => Ok(path::join(&self.prefix, &rel)),
            Err(e) => {
                if e == path::PathError::Escapes {
                    self.escapes = self.escapes.saturating_add(1);
                }
                Err(e.into())
            }
        }
    }

    /// Makes sure the sandbox root exists in the backend (first use).
    fn ensure_root(&mut self, be: &mut dyn AppFs) -> Result<(), FsError> {
        if self.rooted {
            return Ok(());
        }
        be.mkdir_all(&self.prefix)?;
        if self.perm == FsPerm::Own {
            self.used = be.tree_size(&self.prefix)?;
        }
        self.rooted = true;
        Ok(())
    }

    fn charge(&mut self, bytes: u64) -> Result<(), FsError> {
        if self.used.saturating_add(bytes) > self.quota {
            return Err(FsError::NoSpace);
        }
        self.used += bytes;
        Ok(())
    }

    fn refund(&mut self, bytes: u64) {
        self.used = self.used.saturating_sub(bytes);
    }

    fn fd_mut(&mut self, fd: i32) -> Result<&mut OpenFile, FsError> {
        let i = usize::try_from(fd.wrapping_sub(1)).map_err(|_| FsError::BadFd)?;
        self.fds
            .get_mut(i)
            .and_then(|f| f.as_mut())
            .ok_or(FsError::BadFd)
    }

    // ---------------------------------------------------------------- operations

    /// `fs_open`: returns the descriptor (>= 1).
    pub fn open(&mut self, be: &mut dyn AppFs, raw: &[u8], flags: u32) -> Result<i32, FsError> {
        if flags & !O_ALL != 0 || flags & (O_READ | O_WRITE) == 0 {
            return Err(FsError::Invalid);
        }
        if flags & (O_CREATE | O_TRUNC | O_APPEND) != 0 && flags & O_WRITE == 0 {
            return Err(FsError::Invalid);
        }
        let real = self.resolve(raw)?;
        let slot = self
            .fds
            .iter()
            .position(|f| f.is_none())
            .ok_or(FsError::TooManyFds)?;
        self.ensure_root(be)?;
        match be.stat(&real) {
            Ok(Stat {
                kind: Kind::Dir, ..
            }) => return Err(FsError::IsDir),
            Ok(Stat {
                kind: Kind::File,
                size,
            }) => {
                if flags & O_TRUNC != 0 {
                    be.set_len(&real, 0)?;
                    self.refund(size);
                }
            }
            Err(FsError::NotFound) if flags & O_CREATE != 0 => {
                if real == self.prefix {
                    return Err(FsError::IsDir);
                }
                self.charge(ENTRY_OVERHEAD)?;
                if let Err(e) = be.create(&real) {
                    self.refund(ENTRY_OVERHEAD);
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        }
        self.fds[slot] = Some(OpenFile {
            path: real,
            flags,
            pos: 0,
        });
        Ok(slot as i32 + 1)
    }

    pub fn close(&mut self, fd: i32) -> Result<(), FsError> {
        self.fd_mut(fd)?;
        self.fds[(fd - 1) as usize] = None;
        Ok(())
    }

    pub fn read(&mut self, be: &mut dyn AppFs, fd: i32, buf: &mut [u8]) -> Result<usize, FsError> {
        let f = self.fd_mut(fd)?;
        if f.flags & O_READ == 0 {
            return Err(FsError::BadFd);
        }
        let n = buf.len().min(MAX_IO);
        let got = be.read_at(&f.path, f.pos, &mut buf[..n])?;
        f.pos += got as u64;
        Ok(got)
    }

    /// Writes `data` (at most [`MAX_IO`] bytes per call). Past the quota the
    /// write is partial; with nothing fitting it is `NoSpace`.
    pub fn write(&mut self, be: &mut dyn AppFs, fd: i32, data: &[u8]) -> Result<usize, FsError> {
        let (path, flags, mut pos) = {
            let f = self.fd_mut(fd)?;
            if f.flags & O_WRITE == 0 {
                return Err(FsError::BadFd);
            }
            (f.path.clone(), f.flags, f.pos)
        };
        let mut len = data.len().min(MAX_IO);
        if len == 0 {
            return Ok(0);
        }
        let cur = be.stat(&path)?.size;
        if flags & O_APPEND != 0 {
            pos = cur;
        }
        if pos >= MAX_OFFSET {
            return Err(FsError::NoSpace);
        }
        len = len.min((MAX_OFFSET - pos) as usize);
        let grow = |len: usize| (pos + len as u64).saturating_sub(cur);
        let room = self.quota.saturating_sub(self.used);
        if grow(len) > room {
            // Keep only the bytes that fit under the quota (the hole before
            // `pos` is charged too).
            let hole = pos.saturating_sub(cur);
            if hole >= room {
                return Err(FsError::NoSpace);
            }
            len = (room - hole) as usize;
            if len == 0 {
                return Err(FsError::NoSpace);
            }
        }
        let g = grow(len);
        let n = be.write_at(&path, pos, &data[..len])?;
        self.used += g.min(grow(n));
        let f = self.fd_mut(fd)?;
        f.pos = pos + n as u64;
        Ok(n)
    }

    /// `fs_seek`; returns the new offset.
    pub fn seek(
        &mut self,
        be: &mut dyn AppFs,
        fd: i32,
        off: i64,
        whence: i32,
    ) -> Result<u64, FsError> {
        let (path, pos) = {
            let f = self.fd_mut(fd)?;
            (f.path.clone(), f.pos)
        };
        let base: i64 = match whence {
            SEEK_SET => 0,
            SEEK_CUR => i64::try_from(pos).map_err(|_| FsError::Invalid)?,
            SEEK_END => i64::try_from(be.stat(&path)?.size).map_err(|_| FsError::Invalid)?,
            _ => return Err(FsError::Invalid),
        };
        let target = base.checked_add(off).ok_or(FsError::Invalid)?;
        if target < 0 || target as u64 > MAX_OFFSET {
            return Err(FsError::Invalid);
        }
        self.fd_mut(fd)?.pos = target as u64;
        Ok(target as u64)
    }

    pub fn stat(&mut self, be: &mut dyn AppFs, raw: &[u8]) -> Result<Stat, FsError> {
        let real = self.resolve(raw)?;
        self.ensure_root(be)?;
        be.stat(&real)
    }

    /// `fs_readdir`: the `index`-th entry of the folder at `raw`.
    pub fn read_dir(
        &mut self,
        be: &mut dyn AppFs,
        raw: &[u8],
        index: usize,
    ) -> Result<Option<DirEntry>, FsError> {
        let real = self.resolve(raw)?;
        self.ensure_root(be)?;
        be.read_dir(&real, index)
    }

    pub fn mkdir(&mut self, be: &mut dyn AppFs, raw: &[u8]) -> Result<(), FsError> {
        let real = self.resolve(raw)?;
        self.ensure_root(be)?;
        if real == self.prefix {
            return Err(FsError::Exists);
        }
        self.charge(ENTRY_OVERHEAD)?;
        be.mkdir(&real).inspect_err(|_| self.refund(ENTRY_OVERHEAD))
    }

    /// Removes a file or an empty folder. The sandbox root cannot be removed.
    pub fn unlink(&mut self, be: &mut dyn AppFs, raw: &[u8]) -> Result<(), FsError> {
        let real = self.resolve(raw)?;
        self.ensure_root(be)?;
        if real == self.prefix {
            return Err(FsError::Perm);
        }
        let size = be.stat(&real)?.size;
        be.remove(&real)?;
        self.refund(ENTRY_OVERHEAD + size);
        Ok(())
    }

    /// Renames within the sandbox; descriptors on the moved entry follow it.
    pub fn rename(&mut self, be: &mut dyn AppFs, from: &[u8], to: &[u8]) -> Result<(), FsError> {
        let a = self.resolve(from)?;
        let b = self.resolve(to)?;
        self.ensure_root(be)?;
        if a == self.prefix || b == self.prefix {
            return Err(FsError::Perm);
        }
        be.rename(&a, &b)?;
        for f in self.fds.iter_mut().flatten() {
            if within(&f.path, &a) {
                let mut np = b.clone();
                np.push_str(&f.path[a.len()..]);
                f.path = np;
            }
        }
        Ok(())
    }
}
