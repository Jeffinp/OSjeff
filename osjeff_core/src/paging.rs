//! Pure x86_64 paging arithmetic and thread-stack layout with a guard page.
//!
//! The kernel walks the bootloader's page tables (through the physical-memory
//! mapping) to take one page out of a thread stack's block: a *guard page* below
//! the stack, whose `PRESENT` bit is cleared so a stack overflow faults instead of
//! silently writing into whatever the heap put next to it. The `unsafe` walk lives
//! in `kernel/src/vm.rs`; the decisions it makes (which table index at which
//! level, whether an entry may be descended into, how a block splits into guard
//! and stack, whether a faulting address hit a guard) are here, where they are
//! tested on the host.

/// Size of a (small) page.
pub const PAGE_SIZE: u64 = 4096;
/// Entries per page table.
pub const ENTRIES: usize = 512;

/// Page-table entry bit 0: the entry maps something.
pub const ENTRY_PRESENT: u64 = 1 << 0;
/// Page-table entry bit 7 (levels 2 and 3): the entry maps a 2 MiB / 1 GiB page
/// directly instead of pointing to a lower table.
pub const ENTRY_HUGE: u64 = 1 << 7;
/// Bits 12..51 of an entry: the physical address of the next table / the frame.
pub const ENTRY_ADDR_MASK: u64 = 0x000f_ffff_ffff_f000;

/// Round `addr` down to a page boundary.
pub const fn page_floor(addr: u64) -> u64 {
    addr & !(PAGE_SIZE - 1)
}

/// Round `addr` up to a page boundary (`None` on overflow).
pub const fn page_ceil(addr: u64) -> Option<u64> {
    match addr.checked_add(PAGE_SIZE - 1) {
        Some(v) => Some(v & !(PAGE_SIZE - 1)),
        None => None,
    }
}

/// `true` if `addr` is a canonical 48-bit virtual address (bits 48..63 copy bit 47).
pub const fn is_canonical(addr: u64) -> bool {
    let top = addr >> 47;
    top == 0 || top == 0x1_ffff
}

/// Table indices of `vaddr`, top level first: `[PML4, PDPT, PD, PT]`.
pub const fn table_indices(vaddr: u64) -> [usize; 4] {
    [
        ((vaddr >> 39) & 0x1ff) as usize,
        ((vaddr >> 30) & 0x1ff) as usize,
        ((vaddr >> 21) & 0x1ff) as usize,
        ((vaddr >> 12) & 0x1ff) as usize,
    ]
}

/// What a table entry means to a walk at `level` (4 = PML4 .. 1 = PT).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Not present: nothing is mapped here.
    Absent,
    /// Maps a 1 GiB (level 3) or 2 MiB (level 2) page: there is no 4 KiB entry to edit.
    Huge,
    /// Points to the next table, at this physical address.
    Table(u64),
    /// A level-1 entry: the 4 KiB mapping itself (physical frame address).
    Leaf(u64),
}

/// Interpret `entry` found at `level` (4..=1) of a walk.
pub const fn step(level: u8, entry: u64) -> Step {
    if entry & ENTRY_PRESENT == 0 {
        return Step::Absent;
    }
    let phys = entry & ENTRY_ADDR_MASK;
    match level {
        1 => Step::Leaf(phys),
        // Bit 7 is reserved (must be 0) in a PML4 entry; only levels 3 and 2 can be huge.
        3 | 2 if entry & ENTRY_HUGE != 0 => Step::Huge,
        _ => Step::Table(phys),
    }
}

/// `entry` with the `PRESENT` bit cleared (everything else kept, so the mapping can be restored).
pub const fn without_present(entry: u64) -> u64 {
    entry & !ENTRY_PRESENT
}

/// Bytes of the page-aligned block that holds a stack of `usable` bytes plus its guard page below
/// (`usable` rounded up to whole pages). `None` if `usable` is zero or the sum overflows.
pub const fn guarded_block_size(usable: usize) -> Option<usize> {
    if usable == 0 {
        return None;
    }
    let page = PAGE_SIZE as usize;
    let Some(rounded) = usable.checked_add(page - 1) else {
        return None;
    };
    match (rounded / page).checked_add(1) {
        Some(pages) => pages.checked_mul(page),
        None => None,
    }
}

/// Where the parts of a guarded stack block sit in memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackRegions {
    /// First byte of the guard page (the block's base).
    pub guard_start: u64,
    /// Lowest usable stack address (just above the guard page).
    pub stack_bottom: u64,
    /// One past the highest stack byte (the initial stack pointer is derived from it).
    pub stack_top: u64,
}

/// Split a block that starts at `base` (page aligned) and holds a stack of `usable` bytes plus a
/// guard page. `None` if `base` is not page aligned, `usable` is zero, or the addresses overflow.
pub fn stack_regions(base: u64, usable: usize) -> Option<StackRegions> {
    if !base.is_multiple_of(PAGE_SIZE) {
        return None;
    }
    let size = guarded_block_size(usable)? as u64;
    let stack_bottom = base.checked_add(PAGE_SIZE)?;
    let stack_top = base.checked_add(size)?;
    Some(StackRegions {
        guard_start: base,
        stack_bottom,
        stack_top,
    })
}

/// `true` if `addr` lies in the one-page guard that starts at `guard_start`.
pub const fn guard_hit(addr: u64, guard_start: u64) -> bool {
    addr >= guard_start && addr - guard_start < PAGE_SIZE
}

/// The stack pointer a fresh thread starts with: the top of its stack, 16-aligned, minus one word
/// (so `rsp ≡ 8 (mod 16)`, as if the entry function had just been called).
pub const fn initial_rsp(stack_top: u64) -> u64 {
    (stack_top & !0xF) - 8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_rounding() {
        assert_eq!(page_floor(0), 0);
        assert_eq!(page_floor(4095), 0);
        assert_eq!(page_floor(4096), 4096);
        assert_eq!(page_floor(0x1234_5678), 0x1234_5000);
        assert_eq!(page_ceil(0), Some(0));
        assert_eq!(page_ceil(1), Some(4096));
        assert_eq!(page_ceil(4096), Some(4096));
        assert_eq!(page_ceil(u64::MAX), None);
    }

    #[test]
    fn canonical_addresses() {
        assert!(is_canonical(0));
        assert!(is_canonical(0x0000_7fff_ffff_ffff));
        assert!(!is_canonical(0x0000_8000_0000_0000));
        assert!(!is_canonical(0x8000_0000_0000_0000));
        assert!(is_canonical(0xffff_8000_0000_0000));
        assert!(is_canonical(u64::MAX));
    }

    #[test]
    fn indices_split_the_address() {
        assert_eq!(table_indices(0), [0, 0, 0, 0]);
        assert_eq!(table_indices(0x1000), [0, 0, 0, 1]);
        assert_eq!(table_indices(0x20_0000), [0, 0, 1, 0]);
        assert_eq!(table_indices(0x4000_0000), [0, 1, 0, 0]);
        assert_eq!(table_indices(0x80_0000_0000), [1, 0, 0, 0]);
        // 0x100_0000_0000 (the kernel image in this system) is PML4 index 2.
        assert_eq!(table_indices(0x100_0000_0000), [2, 0, 0, 0]);
        // Highest address: every index is 511.
        assert_eq!(table_indices(u64::MAX), [511, 511, 511, 511]);
        // The page offset does not affect the indices.
        assert_eq!(table_indices(0x1234_5fff), table_indices(0x1234_5000));
    }

    #[test]
    fn indices_round_trip() {
        for v in [
            0x1000u64,
            0x1000_0016_c000,
            0x18_0000_1000,
            0x7fff_ffff_f000,
        ] {
            let [a, b, c, d] = table_indices(v);
            let rebuilt =
                ((a as u64) << 39) | ((b as u64) << 30) | ((c as u64) << 21) | ((d as u64) << 12);
            assert_eq!(rebuilt, v & 0x0000_ffff_ffff_f000);
        }
    }

    #[test]
    fn walk_steps() {
        let frame = 0x1234_5000;
        // Not present at any level.
        for level in 1..=4 {
            assert_eq!(step(level, 0), Step::Absent);
            assert_eq!(step(level, frame | 0x2), Step::Absent); // writable but not present
        }
        // Present, no huge bit: descend (levels 4..2), or the leaf itself (level 1).
        assert_eq!(step(4, frame | 3), Step::Table(frame));
        assert_eq!(step(3, frame | 3), Step::Table(frame));
        assert_eq!(step(2, frame | 3), Step::Table(frame));
        assert_eq!(step(1, frame | 3), Step::Leaf(frame));
        // Huge pages stop the walk at levels 3 and 2 only.
        assert_eq!(step(3, frame | 3 | ENTRY_HUGE), Step::Huge);
        assert_eq!(step(2, frame | 3 | ENTRY_HUGE), Step::Huge);
        // Bit 7 at level 4 is reserved, and at level 1 it is the PAT bit: neither is "huge".
        assert_eq!(step(4, frame | 3 | ENTRY_HUGE), Step::Table(frame));
        assert_eq!(step(1, frame | 3 | ENTRY_HUGE), Step::Leaf(frame));
        // NX (bit 63) and the available bits never leak into the address.
        assert_eq!(
            step(1, frame | 3 | 1 << 63 | 0x7ff0_0000_0000_0000),
            Step::Leaf(frame)
        );
    }

    #[test]
    fn clearing_present_keeps_the_rest() {
        let e = 0x8000_0000_1234_5000 | 0x3 | (1 << 5);
        let c = without_present(e);
        assert_eq!(c & ENTRY_PRESENT, 0);
        assert_eq!(c | ENTRY_PRESENT, e);
        assert_eq!(step(1, c), Step::Absent);
        // Idempotent.
        assert_eq!(without_present(c), c);
    }

    #[test]
    fn block_size_adds_one_guard_page() {
        assert_eq!(guarded_block_size(0), None);
        assert_eq!(guarded_block_size(1), Some(8192));
        assert_eq!(guarded_block_size(4096), Some(8192));
        assert_eq!(guarded_block_size(4097), Some(12288));
        assert_eq!(guarded_block_size(128 * 1024), Some(128 * 1024 + 4096));
        assert_eq!(guarded_block_size(usize::MAX), None);
    }

    #[test]
    fn regions_of_a_stack_block() {
        let base = 0x1000_0116_5000;
        let r = stack_regions(base, 128 * 1024).unwrap();
        assert_eq!(r.guard_start, base);
        assert_eq!(r.stack_bottom, base + 4096);
        assert_eq!(r.stack_top, base + 4096 + 128 * 1024);
        assert_eq!(r.stack_bottom % PAGE_SIZE, 0);
        assert_eq!(r.stack_top % PAGE_SIZE, 0);
        // Unaligned / empty / overflowing blocks are refused.
        assert_eq!(stack_regions(base + 8, 4096), None);
        assert_eq!(stack_regions(base, 0), None);
        assert_eq!(stack_regions(u64::MAX - 4095, 4096), None);
    }

    #[test]
    fn guard_hits() {
        let g = 0x1000_0116_5000u64;
        assert!(guard_hit(g, g));
        assert!(guard_hit(g + 4095, g));
        assert!(!guard_hit(g + 4096, g)); // first stack byte
        assert!(!guard_hit(g - 1, g)); // memory below the block
        assert!(!guard_hit(0, g));
        // A guard at the very end of the address space does not overflow.
        assert!(guard_hit(u64::MAX, u64::MAX - 4095));
    }

    #[test]
    fn initial_rsp_is_8_mod_16() {
        for top in [0x1000u64, 0x1001, 0x100f, 0x1_0000_1000, 0x1000_0118_5000] {
            let rsp = initial_rsp(top);
            assert_eq!(rsp % 16, 8);
            assert!(rsp < top);
            assert!(top - rsp <= 24);
        }
    }

    #[test]
    fn a_stack_walks_down_into_its_guard_exactly() {
        // Probing every page from the top (what rustc's stack probes do for a big frame) must land in
        // the guard page, never skip over it, whatever the starting offset inside the top page.
        let r = stack_regions(0x4000_0000, 128 * 1024).unwrap();
        for start_off in [8u64, 100, 4000, 4095] {
            let mut sp = r.stack_top - start_off;
            while sp >= r.stack_bottom {
                sp -= PAGE_SIZE;
            }
            assert!(guard_hit(sp, r.guard_start), "start offset {start_off}");
        }
    }
}
