//! bitmap (split out of `mod.rs`).

use super::*;

pub(super) fn bitmap_image(words: &[u64], idx: u32) -> Block {
    let mut b = [0u8; BLOCK_SIZE];
    let first = idx as usize * BITMAP_WORDS_PER_BLOCK as usize;
    for k in 0..BITMAP_WORDS_PER_BLOCK as usize {
        let w = words.get(first + k).copied().unwrap_or(!0u64);
        b[8 + 8 * k..16 + 8 * k].copy_from_slice(&w.to_le_bytes());
    }
    let c = crc32(&b[4..]);
    wr32(&mut b, 0, c);
    b
}

/// Check a bitmap block and copy its words into `words`.
pub(super) fn load_bitmap_block(b: &[u8], idx: u32, words: &mut [u64]) -> Result<(), FsError> {
    if rd32(b, 0) != crc32(&b[4..BLOCK_SIZE]) {
        return Err(FsError::Corrupt("bitmap checksum"));
    }
    let first = idx as usize * BITMAP_WORDS_PER_BLOCK as usize;
    for k in 0..BITMAP_WORDS_PER_BLOCK as usize {
        if let Some(w) = words.get_mut(first + k) {
            let mut a = [0u8; 8];
            a.copy_from_slice(&b[8 + 8 * k..16 + 8 * k]);
            *w = u64::from_le_bytes(a);
        }
    }
    Ok(())
}

pub(super) fn verify_typed(b: &Block, ext: bool, owner: u32) -> Result<(), FsError> {
    if rd32(b, 0) != crc32(&b[4..]) {
        return Err(FsError::Corrupt("block checksum"));
    }
    if ext {
        if b[4..8] != EXT_MAGIC || rd32(b, 16) != owner {
            return Err(FsError::Corrupt("extent block header"));
        }
    } else if b[4..8] != DIR_MAGIC || rd32(b, 8) != owner || rd16(b, 12) as usize > BLOCK_SIZE - 16
    {
        return Err(FsError::Corrupt("directory block header"));
    }
    Ok(())
}
