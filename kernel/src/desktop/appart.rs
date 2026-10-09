//! Cache of the app-interior art (`kitsune_core::appart`): the coloured file icons of
//! Arquivos and the monochrome tool glyphs of the toolbars. Each (kind, size, colour) is
//! drawn once and then blitted; the cache is bounded and used only by the compositor thread.

use crate::fb::Canvas;
use crate::sync::RacyCell;
use alloc::vec::Vec;
use kitsune_core::appart::{self, FileKind, Tool};
use kitsune_core::raster::Surface;

const MAX_FILE: usize = 48;
const MAX_TOOL: usize = 160;

struct Cache {
    files: Vec<(FileKind, u16, u32, Surface)>,
    tools: Vec<(Tool, u16, u32, Surface)>,
}

static CACHE: RacyCell<Option<Cache>> = RacyCell::new(None);

fn cache() -> &'static mut Cache {
    // SAFETY: only the compositor thread draws app art; callers use the surface at once and
    // keep no reference across calls.
    // NOTE: not guaranteed by the type: safe fn returning `&'static mut`.
    let slot = unsafe { &mut *CACHE.get() };
    slot.get_or_insert_with(|| Cache {
        files: Vec::new(),
        tools: Vec::new(),
    })
}

/// The icon of `kind` at `size` px, tinted with the current accent.
pub(crate) fn file_icon(kind: FileKind, size: i32) -> &'static Surface {
    let size = size.clamp(8, 256) as u16;
    let accent = crate::theme::accent();
    let key = ((accent.r as u32) << 16) | ((accent.g as u32) << 8) | accent.b as u32;
    let c = cache();
    if let Some(i) = c
        .files
        .iter()
        .position(|(k, s, a, _)| *k == kind && *s == size && *a == key)
    {
        return &c.files[i].3;
    }
    if c.files.len() >= MAX_FILE {
        c.files.clear();
    }
    let s = appart::file_icon(kind, size as usize, key, crate::fb::masks());
    c.files.push((kind, size, key, s));
    &c.files.last().expect("just pushed").3
}

/// Blit a file icon with its top-left at `(x, y)`.
pub(crate) fn blit_file(c: &mut Canvas, kind: FileKind, x: i32, y: i32, size: i32, opacity: u32) {
    c.blit_surface(file_icon(kind, size), x, y, opacity);
}

/// The tool glyph `t` at `size` px in straight ARGB `argb`.
pub(crate) fn tool(t: Tool, size: i32, argb: u32) -> &'static Surface {
    let size = size.clamp(8, 128) as u16;
    let c = cache();
    if let Some(i) = c
        .tools
        .iter()
        .position(|(k, s, a, _)| *k == t && *s == size && *a == argb)
    {
        return &c.tools[i].3;
    }
    if c.tools.len() >= MAX_TOOL {
        c.tools.clear();
    }
    let s = appart::tool_glyph(t, size as usize, argb);
    c.tools.push((t, size, argb, s));
    &c.tools.last().expect("just pushed").3
}

/// Draw a tool glyph with its top-left at `(x, y)` in `argb` (opacity through the alpha of
/// the colour is not supported: pass `opacity` 0..=256 instead).
pub(crate) fn blit_tool(
    c: &mut Canvas,
    t: Tool,
    x: i32,
    y: i32,
    size: i32,
    rgb: u32,
    opacity: u32,
) {
    c.blit_surface(
        tool(t, size, 0xFF00_0000 | (rgb & 0xFF_FFFF)),
        x,
        y,
        opacity,
    );
}
