//! Directory blocks: packed variable-length entries.

use super::crc32::crc32;
use super::extent::cut_tail;
use super::inode::{Extent as Ext, Inode, Kind};
use super::layout::{DIR_MAGIC, rd16, rd32, wr16, wr32};
use super::{Fs3, FsError, Ino};
use crate::storage::blockcache::{BLOCK_SIZE, Block};
use crate::storage::blockdev::BlockDevice;
use alloc::vec::Vec;

/// First byte of the entries area.
pub(super) const DIR_HDR: usize = 16;
/// Bytes of entries per directory block.
pub(super) const DIR_CAP: usize = BLOCK_SIZE - DIR_HDR;

/// A parsed entry; `len` is its encoded size.
pub(super) struct RawEnt<'a> {
    pub ino: u32,
    pub kind: u8,
    pub name: &'a [u8],
    pub len: usize,
}

/// Parse the entry at `off` (the entries area ends at `end`).
pub(super) fn parse_entry(b: &[u8], off: usize, end: usize) -> Option<RawEnt<'_>> {
    if off + 6 > end || end > b.len() {
        return None;
    }
    let nl = b[off + 4] as usize;
    if nl == 0 || off + 6 + nl > end {
        return None;
    }
    Some(RawEnt {
        ino: rd32(b, off),
        kind: b[off + 5],
        name: &b[off + 6..off + 6 + nl],
        len: 6 + nl,
    })
}

fn seal(b: &mut Block) {
    let c = crc32(&b[4..]);
    wr32(b, 0, c);
}

impl<D: BlockDevice> Fs3<D> {
    /// Physical blocks of directory `ino`, in order; the extents must tile
    /// `0..nblocks` with no hole.
    pub(super) fn dir_phys_blocks(&mut self, ino: Ino, dir: &Inode) -> Result<Vec<u32>, FsError> {
        let exts = self.load_extents(ino, dir)?;
        let mut blocks: Vec<u32> = Vec::with_capacity(dir.nblocks as usize);
        let mut expect = 0u64;
        for e in &exts {
            if e.lblk as u64 != expect {
                return Err(FsError::Corrupt("hole in a directory"));
            }
            for k in 0..e.len {
                blocks.push(e.pblk + k);
            }
            expect = e.lend();
        }
        if blocks.len() != dir.nblocks as usize {
            return Err(FsError::Corrupt("directory block count"));
        }
        Ok(blocks)
    }

    /// Look `name` up in directory `dir_ino` (whose inode is `dir`).
    pub(super) fn dir_find(
        &mut self,
        dir_ino: Ino,
        dir: &Inode,
        name: &[u8],
    ) -> Result<Option<(Ino, Kind)>, FsError> {
        if dir.kind != Kind::Dir {
            return Err(FsError::NotDir);
        }
        let blocks = self.dir_phys_blocks(dir_ino, dir)?;
        for pb in blocks {
            let b = self.typed_block(pb, false, dir_ino)?;
            let end = DIR_HDR + rd16(b, 12) as usize;
            let mut off = DIR_HDR;
            while off < end {
                let e = parse_entry(b, off, end).ok_or(FsError::Corrupt("directory entry"))?;
                if e.name == name {
                    let kind = Kind::from_u8(e.kind).ok_or(FsError::Corrupt("entry kind"))?;
                    return Ok(Some((e.ino, kind)));
                }
                off += e.len;
            }
        }
        Ok(None)
    }

    /// Every entry of directory `dir_ino`, in storage order.
    pub(super) fn dir_list(&mut self, dir_ino: Ino) -> Result<Vec<(Vec<u8>, Ino, Kind)>, FsError> {
        let dir = self.read_inode(dir_ino)?;
        if dir.kind != Kind::Dir {
            return Err(FsError::NotDir);
        }
        let blocks = self.dir_phys_blocks(dir_ino, &dir)?;
        let mut out = Vec::new();
        for pb in blocks {
            let b = self.typed_block(pb, false, dir_ino)?;
            let end = DIR_HDR + rd16(b, 12) as usize;
            let mut off = DIR_HDR;
            while off < end {
                let e = parse_entry(b, off, end).ok_or(FsError::Corrupt("directory entry"))?;
                let kind = Kind::from_u8(e.kind).ok_or(FsError::Corrupt("entry kind"))?;
                out.push((e.name.to_vec(), e.ino, kind));
                off += e.len;
            }
        }
        Ok(out)
    }

    /// Add `name -> ino` to a directory; `Exists` if the name is taken.
    pub(super) fn dir_insert(
        &mut self,
        dir_ino: Ino,
        name: &[u8],
        ino: Ino,
        kind: Kind,
        now: u64,
    ) -> Result<(), FsError> {
        let mut dir = self.read_inode(dir_ino)?;
        if dir.kind != Kind::Dir {
            return Err(FsError::NotDir);
        }
        let blocks = self.dir_phys_blocks(dir_ino, &dir)?;
        let need = 6 + name.len();
        let mut room: Option<u32> = None;
        for &pb in &blocks {
            let b = self.typed_block(pb, false, dir_ino)?;
            let used = rd16(b, 12) as usize;
            let end = DIR_HDR + used;
            let mut off = DIR_HDR;
            while off < end {
                let e = parse_entry(b, off, end).ok_or(FsError::Corrupt("directory entry"))?;
                if e.name == name {
                    return Err(FsError::Exists);
                }
                off += e.len;
            }
            if room.is_none() && used + need <= DIR_CAP {
                room = Some(pb);
            }
        }
        let write_entry = |b: &mut Block, used: usize| {
            let o = DIR_HDR + used;
            wr32(b, o, ino);
            b[o + 4] = name.len() as u8;
            b[o + 5] = kind.to_u8();
            b[o + 6..o + 6 + name.len()].copy_from_slice(name);
        };
        if let Some(pb) = room {
            let b = self.meta_mut(pb)?;
            let used = rd16(b, 12) as usize;
            write_entry(b, used);
            wr16(b, 12, (used + need) as u16);
            let cnt = rd16(b, 14);
            wr16(b, 14, cnt.saturating_add(1));
            seal(b);
        } else {
            let hint = blocks.last().map_or(self.alloc_hint, |&b| b + 1);
            let r = self.alloc_blocks(1, hint)?;
            let pb = r[0].0;
            let mut exts = self.load_extents(dir_ino, &dir)?;
            let lb = dir.nblocks;
            match exts.last_mut() {
                Some(l) if l.lend() == lb as u64 && l.pend() == pb as u64 => l.len += 1,
                _ => exts.push(Ext {
                    lblk: lb,
                    len: 1,
                    pblk: pb,
                }),
            }
            self.store_extents(dir_ino, &mut dir, &exts)?;
            dir.size = dir.nblocks as u64 * BLOCK_SIZE as u64;
            let mut img: Block = [0u8; BLOCK_SIZE];
            img[4..8].copy_from_slice(&DIR_MAGIC);
            wr32(&mut img, 8, dir_ino);
            write_entry(&mut img, 0);
            wr16(&mut img, 12, need as u16);
            wr16(&mut img, 14, 1);
            seal(&mut img);
            self.put_meta(pb, img)?;
        }
        dir.nentries = dir.nentries.saturating_add(1);
        dir.mtime = now;
        self.write_inode(dir_ino, &dir)
    }

    /// Remove `name` from a directory, returning what it pointed at. An emptied
    /// last block is given back.
    pub(super) fn dir_remove(&mut self, dir_ino: Ino, name: &[u8]) -> Result<(Ino, Kind), FsError> {
        let mut dir = self.read_inode(dir_ino)?;
        if dir.kind != Kind::Dir {
            return Err(FsError::NotDir);
        }
        let blocks = self.dir_phys_blocks(dir_ino, &dir)?;
        for &pb in blocks.iter() {
            let found = {
                let b = self.typed_block(pb, false, dir_ino)?;
                let used = rd16(b, 12) as usize;
                let end = DIR_HDR + used;
                let mut off = DIR_HDR;
                let mut hit = None;
                while off < end {
                    let e = parse_entry(b, off, end).ok_or(FsError::Corrupt("directory entry"))?;
                    if e.name == name {
                        let kind = Kind::from_u8(e.kind).ok_or(FsError::Corrupt("entry kind"))?;
                        hit = Some((off, e.len, e.ino, kind));
                        break;
                    }
                    off += e.len;
                }
                hit
            };
            let Some((off, elen, ino, kind)) = found else {
                continue;
            };
            let b = self.meta_mut(pb)?;
            let used = rd16(b, 12) as usize;
            let end = DIR_HDR + used;
            b.copy_within(off + elen..end, off);
            b[end - elen..end].fill(0);
            wr16(b, 12, (used - elen) as u16);
            let cnt = rd16(b, 14);
            wr16(b, 14, cnt.saturating_sub(1));
            seal(b);
            if used == elen {
                // Give back every trailing block that is now empty.
                let mut keep = blocks.len();
                while keep > 0 {
                    let tb = self.typed_block(blocks[keep - 1], false, dir_ino)?;
                    if rd16(tb, 12) != 0 {
                        break;
                    }
                    keep -= 1;
                }
                if keep < blocks.len() {
                    let mut exts = self.load_extents(dir_ino, &dir)?;
                    let mut freed = Vec::new();
                    cut_tail(&mut exts, keep as u32, &mut freed);
                    for (s, l) in freed {
                        for k in 0..l {
                            self.tx.meta.remove(&(s + k));
                        }
                        self.tx.to_free.push((s, l));
                    }
                    self.store_extents(dir_ino, &mut dir, &exts)?;
                    dir.size = dir.nblocks as u64 * BLOCK_SIZE as u64;
                }
            }
            dir.nentries = dir
                .nentries
                .checked_sub(1)
                .ok_or(FsError::Corrupt("directory entry count"))?;
            self.write_inode(dir_ino, &dir)?;
            return Ok((ino, kind));
        }
        Err(FsError::NotFound)
    }
}
