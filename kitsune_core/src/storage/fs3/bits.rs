//! Bit-vector helpers for the allocation bitmaps (`1` = used).
//!
//! A bitmap of `n` bits is stored in `ceil(n/64)` words; the padding bits of
//! the last word are kept at `1` so they are never allocated.

use alloc::vec::Vec;

/// A fresh bitmap of `nbits` bits, all free (padding bits set).
pub fn new(nbits: u32) -> Vec<u64> {
    let words = (nbits as usize).div_ceil(64);
    let mut v = alloc::vec![0u64; words];
    fix_padding(&mut v, nbits);
    v
}

/// Force the padding bits (index >= `nbits`) to 1.
pub fn fix_padding(v: &mut [u64], nbits: u32) {
    let rem = nbits % 64;
    if rem != 0
        && let Some(last) = v.last_mut()
    {
        *last |= !0u64 << rem;
    }
}

pub fn get(v: &[u64], i: u32) -> bool {
    v.get((i / 64) as usize)
        .is_some_and(|w| w & (1u64 << (i % 64)) != 0)
}

pub fn set(v: &mut [u64], i: u32) {
    if let Some(w) = v.get_mut((i / 64) as usize) {
        *w |= 1u64 << (i % 64);
    }
}

pub fn clear(v: &mut [u64], i: u32) {
    if let Some(w) = v.get_mut((i / 64) as usize) {
        *w &= !(1u64 << (i % 64));
    }
}

/// Number of zero bits among the first `nbits`.
pub fn count_free(v: &[u64], nbits: u32) -> u32 {
    let mut used = 0u32;
    for (wi, w) in v.iter().enumerate() {
        let base = wi as u64 * 64;
        let valid = (nbits as u64).saturating_sub(base).min(64);
        let mask = if valid == 64 {
            !0u64
        } else {
            (1u64 << valid) - 1
        };
        used += (w & mask).count_ones();
    }
    nbits - used
}

/// First index in `[from, to)` whose bit equals `want` (`true` = set).
pub fn next_with(v: &[u64], from: u32, to: u32, want: bool) -> Option<u32> {
    let mut i = from;
    while i < to {
        let wi = (i / 64) as usize;
        let w = *v.get(wi)?;
        let w = if want { w } else { !w };
        // Mask off bits below `i` within this word.
        let w = w & (!0u64 << (i % 64));
        if w != 0 {
            let idx = (wi as u32) * 64 + w.trailing_zeros();
            return if idx < to { Some(idx) } else { None };
        }
        i = (wi as u32 + 1).checked_mul(64)?;
    }
    None
}

/// First free (0) bit in `[from, to)`.
pub fn next_free(v: &[u64], from: u32, to: u32) -> Option<u32> {
    next_with(v, from, to, false)
}

/// First used (1) bit in `[from, to)`.
pub fn next_used(v: &[u64], from: u32, to: u32) -> Option<u32> {
    next_with(v, from, to, true)
}

/// Start of the first run of at least `want` free bits inside `[from, to)`.
pub fn find_run(v: &[u64], from: u32, to: u32, want: u32) -> Option<u32> {
    let mut p = from;
    loop {
        let s = next_free(v, p, to)?;
        let e = next_used(v, s, to).unwrap_or(to);
        if e - s >= want {
            return Some(s);
        }
        p = e;
    }
}

#[cfg(test)]
mod tests;
