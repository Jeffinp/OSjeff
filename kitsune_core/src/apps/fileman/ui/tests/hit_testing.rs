use super::*;

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
