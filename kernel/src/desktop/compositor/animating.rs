//! Drawing a window while it opens, closes, minimises or zooms: it is rendered once at its resting
//! size into an offscreen texture and resampled into each frame's rectangle with its fade and its
//! rounded corners, under a shadow that fades with it. Part of the window layer's painting (see
//! `paint.rs`); the engine knows nothing about it except that such a window is not opaque.

use super::super::*;

impl Desktop {
    /// Draws an animating window: see `chrome.rs` / `Canvas::blit_scaled`. The window
    /// is rendered once at its resting size into the offscreen texture, then
    /// resampled into this frame's rectangle with its fade and rounded corners,
    /// under a shadow that fades with it.
    pub(crate) fn draw_animating(&self, c: &mut Canvas, w: &Win, focused: bool) {
        let r = w.rect;
        let (dest, alpha) = match (&w.anim, &w.zoom) {
            (Some(a), _) => {
                let f = a.frame(r, self.dock_target(w));
                (f.rect, f.alpha as u32)
            }
            (None, Some(z)) => {
                // A zoom draws the real window at the in-flight rectangle (its content
                // is clipped to it), so the layout is never a squeezed bitmap.
                self.draw_window(c, w, z.rect(), focused, true);
                return;
            }
            _ => (r, 256),
        };
        let bpp = c.bpp();
        let need = (r.w.max(0) * r.h.max(0)) as usize * bpp;
        if need == 0 || need > TEXTURE_BYTES {
            // Too big for the texture: no scale effect, just the window.
            self.draw_window(c, w, r, focused, true);
            return;
        }
        let tex = &mut texture_slice()[..need];
        let key = (w.id, r.w, r.h);
        if self.tex_key.get() != Some(key) {
            let mut info = c.fb_info();
            info.width = r.w as usize;
            info.height = r.h as usize;
            info.stride = r.w as usize;
            info.byte_len = need;
            let mut tc = Canvas::new(tex, info);
            // The corner pixels blend with this colour before the corner mask hides them.
            tc.fill_rect(0, 0, r.w as usize, r.h as usize, theme::window_body());
            self.draw_window(&mut tc, w, Rect::new(0, 0, r.w, r.h), focused, false);
            self.tex_key.set(Some(key));
        }
        if alpha > 0 {
            let rr = kitsune_core::style::R_WINDOW
                .min(dest.w / 2)
                .min(dest.h / 2);
            let hole = Rect::new(dest.x, dest.y + rr, dest.w, (dest.h - 2 * rr).max(0));
            for sh in window_shadow(focused, alpha) {
                c.draw_shadow(dest, sh, hole);
            }
        }
        c.blit_scaled(
            &texture_slice()[..need],
            r.w as usize,
            r.h as usize,
            dest,
            alpha,
            kitsune_core::style::R_WINDOW,
        );
    }
}
