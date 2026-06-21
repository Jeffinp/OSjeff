//! `Desktop` methods: render. Split out of the former monolithic desktop.rs.

use super::*;

impl Desktop {
    // ---- rendering ----

    /// Full recompose of the whole scene into `back`. Used when not animating
    /// (input/clock changes). The animation path uses the cheaper damage-based
    /// [`render_anim_frame`] instead.
    pub fn render(&self, back: &mut [u8], info: bootloader_api::info::FrameBufferInfo, time: Time) {
        let mut c = Canvas::new(back, info);
        for i in 0..WIN_COUNT {
            let w = self.order[i];
            if !self.windows[w].visible {
                continue;
            }
            let focused = self.focused() == Some(w);
            let rect = self.window_box(w);
            match self.windows[w].anim {
                Some(_) => self.draw_animating(&mut c, w, rect, focused),
                None => self.draw_window(&mut c, w, rect, focused, self.drag.is_none()),
            }
        }
        draw_clock(&mut c, time);
        self.draw_overlay(&mut c);
    }

    /// Draws the transient overlays (right-click menu + start panel) on top of
    /// the composed scene. Separated so the compositor can repaint only their
    /// region on a hover change instead of recomposing the whole desktop.
    pub fn draw_overlay(&self, c: &mut Canvas) {
        if let Some((mx, my)) = self.menu {
            self.draw_menu(c, mx, my);
        }
        if self.start_open {
            self.draw_start(c);
        }
    }

    /// Draws an animating window: snapshot the backdrop, draw the window (no
    /// shadow), then fade toward the snapshot so lower windows show through.
    pub(crate) fn draw_animating(&self, c: &mut Canvas, w: usize, rect: Rect, focused: bool) {
        let alpha = match self.windows[w].anim {
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
    /// animating, and their z-order). When it changes, the cached static layer
    /// must be rebuilt.
    pub fn anim_signature(&self) -> u32 {
        let mut s = 0u32;
        for w in 0..WIN_COUNT {
            if self.windows[w].visible {
                s |= 1 << w;
            }
            if self.windows[w].anim.is_some() {
                s |= 1 << (w + 8);
            }
        }
        for (i, &w) in self.order.iter().enumerate() {
            s |= (w as u32 & 0x3) << (16 + i * 2);
        }
        // Fold in the drag target so the static layer is rebuilt when a drag
        // starts (the window leaves the static layer) and ends (it rejoins it),
        // but stays stable mid-drag so it is composed only once.
        if let Some(d) = &self.drag {
            s |= 1 << (24 + d.win as u32);
        }
        s
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
        for i in 0..WIN_COUNT {
            let w = self.order[i];
            if self.windows[w].visible && !self.is_dynamic(w) {
                let focused = self.focused() == Some(w);
                self.draw_window(&mut c, w, self.windows[w].rect, focused, true);
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

        // Damage = last frame's region + every animating window's box now.
        let mut damage = prev_damage;
        let mut lowest_anim_z = WIN_COUNT;
        for i in 0..WIN_COUNT {
            let w = self.order[i];
            if self.windows[w].visible && self.is_dynamic(w) {
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
            for i in 0..WIN_COUNT {
                let w = self.order[i];
                if self.windows[w].visible && self.is_dynamic(w) {
                    let focused = self.focused() == Some(w);
                    if self.windows[w].anim.is_some() {
                        self.draw_animating(&mut c, w, self.window_box(w), focused);
                    } else {
                        // Dragged window: opaque, no shadow (matches the steady
                        // drag look and keeps the damage rect tight to the body).
                        self.draw_window(&mut c, w, self.window_box(w), focused, false);
                    }
                }
            }
        }

        // Re-assert any static window that sits above an animating one (so the
        // animating window doesn't paint over a window that is in front of it).
        for i in (lowest_anim_z + 1)..WIN_COUNT {
            let w = self.order[i];
            if self.windows[w].visible
                && !self.is_dynamic(w)
                && let Some(clip) = self.windows[w].rect.intersection(&damage)
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
        win: usize,
        r: Rect,
        focused: bool,
        shadow: bool,
    ) {
        let x = r.x.max(0) as usize;
        let y = r.y.max(0) as usize;
        let w = r.w as usize;
        let h = r.h as usize;
        let th = TITLE_H as usize;

        // Soft drop shadow (two layers). Skipped while dragging/animating so the
        // alpha fill never costs on the hot path.
        if shadow {
            for &(off, exp, a) in &[(6usize, 4usize, 28u16), (14, 12, 14)] {
                let sx = x.saturating_sub(exp);
                let sy = y + off;
                c.fill_round_rect_alpha(sx, sy, w + exp * 2, h + exp, 14 + exp, theme::SHADOW, a);
            }
        }

        let radius = 12usize;
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

        // App indicator dot + title.
        c.fill_round_rect(x + 12, y + 11, 8, 8, 4, accent);
        font::draw_text(
            c,
            x + 28,
            y + 8,
            self.windows[win].title,
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

        match self.windows[win].kind {
            Kind::Terminal => self.draw_terminal(c, x, y, focused),
            Kind::Editor => self.draw_editor(c, x, y, focused),
            Kind::TaskMgr => self.draw_taskmgr(c, x, y),
            Kind::Calculator => self.draw_calculator(c, r, focused),
        }
    }
}
