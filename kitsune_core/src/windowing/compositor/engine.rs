//! The damage engine: decides what to repaint, from nothing but scene descriptions.
//!
//! [`Engine::plan`] compares the scene it is given with the one it planned last time (what is in
//! the back buffer) and derives the **damage**: the pixels that may differ. Then it orders the
//! painting: every layer, bottom to top, is painted over the damage that falls in its footprint,
//! minus what an opaque layer above hides. The back buffer is reset (wallpaper painted) wherever
//! the damage is not hidden, so every repainted pixel is recomputed from the bottom of the stack:
//! a shadow, a translucent fill or a corner can never be applied twice, and a window never
//! "keeps" a shadow it no longer has.
//!
//! What counts as a change (anything else does not repaint):
//! - a layer appeared or vanished: its footprint;
//! - a layer's footprint or opaque area moved: the old and the new footprint;
//! - a layer's `look` differs: its footprint;
//! - a layer reports `dirty`: that rectangle;
//! - two layers swapped places: where their footprints overlap;
//! - the owner called [`Engine::invalidate`] or [`Engine::invalidate_all`].
//!
//! Special cases of "drag a window" (old + new rectangle), "a clock tick" (one invalidated
//! rectangle), "a chart moves" (`dirty`) are all just instances of the above: there is one code
//! path.

use super::region::Region;
use super::scene::{Layer, LayerId, Scene};
use crate::windowing::window::Rect;
use alloc::vec::Vec;

/// Switches that exist only so the tests can break the engine on purpose and check that the
/// differential test notices (a test that cannot fail proves nothing). All `true` in production.
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    /// A layer that moved or resized damages where it was as well as where it is.
    pub damage_old_footprint: bool,
    /// Two layers that swapped z-order damage their overlap.
    pub damage_z_flips: bool,
    /// A layer that vanished damages where it was.
    pub damage_removed: bool,
    /// A changed `look` damages the footprint.
    pub damage_look: bool,
    /// `Layer::dirty` damages its rectangle.
    pub damage_dirty: bool,
    /// Skip painting what an opaque layer above hides.
    pub cull_hidden: bool,
}

impl Policy {
    pub const CORRECT: Policy = Policy {
        damage_old_footprint: true,
        damage_z_flips: true,
        damage_removed: true,
        damage_look: true,
        damage_dirty: true,
        cull_hidden: true,
    };
}

impl Default for Policy {
    fn default() -> Self {
        Policy::CORRECT
    }
}

/// Paint `layer` into the back buffer limited to `clip` (see the contract on [`Layer`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Step {
    pub layer: LayerId,
    pub clip: Rect,
}

/// What to do for one frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// The pixels that were recomputed: upload exactly these.
    pub damage: Region,
    /// Paint calls, bottom to top (the wallpaper is [`LayerId::WALLPAPER`]).
    pub steps: Vec<Step>,
}

impl Plan {
    /// Nothing to repaint or upload.
    pub fn is_empty(&self) -> bool {
        self.damage.is_empty()
    }

    /// Run the steps on `painter`, in order.
    pub fn paint(&self, painter: &mut impl Painter) {
        for s in &self.steps {
            painter.paint(s.layer, s.clip);
        }
    }
}

/// The side that owns pixels (the kernel's framebuffer code, or the model painter of the tests).
pub trait Painter {
    fn paint(&mut self, layer: LayerId, clip: Rect);
}

/// What the previous plan left in the back buffer, per layer.
#[derive(Clone, Copy, Debug)]
struct Snap {
    id: LayerId,
    footprint: Rect,
    opaque: Rect,
    look: u64,
}

impl From<&Layer> for Snap {
    fn from(l: &Layer) -> Snap {
        Snap {
            id: l.id,
            footprint: l.footprint,
            opaque: l.opaque,
            look: l.look,
        }
    }
}

/// Past this many rectangles in one layer's visible region the exact (culled) region is dropped
/// for the plain damage-in-footprint one: many tiny paint calls cost more than the pixels saved.
const MAX_VISIBLE_RECTS: usize = 6;

/// Fewer paint calls for one layer: every pair of its rectangles whose bounding box lies entirely
/// inside the damage is painted as one. Painting a pixel of the damage that the layer did not
/// strictly need is harmless (the pixel was reset by the layers below, or an opaque layer above
/// overwrites it), and a paint call has a fixed cost (an app lays its window out) that can be
/// worth far more than a few extra pixels. Pixels outside the damage are never painted: they
/// already hold their final colour and would get a shadow or a fill twice.
fn merge_for_paint(rects: &[Rect], damage: &Region) -> Vec<Rect> {
    let mut v: Vec<Rect> = rects.to_vec();
    'again: loop {
        for i in 0..v.len() {
            for j in i + 1..v.len() {
                // The box of the pair, grown over every other rectangle it touches so the
                // rectangles stay disjoint (a pixel painted twice would get its shadow twice).
                let mut bb = v[i].union(&v[j]);
                let mut members = alloc::vec![i, j];
                while let Some(k) =
                    (0..v.len()).find(|k| !members.contains(k) && v[*k].intersection(&bb).is_some())
                {
                    bb = bb.union(&v[k]);
                    members.push(k);
                }
                if !damage.covers(&bb) {
                    continue;
                }
                members.sort_unstable_by(|a, b| b.cmp(a));
                for m in &members {
                    v.swap_remove(*m);
                }
                v.push(bb);
                continue 'again;
            }
        }
        return v;
    }
}

/// Plans repaints. One per screen.
pub struct Engine {
    screen: Rect,
    prev: Vec<Snap>,
    /// Damage queued by [`Engine::invalidate`] since the last plan.
    queued: Region,
    /// The next plan repaints everything (start, or [`Engine::invalidate_all`]).
    all: bool,
    pub policy: Policy,
}

impl Engine {
    /// An engine for a screen of `w x h` pixels. Nothing is on it yet: the first plan repaints
    /// everything.
    pub fn new(w: i32, h: i32) -> Engine {
        Engine {
            screen: Rect::new(0, 0, w, h),
            prev: Vec::new(),
            queued: Region::new(),
            all: true,
            policy: Policy::CORRECT,
        }
    }

    pub fn screen(&self) -> Rect {
        self.screen
    }

    /// The pixels of `r` may differ from what was painted (an area of content changed, a clock
    /// ticked): repaint them at the next plan.
    pub fn invalidate(&mut self, r: Rect) {
        if let Some(r) = r.intersection(&self.screen) {
            self.queued.add(r);
        }
    }

    /// Repaint the whole screen at the next plan (the wallpaper changed, a reference frame).
    pub fn invalidate_all(&mut self) {
        self.all = true;
    }

    /// Plan the repaint that turns the screen painted for the previous scene into `scene`.
    pub fn plan(&mut self, scene: &Scene) -> Plan {
        let mut damage = if self.all {
            Region::from_rect(self.screen)
        } else {
            self.damage_of(scene)
        };
        damage.add_region(&self.queued);
        damage.clip_to(&self.screen);
        self.queued.clear();
        self.all = false;
        self.prev.clear();
        self.prev.extend(scene.layers.iter().map(Snap::from));
        if damage.is_empty() {
            return Plan::default();
        }
        let steps = self.steps(scene, &damage);
        Plan { damage, steps }
    }

    /// Like [`Engine::plan`] but returns the reference painting ([`Engine::full_plan`]) of `scene`:
    /// the debug mode that recomposes everything from scratch every frame, while the engine keeps
    /// following the scene so leaving the mode needs no special care.
    pub fn plan_reference(&mut self, scene: &Scene) -> Plan {
        self.queued.clear();
        self.all = false;
        self.prev.clear();
        self.prev.extend(scene.layers.iter().map(Snap::from));
        self.full_plan(scene)
    }

    /// The reference painting of `scene`: every layer over the whole screen, nothing skipped and
    /// nothing trusted (not even the footprints: a layer that draws outside the one it declared
    /// shows up as a difference with the incremental plans). What those plans must be
    /// indistinguishable from.
    pub fn full_plan(&self, scene: &Scene) -> Plan {
        let mut steps = alloc::vec![Step {
            layer: LayerId::WALLPAPER,
            clip: self.screen,
        }];
        for l in &scene.layers {
            steps.push(Step {
                layer: l.id,
                clip: self.screen,
            });
        }
        Plan {
            damage: Region::from_rect(self.screen),
            steps,
        }
    }

    /// Damage implied by the difference between the previous scene and `scene`.
    fn damage_of(&self, scene: &Scene) -> Region {
        let p = self.policy;
        let mut d = Region::new();
        if p.damage_removed {
            for old in &self.prev {
                if scene.index_of(old.id).is_none() {
                    d.add(old.footprint);
                }
            }
        }
        for l in &scene.layers {
            match self.prev.iter().find(|s| s.id == l.id) {
                None => d.add(l.footprint),
                Some(old) => {
                    if old.footprint != l.footprint || old.opaque != l.opaque {
                        if p.damage_old_footprint {
                            d.add(old.footprint);
                        }
                        d.add(l.footprint);
                    } else if old.look != l.look && p.damage_look {
                        d.add(l.footprint);
                    }
                }
            }
            if let (Some(r), true) = (l.dirty, p.damage_dirty)
                && let Some(r) = r.intersection(&l.footprint)
            {
                d.add(r);
            }
        }
        if p.damage_z_flips {
            self.z_flips(scene, &mut d);
        }
        d
    }

    /// Damage the overlap of every pair of layers that exist in both scenes and changed their
    /// relative order.
    fn z_flips(&self, scene: &Scene, d: &mut Region) {
        // Previous rank of each layer that survives, in new order.
        let ranks: Vec<(usize, &Layer)> = scene
            .layers
            .iter()
            .filter_map(|l| {
                self.prev
                    .iter()
                    .position(|s| s.id == l.id)
                    .map(|pi| (pi, l))
            })
            .collect();
        if ranks.windows(2).all(|w| w[0].0 < w[1].0) {
            return;
        }
        for (i, (pi, a)) in ranks.iter().enumerate() {
            for (pj, b) in &ranks[i + 1..] {
                if pi > pj {
                    // `a` was above `b` and is below it now.
                    let (oa, ob) = (&self.prev[*pi], &self.prev[*pj]);
                    if let Some(r) = oa.footprint.intersection(&ob.footprint) {
                        d.add(r);
                    }
                    if let Some(r) = a.footprint.intersection(&b.footprint) {
                        d.add(r);
                    }
                }
            }
        }
    }

    /// Paint calls for `damage`: bottom to top, each layer over its visible part.
    fn steps(&self, scene: &Scene, damage: &Region) -> Vec<Step> {
        let n = scene.layers.len();
        let mut visible: Vec<Region> = Vec::with_capacity(n + 1);
        visible.resize_with(n + 1, Region::new);
        // Opaque areas of the layers above, exact (never merged: a merged cover would claim
        // pixels that are not hidden).
        let mut cover: Vec<Rect> = Vec::new();
        for i in (0..n).rev() {
            let l = &scene.layers[i];
            visible[i + 1] = self.visible_part(damage.clipped(&l.footprint), &cover);
            if self.policy.cull_hidden
                && let Some(o) = l.opaque.intersection(&l.footprint)
            {
                cover.push(o);
            }
        }
        visible[0] = self.visible_part(damage.clone(), &cover);
        let mut steps = Vec::new();
        for (idx, region) in visible.iter().enumerate() {
            let layer = if idx == 0 {
                LayerId::WALLPAPER
            } else {
                scene.layers[idx - 1].id
            };
            for clip in merge_for_paint(region.rects(), damage) {
                steps.push(Step { layer, clip });
            }
        }
        steps
    }

    /// `region` minus the pixels hidden under opaque rectangles in `cover`; the unculled region
    /// itself when culling is off or would split it into too many pieces.
    fn visible_part(&self, region: Region, cover: &[Rect]) -> Region {
        if !self.policy.cull_hidden || cover.is_empty() {
            return region;
        }
        let mut v = region.clone();
        for o in cover {
            v.subtract_rect(o);
            if v.len() > MAX_VISIBLE_RECTS {
                return region;
            }
        }
        v
    }
}
