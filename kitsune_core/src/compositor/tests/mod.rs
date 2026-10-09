//! Tests of the compositor core: regions, the engine on hand-made scenes, and the differential
//! tests (incremental plans against full redraws) on the simulated desktop.

mod differential;
mod engine;
mod mutants;
mod region;
mod scenarios;

use crate::compositor::sim::{Op, Sim, Source, SplitMix, random_op};

/// Run `frames` frames of seeded random operations (1 to 3 per frame, like several events landing
/// between two frames) and verify every frame. The error names the seed and the operations of the
/// failing frame.
pub(crate) fn run_seed(seed: u64, frames: usize, tweak: impl Fn(&mut Sim)) -> Result<(), String> {
    let mut rng = SplitMix(seed);
    let mut sim = Sim::new();
    tweak(&mut sim);
    for f in 0..frames {
        let k = 1 + rng.below(3) as usize;
        let mut ops: Vec<Op> = Vec::new();
        for _ in 0..k {
            let op = random_op(&sim.world, &mut rng);
            ops.push(op);
        }
        if let Err(e) = sim.frame(&ops) {
            return Err(format!("seed {seed}, frame {f}, ops {ops:?}: {e}"));
        }
    }
    Ok(())
}
