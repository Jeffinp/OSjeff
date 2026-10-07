//! Offline-style consistency checker, run over a mounted (idle) filesystem.
//!
//! It re-reads everything from the cache/device (bitmaps included) and checks
//! the invariants listed in `docs/design/ojfs3.md` §11. It reports problems as
//! data instead of failing at the first one, never panics on a corrupt image,
//! and terminates on cycles (every inode is expanded at most once).

use super::bits;
use super::dir::DIR_HDR;
use super::inode::{FLAG_TRASHED, Kind};
use super::layout::{
    BITMAP_WORDS_PER_BLOCK, ROOT_INO, TRASH_INO, TRASH_NAME, journal_crc_ok, parse_journal_header,
};
use super::ops::check_name;
use super::{Fs3, FsError, load_bitmap_block};
use crate::blockcache::BLOCK_SIZE;
use crate::blockdev::BlockDevice;
use alloc::collections::BTreeSet;
use alloc::vec::Vec;

/// Most issues recorded (the count keeps going in `total_issues`).
pub const MAX_ISSUES: usize = 64;

/// One problem found by [`Fs3::fsck`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FsckIssue {
    /// What is wrong.
    pub what: &'static str,
    /// Inode involved (0 if none).
    pub ino: u32,
    /// Block involved (0 if none).
    pub block: u32,
}

/// Result of [`Fs3::fsck`].
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct FsckReport {
    /// The first [`MAX_ISSUES`] problems.
    pub issues: Vec<FsckIssue>,
    /// Total problems found (may exceed `issues.len()`).
    pub total_issues: u64,
    pub inodes_checked: u32,
    pub files: u32,
    pub dirs: u32,
    pub extents: u64,
    /// Blocks owned by files, directories and extent chains.
    pub data_blocks: u32,
}

impl FsckReport {
    /// True if no problem was found.
    pub fn is_clean(&self) -> bool {
        self.total_issues == 0
    }

    /// True if an issue with this description was recorded.
    pub fn has(&self, what: &str) -> bool {
        self.issues.iter().any(|i| i.what == what)
    }

    fn add(&mut self, what: &'static str, ino: u32, block: u32) {
        self.total_issues += 1;
        if self.issues.len() < MAX_ISSUES {
            self.issues.push(FsckIssue { what, ino, block });
        }
    }
}

impl<D: BlockDevice> Fs3<D> {
    /// Verify every invariant. Needs no transaction in progress (always true
    /// between public calls). I/O errors abort the check; inconsistencies are
    /// reported in the returned [`FsckReport`].
    pub fn fsck(&mut self) -> Result<FsckReport, FsError> {
        self.ready()?;
        let geo = self.geo;
        let mut rep = FsckReport::default();

        // 1. The journal must be idle between operations.
        let mut h = [0u8; BLOCK_SIZE];
        self.cache.read(geo.jhdr as u64, &mut h)?;
        if let Some(p) = parse_journal_header(&h, geo.journal_blocks) {
            let n = p.targets.len();
            if n > 0 {
                let mut payload = alloc::vec![0u8; n * BLOCK_SIZE];
                self.cache.read_many(geo.jpayload as u64, &mut payload)?;
                if journal_crc_ok(&h, &payload) {
                    rep.add(
                        "journal holds a committed, unretired transaction",
                        0,
                        geo.jhdr,
                    );
                }
            }
        }

        // 2. Bitmaps as they are on the medium.
        let mut disk_b = bits::new(geo.total_blocks);
        let mut raw = alloc::vec![0u8; geo.bbitmap_blocks as usize * BLOCK_SIZE];
        self.cache.read_many(geo.bbitmap_start as u64, &mut raw)?;
        for i in 0..geo.bbitmap_blocks {
            let o = i as usize * BLOCK_SIZE;
            if load_bitmap_block(&raw[o..o + BLOCK_SIZE], i, &mut disk_b).is_err() {
                rep.add("block bitmap checksum", 0, geo.bbitmap_start + i);
            }
        }
        bits::fix_padding(&mut disk_b, geo.total_blocks);
        let mut disk_i = bits::new(geo.inode_count);
        let mut raw = alloc::vec![0u8; geo.ibitmap_blocks as usize * BLOCK_SIZE];
        self.cache.read_many(geo.ibitmap_start as u64, &mut raw)?;
        for i in 0..geo.ibitmap_blocks {
            let o = i as usize * BLOCK_SIZE;
            if load_bitmap_block(&raw[o..o + BLOCK_SIZE], i, &mut disk_i).is_err() {
                rep.add("inode bitmap checksum", 0, geo.ibitmap_start + i);
            }
        }
        bits::fix_padding(&mut disk_i, geo.inode_count);
        if disk_b != self.bbits || disk_i != self.ibits {
            rep.add("in-memory bitmaps differ from the medium", 0, 0);
        }
        if self.free_blocks != bits::count_free(&disk_b, geo.total_blocks) {
            rep.add("free block counter", 0, 0);
        }
        if self.free_inodes != bits::count_free(&disk_i, geo.inode_count) {
            rep.add("free inode counter", 0, 0);
        }

        // 3. Blocks claimed by structures.
        let mut claimed = bits::new(geo.total_blocks);
        for b in 0..geo.data_start {
            bits::set(&mut claimed, b);
        }
        bits::set(&mut claimed, geo.backup_sb());

        // 4. Walk the tree from the root.
        let n_ino = geo.inode_count as usize;
        let mut refs = alloc::vec![0u8; n_ino + 1];
        let mut visited: Vec<u32> = Vec::new();
        refs[ROOT_INO as usize] = 1;
        let mut stack: Vec<u32> = alloc::vec![ROOT_INO];
        let mut saw_trash_entry = 0u32;
        while let Some(dir_ino) = stack.pop() {
            visited.push(dir_ino);
            let dir = match self.read_inode(dir_ino) {
                Ok(d) => d,
                Err(FsError::Io(e)) => return Err(FsError::Io(e)),
                Err(_) => {
                    rep.add("directory inode unreadable", dir_ino, 0);
                    continue;
                }
            };
            if dir.kind != Kind::Dir {
                rep.add("directory entry expects a directory", dir_ino, 0);
                continue;
            }
            rep.dirs += 1;
            if dir.size != dir.nblocks as u64 * BLOCK_SIZE as u64 {
                rep.add("directory size != blocks", dir_ino, 0);
            }
            let entries = match self.dir_list(dir_ino) {
                Ok(e) => e,
                Err(FsError::Io(e)) => return Err(FsError::Io(e)),
                Err(_) => {
                    rep.add("directory content unreadable", dir_ino, 0);
                    continue;
                }
            };
            // Directory block headers: `used` must equal the sum of its entries
            // (re-derive from the listing: count only).
            if entries.len() != dir.nentries as usize {
                rep.add("directory entry count", dir_ino, 0);
            }
            let mut names: BTreeSet<&[u8]> = BTreeSet::new();
            for (name, ino, kind) in &entries {
                if check_name(name).is_err() {
                    rep.add("invalid name in directory", dir_ino, 0);
                }
                if !names.insert(name.as_slice()) {
                    rep.add("duplicate name in directory", dir_ino, 0);
                }
                let (ino, kind) = (*ino, *kind);
                if ino == 0 || ino as usize > n_ino || !bits::get(&disk_i, ino - 1) {
                    rep.add("entry points at an unallocated inode", ino, 0);
                    continue;
                }
                if dir_ino == ROOT_INO && name.as_slice() == TRASH_NAME {
                    saw_trash_entry += 1;
                    if ino != TRASH_INO {
                        rep.add(".trash entry is not the trash inode", ino, 0);
                    }
                }
                refs[ino as usize] = refs[ino as usize].saturating_add(1);
                if refs[ino as usize] > 1 {
                    rep.add("inode referenced more than once", ino, 0);
                    continue;
                }
                let child = match self.read_inode(ino) {
                    Ok(c) => c,
                    Err(FsError::Io(e)) => return Err(FsError::Io(e)),
                    Err(_) => {
                        rep.add("inode checksum", ino, 0);
                        continue;
                    }
                };
                if child.kind != kind {
                    rep.add("entry kind differs from inode kind", ino, 0);
                }
                if child.parent != dir_ino {
                    rep.add("inode parent pointer", ino, 0);
                }
                if child.nlink != 1 {
                    rep.add("link count", ino, 0);
                }
                let in_trash_dir = dir_ino == TRASH_INO;
                let flagged = child.flags & FLAG_TRASHED != 0;
                if in_trash_dir != flagged {
                    rep.add("trash flag does not match location", ino, 0);
                }
                if flagged && child.trash_name.is_empty() {
                    rep.add("trashed entry lacks its original name", ino, 0);
                }
                if child.kind == Kind::Dir {
                    stack.push(ino);
                } else {
                    visited.push(ino);
                    rep.files += 1;
                }
            }
        }
        if saw_trash_entry != 1 {
            rep.add("root must hold exactly one .trash entry", ROOT_INO, 0);
        }

        // 5. Per-inode extents, sizes, block ownership.
        for &ino in &visited {
            rep.inodes_checked += 1;
            let node = match self.read_inode(ino) {
                Ok(n) => n,
                Err(FsError::Io(e)) => return Err(FsError::Io(e)),
                Err(_) => continue,
            };
            let exts = match self.load_extents(ino, &node) {
                Ok(e) => e,
                Err(FsError::Io(e)) => return Err(FsError::Io(e)),
                Err(_) => {
                    rep.add("extent list invalid", ino, 0);
                    continue;
                }
            };
            let chain = match self.walk_chain(ino, node.ext_chain) {
                Ok(c) => c,
                Err(FsError::Io(e)) => return Err(FsError::Io(e)),
                Err(_) => {
                    rep.add("extent chain invalid", ino, 0);
                    continue;
                }
            };
            rep.extents += exts.len() as u64;
            let mut total = 0u64;
            for e in &exts {
                total += e.len as u64;
                for b in e.pblk..e.pblk + e.len {
                    if bits::get(&claimed, b) {
                        rep.add("block owned by two structures", ino, b);
                    } else {
                        bits::set(&mut claimed, b);
                        rep.data_blocks += 1;
                    }
                }
            }
            for &b in &chain {
                if bits::get(&claimed, b) {
                    rep.add("block owned by two structures", ino, b);
                } else {
                    bits::set(&mut claimed, b);
                    rep.data_blocks += 1;
                }
            }
            if total != node.nblocks as u64 {
                rep.add("nblocks != sum of extent lengths", ino, 0);
            }
            if node.nextents as usize != exts.len() {
                rep.add("nextents", ino, 0);
            }
            let want_chain = exts.len().saturating_sub(super::inode::INLINE_EXTENTS);
            if chain.len() != want_chain.div_ceil(super::inode::EXTENTS_PER_BLOCK) {
                rep.add("extent chain length", ino, 0);
            }
            if node.kind == Kind::File {
                let need = node.size.div_ceil(BLOCK_SIZE as u64);
                if let Some(last) = exts.last()
                    && last.lend() > need
                {
                    rep.add("extent beyond end of file", ino, 0);
                }
                // The tail of the last block must be zero.
                let rem = (node.size % BLOCK_SIZE as u64) as usize;
                if rem != 0 {
                    let lb = (node.size / BLOCK_SIZE as u64) as u32;
                    if let Some(pb) = super::extent::map_block(&exts, lb) {
                        let mut blk = [0u8; BLOCK_SIZE];
                        self.cache.read(pb as u64, &mut blk)?;
                        if blk[rem..].iter().any(|&x| x != 0) {
                            rep.add("non-zero bytes past end of file", ino, pb);
                        }
                    }
                }
            }
            let _ = DIR_HDR;
        }

        // 6. Orphans and bitmap agreement.
        for ino in 1..=geo.inode_count {
            if bits::get(&disk_i, ino - 1) && refs[ino as usize] == 0 {
                rep.add("allocated inode is unreachable", ino, 0);
            }
        }
        let mut word = 0usize;
        let _ = BITMAP_WORDS_PER_BLOCK;
        while word < claimed.len() {
            if claimed[word] != disk_b[word] {
                // Find the first differing block for the report.
                let diff = claimed[word] ^ disk_b[word];
                let b = word as u32 * 64 + diff.trailing_zeros();
                if b < geo.total_blocks {
                    if bits::get(&claimed, b) {
                        rep.add("block in use but marked free", 0, b);
                    } else {
                        rep.add("block marked used but unowned (leak)", 0, b);
                    }
                }
            }
            word += 1;
        }
        Ok(rep)
    }
}
