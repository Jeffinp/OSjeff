//! The model painter: draws every layer of the simulated world with recognisable patterns.
//!
//! Every window has its own colours, a title strip, a content area that depends on a version
//! counter, a chart area that depends on another, rounded corners that are left alone and a
//! translucent shadow ring that darkens whatever is under it (integer maths, so applying it twice
//! or in another order gives different pixels: any compositing mistake shows). Overlays are
//! translucent. The painter honours the contract of [`super::super::Layer`] and *checks* the part
//! the engine relies on: it reports every write outside the layer's declared footprint.

use super::model::{WinView, ids};
use crate::windowing::compositor::LayerId;
use crate::windowing::window::Rect;
use alloc::string::String;
use alloc::vec::Vec;

/// Screen size of the model.
pub const W: i32 = 192;
pub const H: i32 = 128;
pub const SCREEN: Rect = Rect::new(0, 0, W, H);
/// Height of the panel strip at the top.
pub const PANEL_H: i32 = 8;
/// Rounded corner radius of windows (the corner pixels outside the quarter circle stay transparent).
pub const RADIUS: i32 = 3;
/// How far the shadow reaches: left, right, up, down (it is shifted down by `SHADOW_DY`).
pub const SHADOW_R: i32 = 6;
pub const SHADOW_DY: i32 = 3;

pub fn rgb(r: u32, g: u32, b: u32) -> u32 {
    ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF)
}

/// `dst` darkened by `a / 256`.
pub fn darken(dst: u32, a: u32) -> u32 {
    let f = |c: u32| (c * (256 - a)) >> 8;
    rgb(f(dst >> 16 & 0xFF), f(dst >> 8 & 0xFF), f(dst & 0xFF))
}

/// `src` over `dst` with opacity `a / 255`.
pub fn blend(dst: u32, src: u32, a: u32) -> u32 {
    let f = |d: u32, s: u32| (s * a + d * (255 - a)) / 255;
    rgb(
        f(dst >> 16 & 0xFF, src >> 16 & 0xFF),
        f(dst >> 8 & 0xFF, src >> 8 & 0xFF),
        f(dst & 0xFF, src & 0xFF),
    )
}

/// A buffer the painter writes through, checking the declared footprint.
pub struct Canvas<'a> {
    pub buf: &'a mut [u32],
    /// Where the layer being painted declared it draws (intersected with the clip when checking
    /// is for the clip, with the footprint when checking the declaration).
    pub footprint: Rect,
    pub clip: Rect,
    pub layer: LayerId,
    pub violations: &'a mut Vec<String>,
}

impl Canvas<'_> {
    fn idx(x: i32, y: i32) -> Option<usize> {
        SCREEN.contains(x, y).then(|| (y * W + x) as usize)
    }

    pub fn get(&self, x: i32, y: i32) -> u32 {
        Self::idx(x, y).map_or(0, |i| self.buf[i])
    }

    /// Write one pixel: ignored outside the clip and the screen; a write outside the footprint
    /// is recorded as a violation (and still done, to show what it breaks).
    pub fn put(&mut self, x: i32, y: i32, v: u32) {
        if !self.clip.contains(x, y) {
            return;
        }
        let Some(i) = Self::idx(x, y) else { return };
        if !self.footprint.contains(x, y) && self.violations.len() < 8 {
            self.violations.push(alloc::format!(
                "layer {:?} wrote ({x},{y}) outside its footprint {:?}",
                self.layer,
                self.footprint
            ));
        }
        self.buf[i] = v;
    }
}

/// The wallpaper: a gradient with a diagonal texture.
pub fn wallpaper(x: i32, y: i32) -> u32 {
    rgb(
        (x * 255 / W) as u32,
        (y * 255 / H) as u32,
        ((x + y) * 3 % 256) as u32,
    )
}

/// Every pixel of the rectangle painted with the wallpaper.
pub fn paint_wallpaper(c: &mut Canvas) {
    let Some(r) = c.clip.intersection(&SCREEN) else {
        return;
    };
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            c.put(x, y, wallpaper(x, y));
        }
    }
}

/// Opacity of the shadow of `rect` at `(x, y)` (0 right at the reach, `strength` at the edge).
fn shadow_alpha(rect: &Rect, strength: i32, x: i32, y: i32) -> u32 {
    let dx = (rect.x - x).max(x - (rect.right() - 1)).max(0);
    let sy0 = rect.y + SHADOW_DY;
    let dy = (sy0 - y).max(y - (sy0 + rect.h - 1)).max(0);
    let d = dx + dy;
    if d > SHADOW_R {
        0
    } else {
        (strength * (SHADOW_R + 1 - d) / (SHADOW_R + 1)) as u32
    }
}

/// Is `(x, y)` one of the corner pixels of `rect` that the rounded shape leaves out?
fn in_cut_corner(rect: &Rect, x: i32, y: i32) -> bool {
    let (lx, ly) = (x - rect.x, y - rect.y);
    let cx = if lx < RADIUS {
        RADIUS - 1 - lx
    } else if lx >= rect.w - RADIUS {
        lx - (rect.w - RADIUS)
    } else {
        return false;
    };
    let cy = if ly < RADIUS {
        RADIUS - 1 - ly
    } else if ly >= rect.h - RADIUS {
        ly - (rect.h - RADIUS)
    } else {
        return false;
    };
    cx + cy > RADIUS - 1
}

/// Window colour at local `(lx, ly)`.
fn window_pixel(w: &WinView, lx: i32, ly: i32) -> u32 {
    let base = w.id * 47 % 200 + 28;
    if ly < 6 {
        return rgb(base, 255 - base, if w.focused { 150 } else { 90 });
    }
    let chart = Rect::new(4, w.rect.h - 14, w.rect.w - 8, 10);
    if chart.contains(lx, ly) {
        return rgb(w.part * 37, 200, lx as u32 * 5);
    }
    rgb(
        base + lx as u32 * 2 + w.ver * 7,
        base * 3 + ly as u32 * 3 + w.ver * 11,
        (lx ^ ly) as u32 + w.id * 13 + w.ver,
    )
}

/// Paint a window: shadow ring, then body (blended when it fades).
pub fn paint_window(c: &mut Canvas, w: &WinView) {
    let extent = w.paint_extent();
    let Some(area) = extent
        .intersection(&c.clip)
        .and_then(|r| r.intersection(&SCREEN))
    else {
        return;
    };
    let strength = if w.focused { 110 } else { 60 };
    let rounded = !w.maximized;
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let inside = w.rect.contains(x, y);
            if !inside || (rounded && in_cut_corner(&w.rect, x, y)) {
                if w.shadow {
                    let a = shadow_alpha(&w.rect, strength, x, y);
                    if a > 0 {
                        let d = c.get(x, y);
                        c.put(x, y, darken(d, a));
                    }
                }
                continue;
            }
            let src = window_pixel(w, x - w.rect.x, y - w.rect.y);
            if w.alpha >= 255 {
                c.put(x, y, src);
            } else {
                let d = c.get(x, y);
                c.put(x, y, blend(d, src, w.alpha));
            }
        }
    }
}

/// Paint the panel: an opaque strip, a label that depends on the focused window and a clock.
pub fn paint_panel(c: &mut Canvas, focused: u32, clock: u32) {
    let Some(area) = c.clip.intersection(&Rect::new(0, 0, W, PANEL_H)) else {
        return;
    };
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let v = if super::model::clock_rect().contains(x, y) {
                rgb(clock * 41, 220, 40 + y as u32)
            } else if x < 24 {
                rgb(focused * 61, 90, 160)
            } else {
                rgb(30 + (x as u32 & 7), 30, 60)
            };
            c.put(x, y, v);
        }
    }
}

/// Paint a translucent rectangle with an optional shadow and an opaque 1 px frame.
pub fn paint_glass(c: &mut Canvas, rect: Rect, shadow: bool, tint: u32, alpha: u32, extra: u32) {
    let extent = if shadow {
        super::model::shadow_extent(&rect)
    } else {
        rect
    };
    let Some(area) = extent
        .intersection(&c.clip)
        .and_then(|r| r.intersection(&SCREEN))
    else {
        return;
    };
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let d = c.get(x, y);
            if !rect.contains(x, y) {
                let a = shadow_alpha(&rect, 80, x, y);
                if a > 0 {
                    c.put(x, y, darken(d, a));
                }
                continue;
            }
            let frame =
                x == rect.x || y == rect.y || x == rect.right() - 1 || y == rect.bottom() - 1;
            if frame {
                c.put(x, y, rgb(240, 240 - extra, 200));
            } else {
                c.put(x, y, blend(d, tint ^ (extra * 0x010101), alpha));
            }
        }
    }
}

/// Paint the snap preview: a translucent 2 px outline.
pub fn paint_outline(c: &mut Canvas, rect: Rect) {
    let Some(area) = rect
        .intersection(&c.clip)
        .and_then(|r| r.intersection(&SCREEN))
    else {
        return;
    };
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let edge =
                x < rect.x + 2 || y < rect.y + 2 || x >= rect.right() - 2 || y >= rect.bottom() - 2;
            if edge {
                let d = c.get(x, y);
                c.put(x, y, blend(d, rgb(60, 140, 255), 150));
            }
        }
    }
}

/// Paint the taskbar: shadow, a translucent bar and a few opaque icons (one lit when hovered).
pub fn paint_taskbar(c: &mut Canvas, hover: u32) {
    let bar = super::model::taskbar_rect();
    paint_glass(c, bar, true, 0x203040, 120, 0);
    for i in 0..4 {
        let icon = Rect::new(bar.x + 6 + i * 18, bar.y + 3, 10, 8);
        let Some(r) = icon.intersection(&c.clip) else {
            continue;
        };
        let lit = hover == i as u32 + 1;
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                c.put(
                    x,
                    y,
                    rgb(if lit { 255 } else { 90 }, 80 + i as u32 * 40, 120),
                );
            }
        }
    }
}

/// Dispatch one layer by id.
pub fn paint_layer(c: &mut Canvas, id: LayerId, world: &super::model::World) {
    match id {
        LayerId::WALLPAPER => paint_wallpaper(c),
        ids::PANEL => paint_panel(c, world.focus_id(), world.clock),
        ids::TASKBAR => paint_taskbar(c, world.hover),
        ids::POPOVER => {
            if let Some(r) = world.popover {
                paint_glass(c, r, true, 0x101828, 170, world.popover_ver);
            }
        }
        ids::TOAST => {
            if let Some(r) = world.toast {
                paint_glass(c, r, false, 0x402010, 190, 0);
            }
        }
        ids::SNAP => {
            if let Some(r) = world.snap {
                paint_outline(c, r);
            }
        }
        other => {
            if let Some(v) = world.view_of(other.0) {
                paint_window(c, &v);
            }
        }
    }
}
