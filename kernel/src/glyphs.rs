//! Cache of the monochrome UI glyphs (search, network, chevrons, ...): each
//! (glyph, size, colour) is drawn once by `osjeff_core::iconart::glyph`.

use crate::sync::RacyCell;
use alloc::vec::Vec;
use osjeff_core::iconart::{self, Glyph};
use osjeff_core::raster::Surface;

const MAX: usize = 96;

static CACHE: RacyCell<Vec<(Glyph, u16, u32, Surface)>> = RacyCell::new(Vec::new());

/// The glyph `g` at `size` px in straight ARGB colour `argb` (cached).
pub fn get(g: Glyph, size: usize, argb: u32) -> &'static Surface {
    // SAFETY: compositor thread only; the reference is used immediately by the caller.
    // NOTE: not guaranteed by the type: safe fn returning a `'static` reference.
    let cache = unsafe { &mut *CACHE.get() };
    if let Some(i) = cache
        .iter()
        .position(|(gg, s, c, _)| *gg == g && *s as usize == size && *c == argb)
    {
        return &cache[i].3;
    }
    if cache.len() >= MAX {
        cache.clear();
    }
    cache.push((g, size as u16, argb, iconart::glyph(g, size, argb)));
    &cache.last().expect("just pushed").3
}
