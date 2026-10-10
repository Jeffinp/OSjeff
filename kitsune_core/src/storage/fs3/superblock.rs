//! superblock (split out of `mod.rs`).

use super::*;

pub(super) enum SbState {
    Valid(Superblock),
    /// The `OJF3` magic is there but the checksum/geometry is not.
    Damaged,
    Absent,
}

/// Total v3 blocks a device of `sectors` sectors can hold.
pub(super) fn blocks_for(sectors: u64) -> u32 {
    let b = sectors.saturating_sub(FS_START_LBA) / SECTORS_PER_BLOCK;
    b.min(u32::MAX as u64) as u32
}

pub(super) fn read_sb_state<D: BlockDevice>(dev: &mut D) -> Result<SbState, IoError> {
    let sectors = dev.sector_count();
    let tb = blocks_for(sectors);
    if tb == 0 {
        return Ok(SbState::Absent);
    }
    let mut damaged = false;
    let mut s = [0u8; SECTOR_SIZE];
    dev.read_sectors(FS_START_LBA, &mut s)?;
    match Superblock::decode(&s) {
        Some(sb) if sb.geo.total_blocks <= tb => return Ok(SbState::Valid(sb)),
        Some(_) => damaged = true,
        None => damaged |= s[4..8] == layout::MAGIC,
    }
    let backup = FS_START_LBA + (tb as u64 - 1) * SECTORS_PER_BLOCK;
    dev.read_sectors(backup, &mut s)?;
    match Superblock::decode(&s) {
        Some(sb) if sb.geo.total_blocks == tb => return Ok(SbState::Valid(sb)),
        Some(_) => damaged = true,
        None => damaged |= s[4..8] == layout::MAGIC,
    }
    Ok(if damaged {
        SbState::Damaged
    } else {
        SbState::Absent
    })
}

/// Classify what is on `dev` (reads at most the first 136 sectors).
pub fn detect<D: BlockDevice>(dev: &mut D) -> Result<Detected, IoError> {
    match read_sb_state(dev)? {
        SbState::Valid(_) => return Ok(Detected::V3),
        SbState::Damaged => return Ok(Detected::Unknown),
        SbState::Absent => {}
    }
    let n = dev.sector_count().min(FS_START_LBA + SECTORS_PER_BLOCK) as usize;
    if n == 0 {
        return Ok(Detected::Blank);
    }
    let mut buf = alloc::vec![0u8; n * SECTOR_SIZE];
    dev.read_sectors(0, &mut buf)?;
    if buf[..4] == V2_MAGIC {
        return Ok(Detected::V2);
    }
    if buf.iter().all(|&b| b == 0) {
        Ok(Detected::Blank)
    } else {
        Ok(Detected::Unknown)
    }
}
