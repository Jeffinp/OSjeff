//! Translucent "glass" surfaces: a blurred copy of what is behind a panel (taken
//! when the panel opens and kept until it closes), a tint over it and a hairline
//! edge. This is how menus, popovers, Spotlight, Launchpad, toasts and the dock
//! get their look without any per-frame blur.

// Toolkit: parts of this API are consumed by the chrome, the rest is for the apps (wave 2).
#![allow(dead_code)]

use crate::desktop::*;
use kitsune_core::raster::Surface;

/// A blurred snapshot of a screen region.
pub(crate) struct Backdrop {
    pub rect: Rect,
    pub px: Vec<u32>,
}

impl Backdrop {
    /// Snapshot `rect` of `c`, blur it (box blur of `radius` pixels, done at reduced
    /// resolution for big regions so the cost stays bounded) and keep the result.
    pub(crate) fn capture(c: &Canvas, rect: Rect, radius: usize) -> Backdrop {
        let rect = rect.clamped_to(c.width() as i32, c.height() as i32);
        let mut px = Vec::new();
        c.read_region(rect, &mut px);
        let (w, h) = (rect.w.max(0) as usize, rect.h.max(0) as usize);
        if w < 4 || h < 4 {
            return Backdrop { rect, px };
        }
        let f = match w * h {
            0..=60_000 => 1,
            60_001..=250_000 => 2,
            _ => 4,
        };
        let mut s = Surface {
            w,
            h,
            px: core::mem::take(&mut px),
        };
        if f == 1 {
            s.blur(radius.max(1), 2);
        } else {
            let mut small = s.resized((w / f).max(1), (h / f).max(1));
            small.blur((radius / f).max(2), 2);
            s = small.resized(w, h);
        }
        Backdrop { rect, px: s.px }
    }

    /// Draw the snapshot inside `dest` (rounded by `radius`) with `alpha` (0..=256).
    pub(crate) fn draw(&self, c: &mut Canvas, dest: Rect, radius: i32, alpha: u32) {
        c.blit_pixels(&self.px, self.rect, dest, radius, alpha);
    }
}

/// Lazily captured backdrop owned by an overlay (drawing takes `&self`).
#[derive(Default)]
pub(crate) struct BackdropSlot(core::cell::RefCell<Option<Backdrop>>);

impl BackdropSlot {
    /// Capture on first use, then draw. `alpha` fades the whole glass (0..=256).
    pub(crate) fn draw(&self, c: &mut Canvas, rect: Rect, radius: i32, blur: usize, alpha: u32) {
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(Backdrop::capture(c, rect, blur));
        }
        if let Some(b) = slot.as_ref() {
            b.draw(c, rect, radius, alpha);
        }
    }

    /// Capture `rect` now if nothing was captured yet: for a panel that grows (Busca), so
    /// the backdrop covers the largest size it will take.
    pub(crate) fn ensure(&self, c: &Canvas, rect: Rect, blur: usize) {
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(Backdrop::capture(c, rect, blur));
        }
    }

    pub(crate) fn clear(&self) {
        *self.0.borrow_mut() = None;
    }
}

/// A glass panel: shadow, blurred backdrop, tint, hairline. `fade` is 0..=256.
#[allow(clippy::too_many_arguments)]
pub(crate) fn panel(
    c: &mut Canvas,
    r: Rect,
    radius: i32,
    slot: &BackdropSlot,
    blur: usize,
    tint_argb: u32,
    edge_argb: u32,
    shadow: Shadow,
    fade: u32,
) {
    if fade == 0 {
        return;
    }
    // Shadow first (it lies outside the panel; the hole is the panel itself).
    let hole = Rect::new(r.x, r.y + radius, r.w, (r.h - 2 * radius).max(0));
    c.draw_shadow(
        r,
        Shadow {
            alpha: shadow.alpha * fade / 256,
            ..shadow
        },
        hole,
    );
    slot.draw(c, r, radius, blur, fade);
    let (col, a) = theme::tint(tint_argb);
    c.fill_rrect(
        r,
        radius,
        Corner::Circle,
        col,
        (a as u32 * fade / 256) as u16,
    );
    let (ec, ea) = theme::tint(edge_argb);
    c.stroke_rrect(
        r,
        radius,
        Corner::Circle,
        ec,
        (ea as u32 * fade / 256) as u16,
    );
}
