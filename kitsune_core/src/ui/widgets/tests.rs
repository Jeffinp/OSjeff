use super::*;

#[test]
fn segments_tile_the_strip_exactly() {
    for n in 1..8usize {
        let r = Rect::new(10, 20, 203, 28);
        let segs = segmented_rects(r, n);
        assert_eq!(segs.len(), n);
        assert_eq!(segs[0].x, r.x + SEG_PAD);
        assert_eq!(segs.last().unwrap().right(), r.right() - SEG_PAD);
        for w in segs.windows(2) {
            assert_eq!(w[0].right(), w[1].x);
        }
        assert!(segs.iter().all(|s| s.h == r.h - 2 * SEG_PAD));
    }
    assert!(segmented_rects(Rect::new(0, 0, 10, 10), 0).is_empty());
}

#[test]
fn segmented_hit_includes_the_padding() {
    let r = Rect::new(0, 0, 200, 28);
    assert_eq!(segmented_hit(r, 4, 1, 14), Some(0));
    assert_eq!(segmented_hit(r, 4, 199, 14), Some(3));
    assert_eq!(segmented_hit(r, 4, 100, 14), Some(2));
    assert_eq!(segmented_hit(r, 4, 100, 40), None);
    assert_eq!(segmented_hit(r, 4, -1, 14), None);
}

#[test]
fn switch_knob_travels_between_the_ends() {
    let r = switch_rect(100, 50);
    let off = switch_knob(r, 0);
    let on = switch_knob(r, 256);
    assert_eq!(off.x, r.x + 2);
    assert_eq!(on.right(), r.right() - 2);
    assert_eq!((off.w, off.h), (SWITCH_H - 4, SWITCH_H - 4));
    let mid = switch_knob(r, 128);
    assert!(mid.x > off.x && mid.x < on.x);
    // Out-of-range t is clamped.
    assert_eq!(switch_knob(r, -5), off);
    assert_eq!(switch_knob(r, 999), on);
}

#[test]
fn slider_value_and_knob_are_inverse() {
    let r = Rect::new(20, 100, 220, 24);
    for v in [0, 1, 25, 50, 99, 100] {
        let x = slider_knob_x(r, v, 0, 100);
        let back = slider_value(r, x, 0, 100);
        assert!((back - v).abs() <= 1, "{v} -> {x} -> {back}");
    }
    // Dragging beyond the ends clamps.
    assert_eq!(slider_value(r, -500, 0, 100), 0);
    assert_eq!(slider_value(r, 5000, 0, 100), 100);
    assert!(slider_value(r, 100, -50, 50) < 0);
}

#[test]
fn scrollbar_fades_after_the_hold() {
    let mut s = ScrollbarFade::new();
    assert_eq!(s.alpha(10_000), 0);
    assert!(!s.active(10_000));
    s.touch(1000);
    assert_eq!(s.alpha(1000), 256);
    assert_eq!(s.alpha(1000 + SCROLLBAR_HOLD_MS), 256);
    let mid = s.alpha(1000 + SCROLLBAR_HOLD_MS + SCROLLBAR_FADE_MS / 2);
    assert!(mid > 100 && mid < 156, "{mid}");
    assert_eq!(s.alpha(1000 + SCROLLBAR_HOLD_MS + SCROLLBAR_FADE_MS), 0);
    assert!(!s.active(5000));
    s.touch(5000);
    assert!(s.active(5100));
    // The tick counter wrapping does not hide the bar.
    let mut w = ScrollbarFade::new();
    w.touch(u32::MAX - 10);
    assert_eq!(w.alpha(5), 256);
}

#[test]
fn thumb_follows_the_scroll_position() {
    assert_eq!(scroll_thumb(100, 10, 10, 0, 16), (0, 100)); // everything fits
    let (o0, l0) = scroll_thumb(100, 100, 10, 0, 16);
    assert_eq!((o0, l0), (0, 16)); // min length
    let (o1, l1) = scroll_thumb(100, 100, 10, 90, 16);
    assert_eq!(o1 + l1, 100);
    let (om, _) = scroll_thumb(100, 100, 10, 45, 16);
    assert!(om > 30 && om < 60);
    // Out-of-range top is clamped.
    assert_eq!(scroll_thumb(100, 100, 10, 500, 16), (o1, l1));
    assert_eq!(scroll_thumb(0, 100, 10, 5, 16), (0, 0));
}
