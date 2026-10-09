use super::*;

#[test]
fn plain_click_selects_one() {
    let mut s = sel(5);
    s.click(2, false, false);
    assert_eq!(s.selected(), vec![2]);
    s.click(4, false, false);
    assert_eq!(s.selected(), vec![4]);
    assert_eq!(s.count(), 1);
    assert_eq!(s.cursor(), 4);
}

#[test]
fn ctrl_click_toggles() {
    let mut s = sel(5);
    s.click(1, true, false);
    s.click(3, true, false);
    assert_eq!(s.selected(), vec![1, 3]);
    s.click(1, true, false);
    assert_eq!(s.selected(), vec![3]);
    assert_eq!(s.count(), 1);
}

#[test]
fn shift_click_selects_a_range_from_the_anchor() {
    let mut s = sel(10);
    s.click(2, false, false);
    s.click(5, false, true);
    assert_eq!(s.selected(), vec![2, 3, 4, 5]);
    s.click(0, false, true);
    assert_eq!(s.selected(), vec![0, 1, 2]);
}

#[test]
fn ctrl_shift_click_adds_the_range() {
    let mut s = sel(10);
    s.click(1, false, false);
    s.click(5, true, false);
    s.click(8, true, true);
    assert_eq!(s.selected(), vec![1, 5, 6, 7, 8]);
}

#[test]
fn select_all_and_clear() {
    let mut s = sel(4);
    s.select_all();
    assert_eq!(s.count(), 4);
    assert_eq!(s.selected(), vec![0, 1, 2, 3]);
    s.clear();
    assert_eq!(s.count(), 0);
}

#[test]
fn arrow_keys_move_and_shift_extends() {
    let mut s = sel(6);
    s.move_cursor(1, false);
    assert_eq!(s.selected(), vec![1]);
    s.move_cursor(2, true);
    assert_eq!(s.selected(), vec![1, 2, 3]);
    s.move_cursor(-1, true);
    assert_eq!(s.selected(), vec![1, 2]);
    s.move_cursor(100, false);
    assert_eq!(s.selected(), vec![5]);
    s.move_cursor(-100, false);
    assert_eq!(s.selected(), vec![0]);
}

#[test]
fn selection_ignores_out_of_range_and_empty() {
    let mut s = sel(3);
    s.click(7, false, false);
    assert_eq!(s.count(), 0);
    let mut e = sel(0);
    e.move_cursor(1, false);
    e.only(0);
    e.select_all();
    assert_eq!(e.count(), 0);
    assert!(e.is_empty());
    assert!(!e.is_selected(0));
}

#[test]
fn selection_count_matches_the_mask_under_random_clicks() {
    let mut s = sel(50);
    let mut x = 12345u32;
    for _ in 0..500 {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
        let i = (x >> 8) as usize % 50;
        s.click(i, x & 1 != 0, x & 2 != 0);
        assert_eq!(s.count(), s.selected().len());
    }
}
