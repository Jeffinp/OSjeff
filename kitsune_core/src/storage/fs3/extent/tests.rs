use super::*;

fn e(l: u32, n: u32, p: u32) -> Extent {
    Extent {
        lblk: l,
        len: n,
        pblk: p,
    }
}

#[test]
fn map_block_finds_inside_and_misses_holes() {
    let v = [e(0, 4, 100), e(10, 2, 200)];
    assert_eq!(map_block(&v, 0), Some(100));
    assert_eq!(map_block(&v, 3), Some(103));
    assert_eq!(map_block(&v, 4), None);
    assert_eq!(map_block(&v, 9), None);
    assert_eq!(map_block(&v, 10), Some(200));
    assert_eq!(map_block(&v, 11), Some(201));
    assert_eq!(map_block(&v, 12), None);
    assert_eq!(map_block(&[], 0), None);
}

#[test]
fn coalesce_merges_only_doubly_adjacent() {
    let mut v = alloc::vec![e(0, 2, 10), e(2, 2, 12), e(4, 1, 99), e(5, 1, 100)];
    coalesce(&mut v);
    assert_eq!(v, [e(0, 4, 10), e(4, 2, 99)]);
    let mut w = alloc::vec![e(0, 2, 10), e(2, 2, 20)];
    coalesce(&mut w);
    assert_eq!(w.len(), 2);
}

#[test]
fn remap_splits_and_reports_freed_ranges() {
    let mut v = alloc::vec![e(0, 10, 100)];
    let mut freed = Vec::new();
    remap(&mut v, 3, 4, &[(500, 4)], &mut freed);
    assert_eq!(v, [e(0, 3, 100), e(3, 4, 500), e(7, 3, 107)]);
    assert_eq!(freed, [(103, 4)]);
}

#[test]
fn remap_over_a_hole_frees_nothing_and_inserts() {
    let mut v = alloc::vec![e(0, 2, 10), e(8, 2, 30)];
    let mut freed = Vec::new();
    remap(&mut v, 4, 2, &[(50, 2)], &mut freed);
    assert!(freed.is_empty());
    assert_eq!(v, [e(0, 2, 10), e(4, 2, 50), e(8, 2, 30)]);
}

#[test]
fn remap_spanning_several_extents() {
    let mut v = alloc::vec![e(0, 3, 10), e(3, 3, 20), e(6, 3, 30)];
    let mut freed = Vec::new();
    remap(&mut v, 2, 5, &[(100, 2), (200, 3)], &mut freed);
    assert_eq!(freed, [(12, 1), (20, 3), (30, 1)]);
    assert_eq!(v, [e(0, 2, 10), e(2, 2, 100), e(4, 3, 200), e(7, 2, 31)]);
}

#[test]
fn remap_merges_with_neighbours_when_contiguous() {
    let mut v = alloc::vec![e(0, 2, 10)];
    let mut freed = Vec::new();
    remap(&mut v, 2, 2, &[(12, 2)], &mut freed);
    assert_eq!(v, [e(0, 4, 10)]);
}

#[test]
fn cut_tail_keeps_prefix() {
    let mut v = alloc::vec![e(0, 4, 10), e(6, 4, 30)];
    let mut freed = Vec::new();
    cut_tail(&mut v, 8, &mut freed);
    assert_eq!(v, [e(0, 4, 10), e(6, 2, 30)]);
    assert_eq!(freed, [(32, 2)]);
    let mut freed = Vec::new();
    cut_tail(&mut v, 0, &mut freed);
    assert!(v.is_empty());
    assert_eq!(freed, [(10, 4), (30, 2)]);
}
