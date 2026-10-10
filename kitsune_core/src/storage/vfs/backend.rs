//! backend (split out of `vfs.rs`).

use super::*;

/// The filesystem operations the desktop uses, object-safe so the kernel can hand
/// a `&mut dyn Backend` of either the ATA volume or the RAM volume to the same code.
/// Every `Ok` of a mutating call is already durable on a real disk.
pub trait Backend {
    fn stat(&mut self, path: &[u8]) -> Result<Info>;
    fn readdir(&mut self, path: &[u8]) -> Result<Vec<Entry>>;
    fn read_file(&mut self, path: &[u8]) -> Result<Vec<u8>>;
    fn read_at(&mut self, path: &[u8], off: u64, buf: &mut [u8]) -> Result<usize>;
    /// Create or replace a whole file, atomically.
    fn write_file(&mut self, path: &[u8], data: &[u8], now: u64) -> Result<()>;
    fn create(&mut self, path: &[u8], now: u64) -> Result<()>;
    fn write_at(&mut self, path: &[u8], off: u64, data: &[u8], now: u64) -> Result<()>;
    fn append(&mut self, path: &[u8], data: &[u8], now: u64) -> Result<()>;
    /// Set a file's size: shrinking frees blocks, growing leaves a hole of zeros.
    fn truncate(&mut self, path: &[u8], size: u64, now: u64) -> Result<()>;
    /// Names and kinds of a folder's entries (storage order); cheaper than
    /// [`readdir`](Self::readdir) because it reads no inode.
    fn names(&mut self, path: &[u8]) -> Result<Vec<(Vec<u8>, EntryKind)>>;
    fn mkdir(&mut self, path: &[u8], now: u64) -> Result<()>;
    /// Rename or move; an existing destination is an error.
    fn rename(&mut self, from: &[u8], to: &[u8], now: u64) -> Result<()>;
    /// Permanent delete (file or whole tree).
    fn remove_all(&mut self, path: &[u8]) -> Result<()>;
    /// Move to the trash.
    fn trash(&mut self, path: &[u8], now: u64) -> Result<()>;
    fn trash_list(&mut self) -> Result<Vec<TrashItem>>;
    /// Restore; returns the path it came back to.
    fn trash_restore(&mut self, id: &[u8], now: u64) -> Result<Vec<u8>>;
    fn trash_purge(&mut self, id: &[u8]) -> Result<()>;
    fn empty_trash(&mut self) -> Result<()>;
    fn usage(&mut self) -> Usage;
}

impl<D: BlockDevice> Backend for Fs3<D> {
    fn stat(&mut self, path: &[u8]) -> Result<Info> {
        Ok(Fs3::stat(self, path)?.into())
    }
    fn readdir(&mut self, path: &[u8]) -> Result<Vec<Entry>> {
        Ok(Fs3::readdir(self, path)?
            .into_iter()
            .map(Entry::from)
            .collect())
    }
    fn read_file(&mut self, path: &[u8]) -> Result<Vec<u8>> {
        Ok(Fs3::read_file(self, path)?)
    }
    fn read_at(&mut self, path: &[u8], off: u64, buf: &mut [u8]) -> Result<usize> {
        let ino = Fs3::open(self, path)?;
        Ok(Fs3::read_at(self, ino, off, buf)?)
    }
    fn write_file(&mut self, path: &[u8], data: &[u8], now: u64) -> Result<()> {
        Ok(Fs3::write_file(self, path, data, now)?)
    }
    fn create(&mut self, path: &[u8], now: u64) -> Result<()> {
        Fs3::create(self, path, now)?;
        Ok(())
    }
    fn write_at(&mut self, path: &[u8], off: u64, data: &[u8], now: u64) -> Result<()> {
        let ino = Fs3::open(self, path)?;
        Ok(Fs3::write_at(self, ino, off, data, now)?)
    }
    fn append(&mut self, path: &[u8], data: &[u8], now: u64) -> Result<()> {
        let ino = Fs3::open(self, path)?;
        Ok(Fs3::append(self, ino, data, now)?)
    }
    fn truncate(&mut self, path: &[u8], size: u64, now: u64) -> Result<()> {
        let ino = Fs3::open(self, path)?;
        Ok(Fs3::truncate(self, ino, size, now)?)
    }
    fn names(&mut self, path: &[u8]) -> Result<Vec<(Vec<u8>, EntryKind)>> {
        Ok(Fs3::readdir_names(self, path)?
            .into_iter()
            .map(|(n, k)| (n, k.into()))
            .collect())
    }
    fn mkdir(&mut self, path: &[u8], now: u64) -> Result<()> {
        Fs3::mkdir(self, path, now)?;
        Ok(())
    }
    fn rename(&mut self, from: &[u8], to: &[u8], now: u64) -> Result<()> {
        Ok(Fs3::rename(self, from, to, now)?)
    }
    fn remove_all(&mut self, path: &[u8]) -> Result<()> {
        Ok(Fs3::remove_all(self, path)?)
    }
    fn trash(&mut self, path: &[u8], now: u64) -> Result<()> {
        Ok(Fs3::trash(self, path, now)?)
    }
    fn trash_list(&mut self) -> Result<Vec<TrashItem>> {
        Ok(Fs3::trash_list(self)?
            .into_iter()
            .map(TrashItem::from)
            .collect())
    }
    fn trash_restore(&mut self, id: &[u8], now: u64) -> Result<Vec<u8>> {
        Ok(Fs3::trash_restore(self, id, now)?)
    }
    fn trash_purge(&mut self, id: &[u8]) -> Result<()> {
        Ok(Fs3::trash_purge(self, id)?)
    }
    fn empty_trash(&mut self) -> Result<()> {
        Ok(Fs3::empty_trash(self)?)
    }
    fn usage(&mut self) -> Usage {
        Fs3::statfs(self).into()
    }
}
