//! On-disk constants, geometry, superblock and journal header of OJFS v3.
//! See `docs/design/ojfs3.md` for the byte-level tables.

use super::crc32::{Crc32, crc32};
use crate::storage::blockcache::{BLOCK_SIZE, Block};
use alloc::vec::Vec;

/// LBA where the v3 area starts (the 64 KiB before it belong to the v2 image).
pub const FS_START_LBA: u64 = 128;
/// Smallest disk that can hold a v3 filesystem: 1 MiB.
pub const MIN_DISK_SECTORS: u64 = 2048;
/// Format version stored in the superblock.
pub const VERSION: u32 = 1;
/// Superblock magic.
pub const MAGIC: [u8; 4] = *b"OJF3";
pub(super) const JOURNAL_MAGIC: [u8; 4] = *b"OJJ3";
pub(super) const DIR_MAGIC: [u8; 4] = *b"OJD3";
pub(super) const EXT_MAGIC: [u8; 4] = *b"OJX3";

/// Bytes of an inode.
pub const INODE_SIZE: usize = 512;
/// Inodes per block.
pub const INODES_PER_BLOCK: u32 = (BLOCK_SIZE / INODE_SIZE) as u32;
/// Bitmap words (u64) per bitmap block: header is 8 bytes, the rest is bits.
pub(super) const BITMAP_WORDS_PER_BLOCK: u32 = ((BLOCK_SIZE - 8) / 8) as u32;
/// Bits per bitmap block (32 704).
pub const BITS_PER_BITMAP_BLOCK: u32 = BITMAP_WORDS_PER_BLOCK * 64;
/// Maximum journal payload blocks (targets must fit the header block).
pub const MAX_JOURNAL_BLOCKS: u32 = ((BLOCK_SIZE - 24) / 4) as u32;

/// Fixed inode of the root directory.
pub const ROOT_INO: u32 = 1;
/// Fixed inode of `/.trash`.
pub const TRASH_INO: u32 = 2;
/// Name of the trash directory under the root.
pub const TRASH_NAME: &[u8] = b".trash";

pub(super) fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
pub(super) fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
pub(super) fn rd64(b: &[u8], o: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(a)
}
pub(super) fn wr16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
pub(super) fn wr32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
pub(super) fn wr64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

/// Positions of every region, in blocks relative to the start of the v3 area.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Geometry {
    pub total_blocks: u32,
    /// Journal payload blocks (J).
    pub journal_blocks: u32,
    pub inode_count: u32,
    /// Journal header/commit block (always 1).
    pub jhdr: u32,
    /// First journal payload block (always 2).
    pub jpayload: u32,
    pub bbitmap_start: u32,
    pub bbitmap_blocks: u32,
    pub ibitmap_start: u32,
    pub ibitmap_blocks: u32,
    pub itable_start: u32,
    pub itable_blocks: u32,
    pub data_start: u32,
}

impl Geometry {
    /// Compute the layout for the given parameters, or `None` if they do not
    /// describe a usable filesystem.
    pub fn compute(total_blocks: u32, journal_blocks: u32, inode_count: u32) -> Option<Geometry> {
        if journal_blocks == 0 || journal_blocks > MAX_JOURNAL_BLOCKS {
            return None;
        }
        if inode_count < INODES_PER_BLOCK || !inode_count.is_multiple_of(INODES_PER_BLOCK) {
            return None;
        }
        let total = total_blocks as u64;
        let bb = total.div_ceil(BITS_PER_BITMAP_BLOCK as u64);
        let ib = (inode_count as u64).div_ceil(BITS_PER_BITMAP_BLOCK as u64);
        let it = (inode_count / INODES_PER_BLOCK) as u64;
        let jpayload = 2u64;
        let bbitmap_start = jpayload + journal_blocks as u64;
        let ibitmap_start = bbitmap_start + bb;
        let itable_start = ibitmap_start + ib;
        let data_start = itable_start + it;
        // At least 4 data blocks plus the trailing superblock copy.
        if data_start + 4 + 1 > total {
            return None;
        }
        Some(Geometry {
            total_blocks,
            journal_blocks,
            inode_count,
            jhdr: 1,
            jpayload: jpayload as u32,
            bbitmap_start: bbitmap_start as u32,
            bbitmap_blocks: bb as u32,
            ibitmap_start: ibitmap_start as u32,
            ibitmap_blocks: ib as u32,
            itable_start: itable_start as u32,
            itable_blocks: it as u32,
            data_start: data_start as u32,
        })
    }

    /// Default journal size and inode count for a filesystem of `total` blocks.
    pub fn defaults(total_blocks: u32) -> (u32, u32) {
        let j = (total_blocks / 16).clamp(24, 256);
        let ino = (total_blocks / 4).max(64);
        let ino = ino.div_ceil(INODES_PER_BLOCK) * INODES_PER_BLOCK;
        (j, ino)
    }

    /// Block holding the superblock copy.
    pub fn backup_sb(&self) -> u32 {
        self.total_blocks - 1
    }
}

/// The (immutable) superblock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Superblock {
    pub uuid: [u8; 16],
    pub created: u64,
    pub geo: Geometry,
}

impl Superblock {
    /// Serialize into the first 512 bytes of a sector.
    pub fn encode(&self) -> [u8; 512] {
        let mut b = [0u8; 512];
        b[4..8].copy_from_slice(&MAGIC);
        wr32(&mut b, 8, VERSION);
        wr32(&mut b, 12, BLOCK_SIZE as u32);
        b[16..32].copy_from_slice(&self.uuid);
        let g = &self.geo;
        wr32(&mut b, 32, g.total_blocks);
        wr32(&mut b, 36, g.journal_blocks);
        wr32(&mut b, 40, g.inode_count);
        wr32(&mut b, 44, g.bbitmap_start);
        wr32(&mut b, 48, g.bbitmap_blocks);
        wr32(&mut b, 52, g.ibitmap_start);
        wr32(&mut b, 56, g.ibitmap_blocks);
        wr32(&mut b, 60, g.itable_start);
        wr32(&mut b, 64, g.itable_blocks);
        wr32(&mut b, 68, g.data_start);
        wr64(&mut b, 72, self.created);
        let c = crc32(&b[4..]);
        wr32(&mut b, 0, c);
        b
    }

    /// Parse and fully validate a superblock sector.
    pub fn decode(b: &[u8]) -> Option<Superblock> {
        if b.len() < 512 || b[4..8] != MAGIC {
            return None;
        }
        if rd32(b, 0) != crc32(&b[4..512]) {
            return None;
        }
        if rd32(b, 8) != VERSION || rd32(b, 12) != BLOCK_SIZE as u32 {
            return None;
        }
        let geo = Geometry::compute(rd32(b, 32), rd32(b, 36), rd32(b, 40))?;
        let stored = [
            rd32(b, 44),
            rd32(b, 48),
            rd32(b, 52),
            rd32(b, 56),
            rd32(b, 60),
            rd32(b, 64),
            rd32(b, 68),
        ];
        let want = [
            geo.bbitmap_start,
            geo.bbitmap_blocks,
            geo.ibitmap_start,
            geo.ibitmap_blocks,
            geo.itable_start,
            geo.itable_blocks,
            geo.data_start,
        ];
        if stored != want {
            return None;
        }
        let mut uuid = [0u8; 16];
        uuid.copy_from_slice(&b[16..32]);
        Some(Superblock {
            uuid,
            created: rd64(b, 72),
            geo,
        })
    }
}

/// Decoded journal header.
pub(super) struct JournalHeader {
    pub seq: u64,
    pub targets: Vec<u32>,
}

/// Build the commit block for `payload` (in order) going to `targets`.
pub(super) fn build_journal_header(seq: u64, targets: &[u32], payload: &[&Block]) -> Block {
    debug_assert_eq!(targets.len(), payload.len());
    let mut h = [0u8; BLOCK_SIZE];
    h[4..8].copy_from_slice(&JOURNAL_MAGIC);
    wr64(&mut h, 8, seq);
    wr32(&mut h, 16, targets.len() as u32);
    for (i, &t) in targets.iter().enumerate() {
        wr32(&mut h, 24 + 4 * i, t);
    }
    let mut c = Crc32::new();
    c.update(&h[4..]);
    for p in payload {
        c.update(&p[..]);
    }
    wr32(&mut h, 0, c.finalize());
    h
}

/// Parse a journal header block **without** checking the checksum (which also
/// covers the payload; see [`journal_crc_ok`]).
pub(super) fn parse_journal_header(h: &Block, max_blocks: u32) -> Option<JournalHeader> {
    if h[4..8] != JOURNAL_MAGIC {
        return None;
    }
    let n = rd32(h, 16);
    if n > max_blocks {
        return None;
    }
    let targets = (0..n as usize).map(|i| rd32(h, 24 + 4 * i)).collect();
    Some(JournalHeader {
        seq: rd64(h, 8),
        targets,
    })
}

/// True if the header's checksum matches the header body plus `payload`.
pub(super) fn journal_crc_ok(h: &Block, payload: &[u8]) -> bool {
    let mut c = Crc32::new();
    c.update(&h[4..]);
    c.update(payload);
    c.finalize() == rd32(h, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_for_minimum_disk_is_sane() {
        let total = ((MIN_DISK_SECTORS - FS_START_LBA) / 8) as u32;
        assert_eq!(total, 240);
        let (j, i) = Geometry::defaults(total);
        assert_eq!((j, i), (24, 64));
        let g = Geometry::compute(total, j, i).unwrap();
        assert_eq!(g.bbitmap_start, 26);
        assert_eq!(g.ibitmap_start, 27);
        assert_eq!(g.itable_start, 28);
        assert_eq!(g.itable_blocks, 8);
        assert_eq!(g.data_start, 36);
        assert_eq!(g.backup_sb(), 239);
    }

    #[test]
    fn geometry_rejects_unusable_parameters() {
        assert!(Geometry::compute(240, 0, 64).is_none());
        assert!(Geometry::compute(240, MAX_JOURNAL_BLOCKS + 1, 64).is_none());
        assert!(Geometry::compute(240, 24, 63).is_none());
        assert!(Geometry::compute(240, 24, 0).is_none());
        assert!(Geometry::compute(40, 24, 64).is_none());
        assert!(Geometry::compute(0, 1, 8).is_none());
        assert!(Geometry::compute(1000, 256, u32::MAX - 7).is_none());
    }

    #[test]
    fn geometry_regions_do_not_overlap_and_grow_with_size() {
        for total in [240u32, 1000, 32_704, 32_705, 100_000, 1_000_000] {
            let (j, i) = Geometry::defaults(total);
            let g = Geometry::compute(total, j, i).unwrap();
            assert_eq!(g.bbitmap_blocks, total.div_ceil(BITS_PER_BITMAP_BLOCK));
            assert!(g.jpayload + g.journal_blocks == g.bbitmap_start);
            assert!(g.bbitmap_start + g.bbitmap_blocks == g.ibitmap_start);
            assert!(g.ibitmap_start + g.ibitmap_blocks == g.itable_start);
            assert!(g.itable_start + g.itable_blocks == g.data_start);
            assert!(g.data_start < g.backup_sb());
        }
    }

    fn sb() -> Superblock {
        let g = Geometry::compute(240, 24, 64).unwrap();
        Superblock {
            uuid: [7; 16],
            created: 1_700_000_000,
            geo: g,
        }
    }

    #[test]
    fn superblock_roundtrip() {
        let s = sb();
        let e = s.encode();
        assert_eq!(Superblock::decode(&e), Some(s));
        assert_eq!(&e[4..8], b"OJF3");
    }

    #[test]
    fn superblock_rejects_every_single_bit_flip() {
        let e = sb().encode();
        for bit in 0..512 * 8 {
            let mut c = e;
            c[bit / 8] ^= 1 << (bit % 8);
            assert!(Superblock::decode(&c).is_none(), "bit {bit} accepted");
        }
    }

    #[test]
    fn superblock_rejects_inconsistent_geometry_even_with_valid_crc() {
        let mut e = sb().encode();
        wr32(&mut e, 68, 99); // data_start lies
        let c = crc32(&e[4..]);
        wr32(&mut e, 0, c);
        assert!(Superblock::decode(&e).is_none());
        assert!(Superblock::decode(&e[..100]).is_none());
        assert!(Superblock::decode(&[0u8; 512]).is_none());
    }

    #[test]
    fn journal_header_roundtrip_and_crc_covers_payload() {
        let p1 = [1u8; BLOCK_SIZE];
        let p2 = [2u8; BLOCK_SIZE];
        let h = build_journal_header(9, &[40, 41], &[&p1, &p2]);
        let parsed = parse_journal_header(&h, 24).unwrap();
        assert_eq!(parsed.seq, 9);
        assert_eq!(parsed.targets, [40, 41]);
        let mut payload = Vec::new();
        payload.extend_from_slice(&p1);
        payload.extend_from_slice(&p2);
        assert!(journal_crc_ok(&h, &payload));
        payload[5000] ^= 1;
        assert!(!journal_crc_ok(&h, &payload));
        assert!(parse_journal_header(&h, 1).is_none(), "n > max rejected");
    }

    #[test]
    fn empty_journal_header_is_valid_with_n_zero() {
        let h = build_journal_header(3, &[], &[]);
        let p = parse_journal_header(&h, 24).unwrap();
        assert!(p.targets.is_empty());
        assert!(journal_crc_ok(&h, &[]));
        assert!(parse_journal_header(&[0u8; BLOCK_SIZE], 24).is_none());
    }
}
