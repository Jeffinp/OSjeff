//! `Desktop` methods: render. Split out of the former monolithic desktop.rs.

use super::*;

impl Desktop {
    // ---- rendering ----

    /// Full recompose of the whole scene into `back`. Used when not animating
    /// (input/clock changes). The animation path uses the cheaper damage-based
    /// [`render_anim_frame`] instead.
    pub fn render(&self, back: &mut [u8], info: bootloader_api::info::FrameBufferInfo, time: Time) {
        let mut c = Canvas::new(back, info);
        self.draw_dock_dots(&mut c);
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
        draw_clock(&mut c, time);
        self.draw_overlay(&mut c);
    }

    /// Draws the transient overlays (context menu, start panel, Alt+Tab switcher)
    /// on top of the composed scene. Separated so the compositor can repaint only
    /// their region on a hover change instead of recomposing the whole desktop.
    pub fn draw_overlay(&self, c: &mut Canvas) {
        if let Some(m) = self.menu {
            self.draw_menu(c, m);
        }
        if self.start_open {
            self.draw_start(c);
        }
        if let Some(sw) = &self.switcher {
            self.draw_switcher(c, sw);
        }
    }

    /// A dot under the dock icon of every app that has a minimized window, so a
    /// hidden window can be found (and restored by clicking the icon). Drawn
    /// before the windows: the dock is part of the desktop background.
    fn draw_dock_dots(&self, c: &mut Canvas) {
        if !self.wm.windows().iter().any(|w| w.minimized) {
            return;
        }
        let (_, icons) = dock_layout(self.sw, self.sh);
        for kind in Kind::ALL {
            if kind.in_dock()
                && self
                    .wm
                    .windows()
                    .iter()
                    .any(|w| w.minimized && w.app.kind() == kind)
            {
                // Kinds past the dock (the viewer) have no icon to mark.
                let Some(r) = icons.get(kind.index() + 1).copied() else {
                    continue;
                };
                c.fill_round_rect(
                    (r.x + r.w / 2 - 3) as usize,
                    (r.bottom() + 4) as usize,
                    6,
                    6,
                    3,
                    theme::accent(),
                );
            }
        }
    }

    /// Draws a window that is opening, closing, minimising or zooming: the window
    /// is rendered once at its resting size into the offscreen texture, then
    /// resampled into this frame's rectangle with its fade and rounded corners
    /// (`Canvas::blit_scaled`), under a shadow that fades with it.
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
            tc.fill_rect(0, 0, r.w as usize, r.h as usize, theme::WINDOW_BODY);
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
        self.wm.signature(self.drag.as_ref().map(|d| d.win))
    }

    /// Composes the static layer (non-animating windows + clock) into `buf`,
    /// which must already contain the wallpaper. Done once per animation.
    pub fn compose_static(
        &self,
        buf: &mut [u8],
        info: bootloader_api::info::FrameBufferInfo,
        time: Time,
    ) {
        let mut c = Canvas::new(buf, info);
        self.draw_dock_dots(&mut c);
        let focused = self.focused();
        for w in self.wm.windows() {
            if w.shown() && !self.is_dynamic(w) {
                self.draw_window(&mut c, w, w.rect, focused == Some(w.id), !w.maximized);
            }
        }
        draw_clock(&mut c, time);
    }

    /// Renders one animation frame using damage tracking: only the rectangle
    /// covering the animating window(s) (this frame and last) is touched.
    /// Returns that damage rect (caller blits just this region).
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
        let damage = damage.clamped_to(sw, sh);
        if damage.is_empty() {
            return damage;
        }

        // Restore the cached static scene over the damaged region.
        copy_region(back, static_buf, info, damage);

        // Draw the animating windows on top.
        {
            let mut c = Canvas::new(back, info);
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

        damage
    }

    /// Draws the mouse cursor. Called as an overlay directly onto the
    /// framebuffer so cursor moves don't require a full scene recompose.
    pub fn draw_cursor_overlay(&self, c: &mut Canvas) {
        self.draw_cursor(c);
    }

    pub(crate) fn draw_window(
        &self,
        c: &mut Canvas,
        win: &Win,
        r: Rect,
        focused: bool,
        shadow: bool,
    ) {
        let x = r.x.max(0) as usize;
        let y = r.y.max(0) as usize;
        let w = r.w as usize;
        let h = r.h as usize;
        let th = TITLE_H as usize;

        let radius = 12usize;

        // Soft drop shadow (two layers). Skipped while dragging/animating so the
        // alpha fill never costs on the hot path. The window body painted below
        // is opaque on every full-width row, i.e. rows `[y + r, y + h - r)` of
        // `fill_round_rect` (r = corner radius actually used), so the shadow is
        // not blended under it: the body would overwrite those pixels anyway,
        // and skipping them removes ~85 % of the blend work for identical output.
        if shadow {
            let body = Rect::new(x as i32, y as i32, w as i32, h as i32);
            let rr = radius.min(w / 2).min(h / 2) as i32;
            let hole = Rect::new(body.x, body.y + rr, body.w, (body.h - 2 * rr).max(0));
            for sh in [
                Shadow {
                    blur: 20,
                    dy: 14,
                    alpha: 70,
                },
                Shadow {
                    blur: 6,
                    dy: 4,
                    alpha: 64,
                },
            ] {
                c.draw_shadow(body, sh, hole);
            }
        }
        // Body + dark header (anti-aliased corners; the header is the same rounded
        // rectangle clipped to the title band, so only its top corners round).
        let body = Rect::new(x as i32, y as i32, w as i32, h as i32);
        c.fill_rrect(body, radius as i32, Corner::Circle, theme::WINDOW_BODY, 256);
        let header = if focused {
            theme::HEADER
        } else {
            theme::HEADER_DIM
        };
        let saved = c.set_clip(
            Rect::new(x as i32, y as i32, w as i32, th as i32)
                .intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        );
        c.fill_rrect(body, radius as i32, Corner::Circle, header, 256);
        c.restore_clip(saved);
        // Accent top line marks focus (teal) vs unfocused (muted).
        let accent = if focused {
            theme::accent()
        } else {
            theme::TEXT_MUTED
        };
        c.fill_round_rect(x + radius, y, w - radius * 2, 3, 1, accent);

        // App indicator dot + title (clipped before the title-bar buttons).
        c.fill_round_rect(x + 12, y + 11, 8, 8, 4, accent);
        let room = (r.min_rect().x - (r.x + 28) - 8).max(0);
        let ty = crate::text::center_y(
            y as i32,
            th as i32,
            crate::text::BODY,
            crate::text::Weight::Medium,
        );
        crate::text::draw_ellipsis(
            c,
            x as i32 + 28,
            ty,
            room,
            &win.app.title,
            crate::text::BODY,
            crate::text::Weight::Medium,
            theme::HEADER_TEXT,
        );

        // Close button (circular).
        let cb = r.close_rect();
        let cbs = cb.w as usize;
        c.fill_round_rect(
            cb.x.max(0) as usize,
            cb.y.max(0) as usize,
            cbs,
            cbs,
            cbs / 2,
            theme::CLOSE,
        );
        // Minimize / maximize appear (with glyphs on all three) while the pointer
        // is over the window, so an idle desktop keeps its calm title bars.
        if self.hover == Some(win.id) {
            self.draw_title_buttons(c, win, r);
        }

        let cost_t0 = crate::io::rdtsc();
        // While zooming, the app is laid out for the final size: keep it inside the
        // rectangle being drawn.
        let saved_clip = win.zoom.is_some().then(|| {
            let clip = r
                .intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0));
            c.set_clip(clip)
        });
        match &win.app.app {
            App::Terminal(t) => self.draw_terminal(c, r, t, focused),
            App::Editor(e) => self.draw_editor(c, r, e, focused),
            App::TaskMgr => self.draw_taskmgr(c, r),
            App::Calculator(k) => self.draw_calculator(c, r, k),
            App::Browser(b) => self.draw_browser(c, r, focused, b),
            App::Wasm(w) => self.draw_wasm(c, r, w),
            App::Files(f) => self.draw_files(c, r, f),
            App::Monitor(m) => self.draw_monitor(c, r, m),
            App::Settings(s) => self.draw_settings(c, r, s),
            App::Log(l) => self.draw_log(c, r, l),
            App::Viewer(v) => self.draw_viewer(c, r, v),
        }
        if let Some(cs) = saved_clip {
            c.restore_clip(cs);
        }
        // What this window cost to draw (the monitor's per-app CPU figure).
        win.app
            .cost
            .set(win.app.cost.get() + crate::io::rdtsc().wrapping_sub(cost_t0));
    }

    /// Minimize and maximize / restore buttons plus the glyphs on all three.
    fn draw_title_buttons(&self, c: &mut Canvas, win: &Win, r: Rect) {
        let dark = theme::HEADER;
        let circle = |c: &mut Canvas, b: Rect, color: Color| {
            c.fill_round_rect(
                b.x.max(0) as usize,
                b.y.max(0) as usize,
                b.w as usize,
                b.w as usize,
                b.w as usize / 2,
                color,
            );
        };
        let center = |b: Rect| (b.x + b.w / 2, b.y + b.h / 2);
        let rect = |c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, col: Color| {
            c.fill_rect(
                x.max(0) as usize,
                y.max(0) as usize,
                w as usize,
                h as usize,
                col,
            );
        };

        // Minimize: a dash.
        let mn = r.min_rect();
        circle(c, mn, theme::MINIMIZE);
        let (cx, cy) = center(mn);
        rect(c, cx - 4, cy + 1, 8, 2, dark);

        // Maximize (a square) or, when maximized, restore (two squares).
        if win.resizable {
            let mx = r.max_rect();
            circle(c, mx, theme::MAXIMIZE);
            let (cx, cy) = center(mx);
            if win.maximized {
                rect(c, cx - 2, cy - 5, 7, 7, dark);
                rect(c, cx - 1, cy - 4, 5, 5, theme::MAXIMIZE);
                rect(c, cx - 5, cy - 2, 7, 7, dark);
                rect(c, cx - 4, cy - 1, 5, 5, theme::MAXIMIZE);
            } else {
                rect(c, cx - 4, cy - 4, 8, 8, dark);
                rect(c, cx - 3, cy - 3, 6, 6, theme::MAXIMIZE);
            }
        }

        // Close: a cross.
        let (cx, cy) = center(r.close_rect());
        for i in 0..7 {
            rect(c, cx - 3 + i, cy - 3 + i, 2, 2, dark);
            rect(c, cx + 2 - i, cy - 3 + i, 2, 2, dark);
        }
    }
}
