//! A property test that cannot fail proves nothing: break the engine, or the owner of the scene,
//! on purpose in each way a real bug could and demand that the differential test notices.

use super::run_seed;
use crate::windowing::compositor::Policy;
use crate::windowing::compositor::sim::Sim;

/// The differential test fails on at least one of `seeds` seeds with `tweak` applied.
fn caught(tweak: impl Fn(&mut Sim) + Copy) -> bool {
    (0..400u64).any(|seed| run_seed(seed, 60, tweak).is_err())
}

#[test]
fn the_unbroken_engine_passes_the_same_seeds() {
    for seed in 0..400u64 {
        run_seed(seed, 60, |_| {}).unwrap();
    }
}

#[test]
fn forgetting_the_old_footprint_of_a_moved_layer_is_caught() {
    assert!(caught(|s| {
        s.engine.policy = Policy {
            damage_old_footprint: false,
            ..Policy::CORRECT
        }
    }));
}

#[test]
fn forgetting_z_order_swaps_is_caught() {
    assert!(caught(|s| {
        s.engine.policy = Policy {
            damage_z_flips: false,
            ..Policy::CORRECT
        }
    }));
}

#[test]
fn forgetting_what_a_vanished_layer_covered_is_caught() {
    assert!(caught(|s| {
        s.engine.policy = Policy {
            damage_removed: false,
            ..Policy::CORRECT
        }
    }));
}

#[test]
fn ignoring_a_new_look_is_caught() {
    assert!(caught(|s| {
        s.engine.policy = Policy {
            damage_look: false,
            ..Policy::CORRECT
        }
    }));
}

#[test]
fn ignoring_dirty_rectangles_is_caught() {
    assert!(caught(|s| {
        s.engine.policy = Policy {
            damage_dirty: false,
            ..Policy::CORRECT
        }
    }));
}

#[test]
fn omitting_the_shadow_from_the_footprint_is_caught() {
    assert!(caught(|s| s.faults.footprint_without_shadow = true));
}

#[test]
fn declaring_rounded_corners_opaque_is_caught() {
    assert!(caught(|s| s.faults.opaque_whole_rect = true));
}

#[test]
fn forgetting_to_report_a_partial_update_is_caught() {
    assert!(caught(|s| s.faults.forget_dirty = true));
}

#[test]
fn forgetting_to_bump_the_look_is_caught() {
    assert!(caught(|s| s.faults.forget_look = true));
}

#[test]
fn forgetting_to_invalidate_the_clock_is_caught() {
    assert!(caught(|s| s.faults.forget_invalidate = true));
}
