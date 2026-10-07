//! Inode and extent encoding.

use super::crc32::crc32;
use super::layout::{INODE_SIZE, rd32, rd64, wr32, wr64};
use alloc::vec::Vec;

/// Extents stored directly in the inode.
pub const INLINE_EXTENTS: usize = 12;
/// Bytes of an encoded extent.
pub const EXTENT_SIZE: usize = 12;
/// Extents per indirect extent block.
pub const EXTENTS_PER_BLOCK: usize = (4096 - 24) / EXTENT_SIZE;
/// Longest name (also the longest stored original trash name).
pub const MAX_NAME: usize = 255;

/// Inode flag: this entry sits directly in `/.trash`.
pub const FLAG_TRASHED: u8 = 1;

const OFF_TRASH_NAME: usize = 224;
const OFF_INLINE: usize = 80;

/// What an inode is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    File,
    Dir,
}

impl Kind {
    pub(super) fn to_u8(self) -> u8 {
        match self {
            Kind::File => 1,
            Kind::Dir => 2,
        }
    }
    pub(super) fn from_u8(v: u8) -> Option<Kind> {
        match v {
            1 => Some(Kind::File),
            2 => Some(Kind::Dir),
            _ => None,
        }
    }
}

/// `len` logical blocks from `lblk` stored at physical blocks from `pblk`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Extent {
    pub lblk: u32,
    pub len: u32,
    pub pblk: u32,
}

impl Extent {
    /// One past the last logical block.
    pub fn lend(&self) -> u64 {
        self.lblk as u64 + self.len as u64
    }
    /// One past the last physical block.
    pub fn pend(&self) -> u64 {
        self.pblk as u64 + self.len as u64
    }
    pub(super) fn encode(&self, b: &mut [u8]) {
        wr32(b, 0, self.lblk);
        wr32(b, 4, self.len);
        wr32(b, 8, self.pblk);
    }
    pub(super) fn decode(b: &[u8]) -> Extent {
        Extent {
            lblk: rd32(b, 0),
            len: rd32(b, 4),
            pblk: rd32(b, 8),
        }
    }
}

/// Decoded inode.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Inode {
    pub kind: Kind,
    pub flags: u8,
    pub mode: u16,
    pub uid: u32,
    pub nlink: u32,
    pub size: u64,
    pub ctime: u64,
    pub mtime: u64,
    pub nblocks: u32,
    pub nextents: u32,
    pub ext_chain: u32,
    pub parent: u32,
    pub nentries: u32,
    pub trash_parent: u32,
    pub trash_time: u64,
    /// Low 32 bits of the original parent's `ctime`: tells a directory that
    /// merely reused the inode number from the one the item was deleted from.
    pub trash_pctime: u32,
    pub trash_name: Vec<u8>,
    pub inline: [Extent; INLINE_EXTENTS],
}

impl Inode {
    /// A fresh inode of `kind` created at `now` inside directory `parent`.
    pub fn new(kind: Kind, now: u64, parent: u32) -> Inode {
        Inode {
            kind,
            flags: 0,
            mode: if kind == Kind::Dir { 0o755 } else { 0o644 },
            uid: 0,
            nlink: 1,
            size: 0,
            ctime: now,
            mtime: now,
            nblocks: 0,
            nextents: 0,
            ext_chain: 0,
            parent,
            nentries: 0,
            trash_parent: 0,
            trash_time: 0,
            trash_pctime: 0,
            trash_name: Vec::new(),
            inline: [Extent::default(); INLINE_EXTENTS],
        }
    }

    /// Serialize into a 512-byte slot, computing the checksum.
    pub fn encode(&self, b: &mut [u8]) {
        let b = &mut b[..INODE_SIZE];
        b.fill(0);
        b[4] = self.kind.to_u8();
        b[5] = self.flags;
        b[6..8].copy_from_slice(&self.mode.to_le_bytes());
        wr32(b, 8, self.uid);
        wr32(b, 12, self.nlink);
        wr64(b, 16, self.size);
        wr64(b, 24, self.ctime);
        wr64(b, 32, self.mtime);
        wr32(b, 40, self.nblocks);
        wr32(b, 44, self.nextents);
        wr32(b, 48, self.ext_chain);
        wr32(b, 52, self.parent);
        wr32(b, 56, self.nentries);
        wr32(b, 60, self.trash_parent);
        wr64(b, 64, self.trash_time);
        let n = self.trash_name.len().min(MAX_NAME);
        b[72] = n as u8;
        wr32(b, 76, self.trash_pctime);
        for (i, e) in self.inline.iter().enumerate() {
            e.encode(&mut b[OFF_INLINE + i * EXTENT_SIZE..]);
        }
        b[OFF_TRASH_NAME..OFF_TRASH_NAME + n].copy_from_slice(&self.trash_name[..n]);
        let c = crc32(&b[4..]);
        wr32(b, 0, c);
    }

    /// Parse a 512-byte slot; `None` if the checksum or any field is invalid.
    pub fn decode(b: &[u8]) -> Option<Inode> {
        if b.len() < INODE_SIZE {
            return None;
        }
        let b = &b[..INODE_SIZE];
        if rd32(b, 0) != crc32(&b[4..]) {
            return None;
        }
        let kind = Kind::from_u8(b[4])?;
        let flags = b[5];
        if flags & !FLAG_TRASHED != 0 {
            return None;
        }
        let tn = b[72] as usize;
        let nextents = rd32(b, 44);
        let mut inline = [Extent::default(); INLINE_EXTENTS];
        for (i, e) in inline.iter_mut().enumerate() {
            *e = Extent::decode(&b[OFF_INLINE + i * EXTENT_SIZE..]);
        }
        Some(Inode {
            kind,
            flags,
            mode: u16::from_le_bytes([b[6], b[7]]),
            uid: rd32(b, 8),
            nlink: rd32(b, 12),
            size: rd64(b, 16),
            ctime: rd64(b, 24),
            mtime: rd64(b, 32),
            nblocks: rd32(b, 40),
            nextents,
            ext_chain: rd32(b, 48),
            parent: rd32(b, 52),
            nentries: rd32(b, 56),
            trash_parent: rd32(b, 60),
            trash_time: rd64(b, 64),
            trash_pctime: rd32(b, 76),
            trash_name: b[OFF_TRASH_NAME..OFF_TRASH_NAME + tn].to_vec(),
            inline,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Inode {
        let mut i = Inode::new(Kind::File, 1234, 1);
        i.size = 99_999;
        i.nblocks = 25;
        i.nextents = 2;
        i.inline[0] = Extent {
            lblk: 0,
            len: 20,
            pblk: 100,
        };
        i.inline[1] = Extent {
            lblk: 20,
            len: 5,
            pblk: 300,
        };
        i.trash_name = b"orig.txt".to_vec();
        i.flags = FLAG_TRASHED;
        i.trash_parent = 7;
        i.trash_time = 55;
        i.trash_pctime = 0xABCD;
        i
    }

    #[test]
    fn inode_roundtrip() {
        let i = sample();
        let mut buf = [0xEEu8; INODE_SIZE];
        i.encode(&mut buf);
        assert_eq!(Inode::decode(&buf), Some(i));
    }

    #[test]
    fn every_bit_flip_in_an_inode_is_detected() {
        let mut buf = [0u8; INODE_SIZE];
        sample().encode(&mut buf);
        for bit in 0..INODE_SIZE * 8 {
            let mut c = buf;
            c[bit / 8] ^= 1 << (bit % 8);
            assert!(Inode::decode(&c).is_none(), "bit {bit}");
        }
    }

    #[test]
    fn invalid_kind_or_flags_rejected_even_with_valid_crc() {
        let mut buf = [0u8; INODE_SIZE];
        sample().encode(&mut buf);
        let mut bad = buf;
        bad[4] = 9;
        let c = crc32(&bad[4..]);
        wr32(&mut bad, 0, c);
        assert!(Inode::decode(&bad).is_none());
        let mut bad = buf;
        bad[5] = 0x80;
        let c = crc32(&bad[4..]);
        wr32(&mut bad, 0, c);
        assert!(Inode::decode(&bad).is_none());
        assert!(Inode::decode(&buf[..100]).is_none());
        assert!(Inode::decode(&[0u8; INODE_SIZE]).is_none());
    }

    #[test]
    fn kind_codes_roundtrip() {
        for k in [Kind::File, Kind::Dir] {
            assert_eq!(Kind::from_u8(k.to_u8()), Some(k));
        }
        assert_eq!(Kind::from_u8(0), None);
    }

    #[test]
    fn extent_arithmetic() {
        let e = Extent {
            lblk: u32::MAX - 1,
            len: 1,
            pblk: u32::MAX,
        };
        assert_eq!(e.lend(), u32::MAX as u64);
        assert_eq!(e.pend(), u32::MAX as u64 + 1);
        assert_eq!(EXTENTS_PER_BLOCK, 339);
    }

    #[test]
    fn longest_trash_name_fits() {
        let mut i = sample();
        i.trash_name = alloc::vec![b'x'; MAX_NAME];
        let mut buf = [0u8; INODE_SIZE];
        i.encode(&mut buf);
        assert_eq!(Inode::decode(&buf).unwrap().trash_name.len(), MAX_NAME);
    }
}
