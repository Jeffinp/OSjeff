//! Window-manager rules over a z-order list: focus, raise, hit-testing, process
//! lookup and the scene signature used to invalidate the cached static layer.
//!
//! The kernel owns the window records (rect, visibility, animation); these
//! functions only need to *ask* about them, so they take closures and stay pure.
//! `order` is always back-to-front: the last entry is the topmost window.

use crate::window::Rect;

/// The topmost window for which `active` holds (the one that has focus).
pub fn focused(order: &[usize], active: impl Fn(usize) -> bool) -> Option<usize> {
    order.iter().rev().copied().find(|&w| active(w))
}

/// Moves `win` to the end (front) of `order`, keeping everyone else's relative
/// order. Returns `false` and leaves `order` untouched if `win` is not in it.
pub fn bring_to_front(order: &mut [usize], win: usize) -> bool {
    let Some(pos) = order.iter().position(|&w| w == win) else {
        return false;
    };
    order[pos..].rotate_left(1);
    true
}

/// The frontmost window whose *active* rect contains `(px, py)`. `rect_of`
/// returns a window's rect only while it can take clicks (shown and not
/// animating out).
pub fn topmost_at(
    order: &[usize],
    px: i32,
    py: i32,
    rect_of: impl Fn(usize) -> Option<Rect>,
) -> Option<usize> {
    order
        .iter()
        .rev()
        .copied()
        .find(|&w| rect_of(w).is_some_and(|r| r.contains(px, py)))
}

/// Index of the window whose live process id is `pid`. Pid 0 means "no
/// process" and never matches.
pub fn window_of_pid(pids: &[u16], pid: u16) -> Option<usize> {
    if pid == 0 {
        return None;
    }
    pids.iter().position(|&p| p == pid)
}

/// Compact signature of the *static* scene: which windows are visible /
/// animating, their z-order and the drag target. Two scenes that differ in any
/// of those produce different signatures (up to 64-bit hash collisions), for
/// any number of windows. The kernel rebuilds its cached static layer when the
/// value changes.
///
/// The window count is `order.len()`; `visible` / `animating` are queried for
/// every window index in `0..order.len()`.
pub fn scene_signature(
    order: &[usize],
    visible: impl Fn(usize) -> bool,
    animating: impl Fn(usize) -> bool,
    drag: Option<usize>,
) -> u64 {
    // FNV-1a over a fixed-layout byte stream: flags per window, z-order, drag.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |b: u8| {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for w in 0..order.len() {
        feed((visible(w) as u8) | ((animating(w) as u8) << 1));
    }
    for &w in order {
        for b in (w as u32).to_le_bytes() {
            feed(b);
        }
    }
    // +1 so "no drag" and "dragging window 0" differ.
    for b in drag.map_or(0u32, |d| d as u32 + 1).to_le_bytes() {
        feed(b);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_is_the_topmost_active_window() {
        let order = [3, 1, 4, 0, 2];
        assert_eq!(focused(&order, |_| true), Some(2));
        assert_eq!(focused(&order, |w| w != 2), Some(0));
        assert_eq!(focused(&order, |w| w == 3), Some(3));
        assert_eq!(focused(&order, |_| false), None);
        assert_eq!(focused(&[], |_| true), None);
    }

    #[test]
    fn bring_to_front_rotates_only_the_tail() {
        let mut o = [0, 1, 2, 3, 4];
        assert!(bring_to_front(&mut o, 1));
        assert_eq!(o, [0, 2, 3, 4, 1]);
        assert!(bring_to_front(&mut o, 0));
        assert_eq!(o, [2, 3, 4, 1, 0]);
    }

    #[test]
    fn bring_to_front_of_topmost_is_a_noop() {
        let mut o = [0, 1, 2];
        assert!(bring_to_front(&mut o, 2));
        assert_eq!(o, [0, 1, 2]);
    }

    #[test]
    fn bring_to_front_of_unknown_window_does_not_corrupt_order() {
        // The kernel used `position(..).unwrap_or(0)`: an absent window shifted
        // everything left and duplicated the id at the front.
        let mut o = [0, 1, 2];
        assert!(!bring_to_front(&mut o, 9));
        assert_eq!(o, [0, 1, 2]);
        let mut empty: [usize; 0] = [];
        assert!(!bring_to_front(&mut empty, 0));
    }

    #[test]
    fn bring_to_front_keeps_a_permutation() {
        let mut o = [6, 5, 4, 3, 2, 1, 0];
        for w in [3, 3, 0, 6, 2, 5, 1, 4] {
            assert!(bring_to_front(&mut o, w));
            assert_eq!(*o.last().unwrap(), w);
            let mut s = o;
            s.sort_unstable();
            assert_eq!(s, [0, 1, 2, 3, 4, 5, 6]);
        }
    }

    #[test]
    fn topmost_at_prefers_the_front_window() {
        let rects = [
            Some(Rect::new(0, 0, 100, 100)),
            Some(Rect::new(50, 50, 100, 100)),
            None,
        ];
        let rect_of = |w: usize| rects[w];
        // Window 1 is in front of 0 where they overlap.
        assert_eq!(topmost_at(&[2, 0, 1], 60, 60, rect_of), Some(1));
        assert_eq!(topmost_at(&[2, 1, 0], 60, 60, rect_of), Some(0));
        // Only window 0 covers (10, 10); window 1 only (140, 140).
        assert_eq!(topmost_at(&[2, 0, 1], 10, 10, rect_of), Some(0));
        assert_eq!(topmost_at(&[2, 0, 1], 140, 140, rect_of), Some(1));
        assert_eq!(topmost_at(&[2, 0, 1], 500, 500, rect_of), None);
    }

    #[test]
    fn topmost_at_skips_inactive_windows() {
        // Window 1 would win by z-order but is inactive (rect_of -> None).
        let rect_of = |w: usize| (w == 0).then_some(Rect::new(0, 0, 100, 100));
        assert_eq!(topmost_at(&[0, 1], 10, 10, rect_of), Some(0));
    }

    #[test]
    fn topmost_at_uses_half_open_edges() {
        let rect_of = |_w: usize| Some(Rect::new(10, 10, 20, 20));
        assert_eq!(topmost_at(&[0], 10, 10, rect_of), Some(0));
        assert_eq!(topmost_at(&[0], 29, 29, rect_of), Some(0));
        assert_eq!(topmost_at(&[0], 30, 29, rect_of), None);
        assert_eq!(topmost_at(&[0], 9, 10, rect_of), None);
    }

    #[test]
    fn window_of_pid_ignores_zero() {
        let pids = [0u16, 5, 0, 9];
        assert_eq!(window_of_pid(&pids, 5), Some(1));
        assert_eq!(window_of_pid(&pids, 9), Some(3));
        assert_eq!(window_of_pid(&pids, 0), None); // closed windows have pid 0
        assert_eq!(window_of_pid(&pids, 7), None);
        assert_eq!(window_of_pid(&[], 1), None);
    }

    fn sig(order: &[usize], vis: u64, anim: u64, drag: Option<usize>) -> u64 {
        scene_signature(
            order,
            |w| (vis >> w) & 1 == 1,
            |w| (anim >> w) & 1 == 1,
            drag,
        )
    }

    #[test]
    fn signature_is_deterministic() {
        let o = [6, 5, 4, 3, 2, 1, 0];
        assert_eq!(sig(&o, 0b1, 0, None), sig(&o, 0b1, 0, None));
    }

    #[test]
    fn signature_changes_with_each_ingredient() {
        let o = [6, 5, 4, 3, 2, 1, 0];
        let base = sig(&o, 0b0000011, 0, None);
        assert_ne!(base, sig(&o, 0b0000111, 0, None)); // visibility
        assert_ne!(base, sig(&o, 0b0000011, 0b1, None)); // animation
        let mut o2 = o;
        o2.swap(0, 1);
        assert_ne!(base, sig(&o2, 0b0000011, 0, None)); // z-order
        assert_ne!(base, sig(&o, 0b0000011, 0, Some(0))); // drag begins
        assert_ne!(sig(&o, 0b11, 0, Some(0)), sig(&o, 0b11, 0, Some(1)));
        assert_ne!(sig(&o, 0b11, 0, None), sig(&o, 0b11, 0, Some(0)));
    }

    #[test]
    fn visible_and_animating_flags_are_distinct() {
        let o = [0, 1];
        // visible-only on window 0 vs animating-only on window 0.
        assert_ne!(sig(&o, 0b1, 0, None), sig(&o, 0, 0b1, None));
    }

    /// The pre-fix encoding: 3 bits per z-order slot and per drag target.
    fn legacy_signature(order: &[usize], drag: Option<usize>) -> u64 {
        let n = order.len();
        let (zbase, dbase) = (2 * n, 5 * n);
        let mut s = 0u64;
        for (i, &w) in order.iter().enumerate() {
            s |= (w as u64 & 0x7) << (zbase + i * 3);
        }
        if let Some(d) = drag {
            s |= ((d as u64 & 0x7) + 1) << dbase;
        }
        s
    }

    #[test]
    fn regression_nine_or_more_windows_no_longer_collide() {
        // With 9 windows, ids 0 and 8 share their low 3 bits, so swapping their
        // z-order positions left the old signature unchanged and the cached
        // static layer went stale.
        let a: Vec<usize> = (0..9).collect();
        let mut b = a.clone();
        b.swap(0, 8);
        assert_eq!(legacy_signature(&a, None), legacy_signature(&b, None));
        assert_ne!(
            scene_signature(&a, |_| true, |_| false, None),
            scene_signature(&b, |_| true, |_| false, None)
        );
        // Dragging window 8 vs window 0 was also indistinguishable.
        assert_eq!(legacy_signature(&a, Some(0)), legacy_signature(&a, Some(8)));
        assert_ne!(
            scene_signature(&a, |_| true, |_| false, Some(0)),
            scene_signature(&a, |_| true, |_| false, Some(8))
        );
    }

    #[test]
    fn signature_distinguishes_every_swap_with_many_windows() {
        let n = 20;
        let base: Vec<usize> = (0..n).collect();
        let s0 = scene_signature(&base, |_| true, |_| false, None);
        for i in 0..n {
            for j in (i + 1)..n {
                let mut o = base.clone();
                o.swap(i, j);
                assert_ne!(
                    s0,
                    scene_signature(&o, |_| true, |_| false, None),
                    "{i}<->{j}"
                );
            }
        }
    }

    #[test]
    fn signature_with_zero_windows_is_stable() {
        assert_eq!(
            scene_signature(&[], |_| true, |_| true, None),
            scene_signature(&[], |_| false, |_| false, None)
        );
    }
}
