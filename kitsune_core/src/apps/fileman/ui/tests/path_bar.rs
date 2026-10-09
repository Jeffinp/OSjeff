use super::*;

#[test]
fn crumbs_fit_and_hit() {
    let bar = Rect::new(100, 50, 400, 28);
    let c = crumb_layout(bar, &[40, 70, 60]);
    assert_eq!(c.first, 0);
    assert!(c.fold.is_none());
    assert_eq!(c.spans.len(), 3);
    for pair in c.spans.windows(2) {
        assert_eq!(pair[1].1.x - pair[0].1.right(), CRUMB_GAP);
    }
    for (i, r) in &c.spans {
        assert!(r.x >= bar.x + PATH_PAD && r.right() <= bar.right() - PATH_PAD);
        assert_eq!(c.crumb_at(r.x + r.w / 2, r.y + 2), Some(*i));
    }
    assert_eq!(c.spans[0].1.w, 40 + 2 * CRUMB_PAD);
    assert_eq!(c.crumb_at(bar.right() - 2, bar.y + 4), None);
}

#[test]
fn leading_crumbs_fold_but_the_last_stays() {
    let bar = Rect::new(0, 0, 220, 28);
    let c = crumb_layout(bar, &[40, 90, 90, 90]);
    assert!(c.first > 0);
    let fold = c.fold.unwrap();
    assert!(c.fold_hit(fold.x + 3, fold.y + 3));
    assert_eq!(c.spans.last().unwrap().0, 3);
    assert!(c.spans.first().unwrap().1.x >= fold.right());
    assert!(
        c.spans
            .iter()
            .all(|(_, r)| r.right() <= bar.right() - PATH_PAD)
    );
    // A single crumb wider than the bar is cut to the room, never dropped.
    let c = crumb_layout(Rect::new(0, 0, 100, 28), &[40, 500]);
    assert_eq!(c.spans.last().unwrap().0, 1);
    assert!(c.spans.last().unwrap().1.w <= 100 - 2 * PATH_PAD);
    // No crumbs, no layout.
    assert!(crumb_layout(bar, &[]).spans.is_empty());
}

#[test]
fn folding_is_monotonic_in_the_width() {
    let widths = [40, 80, 60, 100, 70];
    let mut last_first = usize::MAX;
    for w in (60..900).step_by(20) {
        let c = crumb_layout(Rect::new(0, 0, w, 28), &widths);
        assert!(c.first <= last_first, "{w}");
        last_first = c.first;
        assert_eq!(c.spans.last().unwrap().0, widths.len() - 1);
    }
}
