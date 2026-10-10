use super::*;

#[test]
fn the_first_failures_are_free() {
    let mut g = LoginGuard::new();
    for i in 0..FREE_ATTEMPTS {
        assert_eq!(g.check("ana", i as u64), Ok(()));
        g.failed("ana", i as u64);
    }
    assert_eq!(g.failures("ana"), FREE_ATTEMPTS);
    assert_eq!(
        g.check("ana", 10),
        Ok(()),
        "still free after exactly the free attempts"
    );
}

#[test]
fn then_each_failure_doubles_the_wait() {
    let mut g = LoginGuard::new();
    let mut now = 0u64;
    for _ in 0..FREE_ATTEMPTS {
        g.failed("ana", now);
    }
    let mut want = BASE_DELAY_MS;
    for _ in 0..5 {
        g.failed("ana", now);
        assert_eq!(g.check("ana", now), Err(Throttled { retry_in_ms: want }));
        now += want;
        assert_eq!(g.check("ana", now), Ok(()));
        want *= 2;
    }
}

#[test]
fn the_wait_has_a_ceiling() {
    let mut g = LoginGuard::new();
    for _ in 0..60 {
        g.failed("ana", 0);
    }
    assert_eq!(
        g.check("ana", 0),
        Err(Throttled {
            retry_in_ms: MAX_DELAY_MS
        })
    );
}

#[test]
fn success_and_time_clear_the_record() {
    let mut g = LoginGuard::new();
    for _ in 0..6 {
        g.failed("ana", 0);
    }
    assert!(g.check("ana", 1).is_err());
    g.succeeded("ana");
    assert_eq!(g.failures("ana"), 0);
    assert_eq!(g.check("ana", 1), Ok(()));
    for _ in 0..6 {
        g.failed("bia", 0);
    }
    assert_eq!(
        g.check("bia", FORGET_AFTER_MS),
        Ok(()),
        "forgotten after a quiet spell"
    );
    assert_eq!(g.failures("bia"), 0);
}

#[test]
fn names_are_independent() {
    let mut g = LoginGuard::new();
    for _ in 0..8 {
        g.failed("ana", 0);
    }
    assert!(g.check("ana", 1).is_err());
    assert_eq!(g.check("bia", 1), Ok(()));
}

#[test]
fn the_table_stays_bounded_and_keeps_recent_offenders() {
    let mut g = LoginGuard::new();
    for i in 0..(MAX_TRACKED * 3) {
        g.failed(&alloc::format!("user{i}"), i as u64);
    }
    assert!(g.entries.len() <= MAX_TRACKED);
    let last = alloc::format!("user{}", MAX_TRACKED * 3 - 1);
    assert_eq!(g.failures(&last), 1);
    assert_eq!(g.failures("user0"), 0);
}

#[test]
fn idle_lock() {
    let mut l = IdleLock::new(60_000, 0);
    assert!(!l.due(59_999));
    assert!(l.due(60_000));
    l.touch(50_000);
    assert!(!l.due(100_000));
    assert!(l.due(110_000));
    l.set_timeout(0);
    assert!(!l.due(u64::MAX), "0 means never");
    assert!(!IdleLock::new(0, 5).due(1_000_000));
}

#[test]
fn a_clock_that_goes_back_does_not_lock_or_panic() {
    let l = IdleLock::new(10, 1_000);
    assert!(!l.due(5));
    let mut g = LoginGuard::new();
    g.failed("ana", 1_000);
    assert_eq!(g.check("ana", 0), Ok(()));
}
