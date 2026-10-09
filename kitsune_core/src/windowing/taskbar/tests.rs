use super::*;

#[test]
fn the_resting_layout_is_centred_and_on_the_grid() {
    let l = layout(1280, 720, 9);
    assert_eq!(l.items.len(), 9);
    assert_eq!(l.panel.h, H);
    assert_eq!(l.panel.bottom(), 720 - BOTTOM);
    assert!(((l.panel.x + l.panel.w / 2) - 640).abs() <= 1);
    assert_eq!(l.apps.x, l.panel.x + PAD_X);
    assert_eq!(l.items[0].x, l.apps.right() + SEP);
    assert_eq!(l.items[1].x, l.items[0].right() + GAP);
    assert_eq!(l.sliver.x, l.items[8].right() + SEP);
    assert_eq!(l.sliver.right() + PAD_X, l.panel.right());
    for r in l.items.iter().chain([&l.apps]) {
        assert_eq!((r.w, r.h), (ICON, ICON));
        assert_eq!(r.y, l.panel.y + PAD_Y);
    }
    // The separators sit between their neighbours.
    assert!(l.apps.right() < l.sep_apps && l.sep_apps < l.items[0].x);
    assert!(l.items[8].right() < l.sep_sliver && l.sep_sliver < l.sliver.x);
    // The bar is not magnifying: a different count only changes the width.
    assert_eq!(layout(1280, 720, 10).panel.w - l.panel.w, ICON + GAP);
    // An empty list still has the Apps button and the sliver.
    let e = layout(1280, 720, 0);
    assert!(e.items.is_empty() && e.sliver.x > e.apps.right());
}

#[test]
fn the_icon_columns_know_the_pointer() {
    let l = layout(1280, 720, 9);
    for (i, r) in l.items.iter().enumerate() {
        assert_eq!(hit(&l, r.x + r.w / 2, r.y + 5), Some(Hit::Item(i)));
        // The whole height of the bar is the target, including its padding.
        assert_eq!(hit(&l, r.x + 3, l.panel.y + 1), Some(Hit::Item(i)));
        assert_eq!(hit(&l, r.x + 3, l.panel.bottom() - 1), Some(Hit::Item(i)));
    }
    assert_eq!(hit(&l, l.apps.x + 5, l.apps.y + 5), Some(Hit::Apps));
    assert_eq!(hit(&l, l.sliver.x + 2, l.sliver.y + 2), Some(Hit::Sliver));
    // The gap belongs to the nearer neighbour.
    let gap = l.items[2].right() + 1;
    assert_eq!(hit(&l, gap, l.items[2].y), Some(Hit::Item(2)));
    assert_eq!(hit(&l, gap + 4, l.items[2].y), Some(Hit::Item(3)));
    // Outside the bar nothing is hit.
    assert_eq!(hit(&l, 10, 700), None);
    assert_eq!(hit(&l, 640, l.panel.y - 1), None);
    assert_eq!(hit(&l, 640, 720), None);
}

#[test]
fn indicators_and_clicks_follow_the_window_state() {
    assert_eq!(indicator(false, false, false), Indicator::None);
    assert_eq!(indicator(true, true, false), Indicator::Pill);
    assert_eq!(indicator(true, false, false), Indicator::Dot { dim: false });
    assert_eq!(indicator(true, false, true), Indicator::Dot { dim: true });
    assert_eq!(click_action(false, false), Click::Launch);
    assert_eq!(click_action(true, false), Click::Focus);
    assert_eq!(click_action(true, true), Click::Minimize);
}

#[test]
fn a_dragged_icon_lands_in_the_nearest_pinned_slot() {
    let l = layout(1280, 720, 9);
    let c = |i: usize| l.items[i].x + l.items[i].w / 2;
    assert_eq!(drop_slot(&l, c(3) + 4, 7), 3);
    assert_eq!(drop_slot(&l, c(0) - 200, 7), 0);
    // Slots past the pinned ones are not drop targets.
    assert_eq!(drop_slot(&l, c(8), 7), 6);
    assert_eq!(drop_slot(&l, c(8), 0), 0);
}

#[test]
fn neighbours_make_room_for_the_dragged_icon() {
    // Drag item 1 to slot 4: items 2, 3, 4 slide left by one.
    let slots: Vec<usize> = (0..6).map(|i| slot_while_dragging(i, 1, 4)).collect();
    assert_eq!(slots, [0, 4, 1, 2, 3, 5]);
    // Drag item 4 to slot 1: items 1, 2, 3 slide right by one.
    let slots: Vec<usize> = (0..6).map(|i| slot_while_dragging(i, 4, 1)).collect();
    assert_eq!(slots, [0, 2, 3, 4, 1, 5]);
    // Always a permutation, and dropping where it started changes nothing.
    for from in 0..6 {
        for to in 0..6 {
            let mut s: Vec<usize> = (0..6).map(|i| slot_while_dragging(i, from, to)).collect();
            s.sort_unstable();
            assert_eq!(s, [0, 1, 2, 3, 4, 5], "{from}->{to}");
        }
        assert!((0..6).all(|i| slot_while_dragging(i, from, from) == i));
    }
}

#[test]
fn reordering_matches_the_slots_it_previewed() {
    let (from, to) = (1usize, 4usize);
    let mut v = alloc::vec!['a', 'b', 'c', 'd', 'e', 'f'];
    let before = v.clone();
    reorder(&mut v, from, to);
    for (i, ch) in before.iter().enumerate() {
        assert_eq!(v[slot_while_dragging(i, from, to)], *ch);
    }
    // Out-of-range and no-op requests leave the list alone.
    let mut w = before.clone();
    reorder(&mut w, 9, 0);
    reorder(&mut w, 0, 9);
    reorder(&mut w, 2, 2);
    assert_eq!(w, before);
}

#[test]
fn tooltips_and_the_paint_zone_stay_on_screen() {
    let l = layout(1280, 720, 9);
    let t = tooltip(l.items[0], l.panel, 90, 24, 1280);
    assert!(t.bottom() < l.panel.y && t.x >= 4);
    let far = tooltip(Rect::new(1270, 660, 40, 40), l.panel, 120, 24, 1280);
    assert!(far.right() <= 1276);
    let z = paint_zone(&l, 1280, 720);
    assert!(
        z.contains(l.panel.x, l.panel.y) && z.contains(l.panel.right() - 1, l.panel.bottom() - 1)
    );
    assert!(z.bottom() <= 720 && z.y < l.panel.y - 40);
}
