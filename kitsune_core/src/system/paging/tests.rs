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
