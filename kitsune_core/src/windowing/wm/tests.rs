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
