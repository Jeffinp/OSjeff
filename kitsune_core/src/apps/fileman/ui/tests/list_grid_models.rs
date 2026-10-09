use super::*;

#[test]
fn list_geometry_is_consistent() {
    let (vw, vh, n) = (600, 300, 100);
    assert_eq!(columns_of(ViewMode::List, vw), 1);
    assert_eq!(content_height(ViewMode::List, vw, 0), 0);
    assert_eq!(
        content_height(ViewMode::List, vw, n),
        2 * LIST_PAD + 100 * ROW_H
    );
    assert_eq!(
        max_scroll(ViewMode::List, vw, vh, n),
        2 * LIST_PAD + 100 * ROW_H - vh
    );
    assert_eq!(max_scroll(ViewMode::List, vw, vh, 3), 0);
    for i in [0usize, 1, 17, 99] {
        let r = item_rect(ViewMode::List, vw, i);
        assert_eq!(item_at(ViewMode::List, vw, 0, r.x + 5, r.y + 5, n), Some(i));
        if r.y + 5 >= 200 {
            assert_eq!(
                item_at(ViewMode::List, vw, 200, r.x + 5, r.y + 5 - 200, n),
                Some(i)
            );
        }
    }
    assert_eq!(item_at(ViewMode::List, vw, 0, 3, 40, n), None); // left margin
    assert_eq!(item_at(ViewMode::List, vw, 0, 100, 1, n), None); // top padding
    assert_eq!(
        item_at(ViewMode::List, vw, 0, 100, LIST_PAD + 5 * ROW_H, 5),
        None
    );
    assert_eq!(item_at(ViewMode::List, vw, 0, -1, 40, n), None);
}

#[test]
fn visible_range_covers_exactly_what_shows() {
    let (vw, vh) = (600, 280);
    assert_eq!(visible_range(ViewMode::List, vw, vh, 0, 0), (0, 0));
    assert_eq!(visible_range(ViewMode::List, vw, vh, 0, 3), (0, 3));
    let (a, b) = visible_range(ViewMode::List, vw, vh, 0, 2000);
    assert_eq!(a, 0);
    assert!(b >= (vh / ROW_H) as usize && b <= (vh / ROW_H) as usize + 2);
    for scroll in [0, 1, 27, 28, 29, 500, 10_000, 55_000] {
        let (a, b) = visible_range(ViewMode::List, vw, vh, scroll, 2000);
        assert!(a <= b && b <= 2000);
        for i in 0..2000usize {
            let r = item_rect(ViewMode::List, vw, i);
            let shows = r.y < scroll + vh && r.bottom() > scroll;
            assert_eq!(shows, i >= a && i < b, "scroll {scroll} item {i}");
        }
    }
    // The grid too.
    for scroll in [0, 50, 96, 333, 4000] {
        let (a, b) = visible_range(ViewMode::Icons, vw, vh, scroll, 500);
        for i in 0..500usize {
            let r = item_rect(ViewMode::Icons, vw, i);
            let shows = r.y < scroll + vh && r.bottom() > scroll;
            if shows {
                assert!(
                    i >= a && i < b,
                    "grid scroll {scroll} item {i} not in {a}..{b}"
                );
            }
        }
        assert!(b - a <= (vh / CELL_H + 2) as usize * columns_of(ViewMode::Icons, vw));
    }
}

#[test]
fn grid_geometry_round_trips() {
    let (vw, n) = (640, 57);
    let cols = columns_of(ViewMode::Icons, vw);
    assert_eq!(cols, ((640 - 2 * GRID_PAD) / CELL_W) as usize);
    assert!(cols >= 5);
    assert_eq!(
        content_height(ViewMode::Icons, vw, n),
        2 * GRID_PAD + n.div_ceil(cols) as i32 * CELL_H
    );
    for i in 0..n {
        let r = item_rect(ViewMode::Icons, vw, i);
        assert!(r.x >= 0 && r.right() <= vw, "{r:?}");
        assert_eq!(
            item_at(ViewMode::Icons, vw, 0, r.x + 3, r.y + 3, n),
            Some(i)
        );
        if r.y + r.h - 2 >= 120 {
            assert_eq!(
                item_at(
                    ViewMode::Icons,
                    vw,
                    120,
                    r.x + r.w - 2,
                    r.y + r.h - 2 - 120,
                    n
                ),
                Some(i)
            );
        }
    }
    // Cells in a row do not overlap.
    let a = item_rect(ViewMode::Icons, vw, 0);
    let b = item_rect(ViewMode::Icons, vw, 1);
    assert!(a.right() <= b.x);
    // A window narrower than one cell still lays out one column.
    assert_eq!(columns_of(ViewMode::Icons, 10), 1);
    // Trailing empty cells of the last row are blank.
    let last = item_rect(ViewMode::Icons, vw, n - 1);
    assert_eq!(
        item_at(ViewMode::Icons, vw, 0, last.right() + CELL_W, last.y + 2, n),
        None
    );
}

#[test]
fn rubber_band_selects_what_it_touches() {
    let (vw, n) = (600, 50);
    // A band over rows 2..=4 of the list.
    let a = item_rect(ViewMode::List, vw, 2);
    let b = item_rect(ViewMode::List, vw, 4);
    let band = band_rect((a.x + 40, a.y + 20), (b.x + 90, b.y + 3));
    assert_eq!(items_in_rect(ViewMode::List, vw, n, band), vec![2, 3, 4]);
    // A band in the margin that touches nothing is empty.
    let blank = band_rect((0, 0), (3, 3));
    assert!(items_in_rect(ViewMode::List, vw, n, blank).is_empty());
    // Past the end.
    let far = Rect::new(10, 100_000, 50, 50);
    assert!(items_in_rect(ViewMode::List, vw, n, far).is_empty());
    // Grid: a band over the first two cells of the first two rows.
    let c0 = item_rect(ViewMode::Icons, vw, 0);
    let cols = columns_of(ViewMode::Icons, vw);
    let c_last = item_rect(ViewMode::Icons, vw, cols + 1);
    let band = band_rect((c0.x + 10, c0.y + 10), (c_last.x + 10, c_last.y + 10));
    assert_eq!(
        items_in_rect(ViewMode::Icons, vw, n, band),
        vec![0, 1, cols, cols + 1]
    );
    // Empty rectangle or empty list.
    assert!(items_in_rect(ViewMode::Icons, vw, 0, band).is_empty());
    assert!(items_in_rect(ViewMode::Icons, vw, n, Rect::new(0, 0, 0, 10)).is_empty());
}

#[test]
fn band_rect_is_normalised_and_inclusive() {
    let r = band_rect((10, 20), (4, 30));
    assert_eq!((r.x, r.y, r.w, r.h), (4, 20, 7, 11));
    let r = band_rect((5, 5), (5, 5));
    assert_eq!((r.w, r.h), (1, 1));
}

#[test]
fn band_selection_adds_or_replaces() {
    assert_eq!(band_selection(&[1, 7], &[3, 2], false), vec![2, 3]);
    assert_eq!(band_selection(&[1, 7], &[3, 7], true), vec![1, 3, 7]);
    assert!(band_selection(&[1], &[], false).is_empty());
}

#[test]
fn drags_start_after_the_threshold_and_scroll_at_the_edges() {
    assert!(!drag_started((10, 10), (12, 13)));
    assert!(drag_started((10, 10), (10 + DRAG_THRESHOLD, 10)));
    assert!(drag_started((10, 10), (10, 10 - DRAG_THRESHOLD)));
    assert_eq!(edge_scroll(150, 100, 300), 0);
    assert!(edge_scroll(101, 100, 300) < 0);
    assert!(edge_scroll(299, 100, 300) > 0);
    assert!(edge_scroll(60, 100, 300) <= edge_scroll(101, 100, 300)); // faster further out
    assert!(edge_scroll(2000, 100, 300).abs() <= 18);
    assert!(edge_scroll(-2000, 100, 300).abs() <= 18);
}

#[test]
fn reveal_moves_the_least() {
    let (vw, vh, n) = (600, 280, 200);
    let max = max_scroll(ViewMode::List, vw, vh, n);
    // Already visible: unchanged.
    assert_eq!(reveal(ViewMode::List, vw, vh, 0, 3, n), 0);
    // Below the fold: the row ends up at the bottom edge.
    let s = reveal(ViewMode::List, vw, vh, 0, 30, n);
    let r = item_rect(ViewMode::List, vw, 30);
    assert!(
        s > 0 && r.bottom() <= s + vh && r.bottom() + 4 >= s + vh - 1,
        "{s}"
    );
    // Above: the row ends up at the top.
    let s2 = reveal(ViewMode::List, vw, vh, 3000, 5, n);
    assert!(s2 <= item_rect(ViewMode::List, vw, 5).y);
    // First and last rows reach the true ends.
    assert_eq!(reveal(ViewMode::List, vw, vh, 500, 0, n), 0);
    assert_eq!(reveal(ViewMode::List, vw, vh, 0, n - 1, n), max);
    // Always within range.
    for i in 0..n {
        let s = reveal(ViewMode::Icons, vw, vh, 77, i.min(n - 1), n);
        assert!(s >= 0 && s <= max_scroll(ViewMode::Icons, vw, vh, n));
    }
}

#[test]
fn arrow_keys_walk_the_list_and_the_grid() {
    let vw = 640;
    let cols = columns_of(ViewMode::Icons, vw);
    assert_eq!(step_index(ViewMode::List, vw, 0, Dir::Up, 5), 0);
    assert_eq!(step_index(ViewMode::List, vw, 0, Dir::Down, 5), 1);
    assert_eq!(step_index(ViewMode::List, vw, 4, Dir::Down, 5), 4);
    assert_eq!(step_index(ViewMode::Icons, vw, 0, Dir::Left, 20), 0);
    assert_eq!(step_index(ViewMode::Icons, vw, 3, Dir::Right, 20), 4);
    assert_eq!(step_index(ViewMode::Icons, vw, cols + 2, Dir::Up, 40), 2);
    assert_eq!(step_index(ViewMode::Icons, vw, 2, Dir::Up, 40), 2);
    assert_eq!(step_index(ViewMode::Icons, vw, 2, Dir::Down, 40), cols + 2);
    // From a full row into a short last row: land on its last item.
    let n = cols + 2;
    assert_eq!(
        step_index(ViewMode::Icons, vw, cols - 1, Dir::Down, n),
        n - 1
    );
    // Already on the last row: stays.
    assert_eq!(step_index(ViewMode::Icons, vw, n - 1, Dir::Down, n), n - 1);
    assert_eq!(step_index(ViewMode::Icons, vw, 0, Dir::Down, 0), 0);
    assert!(page_items(ViewMode::List, vw, 280) >= 8);
    assert_eq!(page_items(ViewMode::Icons, vw, 280) % cols, 0);
    assert!(page_items(ViewMode::List, vw, 0) >= 1);
}
