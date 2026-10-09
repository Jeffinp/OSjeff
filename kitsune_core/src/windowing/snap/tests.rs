use super::*;
use SnapZone as Z;

const W: i32 = 1280;
const H: i32 = 720;
const WORK: Rect = Rect::new(0, 30, 1280, 636);

#[test]
fn the_interior_snaps_nowhere() {
    for (x, y) in [(640, 360), (EDGE, 300), (W - EDGE - 1, 300), (300, EDGE)] {
        assert_eq!(zone_at(x, y, W, H), None, "{x},{y}");
    }
}

#[test]
fn the_top_edge_maximises_and_its_ends_are_the_top_quarters() {
    assert_eq!(zone_at(640, 0, W, H), Some(Z::Maximize));
    assert_eq!(zone_at(CORNER, 3, W, H), Some(Z::Maximize));
    assert_eq!(zone_at(CORNER - 1, 3, W, H), Some(Z::TopLeft));
    assert_eq!(zone_at(W - CORNER, 3, W, H), Some(Z::TopRight));
    assert_eq!(zone_at(W - CORNER - 1, 3, W, H), Some(Z::Maximize));
    assert_eq!(zone_at(0, 0, W, H), Some(Z::TopLeft));
    assert_eq!(zone_at(W - 1, 0, W, H), Some(Z::TopRight));
}

#[test]
fn the_side_edges_give_halves_and_quarters_near_the_corners() {
    assert_eq!(zone_at(0, 360, W, H), Some(Z::Left));
    assert_eq!(zone_at(W - 1, 360, W, H), Some(Z::Right));
    assert_eq!(zone_at(2, CORNER - 1, W, H), Some(Z::TopLeft));
    assert_eq!(zone_at(2, CORNER, W, H), Some(Z::Left));
    assert_eq!(zone_at(W - 2, H - CORNER, W, H), Some(Z::BottomRight));
    assert_eq!(zone_at(W - 2, H - CORNER - 1, W, H), Some(Z::Right));
    assert_eq!(zone_at(1, H - 1, W, H), Some(Z::BottomLeft));
}

#[test]
fn the_middle_of_the_bottom_edge_is_the_taskbars() {
    assert_eq!(zone_at(640, H - 1, W, H), None);
    assert_eq!(zone_at(100, H - 1, W, H), None);
    assert_eq!(zone_at(CORNER - 1, H - 2, W, H), Some(Z::BottomLeft));
    assert_eq!(zone_at(W - 1, H - 1, W, H), Some(Z::BottomRight));
}

#[test]
fn zones_tile_the_work_area_exactly() {
    for work in [WORK, Rect::new(3, 31, 1277, 635)] {
        assert_eq!(zone_rect(Z::Maximize, work), work);
        let (l, r) = (zone_rect(Z::Left, work), zone_rect(Z::Right, work));
        assert_eq!(l.right(), r.x);
        assert_eq!(l.w + r.w, work.w);
        assert_eq!((l.h, r.h), (work.h, work.h));
        let q =
            [Z::TopLeft, Z::TopRight, Z::BottomLeft, Z::BottomRight].map(|z| zone_rect(z, work));
        let area: i64 = q.iter().map(|r| r.w as i64 * r.h as i64).sum();
        assert_eq!(area, work.w as i64 * work.h as i64);
        assert_eq!(q[0].right(), q[1].x);
        assert_eq!(q[0].bottom(), q[2].y);
        assert_eq!(q[3].right(), work.right());
        assert_eq!(q[3].bottom(), work.bottom());
        for r in q {
            assert!(
                work.contains(r.x, r.y) && r.right() <= work.right() && r.bottom() <= work.bottom()
            );
        }
    }
}

#[test]
fn a_large_minimum_size_grows_the_zone_against_its_own_side() {
    // A 720-wide minimum cannot be tiled into a 640-wide half.
    let l = zone_rect_min(Z::Left, WORK, 720, 100);
    assert_eq!((l.x, l.w), (0, 720));
    let r = zone_rect_min(Z::Right, WORK, 720, 100);
    assert_eq!((r.right(), r.w), (W, 720));
    let br = zone_rect_min(Z::BottomRight, WORK, 720, 400);
    assert_eq!((br.right(), br.bottom()), (W, WORK.bottom()));
    assert_eq!(br.h, 400);
    // Never beyond the work area.
    let huge = zone_rect_min(Z::TopLeft, WORK, 5000, 5000);
    assert_eq!((huge.w, huge.h), (WORK.w, WORK.h));
    // A zone already big enough is untouched.
    assert_eq!(
        zone_rect_min(Z::Left, WORK, 200, 200),
        zone_rect(Z::Left, WORK)
    );
}

#[test]
fn the_arrows_walk_between_the_states_and_always_terminate() {
    // From a free window: left, then up gives the top-left quarter, up again maximises.
    let mut cur = None;
    for (key, expect) in [
        (Arrow::Left, Z::Left),
        (Arrow::Up, Z::TopLeft),
        (Arrow::Up, Z::Maximize),
    ] {
        match key_action(cur, key) {
            SnapAct::Zone(z) => {
                assert_eq!(z, expect);
                cur = Some(z);
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(key_action(cur, Arrow::Down), SnapAct::Restore);
    assert_eq!(key_action(None, Arrow::Down), SnapAct::Minimize);
    assert_eq!(key_action(Some(Z::Left), Arrow::Right), SnapAct::Restore);
    assert_eq!(key_action(Some(Z::Maximize), Arrow::Up), SnapAct::Stay);
    // Every (state, key) pair has an answer, and a Zone answer is never the current state.
    let states = [
        None,
        Some(Z::Maximize),
        Some(Z::Left),
        Some(Z::Right),
        Some(Z::TopLeft),
        Some(Z::TopRight),
        Some(Z::BottomLeft),
        Some(Z::BottomRight),
    ];
    for s in states {
        for k in [Arrow::Left, Arrow::Right, Arrow::Up, Arrow::Down] {
            if let SnapAct::Zone(z) = key_action(s, k) {
                assert_ne!(Some(z), s, "{s:?} {k:?}");
            }
        }
    }
}

#[test]
fn the_preview_blend_hits_both_ends() {
    let a = Rect::new(100, 100, 400, 300);
    let b = zone_rect(Z::Left, WORK);
    assert_eq!(lerp_rect(a, b, 0), a);
    assert_eq!(lerp_rect(a, b, 256), b);
    assert_eq!(lerp_rect(a, b, 999), b);
    let m = lerp_rect(a, b, 128);
    assert!(m.w > b.w.min(a.w) && m.w < b.w.max(a.w));
}
