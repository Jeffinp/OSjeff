//! Pure arithmetic for a linked-list heap allocator. The error-prone part —
//! alignment and region-fit math — lives here and is unit-tested. The kernel
//! provides the thin `unsafe` glue that writes free-list nodes into memory.

/// Round `addr` up to a multiple of `align` (which must be a power of two).
pub const fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

/// True when `align` is a non-zero power of two.
pub const fn is_power_of_two(align: usize) -> bool {
    align != 0 && (align & (align - 1)) == 0
}

/// Try to place an allocation of `size` bytes aligned to `align` inside the
/// free region `[region_start, region_start + region_size)`.
///
/// Returns `(alloc_start, excess)` where `excess` is the leftover tail after
/// the allocation. The fit is rejected when it doesn't physically fit, or when
/// the leftover is non-zero but too small to hold a free-list node
/// (`min_block`) — that would strand unrecoverable memory.
pub fn fit_region(
    region_start: usize,
    region_size: usize,
    size: usize,
    align: usize,
    min_block: usize,
) -> Option<(usize, usize)> {
    let alloc_start = align_up(region_start, align);
    let alloc_end = alloc_start.checked_add(size)?;
    let region_end = region_start.checked_add(region_size)?;
    if alloc_end > region_end {
        return None;
    }
    let excess = region_end - alloc_end;
    if excess > 0 && excess < min_block {
        return None;
    }
    Some((alloc_start, excess))
}

/// A placement inside a free region that accounts for *every* byte of it:
/// `front + size + excess == region_size`. Both `front` and `excess` are either
/// zero or at least `min_block`, so each leftover can go back on the free list
/// as a node instead of being stranded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fit {
    /// Bytes between the region start and the (aligned) allocation.
    pub front: usize,
    pub alloc_start: usize,
    /// Bytes between the end of the allocation and the region end.
    pub excess: usize,
}

/// Like [`fit_region`], but also reports the padding *in front of* the
/// allocation so the caller can return it to the free list, instead of leaking
/// it (`fit_region` only reports the tail).
///
/// When aligning would leave a front gap too small to hold a free-list node
/// (`0 < front < min_block`), the allocation is moved to the next aligned
/// address, which makes the gap large enough. Returns `None` if nothing fits
/// (including when any address computation would overflow).
#[inline]
pub fn fit_region_split(
    region_start: usize,
    region_size: usize,
    size: usize,
    align: usize,
    min_block: usize,
) -> Option<Fit> {
    // Cheap early-out for the common case while scanning a long free list.
    if region_size < size || !is_power_of_two(align) {
        return None;
    }
    let region_end = region_start.checked_add(region_size)?;
    let mut alloc_start = region_start.checked_add(align - 1)? & !(align - 1);
    if alloc_start != region_start && alloc_start - region_start < min_block {
        alloc_start = alloc_start.checked_add(align)?;
    }
    let alloc_end = alloc_start.checked_add(size)?;
    if alloc_end > region_end {
        return None;
    }
    let excess = region_end - alloc_end;
    if excess > 0 && excess < min_block {
        return None;
    }
    Some(Fit {
        front: alloc_start - region_start,
        alloc_start,
        excess,
    })
}

/// True when the free region `[a_start, a_start + a_size)` ends exactly where
/// `b_start` begins — i.e. the two are contiguous and can be merged into one.
/// Overflow-safe (a wrapping end can never legitimately touch `b_start`).
pub const fn regions_adjacent(a_start: usize, a_size: usize, b_start: usize) -> bool {
    match a_start.checked_add(a_size) {
        Some(end) => end == b_start,
        None => false,
    }
}

/// Normalize a requested `(size, align)` so every allocation is at least large
/// enough — and aligned enough — to host a free-list node once freed.
pub fn adjust_request(
    size: usize,
    align: usize,
    node_size: usize,
    node_align: usize,
) -> (usize, usize) {
    let align = align.max(node_align);
    let size = align_up(size.max(node_size), node_align);
    (size, align)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn align_up_basic() {
        assert_eq!(align_up(0, 8), 0);
        assert_eq!(align_up(1, 8), 8);
        assert_eq!(align_up(8, 8), 8);
        assert_eq!(align_up(9, 8), 16);
        assert_eq!(align_up(100, 64), 128);
    }

    #[test]
    fn power_of_two_check() {
        assert!(is_power_of_two(1));
        assert!(is_power_of_two(8));
        assert!(is_power_of_two(4096));
        assert!(!is_power_of_two(0));
        assert!(!is_power_of_two(3));
        assert!(!is_power_of_two(48));
    }

    #[test]
    fn fit_exact() {
        // Region [0,100), request 100 aligned 1 -> fits, no excess.
        assert_eq!(fit_region(0, 100, 100, 1, 16), Some((0, 0)));
    }

    #[test]
    fn fit_with_recoverable_excess() {
        // 100 - 40 = 60 leftover >= min_block(16) -> ok.
        assert_eq!(fit_region(0, 100, 40, 1, 16), Some((0, 60)));
    }

    #[test]
    fn reject_excess_too_small_to_hold_node() {
        // leftover 10 < min_block 16 -> stranded -> reject.
        assert_eq!(fit_region(0, 50, 40, 1, 16), None);
    }

    #[test]
    fn fit_respects_alignment_padding() {
        // Region starts at 5, align 8 -> alloc_start 8, needs 8+16=24 <= 5+40=45.
        let (start, excess) = fit_region(5, 40, 16, 8, 8).unwrap();
        assert_eq!(start, 8);
        assert_eq!(start % 8, 0);
        assert_eq!(excess, 45 - 24);
    }

    #[test]
    fn reject_when_too_big() {
        assert_eq!(fit_region(0, 32, 64, 1, 16), None);
    }

    #[test]
    fn reject_when_alignment_pushes_past_end() {
        // Region [10, 16): aligning to 64 -> 64, way past end.
        assert_eq!(fit_region(10, 6, 1, 64, 16), None);
    }

    #[test]
    fn fit_overflow_is_safe() {
        assert_eq!(fit_region(usize::MAX - 4, 8, 16, 1, 16), None);
    }

    #[test]
    fn adjacent_regions_merge() {
        // [0,64) ends at 64, where [64,..) begins -> adjacent.
        assert!(regions_adjacent(0, 64, 64));
        assert!(regions_adjacent(4096, 16, 4112));
    }

    #[test]
    fn non_adjacent_regions_dont_merge() {
        // Gap between end (63) and next start (64).
        assert!(!regions_adjacent(0, 63, 64));
        // Overlap / out of order.
        assert!(!regions_adjacent(0, 100, 64));
        assert!(!regions_adjacent(128, 16, 64));
    }

    #[test]
    fn adjacent_overflow_is_safe() {
        assert!(!regions_adjacent(usize::MAX - 4, 8, 0));
    }

    // ---- fit_region_split ----

    const MIN: usize = 16;

    #[test]
    fn split_fit_aligned_region_has_no_front() {
        let f = fit_region_split(4096, 8192, 100, 8, MIN).unwrap();
        assert_eq!(f.front, 0);
        assert_eq!(f.alloc_start, 4096);
        assert_eq!(f.front + 100 + f.excess, 8192);
    }

    #[test]
    fn split_fit_reports_large_front_padding() {
        // The audit's case: region at 8 mod 4096, request aligned to 4096.
        let f = fit_region_split(4104, 8192, 16, 4096, MIN).unwrap();
        assert_eq!(f.alloc_start, 8192);
        assert_eq!(f.front, 4088);
        assert_eq!(f.front + 16 + f.excess, 8192); // nothing lost
        // The old API hid `front`:
        assert_eq!(
            fit_region(4104, 8192, 16, 4096, MIN).map(|(s, _)| s),
            Some(8192)
        );
    }

    #[test]
    fn split_fit_skips_to_next_slot_when_front_cannot_hold_a_node() {
        // Region at 8: aligning to 16 leaves an 8-byte gap (< 16, no room for a
        // node), so the allocation moves to 24 and the gap becomes 16.
        let f = fit_region_split(8, 200, 32, 16, MIN).unwrap();
        assert_eq!(f.alloc_start, 32);
        assert_eq!(f.front, 24);
        assert_eq!(f.alloc_start % 16, 0);
        assert_eq!(f.front + 32 + f.excess, 200);
    }

    #[test]
    fn split_fit_rejects_when_the_shifted_slot_does_not_fit() {
        assert_eq!(fit_region_split(8, 40, 32, 16, MIN), None);
        // Fits only without the shift: [8, 8+32) needs alignment 8.
        assert!(fit_region_split(8, 32, 32, 8, MIN).is_some());
    }

    #[test]
    fn split_fit_rejects_stranded_tail_like_fit_region() {
        assert_eq!(fit_region_split(0, 50, 40, 1, MIN), None);
        assert_eq!(
            fit_region_split(0, 56, 40, 1, MIN).map(|f| f.excess),
            Some(16)
        );
        assert_eq!(
            fit_region_split(0, 40, 40, 1, MIN).map(|f| f.excess),
            Some(0)
        );
    }

    #[test]
    fn split_fit_rejects_bad_alignment_and_overflow() {
        assert_eq!(fit_region_split(0, 100, 8, 0, MIN), None);
        assert_eq!(fit_region_split(0, 100, 8, 24, MIN), None); // not a power of two
        assert_eq!(fit_region_split(usize::MAX - 4, 8, 16, 1, MIN), None);
        assert_eq!(fit_region_split(usize::MAX - 8, 8, 1, 4096, MIN), None); // align_up would wrap
        assert_eq!(fit_region_split(0, 100, usize::MAX, 8, MIN), None);
    }

    /// Address-sorted free list model mirroring the kernel's allocator: find the
    /// first region that fits, unlink it whole, give back front and tail, and
    /// coalesce neighbours.
    struct Model {
        free: Vec<(usize, usize)>, // (start, size), sorted by start
        use_split: bool,
    }

    impl Model {
        fn insert(&mut self, start: usize, size: usize) {
            let i = self.free.partition_point(|&(s, _)| s < start);
            self.free.insert(i, (start, size));
            // Merge with the successor, then the predecessor.
            if i + 1 < self.free.len() && regions_adjacent(start, size, self.free[i + 1].0) {
                self.free[i].1 += self.free[i + 1].1;
                self.free.remove(i + 1);
            }
            if i > 0 && regions_adjacent(self.free[i - 1].0, self.free[i - 1].1, self.free[i].0) {
                self.free[i - 1].1 += self.free[i].1;
                self.free.remove(i);
            }
        }

        fn alloc(&mut self, size: usize, align: usize) -> Option<(usize, usize)> {
            let (size, align) = adjust_request(size, align, MIN, 8);
            for i in 0..self.free.len() {
                let (rs, rsz) = self.free[i];
                let (alloc_start, front) = if self.use_split {
                    match fit_region_split(rs, rsz, size, align, MIN) {
                        Some(f) => (f.alloc_start, f.front),
                        None => continue,
                    }
                } else {
                    match fit_region(rs, rsz, size, align, MIN) {
                        Some((a, _)) => (a, 0), // old kernel: front gap dropped
                        None => continue,
                    }
                };
                self.free.remove(i);
                let end = alloc_start + size;
                if front > 0 {
                    self.insert(rs, front);
                }
                if rs + rsz > end {
                    self.insert(end, rs + rsz - end);
                }
                return Some((alloc_start, size));
            }
            None
        }

        fn free_bytes(&self) -> usize {
            self.free.iter().map(|&(_, s)| s).sum()
        }
    }

    fn run_fuzz(use_split: bool) -> (usize, usize) {
        const BASE: usize = 0x1000_0008; // deliberately not page aligned
        const SIZE: usize = 1 << 20;
        let mut m = Model {
            free: vec![(BASE, SIZE)],
            use_split,
        };
        let mut rng = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        let mut live: Vec<(usize, usize, usize)> = Vec::new(); // (start, size, align)
        for _ in 0..20_000 {
            if live.len() < 40 && next() % 3 != 0 {
                let size = 1 + (next() % 3000) as usize;
                let align = [8usize, 16, 64, 4096][(next() % 4) as usize];
                if let Some((start, sz)) = m.alloc(size, align) {
                    assert_eq!(start % align.max(8), 0, "misaligned allocation");
                    for &(s, z, _) in &live {
                        assert!(start + sz <= s || s + z <= start, "overlapping allocations");
                    }
                    live.push((start, sz, align));
                }
            } else if !live.is_empty() {
                let i = (next() as usize) % live.len();
                let (s, z, _) = live.swap_remove(i);
                m.insert(s, z);
            }
        }
        for (s, z, _) in live.drain(..) {
            m.insert(s, z);
        }
        (m.free_bytes(), m.free.len())
    }

    #[test]
    fn model_with_split_fit_never_leaks_front_padding() {
        let (bytes, nodes) = run_fuzz(true);
        assert_eq!(bytes, 1 << 20, "bytes leaked");
        assert_eq!(nodes, 1, "heap did not coalesce back to one region");
    }

    #[test]
    fn model_with_old_fit_region_leaks_as_documented_in_the_audit() {
        // Documents the bug the split fit fixes (docs/audit/01-memoria-unsafe.md #5).
        let (bytes, nodes) = run_fuzz(false);
        assert!(bytes < 1 << 20, "expected a leak, free = {bytes}");
        assert!(nodes > 1);
    }

    #[test]
    fn adjust_request_enforces_node_minimums() {
        // Tiny request grows to at least node_size and node_align.
        let (size, align) = adjust_request(1, 1, 16, 8);
        assert_eq!(size, 16);
        assert_eq!(align, 8);
    }

    #[test]
    fn adjust_request_keeps_larger_values() {
        let (size, align) = adjust_request(100, 64, 16, 8);
        assert_eq!(size, align_up(100, 8));
        assert_eq!(align, 64);
    }
}
