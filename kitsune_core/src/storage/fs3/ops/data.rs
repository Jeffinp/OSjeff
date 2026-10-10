//! data (split out of `ops.rs`).

use super::*;

impl<D: BlockDevice> Fs3<D> {
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

    pub(in super::super) fn write_inner(
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

    pub(super) fn truncate_inner(&mut self, ino: Ino, size: u64, now: u64) -> Result<(), FsError> {
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
}
