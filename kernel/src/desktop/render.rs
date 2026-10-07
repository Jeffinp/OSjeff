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
            match w.anim {
                Some(_) => self.draw_animating(&mut c, w, rect, is_focus),
                None => {
                    let shadow = self.drag.is_none() && !w.maximized;
                    self.draw_window(&mut c, w, rect, is_focus, shadow);
                }
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
            if self
                .wm
                .windows()
                .iter()
                .any(|w| w.minimized && w.app.kind() == kind)
            {
                let r = icons[kind.index() + 1];
                c.fill_round_rect(
                    (r.x + r.w / 2 - 3) as usize,
                    (r.bottom() + 4) as usize,
                    6,
                    6,
                    3,
                    theme::ACCENT,
                );
            }
        }
    }

    /// Draws an animating window: snapshot the backdrop, draw the window (no
    /// shadow), then fade toward the snapshot so lower windows show through.
    pub(crate) fn draw_animating(&self, c: &mut Canvas, w: &Win, rect: Rect, focused: bool) {
        let alpha = match w.anim {
            Some(a) => (a.alpha() * 256.0) as u16,
            None => return,
        };
        let scratch = scratch_slice();
        let x = rect.x.max(0) as usize;
        let y = rect.y.max(0) as usize;
        let (rw, rh) = (rect.w as usize, rect.h as usize);
        if rw * rh * c.bpp() <= scratch.len() {
            c.snapshot_region(scratch, x, y, rw, rh);
            self.draw_window(c, w, rect, focused, false);
            c.blend_from_local(scratch, x, y, rw, rh, alpha);
        } else {
            self.draw_window(c, w, rect, focused, false);
        }
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
                damage = damage.union(&self.window_box(w));
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
                    if w.anim.is_some() {
                        self.draw_animating(&mut c, w, self.window_box(w), is_focus);
                    } else {
                        // Dragged window: opaque, no shadow (matches the steady
                        // drag look and keeps the damage rect tight to the body).
                        self.draw_window(&mut c, w, self.window_box(w), is_focus, false);
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
            let r = radius.min(w / 2).min(h / 2);
            let hole = (x, y + r, w, h.saturating_sub(2 * r));
            for &(off, exp, a) in &[(6usize, 4usize, 28u16), (14, 12, 14)] {
                let sx = x.saturating_sub(exp);
                let sy = y + off;
                c.fill_round_rect_alpha_skip(
                    sx,
                    sy,
                    w + exp * 2,
                    h + exp,
                    14 + exp,
                    theme::SHADOW,
                    a,
                    hole,
                );
            }
        }
        // Body + dark header.
        c.fill_round_rect(x, y, w, h, radius, theme::WINDOW_BODY);
        let header = if focused {
            theme::HEADER
        } else {
            theme::HEADER_DIM
        };
        c.fill_rect(x, y + radius, w, th - radius, header);
        c.fill_round_rect(x, y, w, th, radius, header);
        // Accent top line marks focus (teal) vs unfocused (muted).
        let accent = if focused {
            theme::ACCENT
        } else {
            theme::TEXT_MUTED
        };
        c.fill_round_rect(x + radius, y, w - radius * 2, 3, 1, accent);

        // App indicator dot + title (clipped before the title-bar buttons).
        c.fill_round_rect(x + 12, y + 11, 8, 8, 4, accent);
        let title = win.app.title.as_bytes();
        let room = (r.min_rect().x - (r.x + 28) - 6).max(0) as usize / font::cell_w(2);
        font::draw_bytes(
            c,
            x + 28,
            y + 8,
            &title[..title.len().min(room)],
            theme::HEADER_TEXT,
            2,
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

        match &win.app.app {
            App::Terminal(t) => self.draw_terminal(c, r, t, focused),
            App::Editor(e) => self.draw_editor(c, r, e, focused),
            App::TaskMgr => self.draw_taskmgr(c, r),
            App::Calculator(k) => self.draw_calculator(c, r, k),
            App::Browser(b) => self.draw_browser(c, r, focused, b),
            App::Wasm(w) => self.draw_wasm(c, r, w),
            App::Files(f) => self.draw_files(c, r, f),
        }
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
