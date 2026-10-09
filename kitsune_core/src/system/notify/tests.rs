use super::*;

const SW: i32 = 1280;
const SH: i32 = 720;

fn texts(t: &Toasts) -> alloc::vec::Vec<alloc::vec::Vec<u8>> {
    t.iter().map(|x| x.text().to_vec()).collect()
}

#[test]
fn starts_idle() {
    let t = Toasts::new();
    assert!(t.is_idle());
    assert_eq!(t.visible_count(), 0);
    assert!(t.bounds(SW, SH).is_empty());
}

#[test]
fn shows_up_to_three_and_queues_the_rest() {
    let mut t = Toasts::new();
    for (i, m) in [b"a", b"b", b"c", b"d", b"e"].iter().enumerate() {
        let shown = t.push(Level::Warn, *m, i as u32);
        assert_eq!(shown, i < 3, "{i}");
    }
    assert_eq!(texts(&t), [b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);
    assert!(!t.is_idle());
}

#[test]
fn expires_after_four_seconds_and_promotes_the_queue() {
    let mut t = Toasts::new();
    t.push(Level::Error, b"first", 0);
    t.push(Level::Warn, b"second", 1000);
    t.push(Level::Warn, b"third", 1000);
    t.push(Level::Warn, b"fourth", 1000);
    assert!(!t.tick(3999));
    assert_eq!(t.visible_count(), 3);
    // "first" expires; "fourth" takes the freed slot.
    assert!(t.tick(4000));
    assert_eq!(
        texts(&t),
        [b"second".to_vec(), b"third".to_vec(), b"fourth".to_vec()]
    );
    // The promoted one is timed from when it appeared.
    assert!(t.tick(5000)); // second + third (born 1000) expire
    assert_eq!(texts(&t), [b"fourth".to_vec()]);
    assert!(!t.tick(7999));
    assert!(t.tick(8000));
    assert!(t.is_idle());
    assert!(!t.tick(8001));
}

#[test]
fn repeats_do_not_stack() {
    let mut t = Toasts::new();
    t.push(Level::Warn, b"disk slow", 0);
    t.push(Level::Warn, b"disk slow", 3000);
    t.push(Level::Warn, b"disk slow", 3500);
    assert_eq!(t.visible_count(), 1);
    assert_eq!(t.iter().next().unwrap().count, 3);
    // The timer restarted at the last repeat.
    assert!(!t.tick(7000));
    assert!(t.tick(7500));
    // Same text, different level is a different toast.
    t.push(Level::Warn, b"x", 0);
    t.push(Level::Error, b"x", 0);
    assert_eq!(t.visible_count(), 2);
}

#[test]
fn queue_overflow_is_counted_and_never_panics() {
    let mut t = Toasts::new();
    for i in 0..100u32 {
        let msg = [b'a' + (i % 26) as u8, b'0' + (i / 26) as u8];
        t.push(Level::Warn, &msg, 0);
    }
    assert_eq!(t.visible_count(), MAX_VISIBLE);
    assert_eq!(t.dropped as usize, 100 - MAX_VISIBLE - QUEUE_CAP);
}

#[test]
fn click_dismisses_and_compacts() {
    let mut t = Toasts::new();
    t.push(Level::Warn, b"a", 0);
    t.push(Level::Warn, b"b", 0);
    t.push(Level::Warn, b"c", 0);
    t.push(Level::Warn, b"d", 0);
    let r0 = Toasts::rect(0, SW, SH);
    // Click the middle one (slot 1).
    let r1 = Toasts::rect(1, SW, SH);
    assert!(t.click(r1.x + 5, r1.y + 5, SW, SH, 100));
    assert_eq!(texts(&t), [b"a".to_vec(), b"c".to_vec(), b"d".to_vec()]);
    // A click elsewhere is not consumed.
    assert!(!t.click(5, 5, SW, SH, 100));
    // Slot 0 click.
    assert!(t.click(r0.x + 1, r0.y + 1, SW, SH, 100));
    assert_eq!(texts(&t), [b"c".to_vec(), b"d".to_vec()]);
}

#[test]
fn geometry_stacks_down_from_the_top_right() {
    let r0 = Toasts::rect(0, SW, SH);
    let r1 = Toasts::rect(1, SW, SH);
    assert_eq!(r0.right() + crate::ui::chrome::TOAST_MARGIN, SW);
    // Under the menu bar, the next one below with a gap.
    assert!(r0.y > crate::ui::style::MENUBAR_H);
    assert_eq!(r0.bottom() + GAP, r1.y);
    let mut t = Toasts::new();
    t.push(Level::Warn, b"a", 0);
    t.push(Level::Warn, b"b", 0);
    let b = t.bounds(SW, SH);
    assert!(b.contains(r0.x, r0.y) && b.contains(r1.x, r1.y));
    // The strip reaches the screen edge for the slide.
    assert_eq!(b.right(), SW);
}

#[test]
fn slide_comes_in_from_the_right_and_leaves_before_expiring() {
    let mut t = Toasts::new();
    t.push(Level::Info, b"hello", 1000);
    let toast = *t.iter().next().unwrap();
    // Starts fully outside, ends at rest, and only ever eases one way.
    assert_eq!(toast.slide(1000), 256);
    let mut last = 256;
    for ms in (0..=SLIDE_IN_MS).step_by(10) {
        let v = toast.slide(1000 + ms);
        assert!(v <= last);
        last = v;
    }
    assert_eq!(toast.slide(1000 + SLIDE_IN_MS), 0);
    assert_eq!(toast.slide(1000 + 2000), 0);
    // The exit starts SLIDE_OUT_MS before the end and grows.
    let mut last = 0;
    for ms in (LIFETIME_MS - SLIDE_OUT_MS..LIFETIME_MS).step_by(10) {
        let v = toast.slide(1000 + ms);
        assert!(v >= last);
        last = v;
    }
    assert!(last > 200);
    assert!(t.sliding(1000) && !t.sliding(2500) && t.sliding(1000 + LIFETIME_MS - 50));
}

#[test]
fn long_text_is_cut_and_timer_wraps() {
    let mut t = Toasts::new();
    t.push(Level::Info, &[b'x'; 200], u32::MAX - 100);
    assert_eq!(t.iter().next().unwrap().text().len(), TEXT_CAP);
    // The clock wraps past u32::MAX: still expires on time.
    assert!(!t.tick(u32::MAX));
    assert!(t.tick(LIFETIME_MS - 101));
}

#[test]
fn clear_empties_everything() {
    let mut t = Toasts::new();
    for _ in 0..3 {
        t.push(Level::Warn, b"a", 0);
    }
    t.push(Level::Error, b"queued", 0);
    t.clear();
    assert!(t.is_idle());
    assert!(!t.tick(100));
}

#[test]
fn lifetime_is_chosen_per_toast_and_clamped() {
    let mut t = Toasts::new();
    t.set_lifetime_secs(10);
    t.push(Level::Info, b"long", 0);
    t.set_lifetime_secs(2);
    t.push(Level::Info, b"short", 0);
    assert!(!t.tick(1999));
    // The short one is gone at 2 s, the long one at 10 s.
    assert!(t.tick(2000));
    assert_eq!(texts(&t), [b"long".to_vec()]);
    assert!(!t.tick(9999));
    assert!(t.tick(10_000));
    assert!(t.is_idle());
    // Out-of-range requests are clamped.
    let mut t = Toasts::new();
    t.set_lifetime_secs(0);
    t.push(Level::Info, b"a", 0);
    assert!(t.tick(2000));
    t.set_lifetime_secs(999);
    t.push(Level::Info, b"b", 0);
    assert!(!t.tick(14_999));
    assert!(t.tick(15_000));
}

#[test]
fn the_dismiss_line_runs_down_with_the_toast() {
    let mut t = Toasts::new();
    t.set_lifetime_secs(4);
    t.push(Level::Warn, b"x", 1000);
    let toast = *t.iter().next().unwrap();
    assert_eq!(toast.remaining(1000), 256);
    assert_eq!(toast.remaining(3000), 128);
    assert_eq!(toast.remaining(5000), 0);
    assert_eq!(toast.remaining(99_999), 0);
    // The slide-out follows the chosen lifetime too.
    t.set_lifetime_secs(10);
    t.push(Level::Warn, b"y", 0);
    let y = *t.iter().nth(1).unwrap();
    assert_eq!(y.slide(5000), 0);
    assert!(y.slide(10_000 - 50) > 0);
}
