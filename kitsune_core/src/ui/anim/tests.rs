use super::*;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn bezier_endpoints_and_monotonic() {
    for c in [
        curves::ENTER,
        curves::EXIT,
        curves::SWOOP,
        curves::STANDARD,
        curves::LINEAR,
    ] {
        assert_eq!(c.ease(0.0), 0.0);
        assert_eq!(c.ease(1.0), 1.0);
        let mut prev = -1.0;
        let mut t = 0.0;
        while t <= 1.0 {
            let v = c.ease(t);
            assert!(v >= prev - 1e-3, "{c:?} t={t}");
            prev = v;
            t += 0.02;
        }
    }
    // Linear is the identity.
    for t in [0.1f32, 0.37, 0.8] {
        assert!(approx(curves::LINEAR.ease(t), t));
    }
    // ENTER decelerates: it is ahead of linear early on.
    assert!(curves::ENTER.ease(0.25) > 0.5);
    // EXIT accelerates: it is behind.
    assert!(curves::EXIT.ease(0.5) < 0.4);
}

#[test]
fn bezier_ease_clamps_out_of_range() {
    assert_eq!(curves::ENTER.ease(-1.0), 0.0);
    assert_eq!(curves::ENTER.ease(3.0), 1.0);
}

#[test]
fn spring_settles_on_the_target_without_endless_motion() {
    let mut s = Spring::new(0.0, 300.0, 30.0);
    s.set_target(1.0);
    let mut frames = 0;
    while s.step(1.0 / 60.0) {
        frames += 1;
        assert!(frames < 600, "never settles");
    }
    assert!(s.at_rest());
    assert_eq!(s.value(), 1.0);
    // At rest it costs nothing: step reports idle.
    assert!(!s.step(0.016));
}

#[test]
fn spring_result_does_not_depend_on_the_frame_rate() {
    let run = |dt: f32| {
        let mut s = Spring::new(0.0, 260.0, 26.0);
        s.set_target(100.0);
        let mut t = 0.0;
        while t < 0.15 {
            s.step(dt);
            t += dt;
        }
        s.value()
    };
    let (a, b, c) = (run(1.0 / 250.0), run(1.0 / 60.0), run(1.0 / 20.0));
    assert!((a - b).abs() < 3.0 && (a - c).abs() < 6.0, "{a} {b} {c}");
}

#[test]
fn spring_is_interruptible_and_keeps_velocity() {
    let mut s = Spring::new(0.0, 300.0, 30.0);
    s.set_target(100.0);
    for _ in 0..8 {
        s.step(0.01);
    }
    let (v, vel) = (s.value(), s.velocity());
    assert!(v > 0.0 && v < 100.0 && vel > 0.0);
    s.set_target(0.0); // change of mind
    assert!(approx(s.value(), v));
    assert!(approx(s.velocity(), vel));
    while s.step(0.01) {}
    assert_eq!(s.value(), 0.0);
}

#[test]
fn stiff_spring_with_huge_dt_stays_finite() {
    let mut s = Spring::new(0.0, 4000.0, 10.0);
    s.set_target(50.0);
    s.step(1.5);
    assert!(s.value().is_finite() && s.value().abs() < 1000.0);
}

#[test]
fn tween_runs_by_time_and_retargets_from_its_current_value() {
    let mut t = Tween::at(0.0);
    assert!(t.finished());
    t.retarget(10.0, 0.2, curves::LINEAR);
    assert!(t.step(0.1));
    assert!(approx(t.value(), 5.0));
    // Change of mind halfway: continues from 5 toward 0, not from 10.
    t.retarget(0.0, 0.2, curves::LINEAR);
    assert!(approx(t.value(), 5.0));
    t.step(0.1);
    assert!(approx(t.value(), 2.5));
    assert!(!t.step(1.0));
    assert_eq!(t.value(), 0.0);
    // Retargeting to the same goal while running does not restart it.
    let mut u = Tween::at(0.0);
    u.retarget(1.0, 1.0, curves::LINEAR);
    u.step(0.5);
    u.retarget(1.0, 1.0, curves::LINEAR);
    assert!(approx(u.value(), 0.5));
}

#[test]
fn open_starts_invisible_ends_visible() {
    let mut a = Anim::open();
    assert!(approx(a.visibility(), 0.0));
    assert!(!a.is_closing());
    a.step(1.0);
    assert!(a.finished());
    assert!(approx(a.visibility(), 1.0));
    assert!(approx(a.alpha(), 1.0));
}

#[test]
fn close_starts_visible_ends_gone() {
    let mut a = Anim::close();
    assert!(a.is_closing());
    assert_eq!(a.phase(), Phase::Closing);
    assert!(approx(a.visibility(), 1.0));
    a.step(1.0);
    assert!(a.finished());
    assert!(approx(a.visibility(), 0.0));
}

#[test]
fn step_clamps_at_the_end() {
    let mut a = Anim::open();
    a.step(0.7);
    a.step(0.7);
    assert!(a.finished());
    assert!(approx(a.visibility(), 1.0));
    assert!(approx(a.progress(), 1.0));
}

#[test]
fn durations_are_in_real_seconds() {
    let mut a = Anim::open();
    a.step(OPEN_SECS * 0.5);
    assert!(!a.finished());
    a.step(OPEN_SECS * 0.5);
    assert!(a.finished());
    let mut m = Anim::minimize();
    m.step(DOCK_SECS - 0.01);
    assert!(!m.finished());
}

#[test]
fn pop_frame_scales_about_the_centre() {
    let r = Rect::new(100, 100, 400, 300);
    let a = Anim::open();
    let f = a.frame(r, None);
    assert_eq!(f.alpha, 0);
    assert_eq!(f.rect.w, (400.0 * POP_SCALE + 0.5) as i32);
    // Centre preserved (within rounding).
    assert!((f.rect.x + f.rect.w / 2 - 300).abs() <= 1);
    assert!((f.rect.y + f.rect.h / 2 - 250).abs() <= 1);
    let mut done = Anim::open();
    done.step(1.0);
    assert_eq!(
        done.frame(r, None),
        Frame {
            rect: r,
            alpha: 256
        }
    );
    // Closing ends at the small, transparent state.
    let mut c = Anim::close();
    c.step(1.0);
    let f = c.frame(r, None);
    assert_eq!(f.alpha, 0);
    assert!(f.rect.w < r.w);
}

#[test]
fn a_workspace_slide_comes_from_the_side_of_the_change_and_leaves_the_other_way() {
    let r = Rect::new(100, 80, 600, 400);
    // Going right (+1): the incoming window starts to the right, the outgoing one ends left.
    let mut inn = Anim::slide_in(1);
    let start = inn.frame(r, None);
    assert!(start.rect.x > r.x && start.alpha < 16);
    inn.step(SLIDE_SECS);
    assert_eq!(inn.frame(r, None).rect, r);
    assert_eq!(inn.frame(r, None).alpha, 256);
    let mut out = Anim::slide_out(1);
    assert_eq!(out.frame(r, None).rect, r);
    out.step(SLIDE_SECS);
    let end = out.frame(r, None);
    assert!(end.rect.x < r.x && end.alpha == 0);
    // Same size all the way (a slide, not a scale), and left changes mirror right ones.
    assert_eq!((start.rect.w, start.rect.h), (r.w, r.h));
    let left = Anim::slide_in(-1).frame(r, None);
    assert_eq!(left.rect.x - r.x, -(start.rect.x - r.x));
}

#[test]
fn dock_frame_travels_to_the_icon() {
    let r = Rect::new(100, 100, 400, 300);
    let icon = Rect::new(600, 650, 48, 48);
    let mut m = Anim::minimize();
    let start = m.frame(r, Some(icon));
    assert_eq!(start.rect, r);
    m.step(DOCK_SECS * 0.5);
    let mid = m.frame(r, Some(icon));
    assert!(mid.rect.w < r.w && mid.rect.w > icon.w);
    assert!(mid.rect.y > r.y);
    m.step(1.0);
    let end = m.frame(r, Some(icon));
    assert_eq!(end.alpha, 0);
    assert!(end.rect.w <= icon.w + 1 && end.rect.h <= icon.h + 1);
    assert!((end.rect.x + end.rect.w / 2 - 624).abs() <= 1);
    // Restore is the mirror image: starts at the icon, ends at the window.
    let mut re = Anim::restore();
    let s = re.frame(r, Some(icon));
    assert!(s.rect.w <= icon.w + 1);
    re.step(1.0);
    assert_eq!(
        re.frame(r, Some(icon)),
        Frame {
            rect: r,
            alpha: 256
        }
    );
    // Without an icon it falls back to the pop.
    let mut m2 = Anim::minimize();
    m2.step(DOCK_SECS * 0.5);
    assert!(m2.frame(r, None).rect.w > 300);
}

#[test]
fn zoom_moves_between_rects_and_can_be_interrupted() {
    let a = Rect::new(100, 100, 400, 300);
    let b = Rect::new(0, 28, 1280, 600);
    let mut z = Zoom::new(a, b);
    assert_eq!(z.rect(), a);
    let mut n = 0;
    while z.step(1.0 / 60.0) {
        n += 1;
        assert!(n < 300);
        let r = z.rect();
        assert!(r.w >= 1 && r.h >= 1);
    }
    assert!(z.finished());
    assert_eq!(z.rect(), b);
    // Interrupt halfway: restarts from the visible rect.
    let mut z = Zoom::new(a, b);
    for _ in 0..6 {
        z.step(0.01);
    }
    let now = z.rect();
    z.retarget(a);
    assert_eq!(z.rect(), now);
    while z.step(0.01) {}
    assert_eq!(z.rect(), a);
}

#[test]
fn bounce_hops_and_stops() {
    let mut peak = 0.0f32;
    let mut t = 0.0;
    let mut was_active = true;
    while t < 1.2 {
        let (y, active) = bounce(t, 16.0);
        assert!((0.0..=16.001).contains(&y));
        peak = peak.max(y);
        was_active = active;
        t += 0.01;
    }
    assert!(peak > 14.0);
    assert!(!was_active);
    assert_eq!(bounce(5.0, 16.0), (0.0, false));
    // The second hop is lower than the first.
    let first = (0..32)
        .map(|i| bounce(i as f32 * 0.01, 16.0).0)
        .fold(0.0, f32::max);
    let second = (0..32)
        .map(|i| bounce(0.32 + i as f32 * 0.01, 16.0).0)
        .fold(0.0, f32::max);
    assert!(second < first);
}

#[test]
fn reduce_motion_lands_everything_on_its_end_value() {
    set_reduce_motion(true);
    let mut s = Spring::new(0.0, 100.0, 10.0);
    s.set_target(5.0);
    assert!(!s.step(0.001));
    assert_eq!(s.value(), 5.0);
    let mut t = Tween::at(0.0);
    t.retarget(9.0, 1.0, curves::ENTER);
    assert_eq!(t.value(), 9.0);
    let mut a = Anim::open();
    a.step(0.0);
    assert!(a.finished());
    let z = Zoom::new(Rect::new(0, 0, 10, 10), Rect::new(0, 0, 20, 20));
    assert!(z.finished());
    assert_eq!(bounce(0.1, 10.0), (0.0, false));
    set_reduce_motion(false);
    assert!(!reduce_motion());
}

#[test]
fn the_blink_is_eased_and_rests() {
    // Solid, fades out, dark, fades in, solid again, each blink the same.
    assert_eq!(caret_curve(0), 256);
    assert_eq!(caret_curve(419), 256);
    assert!(caret_curve(500) < 256 && caret_curve(500) > 0);
    assert_eq!(caret_curve(700), 0);
    assert!(caret_curve(1000) > 0 && caret_curve(1000) < 256);
    assert_eq!(caret_curve(BLINK_MS), caret_curve(0));
    // Monotonic on the way down and back up.
    let down: Vec<u32> = (420..580).map(caret_curve).collect();
    assert!(down.windows(2).all(|w| w[0] >= w[1]));
    let up: Vec<u32> = (900..1060).map(caret_curve).collect();
    assert!(up.windows(2).all(|w| w[0] <= w[1]));
    assert!(down.iter().chain(&up).all(|&a| a <= 256));
}

#[test]
fn a_caret_holds_after_input_and_rests_after_a_while() {
    assert_eq!(caret_alpha(None), 256);
    assert!(!caret_animating(None));
    assert_eq!(caret_alpha(Some(0)), 256);
    assert_eq!(caret_alpha(Some(BLINK_HOLD_MS - 1)), 256);
    assert_eq!(caret_alpha(Some(BLINK_HOLD_MS + 700)), 0);
    assert_eq!(caret_alpha(Some(BLINK_ACTIVE_MS)), 256);
    assert!(caret_animating(Some(BLINK_ACTIVE_MS - 1)));
    assert!(!caret_animating(Some(BLINK_ACTIVE_MS)));
}
