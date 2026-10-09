//! The key property: for any history of events, the screen built from the engine's incremental
//! plans is byte-identical to a full redraw, after every frame.

use super::run_seed;
use crate::compositor::sim::{Op, Sim};

const SEEDS: u64 = 1500;
const FRAMES: usize = 60;

#[test]
fn incremental_equals_full_redraw_on_random_histories() {
    let mut failures = Vec::new();
    for seed in 0..SEEDS {
        if let Err(e) = run_seed(seed, FRAMES, |_| {}) {
            failures.push(e);
            if failures.len() >= 3 {
                break;
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failing seeds, first:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Long histories with many windows alive (the window cap is 24).
#[test]
fn long_histories_with_many_windows() {
    for seed in 10_000..10_040u64 {
        run_seed(seed, 400, |_| {}).unwrap();
    }
}

#[test]
fn an_idle_frame_after_any_history_plans_nothing() {
    for seed in 0..200u64 {
        let mut sim = Sim::new();
        let mut rng = crate::compositor::sim::SplitMix(seed);
        for _ in 0..30 {
            let op = crate::compositor::sim::random_op(&sim.world, &mut rng);
            sim.frame(&[op]).unwrap();
        }
        sim.frame(&[Op::Idle]).unwrap();
        assert!(
            sim.last_plan.is_empty(),
            "seed {seed}: an idle frame repainted {:?}",
            sim.last_plan.damage
        );
    }
}

/// Doing the same events one frame at a time or all in one frame gives the same screen.
#[test]
fn batching_events_does_not_change_the_result() {
    for seed in 0..300u64 {
        let mut rng = crate::compositor::sim::SplitMix(seed);
        let mut a = Sim::new();
        let mut b = Sim::new();
        let mut batch = Vec::new();
        for _ in 0..24 {
            let op = crate::compositor::sim::random_op(&a.world, &mut rng);
            // Keep the two worlds in step: ops are chosen from `a`, applied to both.
            a.frame(&[op]).unwrap();
            batch.push(op);
            if batch.len() == 4 {
                b.frame(&batch).unwrap();
                batch.clear();
                assert_eq!(a.reference(), b.reference());
            }
        }
    }
}
