use super::*;

#[test]
fn the_bar_creeps_but_never_reaches_the_end_while_loading() {
    let mut b = LoadBar::new();
    assert_eq!(b.alpha(), 0);
    b.start();
    assert!(b.is_loading());
    assert!(b.alpha() > 0 && b.permille() > 0);
    let mut last = b.permille();
    for _ in 0..600 {
        assert!(b.step(0.016));
        assert!(b.permille() >= last);
        last = b.permille();
    }
    assert!(last > 800 && last <= 900, "{last}");
}

#[test]
fn finishing_runs_to_the_end_then_fades_and_goes_idle() {
    let mut b = LoadBar::new();
    b.start();
    for _ in 0..30 {
        b.step(0.016);
    }
    b.finish();
    assert!(!b.is_loading());
    let mut frames = 0;
    while b.step(0.016) {
        frames += 1;
        assert!(frames < 200, "never settles");
    }
    assert_eq!(b.alpha(), 0);
    assert_eq!(b.permille(), 0);
    // Idle really is idle.
    assert!(!b.step(0.016));
}

#[test]
fn a_finish_without_a_start_does_nothing() {
    let mut b = LoadBar::new();
    b.finish();
    assert!(!b.step(0.016));
    assert_eq!(b.alpha(), 0);
}

#[test]
fn a_new_load_restarts_a_fading_bar() {
    let mut b = LoadBar::new();
    b.start();
    b.step(0.5);
    b.finish();
    b.step(0.3);
    b.start();
    assert!(b.is_loading());
    assert_eq!(b.alpha(), 256);
    assert!(b.permille() <= 100);
}

#[test]
fn exp_approximation_is_close_enough() {
    for k in 0..40 {
        let x = k as f32 * 0.1;
        let real = {
            // Taylor to high order as the reference.
            let mut term = 1.0f32;
            let mut sum = 1.0f32;
            for n in 1..40 {
                term *= -x / n as f32;
                sum += term;
            }
            sum
        };
        let a = (-x).exp_approx();
        assert!((a - real).abs() < 0.05, "x={x} {a} vs {real}");
    }
}

#[test]
fn a_flash_holds_then_fades_then_stops() {
    let mut f = Flash::new();
    assert!(!f.active());
    assert_eq!(f.alpha(), 0);
    f.show(1.0);
    assert_eq!(f.alpha(), 256);
    assert!(f.step(0.5));
    assert_eq!(f.alpha(), 256);
    assert!(f.step(0.6)); // into the fade
    let mid = f.alpha();
    assert!(mid > 0 && mid < 256, "{mid}");
    let mut n = 0;
    while f.step(0.016) {
        n += 1;
        assert!(n < 100);
    }
    assert_eq!(f.alpha(), 0);
    assert!(!f.active());
    f.show(0.0);
    f.hide();
    assert!(!f.active());
}
