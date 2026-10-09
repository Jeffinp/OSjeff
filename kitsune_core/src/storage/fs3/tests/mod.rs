//! Tests for OJFS v3. Shared helpers live here; the cases are split by theme.

mod basic;
mod corrupt;
mod crash;
mod data;
mod golden;
mod migrate;
mod model;

use super::*;
use crate::storage::blockdev::RamDisk;
use alloc::vec::Vec;

pub(super) const UUID: [u8; 16] = *b"0123456789abcdef";

/// A formatted filesystem on a RAM disk of `mib` MiB.
pub(super) fn fresh(mib: u64) -> Fs3<RamDisk> {
    Fs3::format(RamDisk::new(mib * 2048), &FormatOptions::new(UUID, 1_000)).unwrap()
}

/// Like [`fresh`] with room for many inodes (for big directory tests).
pub(super) fn fresh_with_inodes(mib: u64, inodes: u32) -> Fs3<RamDisk> {
    let mut o = FormatOptions::new(UUID, 1_000);
    o.inode_count = Some(inodes);
    Fs3::format(RamDisk::new(mib * 2048), &o).unwrap()
}

/// Run `fsck` and assert it is clean, printing the issues otherwise.
pub(super) fn assert_clean<D: crate::storage::blockdev::BlockDevice>(fs: &mut Fs3<D>) {
    let r = fs.fsck().unwrap();
    assert!(r.is_clean(), "fsck found problems: {:?}", r);
}

/// Deterministic xorshift64* PRNG used by the randomized tests.
pub(super) struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}
