//! Window snapping: which part of the work area a window fills when it is dragged
//! to a screen edge (Windows 11 / KDE style: top = maximise, left and right = halves,
//! corners = quarters) and what the `Alt+arrow` keys do next from the current state.
//!
//! Pure integer geometry. The kernel decides *when* (pointer position during a drag,
//! a key press); this module decides *where*.

use crate::window::Rect;

/// Distance from a screen edge, in pixels, at which a dragged pointer snaps.
pub const EDGE: i32 = 6;
/// Length of the corner zone along an edge: a pointer this close to a corner snaps to the
/// quarter instead of the half or the maximised window.
pub const CORNER: i32 = 48;

/// Where a window can be snapped.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapZone {
    /// The whole work area (the maximised state).
    Maximize,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// An arrow key of the `Alt+arrow` snap shortcuts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arrow {
    Left,
    Right,
    Up,
    Down,
}

/// What an `Alt+arrow` press asks the window manager to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapAct {
    /// Put the window in this zone.
    Zone(SnapZone),
    /// Back to the size it had before it was maximised or snapped.
    Restore,
    /// Minimise it (`Alt+Down` on a window that is free).
    Minimize,
    /// Nothing to do.
    Stay,
}

/// The zone for a drag whose pointer is at `(px, py)` on a `sw x sh` screen, or `None` when it is
/// away from every edge. The top edge maximises (its first and last [`CORNER`] pixels are the top
/// quarters), the left and right edges give halves (their first and last [`CORNER`] pixels, the
/// quarters), the bottom edge gives the bottom quarters only (the middle of it belongs to the
/// taskbar).
pub fn zone_at(px: i32, py: i32, sw: i32, sh: i32) -> Option<SnapZone> {
    let left = px < EDGE;
    let right = px >= sw - EDGE;
    let top = py < EDGE;
    let bottom = py >= sh - EDGE;
    if !(left || right || top || bottom) {
        return None;
    }
    let near_left = px < CORNER;
    let near_right = px >= sw - CORNER;
    let near_top = py < CORNER;
    let near_bottom = py >= sh - CORNER;
    if top {
        return Some(if near_left {
            SnapZone::TopLeft
        } else if near_right {
            SnapZone::TopRight
        } else {
            SnapZone::Maximize
        });
    }
    if bottom {
        return if near_left {
            Some(SnapZone::BottomLeft)
        } else if near_right {
            Some(SnapZone::BottomRight)
        } else {
            None
        };
    }
    // Left or right edge, away from the top and bottom rows.
    Some(match (left, near_top, near_bottom) {
        (true, true, _) => SnapZone::TopLeft,
        (true, _, true) => SnapZone::BottomLeft,
        (true, _, _) => SnapZone::Left,
        (false, true, _) => SnapZone::TopRight,
        (false, _, true) => SnapZone::BottomRight,
        (false, _, _) => SnapZone::Right,
    })
}

/// The rectangle `zone` takes inside `work`. Halves and quarters split the area exactly: the
/// right half and the bottom quarters take the odd pixel.
pub fn zone_rect(zone: SnapZone, work: Rect) -> Rect {
    let hw = work.w / 2;
    let hh = work.h / 2;
    let (rx, rw) = (work.x + hw, work.w - hw);
    let (by, bh) = (work.y + hh, work.h - hh);
    match zone {
        SnapZone::Maximize => work,
        SnapZone::Left => Rect::new(work.x, work.y, hw, work.h),
        SnapZone::Right => Rect::new(rx, work.y, rw, work.h),
        SnapZone::TopLeft => Rect::new(work.x, work.y, hw, hh),
        SnapZone::TopRight => Rect::new(rx, work.y, rw, hh),
        SnapZone::BottomLeft => Rect::new(work.x, by, hw, bh),
        SnapZone::BottomRight => Rect::new(rx, by, rw, bh),
    }
}

/// `zone_rect` grown to at least `min_w x min_h`, keeping the window against the side of the work
/// area its zone belongs to and inside it (a window with a large minimum size cannot be tiled
/// smaller than that).
pub fn zone_rect_min(zone: SnapZone, work: Rect, min_w: i32, min_h: i32) -> Rect {
    let r = zone_rect(zone, work);
    let w = r.w.max(min_w).min(work.w);
    let h = r.h.max(min_h).min(work.h);
    let x = match zone {
        SnapZone::Right | SnapZone::TopRight | SnapZone::BottomRight => work.right() - w,
        _ => r.x,
    };
    let y = match zone {
        SnapZone::BottomLeft | SnapZone::BottomRight => work.bottom() - h,
        _ => r.y,
    };
    Rect::new(x, y, w, h)
}

/// What `Alt+arrow` does for a window that is `current` (`None` = free, `Some(Maximize)` =
/// maximised, otherwise snapped). The moves follow the Windows 11 ones: sideways toggles between
/// the halves and the free state, up goes towards maximised, down towards free and then minimised.
pub fn key_action(current: Option<SnapZone>, key: Arrow) -> SnapAct {
    use SnapAct::{Minimize, Restore, Stay, Zone};
    use SnapZone as Z;
    match (current, key) {
        // Free window.
        (None, Arrow::Left) => Zone(Z::Left),
        (None, Arrow::Right) => Zone(Z::Right),
        (None, Arrow::Up) => Zone(Z::Maximize),
        (None, Arrow::Down) => Minimize,
        // Maximised.
        (Some(Z::Maximize), Arrow::Left) => Zone(Z::Left),
        (Some(Z::Maximize), Arrow::Right) => Zone(Z::Right),
        (Some(Z::Maximize), Arrow::Up) => Stay,
        (Some(Z::Maximize), Arrow::Down) => Restore,
        // Halves.
        (Some(Z::Left), Arrow::Left) => Stay,
        (Some(Z::Left), Arrow::Right) => Restore,
        (Some(Z::Left), Arrow::Up) => Zone(Z::TopLeft),
        (Some(Z::Left), Arrow::Down) => Zone(Z::BottomLeft),
        (Some(Z::Right), Arrow::Left) => Restore,
        (Some(Z::Right), Arrow::Right) => Stay,
        (Some(Z::Right), Arrow::Up) => Zone(Z::TopRight),
        (Some(Z::Right), Arrow::Down) => Zone(Z::BottomRight),
        // Top quarters.
        (Some(Z::TopLeft), Arrow::Left) => Stay,
        (Some(Z::TopLeft), Arrow::Right) => Zone(Z::TopRight),
        (Some(Z::TopLeft), Arrow::Up) => Zone(Z::Maximize),
        (Some(Z::TopLeft), Arrow::Down) => Zone(Z::Left),
        (Some(Z::TopRight), Arrow::Left) => Zone(Z::TopLeft),
        (Some(Z::TopRight), Arrow::Right) => Stay,
        (Some(Z::TopRight), Arrow::Up) => Zone(Z::Maximize),
        (Some(Z::TopRight), Arrow::Down) => Zone(Z::Right),
        // Bottom quarters.
        (Some(Z::BottomLeft), Arrow::Left) => Stay,
        (Some(Z::BottomLeft), Arrow::Right) => Zone(Z::BottomRight),
        (Some(Z::BottomLeft), Arrow::Up) => Zone(Z::Left),
        (Some(Z::BottomLeft), Arrow::Down) => Restore,
        (Some(Z::BottomRight), Arrow::Left) => Zone(Z::BottomLeft),
        (Some(Z::BottomRight), Arrow::Right) => Stay,
        (Some(Z::BottomRight), Arrow::Up) => Zone(Z::Right),
        (Some(Z::BottomRight), Arrow::Down) => Restore,
    }
}

/// Linear blend of two rectangles, `t` in `0..=256` (0 = `a`, 256 = `b`): the snap preview
/// travels from the dragged window to the zone this way.
pub fn lerp_rect(a: Rect, b: Rect, t: i32) -> Rect {
    let t = t.clamp(0, 256);
    let l = |x: i32, y: i32| x + (y - x) * t / 256;
    Rect::new(l(a.x, b.x), l(a.y, b.y), l(a.w, b.w), l(a.h, b.h))
}

#[cfg(test)]
mod tests {
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
            let q = [Z::TopLeft, Z::TopRight, Z::BottomLeft, Z::BottomRight]
                .map(|z| zone_rect(z, work));
            let area: i64 = q.iter().map(|r| r.w as i64 * r.h as i64).sum();
            assert_eq!(area, work.w as i64 * work.h as i64);
            assert_eq!(q[0].right(), q[1].x);
            assert_eq!(q[0].bottom(), q[2].y);
            assert_eq!(q[3].right(), work.right());
            assert_eq!(q[3].bottom(), work.bottom());
            for r in q {
                assert!(
                    work.contains(r.x, r.y)
                        && r.right() <= work.right()
                        && r.bottom() <= work.bottom()
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
}
