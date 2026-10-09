use super::*;

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
