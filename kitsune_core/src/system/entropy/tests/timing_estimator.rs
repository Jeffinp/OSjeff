use super::*;

#[test]
fn estimator_credits_nothing_for_a_perfectly_regular_source() {
    let mut e = TimingEstimator::new(jitter::IRQ_MILLIBITS);
    let mut ts = 1_000_000u64;
    for _ in 0..10_000 {
        ts += 4_000_000; // a 250 Hz timer on a 1 GHz TSC in a deterministic emulator
        assert_eq!(e.observe(ts), Verdict::Rejected);
    }
}

#[test]
fn estimator_rejects_stuck_and_alternating_sources() {
    let mut e = TimingEstimator::new(jitter::IRQ_MILLIBITS);
    for _ in 0..100 {
        assert_eq!(e.observe(42), Verdict::Rejected); // same timestamp every time
    }
    // Two alternating intervals: d2 is constant, d3 is zero.
    let mut e = TimingEstimator::new(jitter::IRQ_MILLIBITS);
    let mut ts = 0u64;
    let mut accepted = 0;
    for i in 0..1000 {
        ts += if i % 2 == 0 { 1000 } else { 1500 };
        if matches!(e.observe(ts), Verdict::Accepted(_)) {
            accepted += 1;
        }
    }
    assert_eq!(accepted, 0);
}

#[test]
fn estimator_rejects_a_small_quantization_wobble() {
    // Intervals jitter by +-1 cycle only: below MIN_VARIATION, so no credit.
    let mut e = TimingEstimator::new(jitter::IRQ_MILLIBITS);
    let mut x = Xs(7);
    let mut ts = 0u64;
    let mut accepted = 0;
    for _ in 0..2000 {
        ts += 10_000 + (x.next() & 1);
        if matches!(e.observe(ts), Verdict::Accepted(_)) {
            accepted += 1;
        }
    }
    assert_eq!(accepted, 0);
}

#[test]
fn estimator_credits_genuine_jitter_at_the_configured_rate() {
    let mut e = TimingEstimator::new(jitter::IRQ_MILLIBITS);
    let mut x = Xs(0x1234_5678_9abc_def1);
    let mut ts = 0u64;
    let mut mbits = 0u64;
    let n = 4000u32;
    for _ in 0..n {
        ts += 4_000_000 + (x.next() % 5_000);
        if let Verdict::Accepted(m) = e.observe(ts) {
            mbits += u64::from(m);
        }
    }
    // Nearly every event passes and each is worth at most 0.5 bit.
    assert!(mbits > u64::from(n) * 400, "{mbits}");
    assert!(mbits <= u64::from(n) * 500, "{mbits}");
}

#[test]
fn estimator_needs_history_before_it_credits() {
    let mut e = TimingEstimator::new(500);
    let mut x = Xs(99);
    let mut ts = 0;
    for i in 0..10 {
        ts += 1000 + (x.next() % 997);
        let v = e.observe(ts);
        if i < 3 {
            assert_eq!(v, Verdict::Rejected, "event {i}");
        }
    }
}

#[test]
fn estimator_flags_a_repeating_low_byte() {
    // d1 low byte constant at 0x10 but d2/d3 non-zero (high bytes move): the RCT trips.
    let mut e = TimingEstimator::new(500);
    let mut ts = 0u64;
    let mut late_accepted = 0;
    for i in 0..200u64 {
        ts += 0x1_0010 + ((i * i) << 8) % 0x3000 + 0x10_0000;
        // The first few events are judged before the run is long enough.
        if matches!(e.observe(ts), Verdict::Accepted(_)) && i >= 10 {
            late_accepted += 1;
        }
    }
    assert_eq!(late_accepted, 0);
}
