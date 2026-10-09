//! The retained scene description the compositor plans from: an ordered list of layers.
//!
//! The owner of the pixels (the desktop) describes, once per frame, *what is on screen* in the
//! cheapest terms: for every layer its identity, where it can draw, what it is guaranteed to
//! cover opaquely and a version number that changes whenever its pixels might. It does not say
//! what to repaint. [`super::Engine`] diffs this description against the one it painted last
//! time and works out the damage, so nobody has to remember which of several caches a feature
//! must invalidate: the only duty is to keep `footprint` and `look` truthful.

use crate::window::Rect;
use alloc::vec::Vec;

/// Identity of a layer across frames (a window id, the panel, the taskbar, an overlay...).
/// The compositor never interprets it; the painter does.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct LayerId(pub u32);

impl LayerId {
    /// The wallpaper: implicit, always at the bottom, covers the whole screen opaquely.
    pub const WALLPAPER: LayerId = LayerId(u32::MAX);
}

/// One layer of the scene.
///
/// The contract the painter must honour (the differential tests enforce it on a model painter):
/// 1. **footprint**: painting this layer touches no pixel outside `footprint`. Shadows, glows,
///    tooltips and anything else a layer draws belong in it.
/// 2. **opaque**: every pixel of `opaque` ends up fully opaque whatever was underneath, so the
///    layers below need not be painted there. Empty when unsure (translucent, fading...).
/// 3. **clip invariance**: painting with a clip rectangle gives, inside the clip, exactly the
///    pixels a full paint would give, given the same pixels underneath.
/// 4. **look**: if the layer would paint different pixels than last frame, `look` differs, or
///    `dirty` covers the difference.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Layer {
    pub id: LayerId,
    pub footprint: Rect,
    pub opaque: Rect,
    /// A counter (or any value) that changes whenever the layer's pixels may have changed
    /// anywhere. A change repaints the whole footprint.
    pub look: u64,
    /// The content changed only inside this rectangle since the last plan (a chart, a caret):
    /// repaint just that. `None` when nothing changed that `look` does not already say.
    pub dirty: Option<Rect>,
}

impl Layer {
    /// A layer with an empty opaque area, version 0 and nothing dirty.
    pub const fn new(id: LayerId, footprint: Rect) -> Layer {
        Layer {
            id,
            footprint,
            opaque: Rect::new(0, 0, 0, 0),
            look: 0,
            dirty: None,
        }
    }

    pub const fn with_opaque(mut self, opaque: Rect) -> Layer {
        self.opaque = opaque;
        self
    }

    pub const fn with_look(mut self, look: u64) -> Layer {
        self.look = look;
        self
    }

    pub const fn with_dirty(mut self, dirty: Option<Rect>) -> Layer {
        self.dirty = dirty;
        self
    }
}

/// Layers from the bottom (index 0, just above the wallpaper) to the top.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scene {
    pub layers: Vec<Layer>,
}

impl Scene {
    pub const fn new() -> Scene {
        Scene { layers: Vec::new() }
    }

    /// Put `layer` on top of the ones added so far.
    pub fn push(&mut self, layer: Layer) {
        debug_assert!(
            self.layers.iter().all(|l| l.id != layer.id),
            "layer {:?} added twice",
            layer.id
        );
        self.layers.push(layer);
    }

    pub fn len(&self) -> usize {
        self.layers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// Position of `id` from the bottom.
    pub fn index_of(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }
}

/// Builds a [`Layer::look`] out of the values that decide a layer's pixels (FNV-1a over the
/// words pushed). Order matters; equal inputs give equal looks and different inputs differ with
/// overwhelming probability. The point is to *name* every input in one place, next to the code
/// that builds the layer, instead of scattering invalidation calls over every handler.
#[derive(Clone, Copy, Debug)]
pub struct Look(u64);

impl Look {
    pub const fn new() -> Look {
        Look(0xCBF2_9CE4_8422_2325)
    }

    /// Mix in a 64-bit value.
    pub const fn u(self, v: u64) -> Look {
        // One FNV-1a step per byte would be the textbook form; mixing the whole word with a
        // multiply and a rotate is as good for change detection and much cheaper.
        let x = (self.0 ^ v).wrapping_mul(0x0000_0100_0000_01B3);
        Look(x.rotate_left(29) ^ (x >> 7))
    }

    /// Mix in a signed value.
    pub const fn i(self, v: i32) -> Look {
        self.u(v as u32 as u64)
    }

    /// Mix in a flag.
    pub const fn b(self, v: bool) -> Look {
        self.u(v as u64)
    }

    /// The finished look.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl Default for Look {
    fn default() -> Self {
        Look::new()
    }
}
