//! `ShellFs` over the desktop VFS: absolute normalized paths and a working directory per terminal.

use crate::desktop::services::vfs;
use crate::desktop::*;
use kitsune_core::shell::fs::{DirEntry, FsErr, FsUsage, Kind as FsKind, ShellFs, Stat};

/// The shell's view of the VFS. Holds only the working directory.
pub(crate) struct VfsFs {
    cwd: String,
}

impl VfsFs {
    pub(crate) fn new() -> Self {
        Self {
            cwd: String::from("/"),
        }
    }
}

/// A VFS error as the shell's error kind.
fn map_err(e: vfs::VfsError) -> FsErr {
    use vfs::VfsError as V;
    match e {
        V::NotFound => FsErr::NotFound,
        V::Exists => FsErr::AlreadyExists,
        V::NotDir => FsErr::NotADirectory,
        V::IsDir => FsErr::IsADirectory,
        V::NotEmpty => FsErr::NotEmpty,
        V::InvalidName | V::InvalidPath | V::InvalidMove => FsErr::InvalidPath,
        V::NameTooLong => FsErr::NameTooLong,
        V::Reserved => FsErr::ReadOnly,
        V::PermissionDenied => FsErr::PermissionDenied,
        V::NoSpace | V::NoInodes => FsErr::NoSpace,
        V::TooBig => FsErr::TooBig,
        V::Busy | V::Unavailable | V::Io | V::Corrupt | V::Cancelled => FsErr::Io,
    }
}

fn kind_of(k: vfs::EntryKind) -> FsKind {
    match k {
        vfs::EntryKind::File => FsKind::File,
        vfs::EntryKind::Dir => FsKind::Dir,
    }
}

impl ShellFs for VfsFs {
    fn cwd(&self) -> String {
        self.cwd.clone()
    }

    fn set_cwd(&mut self, path: &str) -> Result<(), FsErr> {
        let p = self.resolve(path);
        match self.stat(&p)?.kind {
            FsKind::Dir => {
                self.cwd = p;
                Ok(())
            }
            FsKind::File => Err(FsErr::NotADirectory),
        }
    }

    fn stat(&self, path: &str) -> Result<Stat, FsErr> {
        let p = self.resolve(path);
        if p == "/" {
            return Ok(Stat {
                kind: FsKind::Dir,
                size: 0,
            });
        }
        let i = vfs::stat(p.as_bytes()).map_err(map_err)?;
        Ok(Stat {
            kind: kind_of(i.kind),
            size: i.size,
        })
    }

    fn read(&mut self, path: &str) -> Result<Vec<u8>, FsErr> {
        vfs::read_file(self.resolve(path).as_bytes()).map_err(map_err)
    }

    fn read_at(&mut self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>, FsErr> {
        vfs::read_range(self.resolve(path).as_bytes(), offset, len).map_err(map_err)
    }

    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), FsErr> {
        vfs::write_file(self.resolve(path).as_bytes(), data).map_err(map_err)
    }

    fn append(&mut self, path: &str, data: &[u8]) -> Result<(), FsErr> {
        vfs::append(self.resolve(path).as_bytes(), data).map_err(map_err)
    }

    fn list(&self, path: &str) -> Result<Vec<DirEntry>, FsErr> {
        let rows = vfs::list(self.resolve(path).as_bytes()).map_err(map_err)?;
        Ok(rows
            .into_iter()
            .map(|e| DirEntry {
                name: String::from_utf8_lossy(&e.name).into_owned(),
                kind: kind_of(e.kind),
                size: e.size,
            })
            .collect())
    }

    fn mkdir(&mut self, path: &str) -> Result<(), FsErr> {
        vfs::mkdir(self.resolve(path).as_bytes()).map_err(map_err)
    }

    /// Files and empty folders go to the trash (restorable from the file
    /// manager), like the desktop's own delete.
    fn remove(&mut self, path: &str) -> Result<(), FsErr> {
        let p = self.resolve(path);
        if self.stat(&p)?.kind == FsKind::Dir && !self.list(&p)?.is_empty() {
            return Err(FsErr::NotEmpty);
        }
        vfs::remove(p.as_bytes()).map_err(map_err)
    }

    /// An existing destination file is replaced (the old one goes to the trash).
    fn rename(&mut self, from: &str, to: &str) -> Result<(), FsErr> {
        let (a, b) = (self.resolve(from), self.resolve(to));
        if a == b {
            return Ok(());
        }
        if let Ok(st) = self.stat(&b) {
            if st.kind == FsKind::Dir {
                return Err(FsErr::AlreadyExists);
            }
            vfs::remove(b.as_bytes()).map_err(map_err)?;
        }
        vfs::rename_path(a.as_bytes(), b.as_bytes()).map_err(map_err)
    }

    fn usage(&self) -> FsUsage {
        let u = vfs::statfs();
        FsUsage {
            total_bytes: u.total,
            used_bytes: u.used(),
            files: 0,
            dirs: 0,
        }
    }
}

// ---- system information ----------------------------------------------------
