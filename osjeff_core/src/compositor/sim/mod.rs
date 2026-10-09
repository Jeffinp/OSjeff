//! A simulated desktop and a model painter, to prove the engine on thousands of random histories.
//!
//! [`Sim`] keeps a [`World`] (windows, overlays, animations), builds the engine's scene from it
//! and paints plans with the model painter into a small framebuffer. After every frame
//! [`Sim::verify`] repaints the same scene from scratch with the reference plan and demands that
//! the two buffers are identical, pixel for pixel. The module is compiled always (it is small) so
//! the fuzz target can use it; nothing in the kernel calls it.

mod model;
mod ops;
mod paint;

use super::{Engine, Painter, Plan, Scene};
use crate::compositor::LayerId;
use crate::window::Rect;
use alloc::string::String;
use alloc::vec::Vec;

pub use model::{Kind, WinView, World, ids};
pub use ops::{Bytes, Op, Source, SplitMix, apply, random_op};
pub use paint::{H, PANEL_H, SCREEN, W};

/// Bugs a test can switch on in the *owner* of the scene (the desktop's scene builder), to check
/// that the differential test would notice them.
#[derive(Clone, Copy, Default, Debug)]
pub struct Faults {
    /// Declare window footprints without their shadows.
    pub footprint_without_shadow: bool,
    /// Declare the whole window rectangle opaque (its rounded corners are not).
    pub opaque_whole_rect: bool,
    /// Forget to report a window's partial update.
    pub forget_dirty: bool,
    /// Forget to bump a window's look when its content changes.
    pub forget_look: bool,
    /// Forget to invalidate the clock.
    pub forget_invalidate: bool,
}

/// Paints the model world, checking every write against the declared footprint.
struct ModelPainter<'a> {
    world: &'a World,
    scene: &'a Scene,
    buf: &'a mut [u32],
    violations: &'a mut Vec<String>,
}

impl Painter for ModelPainter<'_> {
    fn paint(&mut self, layer: LayerId, clip: Rect) {
        let footprint = self
            .scene
            .layers
            .iter()
            .find(|l| l.id == layer)
            .map_or(SCREEN, |l| l.footprint);
        let mut c = paint::Canvas {
            buf: self.buf,
            footprint,
            clip,
            layer,
            violations: self.violations,
        };
        paint::paint_layer(&mut c, layer, self.world);
    }
}

/// One simulated machine: world, engine and the screen the incremental plans maintain.
pub struct Sim {
    pub world: World,
    pub engine: Engine,
    pub faults: Faults,
    screen: Vec<u32>,
    scene: Scene,
    violations: Vec<String>,
    /// Operations applied so far (for the failure report).
    pub history: Vec<Op>,
    /// Paint calls and painted pixels of the last frame (for the cost tests).
    pub last_plan: Plan,
}

impl Default for Sim {
    fn default() -> Self {
        Self::new()
    }
}

impl Sim {
    pub fn new() -> Sim {
        Sim {
            world: World::default(),
            engine: Engine::new(W, H),
            faults: Faults::default(),
            screen: alloc::vec![0; (W * H) as usize],
            scene: Scene::new(),
            violations: Vec::new(),
            history: Vec::new(),
            last_plan: Plan::default(),
        }
    }

    /// Apply one operation (no frame is drawn yet).
    pub fn apply(&mut self, op: Op) {
        self.history.push(op);
        if let Some(r) = apply(&mut self.world, op)
            && !self.faults.forget_invalidate
        {
            self.engine.invalidate(r);
        }
    }

    /// Draw a frame with the incremental plan.
    pub fn render(&mut self) {
        self.scene = self.world.scene(&self.faults);
        let plan = self.engine.plan(&self.scene);
        let mut p = ModelPainter {
            world: &self.world,
            scene: &self.scene,
            buf: &mut self.screen,
            violations: &mut self.violations,
        };
        plan.paint(&mut p);
        self.last_plan = plan;
    }

    /// The screen as the incremental plans left it.
    pub fn screen(&self) -> &[u32] {
        &self.screen
    }

    /// The scene of the last frame.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// The reference: the last scene painted from scratch, every layer in full.
    pub fn reference(&mut self) -> Vec<u32> {
        let mut buf = alloc::vec![0xDEAD_BEEF; (W * H) as usize];
        let plan = self.engine.full_plan(&self.scene);
        let mut p = ModelPainter {
            world: &self.world,
            scene: &self.scene,
            buf: &mut buf,
            violations: &mut self.violations,
        };
        plan.paint(&mut p);
        buf
    }

    /// Compare the incremental screen with the reference. `Err` describes the first difference
    /// (or a painter that wrote outside its footprint).
    pub fn verify(&mut self) -> Result<(), String> {
        let want = self.reference();
        if let Some(v) = self.violations.first() {
            return Err(String::from(v.as_str()));
        }
        let Some(i) = (0..want.len()).find(|&i| want[i] != self.screen[i]) else {
            return Ok(());
        };
        let (x, y) = ((i as i32) % W, (i as i32) / W);
        let diff = (0..want.len())
            .filter(|&j| want[j] != self.screen[j])
            .count();
        let layers: Vec<String> = self
            .scene
            .layers
            .iter()
            .filter(|l| l.footprint.contains(x, y))
            .map(|l| alloc::format!("{:?}", l.id.0))
            .collect();
        Err(alloc::format!(
            "{diff} pixels differ; first at ({x},{y}): incremental {:06X}, reference {:06X}; layers there (bottom to top): [{}]",
            self.screen[i],
            want[i],
            layers.join(",")
        ))
    }

    /// Apply `ops`, draw one frame and verify it.
    pub fn frame(&mut self, ops: &[Op]) -> Result<(), String> {
        for op in ops {
            self.apply(*op);
        }
        self.render();
        self.verify()
    }
}
