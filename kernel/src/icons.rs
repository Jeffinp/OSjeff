//! App icons: procedural squircle tiles (`osjeff_core::iconart`), drawn at 128 px
//! once and cached per size so every blit is a plain surface copy.
//!
//! The cache is bounded (it is cleared when it outgrows [`MAX_SCALED`] entries)
//! and used only from the compositor thread. Icons do not depend on the
//! appearance.

use crate::fb::Canvas;
use crate::sync::RacyCell;
use alloc::vec::Vec;
use osjeff_core::iconart::{self, IconId};
use osjeff_core::raster::Surface;

/// Which app an icon represents.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Brand,
    Launchpad,
    Terminal,
    Editor,
    TaskMgr,
    Calculator,
    Browser,
    WasmApp,
    Files,
    Monitor,
    Settings,
    Log,
    Viewer,
}

impl Icon {
    fn id(self) -> IconId {
        match self {
            Icon::Brand => IconId::Brand,
            Icon::Launchpad => IconId::Launchpad,
            Icon::Terminal => IconId::Terminal,
            Icon::Editor => IconId::Notes,
            Icon::TaskMgr => IconId::Tasks,
            Icon::Calculator => IconId::Calculator,
            Icon::Browser => IconId::Browser,
            Icon::WasmApp => IconId::Apps,
            Icon::Files => IconId::Files,
            Icon::Monitor => IconId::Monitor,
            Icon::Settings => IconId::Settings,
            Icon::Log => IconId::Console,
            Icon::Viewer => IconId::Photos,
        }
    }
}

/// Scaled copies kept before the cache is flushed.
const MAX_SCALED: usize = 160;

struct Cache {
    sources: Vec<(IconId, Surface)>,
    scaled: Vec<(IconId, u16, Surface)>,
}

static CACHE: RacyCell<Option<Cache>> = RacyCell::new(None);

fn cache() -> &'static mut Cache {
    // SAFETY: only the compositor thread draws icons; no reference from an earlier call is
    // kept across calls (callers use the surface immediately).
    // NOTE: not guaranteed by the type: safe fn returning `&'static mut`.
    let slot = unsafe { &mut *CACHE.get() };
    slot.get_or_insert_with(|| Cache {
        sources: Vec::new(),
        scaled: Vec::new(),
    })
}

/// The icon at `size` x `size` (cached).
pub fn surface(icon: Icon, size: i32) -> &'static Surface {
    let size = size.clamp(8, 256) as usize;
    let id = icon.id();
    let c = cache();
    if let Some(i) = c
        .scaled
        .iter()
        .position(|(i, s, _)| *i == id && *s as usize == size)
    {
        return &c.scaled[i].2;
    }
    if c.scaled.len() >= MAX_SCALED {
        c.scaled.clear();
    }
    let si = match c.sources.iter().position(|(i, _)| *i == id) {
        Some(i) => i,
        None => {
            let s = iconart::render(id, crate::fb::masks());
            c.sources.push((id, s));
            c.sources.len() - 1
        }
    };
    let src = &c.sources[si].1;
    let scaled = if size == src.w {
        src.clone()
    } else {
        src.resized(size, size)
    };
    c.scaled.push((id, size as u16, scaled));
    &c.scaled.last().expect("just pushed").2
}

/// Draw `icon` with its top-left at `(x, y)`, `size` pixels square.
pub fn blit(c: &mut Canvas, icon: Icon, x: i32, y: i32, size: i32, opacity: u32) {
    let t0 = crate::trace::t();
    c.blit_surface(surface(icon, size), x, y, opacity);
    crate::trace::prim(crate::trace::Prim::Glyph, t0);
}

/// Bytes held by the icon sources and scaled copies (for the memory log).
pub fn bytes() -> usize {
    let c = cache();
    c.sources.iter().map(|(_, s)| s.px.len() * 4).sum::<usize>()
        + c.scaled
            .iter()
            .map(|(_, _, s)| s.px.len() * 4)
            .sum::<usize>()
}

/// An installed app's own 24x24 (or any size) RGBA icon on a squircle tile, `size`
/// pixels square. Not cached here: the caller keeps the result.
pub fn app_tile(rgba: Option<&[u8]>, size: i32) -> Surface {
    let s = match rgba {
        Some(px) => iconart::wrap_app_icon(px, 24, 24, crate::fb::masks()),
        None => iconart::render(IconId::Apps, crate::fb::masks()),
    };
    s.resized(size.clamp(8, 256) as usize, size.clamp(8, 256) as usize)
}
