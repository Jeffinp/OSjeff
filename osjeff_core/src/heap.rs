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
