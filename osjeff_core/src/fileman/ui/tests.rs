//! Tests of the Arquivos geometry and models.

use super::*;
use alloc::vec;

fn win() -> Rect {
    Rect::new(100, 100, 860, 520)
}

fn lay(mode: ViewMode, preview: bool) -> Layout {
    Layout::of(win(), mode, preview, false)
}

fn crumbs_for(l: &Layout, widths: &[i32]) -> CrumbLayout {
    crumb_layout(l.path, widths)
}

// ---- regions ----

#[test]
fn regions_fit_the_window_and_do_not_overlap() {
    for mode in [ViewMode::List, ViewMode::Icons] {
        for preview in [false, true] {
            let l = lay(mode, preview);
            let w = l.window;
            for r in [
                l.sidebar,
                l.toolbar,
                l.back,
                l.forward,
                l.path,
                l.view_switch,
                l.sort,
                l.search,
                l.preview_btn,
                l.list,
                l.status,
            ] {
                assert!(r.x >= w.x && r.right() <= w.right(), "{r:?}");
                assert!(r.y >= w.y + TITLE_H && r.bottom() <= w.bottom(), "{r:?}");
            }
            // The toolbar controls are in a row, left to right, without touching.
            let row = [
                l.back,
                l.forward,
                l.path,
                l.view_switch,
                l.sort,
                l.search,
                l.preview_btn,
            ];
            for pair in row.windows(2) {
                assert!(pair[0].right() <= pair[1].x, "{:?} {:?}", pair[0], pair[1]);
            }
            assert!(l.toolbar.bottom() <= l.header.y.max(l.list.y));
            assert!(l.header.bottom() <= l.list.y);
            assert!(l.list.bottom() <= l.status.y);
            assert!(l.sidebar.right() <= l.main.x);
            if let Some(p) = l.preview {
                assert!(l.list.right() <= p.x && p.right() == l.main.right());
                assert_eq!(p.bottom(), l.status.y);
            } else {
                assert_eq!(l.list.right(), l.main.right());
            }
        }
    }
    assert_eq!(lay(ViewMode::Icons, false).header.h, 0);
    assert_eq!(lay(ViewMode::List, false).header.h, HEADER_H);
}

#[test]
fn the_search_field_collapses_in_a_narrow_window() {
    let narrow = Layout::of(Rect::new(0, 0, 600, 400), ViewMode::List, false, false);
    assert!(!narrow.search_is_field());
    let open = Layout::of(Rect::new(0, 0, 600, 400), ViewMode::List, false, true);
    assert!(
        open.search_is_field() && open.search.w >= 120,
        "{:?}",
        open.search
    );
    // Open in a narrow window, the field takes the place of the path bar and the two buttons.
    assert_eq!((open.path.w, open.sort.w, open.view_switch.w), (0, 0, 0));
    assert!(open.search.right() < open.preview_btn.x);
    let wide = lay(ViewMode::List, false);
    assert!(wide.search_is_field() && wide.path.w > 200);
}

#[test]
fn the_minimum_window_still_has_a_path_bar_and_a_list() {
    let l = Layout::of(Rect::new(0, 0, 640, 320), ViewMode::List, true, false);
    assert!(l.path.w >= 40, "{:?}", l.path);
    assert!(l.list.w > 150 && l.list.h > 100, "{:?}", l.list);
    assert!(l.preview.is_some_and(|p| p.w <= l.main.w / 2));
}

#[test]
fn layout_survives_tiny_windows() {
    let l = Layout::of(Rect::new(0, 0, 100, 60), ViewMode::List, true, true);
    assert!(l.list.h >= 0 && l.sidebar.h >= 0 && l.path.w >= 0 && l.list.w >= 0);
    assert!(l.status.h >= 0);
    let none = Layout::of(Rect::new(0, 0, 0, 0), ViewMode::Icons, false, false);
    assert!(none.list.h >= 0);
}

#[test]
fn columns_drop_the_date_when_narrow() {
    let wide = Columns::of(300, 560);
    assert!(wide.has_date());
    assert!(wide.name_x < wide.size_x && wide.size_x < wide.date_x && wide.date_x < wide.right);
    assert_eq!(wide.key_at(wide.name_x), SortKey::Name);
    assert_eq!(wide.key_at(wide.size_x + 1), SortKey::Size);
    assert_eq!(wide.key_at(wide.date_x + 1), SortKey::Modified);
    let narrow = Columns::of(300, NARROW_LIST - 1);
    assert!(!narrow.has_date());
    assert_eq!(narrow.key_at(narrow.right - 1), SortKey::Size);
}

#[test]
fn the_sidebar_fits_at_the_minimum_height() {
    let l = Layout::of(Rect::new(0, 0, 640, 320), ViewMode::List, false, false);
    let side = SideLayout::of(l.sidebar);
    for (i, (_, a)) in side.items.iter().enumerate() {
        assert!(
            a.y >= l.sidebar.y && a.bottom() <= l.sidebar.bottom(),
            "{a:?}"
        );
        assert!(a.x >= l.sidebar.x && a.right() <= l.sidebar.right());
        for (_, b) in &side.items[i + 1..] {
            assert!(a.bottom() <= b.y || b.bottom() <= a.y, "{a:?} {b:?}");
        }
    }
    assert!(side.favorites_title.bottom() <= side.items[0].1.y);
    assert!(side.places_title.y >= side.items[4].1.bottom());
    assert!(side.places_title.bottom() <= side.items[5].1.y);
    let places: Vec<Place> = side.items.iter().map(|&(p, _)| p).collect();
    assert_eq!(
        places,
        vec![
            Place::Home,
            Place::Documents,
            Place::Images,
            Place::Apps,
            Place::Trash,
            Place::Disk
        ]
    );
}

// ---- hit testing ----

fn hit(l: &Layout, mode: ViewMode, scroll: i32, n: usize, p: (i32, i32)) -> Option<Hit> {
    let crumbs = crumbs_for(l, &[40, 60, 50]);
    l.hit(
        p.0,
        p.1,
        &HitCtx {
            mode,
            scroll,
            count: n,
            crumbs: &crumbs,
        },
    )
}

fn centre(r: Rect) -> (i32, i32) {
    (r.x + r.w / 2, r.y + r.h / 2)
}

#[test]
fn hit_testing_finds_each_region() {
    let l = lay(ViewMode::List, false);
    let h = |p| hit(&l, ViewMode::List, 0, 20, p);
    assert_eq!(h(centre(l.back)), Some(Hit::Back));
    assert_eq!(h(centre(l.forward)), Some(Hit::Forward));
    assert_eq!(h(centre(l.sort)), Some(Hit::SortButton));
    assert_eq!(h(centre(l.search)), Some(Hit::Search));
    assert_eq!(h(centre(l.preview_btn)), Some(Hit::PreviewButton));
    let v = l.view_switch;
    assert_eq!(h((v.x + 5, v.y + 5)), Some(Hit::View(ViewMode::List)));
    assert_eq!(
        h((v.right() - 5, v.y + 5)),
        Some(Hit::View(ViewMode::Icons))
    );
    let side = SideLayout::of(l.sidebar);
    for (p, r) in side.items {
        assert_eq!(h(centre(r)), Some(Hit::Place(p)));
    }
    assert_eq!(
        h((l.sidebar.x + 2, l.sidebar.bottom() - 2)),
        Some(Hit::Dead)
    );
    let cols = l.columns();
    assert_eq!(
        h((cols.name_x, l.header.y + 5)),
        Some(Hit::Header(SortKey::Name))
    );
    assert_eq!(
        h((cols.size_x + 5, l.header.y + 5)),
        Some(Hit::Header(SortKey::Size))
    );
    assert_eq!(
        h((cols.date_x + 5, l.header.y + 5)),
        Some(Hit::Header(SortKey::Modified))
    );
    assert_eq!(
        h((l.list.x + 30, l.list.y + LIST_PAD + 1)),
        Some(Hit::Item(0))
    );
    assert_eq!(
        h((l.list.x + 30, l.list.y + LIST_PAD + 3 * ROW_H + 1)),
        Some(Hit::Item(3))
    );
    // Scrolled by 10 rows.
    assert_eq!(
        hit(
            &l,
            ViewMode::List,
            10 * ROW_H,
            40,
            (l.list.x + 30, l.list.y + LIST_PAD + 1)
        ),
        Some(Hit::Item(10))
    );
    // Past the last row, and in the side margin of a row, is blank.
    assert_eq!(
        hit(
            &l,
            ViewMode::List,
            0,
            3,
            (l.list.x + 30, l.list.bottom() - 2)
        ),
        Some(Hit::Blank)
    );
    assert_eq!(h((l.list.x + 2, l.list.y + LIST_PAD + 1)), Some(Hit::Blank));
    assert_eq!(h((l.status.x + 20, l.status.y + 5)), Some(Hit::Dead));
    assert_eq!(hit(&l, ViewMode::List, 0, 3, (0, 0)), None);
}

#[test]
fn the_preview_pane_takes_its_own_presses() {
    let l = lay(ViewMode::List, true);
    let p = l.preview.unwrap();
    assert_eq!(
        hit(&l, ViewMode::List, 0, 5, centre(p)),
        Some(Hit::PreviewPane)
    );
}

#[test]
fn hits_in_the_icon_grid() {
    let l = lay(ViewMode::Icons, false);
    let r0 = item_rect(ViewMode::Icons, l.list.w, 0);
    let p = (l.list.x + r0.x + r0.w / 2, l.list.y + r0.y + r0.h / 2);
    assert_eq!(hit(&l, ViewMode::Icons, 0, 30, p), Some(Hit::Item(0)));
    // The gap between two cells is blank.
    let gap = (l.list.x + r0.right() + 2, l.list.y + r0.y + 10);
    assert_eq!(hit(&l, ViewMode::Icons, 0, 30, gap), Some(Hit::Blank));
    assert_eq!(l.header.h, 0);
}

// ---- the path bar ----

#[test]
fn crumbs_fit_and_hit() {
    let bar = Rect::new(100, 50, 400, 28);
    let c = crumb_layout(bar, &[40, 70, 60]);
    assert_eq!(c.first, 0);
    assert!(c.fold.is_none());
    assert_eq!(c.spans.len(), 3);
    for pair in c.spans.windows(2) {
        assert_eq!(pair[1].1.x - pair[0].1.right(), CRUMB_GAP);
    }
    for (i, r) in &c.spans {
        assert!(r.x >= bar.x + PATH_PAD && r.right() <= bar.right() - PATH_PAD);
        assert_eq!(c.crumb_at(r.x + r.w / 2, r.y + 2), Some(*i));
    }
    assert_eq!(c.spans[0].1.w, 40 + 2 * CRUMB_PAD);
    assert_eq!(c.crumb_at(bar.right() - 2, bar.y + 4), None);
}

#[test]
fn leading_crumbs_fold_but_the_last_stays() {
    let bar = Rect::new(0, 0, 220, 28);
    let c = crumb_layout(bar, &[40, 90, 90, 90]);
    assert!(c.first > 0);
    let fold = c.fold.unwrap();
    assert!(c.fold_hit(fold.x + 3, fold.y + 3));
    assert_eq!(c.spans.last().unwrap().0, 3);
    assert!(c.spans.first().unwrap().1.x >= fold.right());
    assert!(
        c.spans
            .iter()
            .all(|(_, r)| r.right() <= bar.right() - PATH_PAD)
    );
    // A single crumb wider than the bar is cut to the room, never dropped.
    let c = crumb_layout(Rect::new(0, 0, 100, 28), &[40, 500]);
    assert_eq!(c.spans.last().unwrap().0, 1);
    assert!(c.spans.last().unwrap().1.w <= 100 - 2 * PATH_PAD);
    // No crumbs, no layout.
    assert!(crumb_layout(bar, &[]).spans.is_empty());
}

#[test]
fn folding_is_monotonic_in_the_width() {
    let widths = [40, 80, 60, 100, 70];
    let mut last_first = usize::MAX;
    for w in (60..900).step_by(20) {
        let c = crumb_layout(Rect::new(0, 0, w, 28), &widths);
        assert!(c.first <= last_first, "{w}");
        last_first = c.first;
        assert_eq!(c.spans.last().unwrap().0, widths.len() - 1);
    }
}

// ---- list and grid models ----

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

// ---- drag and drop ----

fn paths(p: &[&str]) -> Vec<Vec<u8>> {
    p.iter().map(|s| s.as_bytes().to_vec()).collect()
}

#[test]
fn a_folder_cannot_be_dropped_into_itself_or_below() {
    let src = paths(&["/a/docs"]);
    assert!(!can_drop_into(&src, b"/a/docs"));
    assert!(!can_drop_into(&src, b"/a/docs/inner"));
    assert!(!can_drop_into(&src, b"/a/docs/inner/deeper"));
    // A sibling with a similar name is fine.
    assert!(can_drop_into(&src, b"/a/docs2"));
    assert!(can_drop_into(&src, b"/b"));
    assert!(can_drop_into(&src, b"/"));
}

#[test]
fn dropping_where_the_items_already_are_does_nothing() {
    assert!(!can_drop_into(&paths(&["/a/x", "/a/y"]), b"/a"));
    assert!(can_drop_into(&paths(&["/a/x", "/b/y"]), b"/a")); // one of them moves
    assert!(!can_drop_into(&paths(&["/x"]), b"/"));
    assert!(can_drop_into(&paths(&["/a/x"]), b"/"));
    assert!(!can_drop_into(&[], b"/a"));
    assert!(!can_drop_into(&paths(&["/a/x"]), TRASH_PATH));
    assert!(!can_drop_into(&paths(&["/a/x"]), APPS_PATH));
}

#[test]
fn plan_drop_picks_the_operation() {
    let src = paths(&["/a/x"]);
    assert_eq!(
        plan_drop(&src, DropTarget::Folder(b"/b"), false),
        Some(DropOp::Move)
    );
    assert_eq!(
        plan_drop(&src, DropTarget::Folder(b"/b"), true),
        Some(DropOp::Copy)
    );
    assert_eq!(plan_drop(&src, DropTarget::Folder(b"/a"), false), None);
    assert_eq!(
        plan_drop(&src, DropTarget::Trash, false),
        Some(DropOp::Trash)
    );
    assert_eq!(
        plan_drop(&src, DropTarget::Trash, true),
        Some(DropOp::Trash)
    );
    assert_eq!(plan_drop(&src, DropTarget::None, false), None);
    assert_eq!(plan_drop(&[], DropTarget::Trash, false), None);
    assert_eq!(
        plan_drop(&paths(&["/a"]), DropTarget::Folder(b"/a/b"), false),
        None
    );
}

#[test]
fn sidebar_places_are_drop_targets() {
    assert_eq!(place_target(Place::Trash), DropTarget::Trash);
    assert_eq!(place_target(Place::Apps), DropTarget::None);
    assert_eq!(
        place_target(Place::Documents),
        DropTarget::Folder(b"/Documentos")
    );
    assert_eq!(place_target(Place::Disk), DropTarget::Folder(b"/"));
    assert_eq!(place_target(Place::Home), DropTarget::Folder(b"/home"));
    assert_eq!(place_target(Place::Images), DropTarget::Folder(b"/Imagens"));
}

// ---- scrolling ----

#[test]
fn the_scroller_glides_to_its_target_and_stops() {
    let mut s = Scroller::new();
    s.set_max(1000);
    assert_eq!((s.pos(), s.target()), (0, 0));
    s.scroll_by(WHEEL_STEP);
    assert_eq!(s.target(), WHEEL_STEP);
    // It starts moving at once but has not arrived.
    s.step(0.016);
    assert!(s.pos() > 0 && s.pos() < WHEEL_STEP, "{}", s.pos());
    let mut frames = 0;
    while s.step(0.016) {
        frames += 1;
        assert!(frames < 120, "never settles");
    }
    assert_eq!(s.pos(), WHEEL_STEP);
    assert!(s.at_rest());
    // Monotonic: no overshoot past the target for a critically damped spring.
    let mut s = Scroller::new();
    s.set_max(1000);
    s.scroll_by(300);
    let mut prev = 0;
    for _ in 0..200 {
        s.step(0.008);
        assert!(
            s.pos() >= prev && s.pos() <= 300,
            "{} after {prev}",
            s.pos()
        );
        prev = s.pos();
    }
}

#[test]
fn the_scroller_clamps_to_its_range() {
    let mut s = Scroller::new();
    s.set_max(100);
    s.scroll_by(-500);
    assert_eq!(s.target(), 0);
    s.scroll_by(5000);
    assert_eq!(s.target(), 100);
    s.jump(60);
    assert_eq!((s.pos(), s.target()), (60, 60));
    // The content shrank: position and target follow the new range.
    s.set_max(20);
    assert_eq!(s.target(), 20);
    s.step(0.0);
    assert!(s.pos() <= 20);
    s.set_max(-5);
    assert_eq!(s.max(), 0);
    s.scroll_to(50);
    assert_eq!(s.target(), 0);
}

#[test]
fn reduce_motion_lands_at_once() {
    crate::anim::set_reduce_motion(true);
    let mut s = Scroller::new();
    s.set_max(500);
    s.scroll_by(200);
    s.step(0.001);
    assert_eq!(s.pos(), 200);
    crate::anim::set_reduce_motion(false);
}

// ---- search, kinds, previews, dates ----

#[test]
fn search_ignores_case_and_accents() {
    assert!(matches_query("Ação.txt".as_bytes(), b"acao"));
    assert!(matches_query("Ação.txt".as_bytes(), "AÇÃO".as_bytes()));
    assert!(matches_query(b"relatorio-final.pdf", b"FINAL"));
    assert!(!matches_query(b"relatorio.pdf", b"final"));
    assert!(matches_query(b"x", b""));
    assert!(matches_query(b"x", b"   "));
    assert!(matches_query(b"my file", b" my f "));
    assert_eq!(search_key("Ação".as_bytes()), "acao");
}

#[test]
fn kinds_follow_the_extension() {
    assert_eq!(preview_kind(b"a", true), PreviewKind::Folder);
    assert_eq!(preview_kind(b"a.PNG", false), PreviewKind::Image);
    assert_eq!(preview_kind(b"a.wasm", false), PreviewKind::App);
    assert_eq!(preview_kind(b"notas.txt", false), PreviewKind::Text);
    assert_eq!(preview_kind(b"a.bin", false), PreviewKind::Other);
    use crate::appart::FileKind;
    assert_eq!(icon_kind(b"d", true), FileKind::Folder);
    assert_eq!(icon_kind(b"a.bmp", false), FileKind::Image);
    assert_eq!(icon_kind(b"a.bin", false), FileKind::Generic);
    assert_eq!(kind_label(b"d", true), "Pasta");
    assert_eq!(kind_label(b"a.png", false), "Imagem PNG");
    assert_eq!(kind_label(b"a.txt", false), "Texto TXT");
    assert_eq!(kind_label(b"LEIAME", false), "Texto");
    assert_eq!(kind_label(b"a.bin", false), "Arquivo BIN");
    assert_eq!(kind_label(b"a.wasm", false), "Aplicativo");
}

#[test]
fn text_previews_are_clean_and_bounded() {
    let t = text_preview(b"um\ndois\tcom tab\r\ntres\n\n\n", 10, 40);
    assert_eq!(t, vec!["um", "dois    com tab", "tres"]);
    let long = text_preview("x".repeat(500).as_bytes(), 3, 20);
    assert_eq!(long[0].len(), 20);
    let many: Vec<u8> = (0..100)
        .flat_map(|i| alloc::format!("l{i}\n").into_bytes())
        .collect();
    assert_eq!(text_preview(&many, 7, 40).len(), 7);
    assert_eq!(text_preview("ação ✓\n".as_bytes(), 3, 40), vec!["ação ✓"]);
    // Binary data gives nothing to show.
    assert!(text_preview(&[0, 1, 2, 3, 255, 0, 9], 5, 20).is_empty());
    assert!(text_preview(b"", 5, 20).is_empty());
    // Control characters become spaces.
    assert_eq!(
        text_preview(b"abcdefghijklmnopqrstuvwxyz\x07abc", 2, 40),
        vec!["abcdefghijklmnopqrstuvwxyz abc"]
    );
}

#[test]
fn modified_dates_say_today_and_yesterday() {
    // 2026-10-08 14:32:00 UTC; local time is UTC-3.
    let now = 1_791_469_920u64;
    let tz = -3 * 3600;
    assert_eq!(format_modified(0, now, tz), "--");
    assert_eq!(format_modified(now - 600, now, tz), "Hoje, 11:22");
    assert_eq!(format_modified(now - 86_400, now, tz), "Ontem, 11:32");
    assert_eq!(
        format_modified(now - 5 * 86_400, now, tz),
        crate::fileman::format_datetime(now - 5 * 86_400, tz)
    );
    // Just after local midnight is still "today"; the minute before is "yesterday".
    let midnight_local = now - (now as i64 + tz as i64).rem_euclid(86_400) as u64;
    assert!(format_modified(midnight_local, now, tz).starts_with("Hoje"));
    assert!(format_modified(midnight_local - 1, now, tz).starts_with("Ontem"));
    // A file from the future (a clock that moved back) is not "yesterday".
    assert!(!format_modified(now + 10 * 86_400, now, tz).starts_with("Ontem"));
}
