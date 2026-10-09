use super::*;

#[test]
fn the_scroller_glides_to_its_target_and_stops() {
    let mut s = Scroller::new();
    s.set_max(1000);
    assert_eq!((s.pos(), s.target()), (0, 0));
    s.scroll_by(WHEEL_STEP);
    assert_eq!(s.target(), WHEEL_STEP);
    // It starts moving at once but has not arrived.
    s.step(0.016);
    assert!(s.pos() > 0 && s.pos() < WHEEL_STEP, "{}", s.pos());
    let mut frames = 0;
    while s.step(0.016) {
        frames += 1;
        assert!(frames < 120, "never settles");
    }
    assert_eq!(s.pos(), WHEEL_STEP);
    assert!(s.at_rest());
    // Monotonic: no overshoot past the target for a critically damped spring.
    let mut s = Scroller::new();
    s.set_max(1000);
    s.scroll_by(300);
    let mut prev = 0;
    for _ in 0..200 {
        s.step(0.008);
        assert!(
            s.pos() >= prev && s.pos() <= 300,
            "{} after {prev}",
            s.pos()
        );
        prev = s.pos();
    }
}

#[test]
fn the_scroller_clamps_to_its_range() {
    let mut s = Scroller::new();
    s.set_max(100);
    s.scroll_by(-500);
    assert_eq!(s.target(), 0);
    s.scroll_by(5000);
    assert_eq!(s.target(), 100);
    s.jump(60);
    assert_eq!((s.pos(), s.target()), (60, 60));
    // The content shrank: position and target follow the new range.
    s.set_max(20);
    assert_eq!(s.target(), 20);
    s.step(0.0);
    assert!(s.pos() <= 20);
    s.set_max(-5);
    assert_eq!(s.max(), 0);
    s.scroll_to(50);
    assert_eq!(s.target(), 0);
}

#[test]
fn reduce_motion_lands_at_once() {
    crate::ui::anim::set_reduce_motion(true);
    let mut s = Scroller::new();
    s.set_max(500);
    s.scroll_by(200);
    s.step(0.001);
    assert_eq!(s.pos(), 200);
    crate::ui::anim::set_reduce_motion(false);
}
