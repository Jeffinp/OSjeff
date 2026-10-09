//! The taskbar: a floating, centred, rounded bar at the bottom of the screen with the Apps button,
//! the pinned apps, the apps that are running without being pinned, and a *show desktop* sliver
//! at its right end. No magnification: an icon only lifts a little under the pointer.
//!
//! Pure integer geometry and the small rules around it (what a click does, which indicator an
//! icon wears, where a dragged icon lands and how its neighbours make room), shared by the
//! kernel's drawing and hit testing so the two cannot disagree. See `docs/design/ui-identity.md`.

use crate::windowing::window::Rect;
use alloc::vec::Vec;

/// Side of an app icon.
pub const ICON: i32 = 40;
/// Gap between neighbouring icons.
pub const GAP: i32 = 6;
/// Padding inside the bar, left and right / top and bottom.
pub const PAD_X: i32 = 10;
pub const PAD_Y: i32 = 8;
/// Distance from the bar to the bottom of the screen.
pub const BOTTOM: i32 = 8;
/// Space taken by a separator between groups (the line sits in the middle).
pub const SEP: i32 = 14;
/// Width of the show-desktop sliver.
pub const SLIVER_W: i32 = 10;
/// Height of the bar.
pub const H: i32 = ICON + 2 * PAD_Y;
/// How far an icon lifts under the pointer.
pub const LIFT: i32 = 3;
/// Pointer travel before a press on an icon becomes a drag.
pub const DRAG_SLOP: i32 = 6;

/// Where everything sits for `n` app icons on a `sw x sh` screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub panel: Rect,
    /// The Apps button (first).
    pub apps: Rect,
    /// One rectangle per app icon, left to right.
    pub items: Vec<Rect>,
    /// X of the separator lines: after the Apps button and before the sliver.
    pub sep_apps: i32,
    pub sep_sliver: i32,
    /// The show-desktop sliver (full height inside the padding).
    pub sliver: Rect,
}

/// The resting layout for `n` app icons.
pub fn layout(sw: i32, sh: i32, n: usize) -> Layout {
    let n = n as i32;
    let icons_w = if n > 0 { n * ICON + (n - 1) * GAP } else { 0 };
    let total = PAD_X + ICON + SEP + icons_w + SEP + SLIVER_W + PAD_X;
    let y = sh - BOTTOM - H;
    let panel = Rect::new(sw / 2 - total / 2, y, total, H);
    let top = y + PAD_Y;
    let mut x = panel.x + PAD_X;
    let apps = Rect::new(x, top, ICON, ICON);
    x += ICON;
    let sep_apps = x + SEP / 2;
    x += SEP;
    let mut items = Vec::with_capacity(n as usize);
    for _ in 0..n {
        items.push(Rect::new(x, top, ICON, ICON));
        x += ICON + GAP;
    }
    if n > 0 {
        x -= GAP;
    }
    let sep_sliver = x + SEP / 2;
    x += SEP;
    Layout {
        panel,
        apps,
        items,
        sep_apps,
        sep_sliver,
        sliver: Rect::new(x, y + PAD_Y, SLIVER_W, ICON),
    }
}

/// What the pointer is over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    Apps,
    Item(usize),
    Sliver,
}

/// The part of the bar under `(x, y)`. The whole height of the bar counts (a generous target), and
/// the gap between two icons belongs to the nearer one.
pub fn hit(l: &Layout, x: i32, y: i32) -> Option<Hit> {
    if !l.panel.contains(x, y) {
        return None;
    }
    if x >= l.sliver.x - SEP / 2 {
        return Some(Hit::Sliver);
    }
    if x < l.apps.right() + SEP / 2 {
        return Some(Hit::Apps);
    }
    // The nearest icon by its centre.
    l.items
        .iter()
        .enumerate()
        .min_by_key(|(_, r)| (x - (r.x + r.w / 2)).abs())
        .map(|(i, _)| Hit::Item(i))
}

/// The indicator under an icon.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Indicator {
    /// Not running.
    None,
    /// Running (a dot); `dim` when every window is minimised.
    Dot { dim: bool },
    /// The app has the focus (a long pill).
    Pill,
}

pub fn indicator(running: bool, focused: bool, all_minimized: bool) -> Indicator {
    match (running, focused) {
        (false, _) => Indicator::None,
        (true, true) => Indicator::Pill,
        (true, false) => Indicator::Dot { dim: all_minimized },
    }
}

/// What a click on an app's icon does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Click {
    /// Not running: open it.
    Launch,
    /// Running somewhere else (or minimised): bring its latest window forward.
    Focus,
    /// Its window already has the focus: minimise it.
    Minimize,
}

pub fn click_action(running: bool, app_focused: bool) -> Click {
    match (running, app_focused) {
        (false, _) => Click::Launch,
        (true, false) => Click::Focus,
        (true, true) => Click::Minimize,
    }
}

/// The slot a dragged icon (item `from`) lands in when the pointer is at `x`: the slot whose centre
/// is nearest, limited to the first `pinned` slots (only pinned apps reorder).
pub fn drop_slot(l: &Layout, x: i32, pinned: usize) -> usize {
    if pinned == 0 || l.items.is_empty() {
        return 0;
    }
    let last = pinned.min(l.items.len()) - 1;
    l.items[..=last]
        .iter()
        .enumerate()
        .min_by_key(|(_, r)| (x - (r.x + r.w / 2)).abs())
        .map_or(0, |(i, _)| i)
}

/// The slot item `i` takes while item `from` is dragged over slot `to`: the dragged icon goes to
/// `to`, the ones between slide one place the other way.
pub fn slot_while_dragging(i: usize, from: usize, to: usize) -> usize {
    if i == from {
        to
    } else if from < to && i > from && i <= to {
        i - 1
    } else if from > to && i >= to && i < from {
        i + 1
    } else {
        i
    }
}

/// Move item `from` to position `to`, shifting the ones between.
pub fn reorder<T>(v: &mut Vec<T>, from: usize, to: usize) {
    if from >= v.len() || to >= v.len() || from == to {
        return;
    }
    let x = v.remove(from);
    v.insert(to, x);
}

/// The tooltip for `icon`: centred above the bar, kept on screen.
pub fn tooltip(icon: Rect, panel: Rect, w: i32, h: i32, sw: i32) -> Rect {
    let x = (icon.x + icon.w / 2 - w / 2).clamp(4, (sw - w - 4).max(4));
    Rect::new(x, panel.y - h - 8, w, h)
}

/// Region the bar can paint in: its panel plus the room above it for lifted icons, launch hops
/// and the tooltip.
pub fn paint_zone(l: &Layout, sw: i32, sh: i32) -> Rect {
    Rect::new(
        l.panel.x - 40,
        l.panel.y - 64,
        l.panel.w + 80,
        sh - l.panel.y + 64,
    )
    .clamped_to(sw, sh)
}

#[cfg(test)]
mod tests {
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
            z.contains(l.panel.x, l.panel.y)
                && z.contains(l.panel.right() - 1, l.panel.bottom() - 1)
        );
        assert!(z.bottom() <= 720 && z.y < l.panel.y - 40);
    }
}
