//! Fuzz target: the compositor's damage engine (`kitsune_core::compositor`).
//!
//! The input is a script of desktop operations (open, close, move, resize, focus, minimise,
//! restore, snap, maximise, workspace switches, popovers, toasts, animation ticks, live windows
//! that change by themselves, partial edits, clock ticks, idle frames), one to four per frame, run on
//! the simulated desktop. After every frame the screen built from the engine's incremental
//! plans must be byte-identical to a full redraw, the painter must never write outside the
//! footprint it declared, and a frame without changes must plan nothing.
#![no_main]

use libfuzzer_sys::fuzz_target;
use kitsune_core::compositor::sim::{Bytes, Op, Sim, Source, random_op};

const MAX_FRAMES: usize = 400;

fuzz_target!(|data: &[u8]| {
    let mut src = Bytes::new(data);
    let mut sim = Sim::new();
    for frame in 0..MAX_FRAMES {
        if src.exhausted() {
            break;
        }
        let k = 1 + src.below(4) as usize;
        let ops: Vec<Op> = (0..k).map(|_| random_op(&sim.world, &mut src)).collect();
        if let Err(e) = sim.frame(&ops) {
            panic!("incremental != full after {ops:?}: {e}\nhistory: {:?}", sim.history);
        }
        // (the first frame paints everything)
        if frame > 0 && ops.iter().all(|o| *o == Op::Idle) {
            assert!(sim.last_plan.is_empty(), "an idle frame planned work");
        }
    }
});
