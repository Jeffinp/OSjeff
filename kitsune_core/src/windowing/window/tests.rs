use super::*;

fn win() -> Rect {
    Rect::new(100, 100, 400, 300)
}

#[test]
fn contains_is_half_open() {
    let r = win();
    assert!(r.contains(100, 100));
    assert!(r.contains(499, 399));
    assert!(!r.contains(500, 400));
    assert!(!r.contains(99, 100));
}

#[test]
fn buttons_sit_at_the_right_edge_close_outermost() {
    let r = win();
    let (c, m, n) = (r.close_rect(), r.max_rect(), r.min_rect());
    assert_eq!(c, Rect::new(r.right() - BTN_W, r.y, BTN_W, TITLE_H));
    assert_eq!(c.right(), r.right());
    assert_eq!((m.right(), n.right()), (c.x, m.x));
    // The corner pixel of the window is the close button (Fitts: infinite target).
    assert_eq!(
        r.title_button_at(true, true, r.right() - 1, r.y),
        Some(TitleBtn::Close)
    );
    assert_eq!(
        r.title_button_at(true, true, m.x + 3, m.y + 3),
        Some(TitleBtn::Maximize)
    );
    assert_eq!(
        r.title_button_at(true, true, n.x + 3, n.y + 30),
        Some(TitleBtn::Minimize)
    );
    // Below the bar nothing is a button.
    assert_eq!(r.title_button_at(true, true, c.x, r.y + TITLE_H), None);
}

#[test]
fn the_menu_button_sits_left_of_the_buttons_and_is_optional() {
    let r = win();
    let l = r.title_layout(true, true);
    let menu = l.menu.unwrap();
    assert_eq!(menu.right(), l.min.x);
    assert_eq!(menu.w, MENU_W);
    assert_eq!(
        r.title_button_at(true, true, menu.x + 2, menu.y + 2),
        Some(TitleBtn::Menu)
    );
    let none = r.title_layout(true, false);
    assert!(none.menu.is_none());
    assert_eq!(r.title_button_at(true, false, menu.x + 2, menu.y + 2), None);
    // The title text stops before the first button.
    assert_eq!(l.title_right, menu.x - TITLE_GAP);
    assert_eq!(none.title_right, none.min.x - TITLE_GAP);
}

#[test]
fn a_window_that_cannot_be_resized_has_no_maximise_button() {
    let r = win();
    let l = r.title_layout(false, true);
    assert!(l.max.is_none());
    assert_eq!(l.min.right(), l.close.x);
    let x = r.max_rect().x + 3;
    assert_eq!(
        r.title_button_at(false, true, x, r.y + 3),
        Some(TitleBtn::Minimize)
    );
    assert_eq!(
        r.title_button_at(true, true, x, r.y + 3),
        Some(TitleBtn::Maximize)
    );
}

#[test]
fn the_icon_and_title_start_at_the_left_and_never_reach_the_buttons() {
    let r = win();
    let l = r.title_layout(true, true);
    assert_eq!(l.icon.x, r.x + TITLE_PAD);
    assert_eq!(l.icon.w, TITLE_ICON);
    // The icon is vertically centred in the bar.
    assert_eq!(l.icon.y - r.y, TITLE_H - (l.icon.bottom() - r.y));
    assert!(l.title_x > l.icon.right() && l.title_x < l.title_right);
    // A tiny window keeps a non-negative text width.
    let tiny = Rect::new(0, 0, 60, 100).title_layout(true, true);
    assert!(tiny.title_right >= tiny.title_x);
}

#[test]
fn title_excludes_the_buttons() {
    let r = win();
    assert!(r.on_title(r.x + 100, r.y + 10));
    for b in [r.close_rect(), r.max_rect(), r.min_rect()] {
        assert!(!r.on_title(b.x + 1, b.y + 1));
    }
    let menu = r.title_layout(true, true).menu.unwrap();
    assert!(!r.on_title(menu.x + 1, menu.y + 1));
    assert!(r.on_title(menu.x - 4, menu.y + 1));
}

#[test]
fn title_band_height() {
    let r = win();
    assert!(r.on_title(r.x + 5, r.y + TITLE_H - 1));
    assert!(!r.on_title(r.x + 5, r.y + TITLE_H));
}

#[test]
fn body_is_below_title() {
    let r = win();
    let b = r.body();
    assert_eq!(b, Rect::new(100, 100 + TITLE_H, 400, 300 - TITLE_H));
}

#[test]
fn clamp_keeps_window_on_screen() {
    let r = Rect::new(-50, -20, 400, 300);
    assert_eq!(r.clamped_pos(1280, 800), (0, MENUBAR_H));
    let r2 = Rect::new(2000, 2000, 400, 300);
    assert_eq!(r2.clamped_pos(1280, 800), (1280 - 400, 800 - TITLE_H));
}

#[test]
fn union_covers_both() {
    let a = Rect::new(10, 10, 20, 20); // [10,30)x[10,30)
    let b = Rect::new(40, 5, 10, 40); // [40,50)x[5,45)
    let u = a.union(&b);
    assert_eq!(u, Rect::new(10, 5, 40, 40)); // [10,50)x[5,45)
}

#[test]
fn union_with_empty_is_identity() {
    let a = Rect::new(10, 10, 20, 20);
    let empty = Rect::new(0, 0, 0, 0);
    assert_eq!(a.union(&empty), a);
    assert_eq!(empty.union(&a), a);
}

#[test]
fn intersection_overlap() {
    let a = Rect::new(0, 0, 30, 30);
    let b = Rect::new(20, 20, 30, 30);
    assert_eq!(a.intersection(&b), Some(Rect::new(20, 20, 10, 10)));
}

#[test]
fn intersection_disjoint_is_none() {
    let a = Rect::new(0, 0, 10, 10);
    let b = Rect::new(20, 20, 10, 10);
    assert_eq!(a.intersection(&b), None);
    // touching edges (not overlapping) -> None
    assert_eq!(a.intersection(&Rect::new(10, 0, 5, 10)), None);
}

#[test]
fn clamp_to_screen() {
    let r = Rect::new(-5, -5, 20, 20); // [-5,15)
    assert_eq!(r.clamped_to(1280, 800), Rect::new(0, 0, 15, 15));
    let off = Rect::new(1270, 0, 100, 50);
    assert_eq!(off.clamped_to(1280, 800), Rect::new(1270, 0, 10, 50));
    // fully off-screen -> zero area
    assert!(Rect::new(2000, 0, 10, 10).clamped_to(1280, 800).is_empty());
}

#[test]
fn inflate_grows_all_sides() {
    let r = Rect::new(10, 10, 20, 20);
    assert_eq!(r.inflated(5), Rect::new(5, 5, 30, 30));
}

#[test]
fn right_bottom_empty() {
    let r = Rect::new(10, 20, 30, 40);
    assert_eq!(r.right(), 40);
    assert_eq!(r.bottom(), 60);
    assert!(!r.is_empty());
    assert!(Rect::new(0, 0, 0, 5).is_empty());
}

#[test]
fn resize_edges_cover_borders_and_corners() {
    let r = win(); // [100,500) x [100,400)
    assert_eq!(r.resize_edge_at(300, 250), None); // interior
    assert_eq!(r.resize_edge_at(100, 250), Some(ResizeEdge::W));
    assert_eq!(r.resize_edge_at(499, 250), Some(ResizeEdge::E));
    assert_eq!(r.resize_edge_at(300, 100), Some(ResizeEdge::N));
    assert_eq!(r.resize_edge_at(300, 399), Some(ResizeEdge::S));
    assert_eq!(r.resize_edge_at(499, 399), Some(ResizeEdge::SE));
    assert_eq!(r.resize_edge_at(100, 399), Some(ResizeEdge::SW));
    assert_eq!(r.resize_edge_at(100, 100), Some(ResizeEdge::NW));
    assert_eq!(r.resize_edge_at(499, 100), Some(ResizeEdge::NE));
    // Bottom border next to the corner counts as the corner.
    assert_eq!(r.resize_edge_at(490, 399), Some(ResizeEdge::SE));
    assert_eq!(r.resize_edge_at(499, 390), Some(ResizeEdge::SE));
    // Outside the band, and outside the window: no edge.
    assert_eq!(r.resize_edge_at(100 + RESIZE_BAND, 250), None);
    assert_eq!(r.resize_edge_at(500, 250), None);
    assert_eq!(r.resize_edge_at(50, 50), None);
}

const SCREEN: (i32, i32) = (1280, 720);

#[test]
fn resized_follows_the_grabbed_side() {
    let r = win();
    let se = r.resized(ResizeEdge::SE, (30, 20), (100, 100), SCREEN);
    assert_eq!(se, Rect::new(100, 100, 430, 320));
    let nw = r.resized(ResizeEdge::NW, (20, 10), (100, 100), SCREEN);
    assert_eq!(nw, Rect::new(120, 110, 380, 290));
    let e = r.resized(ResizeEdge::E, (-50, 99), (100, 100), SCREEN);
    assert_eq!(e, Rect::new(100, 100, 350, 300)); // dy ignored on E
}

#[test]
fn resized_enforces_the_minimum_size_anchoring_the_far_side() {
    let r = win();
    let shrunk = r.resized(ResizeEdge::SE, (-1000, -1000), (200, 150), SCREEN);
    assert_eq!(shrunk, Rect::new(100, 100, 200, 150));
    let nw = r.resized(ResizeEdge::NW, (1000, 1000), (200, 150), SCREEN);
    assert_eq!(nw, Rect::new(300, 250, 200, 150)); // right/bottom fixed
}

#[test]
fn resized_stays_on_screen() {
    let r = win();
    let big = r.resized(ResizeEdge::SE, (5000, 5000), (100, 100), SCREEN);
    assert_eq!(big.right(), 1280);
    assert_eq!(big.bottom(), 720);
    let nw = r.resized(ResizeEdge::NW, (-5000, -5000), (100, 100), SCREEN);
    // The top edge stops under the panel.
    assert_eq!((nw.x, nw.y), (0, MENUBAR_H));
    assert_eq!((nw.right(), nw.bottom()), (500, 400));
}

#[test]
fn window_id_roundtrips_and_orders() {
    let a = WindowId::from_raw(3);
    assert_eq!(a.raw(), 3);
    assert!(a < WindowId::from_raw(4));
}

#[test]
fn clamp_handles_oversized_window() {
    // Width exceeds screen -> x pinned to 0. Height never constrains y
    // (only the title bar must stay visible), so y is unchanged.
    let r = Rect::new(10, 40, 2000, 2000);
    assert_eq!(r.clamped_pos(1280, 800), (0, 40));
}
