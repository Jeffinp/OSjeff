//! Tests for OJFS v3. Shared helpers live here; the cases are split by theme.

mod basic;

use super::*;
use crate::blockdev::RamDisk;

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
pub(super) fn assert_clean<D: crate::blockdev::BlockDevice>(fs: &mut Fs3<D>) {
    let r = fs.fsck().unwrap();
    assert!(r.is_clean(), "fsck found problems: {:?}", r);
}
