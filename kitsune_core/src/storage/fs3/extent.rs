//! Extent lists: loading, storing (inline + indirect chain) and the pure
//! helpers that remap or cut a list.

use super::crc32::crc32;
use super::inode::{EXTENT_SIZE, EXTENTS_PER_BLOCK, Extent, INLINE_EXTENTS, Inode};
use super::layout::{EXT_MAGIC, rd32, wr32};
use super::{Fs3, FsError};
use crate::storage::blockcache::{BLOCK_SIZE, Block};
use crate::storage::blockdev::BlockDevice;
use alloc::vec::Vec;

/// Physical block of logical block `lb`, if mapped (`exts` sorted, disjoint).
pub(super) fn map_block(exts: &[Extent], lb: u32) -> Option<u32> {
    let i = exts.partition_point(|e| e.lend() <= lb as u64);
    let e = exts.get(i)?;
    if e.lblk <= lb {
        Some(e.pblk + (lb - e.lblk))
    } else {
        None
    }
}

/// Merge neighbours that are adjacent both logically and physically.
pub(super) fn coalesce(exts: &mut Vec<Extent>) {
    let mut out: Vec<Extent> = Vec::with_capacity(exts.len());
    for &e in exts.iter() {
        if let Some(last) = out.last_mut()
            && last.lend() == e.lblk as u64
            && last.pend() == e.pblk as u64
        {
            last.len += e.len;
            continue;
        }
        out.push(e);
    }
    *exts = out;
}

/// Replace the mapping of logical blocks `[lb, lb+n)` with `new_runs` (laid out
/// consecutively from `lb`). The physical ranges that were mapped there are
/// appended to `freed`.
pub(super) fn remap(
    exts: &mut Vec<Extent>,
    lb: u32,
    n: u32,
    new_runs: &[(u32, u32)],
    freed: &mut Vec<(u32, u32)>,
) {
    let (start, end) = (lb as u64, lb as u64 + n as u64);
    let mut out: Vec<Extent> = Vec::with_capacity(exts.len() + new_runs.len() + 2);
    for &e in exts.iter() {
        if e.lend() <= start || e.lblk as u64 >= end {
            out.push(e);
            continue;
        }
        if (e.lblk as u64) < start {
            out.push(Extent {
                lblk: e.lblk,
                len: (start - e.lblk as u64) as u32,
                pblk: e.pblk,
            });
        }
        let ov_s = (e.lblk as u64).max(start);
        let ov_e = e.lend().min(end);
        freed.push((e.pblk + (ov_s - e.lblk as u64) as u32, (ov_e - ov_s) as u32));
        if e.lend() > end {
            out.push(Extent {
                lblk: end as u32,
                len: (e.lend() - end) as u32,
                pblk: e.pblk + (end - e.lblk as u64) as u32,
            });
        }
    }
    let mut cur = lb;
    for &(p, l) in new_runs {
        out.push(Extent {
            lblk: cur,
            len: l,
            pblk: p,
        });
        cur += l;
    }
    out.sort_by_key(|e| e.lblk);
    coalesce(&mut out);
    *exts = out;
}

/// Drop every logical block `>= keep`, appending the freed physical ranges.
pub(super) fn cut_tail(exts: &mut Vec<Extent>, keep: u32, freed: &mut Vec<(u32, u32)>) {
    let mut out = Vec::with_capacity(exts.len());
    for &e in exts.iter() {
        if e.lblk >= keep {
            freed.push((e.pblk, e.len));
        } else if e.lend() > keep as u64 {
            let k = keep - e.lblk;
            out.push(Extent {
                lblk: e.lblk,
                len: k,
                pblk: e.pblk,
            });
            freed.push((e.pblk + k, e.len - k));
        } else {
            out.push(e);
        }
    }
    *exts = out;
}

impl<D: BlockDevice> Fs3<D> {
    /// The data-area block numbers of `ino`'s indirect extent chain.
    pub(super) fn walk_chain(&mut self, ino: u32, first: u32) -> Result<Vec<u32>, FsError> {
        let mut chain = Vec::new();
        let mut cur = first;
        let bound = self.geo.total_blocks as usize / EXTENTS_PER_BLOCK + 2;
        while cur != 0 {
            if chain.len() >= bound {
                return Err(FsError::Corrupt("extent chain too long or cyclic"));
            }
            let next = rd32(self.typed_block(cur, true, ino)?, 8);
            chain.push(cur);
            cur = next;
        }
        Ok(chain)
    }

    /// All extents of `node` (inline + chain), validated: sorted, disjoint,
    /// non-empty, inside the data area.
    pub(super) fn load_extents(&mut self, ino: u32, node: &Inode) -> Result<Vec<Extent>, FsError> {
        let n = node.nextents as usize;
        if n > self.geo.total_blocks as usize {
            return Err(FsError::Corrupt("absurd extent count"));
        }
        let mut out: Vec<Extent> = Vec::with_capacity(n);
        out.extend_from_slice(&node.inline[..n.min(INLINE_EXTENTS)]);
        let mut rest = n.saturating_sub(INLINE_EXTENTS);
        let mut cur = node.ext_chain;
        let mut hops = 0usize;
        while rest > 0 {
            hops += 1;
            if cur == 0 || hops > n {
                return Err(FsError::Corrupt("extent chain shorter than extent count"));
            }
            let b = self.typed_block(cur, true, ino)?;
            let cnt = rd32(b, 12) as usize;
            if cnt == 0 || cnt > EXTENTS_PER_BLOCK || cnt > rest {
                return Err(FsError::Corrupt("extent block count"));
            }
            for k in 0..cnt {
                out.push(Extent::decode(&b[24 + k * EXTENT_SIZE..]));
            }
            rest -= cnt;
            cur = rd32(b, 8);
        }
        if cur != 0 {
            return Err(FsError::Corrupt("extent chain longer than extent count"));
        }
        let mut prev_end = 0u64;
        let hi = self.geo.backup_sb() as u64;
        for e in &out {
            if e.len == 0
                || (e.lblk as u64) < prev_end
                || e.lend() > u32::MAX as u64
                || e.pblk < self.geo.data_start
                || e.pend() > hi
            {
                return Err(FsError::Corrupt("invalid extent"));
            }
            prev_end = e.lend();
        }
        Ok(out)
    }

    /// Store `exts` into `node` (inline part) and the indirect chain, reusing
    /// existing chain blocks, allocating or freeing as the length changes, and
    /// only dirtying chain blocks whose content changed. Recomputes
    /// `nextents`, `nblocks` and `ext_chain`; the caller writes the inode.
    pub(super) fn store_extents(
        &mut self,
        ino: u32,
        node: &mut Inode,
        exts: &[Extent],
    ) -> Result<(), FsError> {
        let old = self.walk_chain(ino, node.ext_chain)?;
        let mut blocks: u64 = 0;
        for e in exts {
            blocks += e.len as u64;
        }
        node.nblocks = u32::try_from(blocks).map_err(|_| FsError::TooBig)?;
        node.nextents = u32::try_from(exts.len()).map_err(|_| FsError::TooBig)?;
        node.inline = [Extent::default(); INLINE_EXTENTS];
        let inline_n = exts.len().min(INLINE_EXTENTS);
        node.inline[..inline_n].copy_from_slice(&exts[..inline_n]);
        let rest = &exts[inline_n..];
        let need = rest.len().div_ceil(EXTENTS_PER_BLOCK);
        let mut chain: Vec<u32> = old.iter().take(need).copied().collect();
        while chain.len() < need {
            let hint = chain.last().map_or(self.alloc_hint, |&b| b + 1);
            let r = self.alloc_blocks(1, hint)?;
            chain.push(r[0].0);
        }
        for &extra in old.iter().skip(need) {
            self.free_meta_block(extra);
        }
        for (i, &blk) in chain.iter().enumerate() {
            let part = &rest[i * EXTENTS_PER_BLOCK..((i + 1) * EXTENTS_PER_BLOCK).min(rest.len())];
            let mut img: Block = [0u8; BLOCK_SIZE];
            img[4..8].copy_from_slice(&EXT_MAGIC);
            wr32(&mut img, 8, chain.get(i + 1).copied().unwrap_or(0));
            wr32(&mut img, 12, part.len() as u32);
            wr32(&mut img, 16, ino);
            for (k, e) in part.iter().enumerate() {
                e.encode(&mut img[24 + k * EXTENT_SIZE..]);
            }
            let c = crc32(&img[4..]);
            wr32(&mut img, 0, c);
            let same = i < old.len() && self.meta_get(blk)?[..] == img[..];
            if !same {
                self.put_meta(blk, img)?;
            }
        }
        node.ext_chain = chain.first().copied().unwrap_or(0);
        Ok(())
    }

    /// Free every block of `node` (data/directory blocks and the extent chain)
    /// at commit. Directory blocks are dropped from the overlay too.
    pub(super) fn release_blocks(&mut self, ino: u32, node: &Inode) -> Result<(), FsError> {
        let exts = self.load_extents(ino, node)?;
        let chain = self.walk_chain(ino, node.ext_chain)?;
        let meta = node.kind == super::inode::Kind::Dir;
        for e in exts {
            if meta {
                for k in 0..e.len {
                    self.tx.meta.remove(&(e.pblk + k));
                }
            }
            self.tx.to_free.push((e.pblk, e.len));
        }
        for b in chain {
            self.free_meta_block(b);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
