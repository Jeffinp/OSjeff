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
mod tests {
    use super::*;

    #[test]
    fn padding_bits_are_never_free() {
        let v = new(70);
        assert_eq!(v.len(), 2);
        assert_eq!(count_free(&v, 70), 70);
        assert_eq!(next_free(&v, 70, 128), None);
        assert!(get(&v, 70) && get(&v, 127));
        assert!(!get(&v, 69));
    }

    #[test]
    fn set_clear_get_count() {
        let mut v = new(200);
        for i in [0u32, 1, 63, 64, 65, 127, 199] {
            set(&mut v, i);
            assert!(get(&v, i));
        }
        assert_eq!(count_free(&v, 200), 193);
        clear(&mut v, 64);
        assert!(!get(&v, 64));
        assert_eq!(count_free(&v, 200), 194);
        assert!(!get(&v, 1000)); // out of range reads as 0, never panics
        set(&mut v, 100_000); // out of range writes are ignored
    }

    #[test]
    fn next_free_and_used_cross_word_boundaries() {
        let mut v = new(300);
        for i in 0..150 {
            set(&mut v, i);
        }
        assert_eq!(next_free(&v, 0, 300), Some(150));
        assert_eq!(next_free(&v, 151, 300), Some(151));
        assert_eq!(next_used(&v, 150, 300), None);
        assert_eq!(next_used(&v, 0, 300), Some(0));
        assert_eq!(next_used(&v, 100, 120), Some(100));
        assert_eq!(next_free(&v, 0, 150), None);
        assert_eq!(next_free(&v, 299, 300), Some(299));
        assert_eq!(next_free(&v, 300, 300), None);
    }

    #[test]
    fn find_run_finds_first_big_enough_gap() {
        let mut v = new(256);
        // used: 0..10, 12..20, 25..256 -> gaps of 2 (10..12), 5 (20..25)
        for i in (0..10).chain(12..20).chain(25..256) {
            set(&mut v, i);
        }
        assert_eq!(find_run(&v, 0, 256, 1), Some(10));
        assert_eq!(find_run(&v, 0, 256, 2), Some(10));
        assert_eq!(find_run(&v, 0, 256, 3), Some(20));
        assert_eq!(find_run(&v, 0, 256, 5), Some(20));
        assert_eq!(find_run(&v, 0, 256, 6), None);
        assert_eq!(find_run(&v, 11, 256, 2), Some(20));
    }

    #[test]
    fn find_run_respects_the_upper_bound() {
        let v = new(256);
        assert_eq!(find_run(&v, 0, 100, 100), Some(0));
        assert_eq!(find_run(&v, 0, 100, 101), None);
        assert_eq!(find_run(&v, 90, 100, 11), None);
    }

    #[test]
    fn zero_bits_vector_is_harmless() {
        let v = new(0);
        assert!(v.is_empty());
        assert_eq!(count_free(&v, 0), 0);
        assert_eq!(next_free(&v, 0, 0), None);
    }
}
