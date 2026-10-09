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
mod tests;
