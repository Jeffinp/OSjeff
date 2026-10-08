//! `Desktop` methods: compositing. The layers, bottom to top: wallpaper (cached in
//! `BG`, with the baked menu-bar glass), windows, the app bar (dock), the menu bar
//! content, overlays (menus, popovers, Apps, Busca, dialogs, Alt+Tab).

use super::*;

impl Desktop {
    // ---- rendering ----

    /// Full recompose of the whole scene into `back`. Used when not animating
    /// (input/clock changes). The animation path uses the cheaper damage-based
    /// [`render_anim_frame`] instead.
    pub fn render(&self, back: &mut [u8], info: bootloader_api::info::FrameBufferInfo, time: Time) {
        let mut c = Canvas::new(back, info);
        let focused = self.focused();
        for w in self.wm.windows() {
            if !w.shown() {
                continue;
            }
            let is_focus = focused == Some(w.id);
            let rect = self.window_box(w);
            if w.anim.is_some() || w.zoom.is_some() {
                self.draw_animating(&mut c, w, is_focus);
            } else {
                self.draw_window(&mut c, w, rect, is_focus, !w.maximized);
            }
        }
        self.draw_dock(&mut c);
        self.draw_menubar(&mut c, time);
        self.draw_overlays_in(&mut c, None);
    }

    /// Draws the app bar and the transient overlays on top of the cached static
    /// layer (which holds neither). Separated so the compositor can repaint only
    /// their region on a hover change instead of recomposing the whole desktop.
    pub fn draw_overlay(&self, c: &mut Canvas) {
        self.draw_dock(c);
        self.draw_overlays_in(c, None);
    }

    /// Like [`draw_overlay`](Self::draw_overlay) but only the part that changed
    /// (`overlay_bounds`), for the hover repaint.
    pub fn draw_overlay_dirty(&self, c: &mut Canvas) {
        let ov = self.overlay_bounds();
        let Some(clip) = ov.intersection(&c.clip_rect()) else {
            return;
        };
        let saved = c.set_clip(clip);
        if ov.intersection(&self.dock_paint_zone()).is_some() {
            self.draw_dock(c);
        }
        self.draw_overlays_in(c, Some(ov));
        c.restore_clip(saved);
        self.overlay_painted();
    }

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
            let rr = 12.min(dest.w / 2).min(dest.h / 2);
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
            12,
        );
    }

    /// Compact signature of the *static* scene (which windows are visible /
    /// animating, their z-order, geometry and the drag target). When it changes,
    /// the cached static layer must be rebuilt. See
    /// [`osjeff_core::winman::WindowManager::signature`].
    pub fn anim_signature(&self) -> u64 {
        let mut h = self.wm.signature(self.drag.as_ref().map(|d| d.win));
        // The menu bar shows the focused app, so a focus change rebuilds the layer too.
        h ^= self.focused().map_or(0, |f| f.raw() as u64) << 32;
        h
    }

    /// Composes the static layer (non-dynamic windows and the menu bar) into `buf`,
    /// which must already contain the wallpaper. Done once per animation.
    pub fn compose_static(
        &self,
        buf: &mut [u8],
        info: bootloader_api::info::FrameBufferInfo,
        time: Time,
    ) {
        let mut c = Canvas::new(buf, info);
        let focused = self.focused();
        for w in self.wm.windows() {
            if w.shown() && !self.is_dynamic(w) {
                self.draw_window(&mut c, w, w.rect, focused == Some(w.id), !w.maximized);
            }
        }
        self.draw_menubar(&mut c, time);
    }

    /// Renders one animation frame using damage tracking: only the rectangle
    /// covering the animating window(s), the dock and the overlays (this frame and
    /// last) is touched. Returns that damage rect (caller blits just this region).
    pub fn render_anim_frame(
        &self,
        back: &mut [u8],
        static_buf: &[u8],
        info: bootloader_api::info::FrameBufferInfo,
        prev_damage: Rect,
    ) -> Rect {
        let (sw, sh) = (info.width as i32, info.height as i32);
        let wins = self.wm.windows();

        // Damage = last frame's region + every animating window's box now.
        let mut damage = prev_damage;
        let mut lowest_anim_z = wins.len();
        for (i, w) in wins.iter().enumerate() {
            if w.shown() && self.is_dynamic(w) {
                damage = damage.union(&shadow_box(self.window_box(w)));
                lowest_anim_z = lowest_anim_z.min(i);
            }
        }
        // The dock is repainted when it moves or something moving crosses it.
        let dock_zone = self.dock_paint_zone();
        if self.dock_animating() || damage.intersection(&dock_zone).is_some() {
            damage = damage.union(&dock_zone);
        }
        if self.overlay_open() {
            damage = damage.union(&self.overlay_bounds_full());
        }
        let damage = damage.clamped_to(sw, sh);
        if damage.is_empty() {
            return damage;
        }

        // Restore the cached static scene over the damaged region.
        copy_region(back, static_buf, info, damage);

        // Draw the animating windows on top.
        {
            let mut c = Canvas::new(back, info);
            c.set_clip(damage);
            let focused = self.focused();
            for w in wins {
                if w.shown() && self.is_dynamic(w) {
                    let is_focus = focused == Some(w.id);
                    if w.anim.is_some() || w.zoom.is_some() {
                        self.draw_animating(&mut c, w, is_focus);
                    } else {
                        // Dragged / live window: drawn over the cached layer with its shadow.
                        self.draw_window(&mut c, w, self.window_box(w), is_focus, !w.maximized);
                    }
                }
            }
        }

        // Re-assert any static window that sits above an animating one (so the
        // animating window doesn't paint over a window that is in front of it).
        for w in wins.iter().skip(lowest_anim_z + 1) {
            if w.shown()
                && !self.is_dynamic(w)
                && let Some(clip) = w.rect.intersection(&damage)
            {
                copy_region(back, static_buf, info, clip);
            }
        }
        // The menu bar is above every window (a shadow may have reached it).
        if let Some(bar) = self.menubar_rect().intersection(&damage) {
            copy_region(back, static_buf, info, bar);
        }

        {
            let mut c = Canvas::new(back, info);
            c.set_clip(damage);
            if damage.intersection(&dock_zone).is_some() {
                self.draw_dock(&mut c);
            }
            if self.overlay_open() {
                self.draw_overlays_in(&mut c, None);
            }
        }
        damage
    }

    /// Draws the mouse cursor. Called as an overlay directly onto the
    /// framebuffer so cursor moves don't require a full scene recompose.
    pub fn draw_cursor_overlay(&self, c: &mut Canvas) {
        self.draw_cursor(c);
    }
}
