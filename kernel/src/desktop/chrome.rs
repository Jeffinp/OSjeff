//! Window chrome: the unified title bar with traffic lights at the left, the
//! hairline outline, the shadow and the focus transition. The window's content is
//! drawn by the app inside the rectangle below the title bar.

use super::*;
use crate::text::{self, BODY, Weight};
use osjeff_core::anim::{Tween, curves};
use osjeff_core::style::R_WINDOW;

/// Seconds the title bar and shadow take to change between focused and not.
const FOCUS_SECS: f32 = 0.12;

/// Per-window focus level (0 = inactive .. 1 = focused) with its transition.
pub(crate) struct FocusMix {
    pub id: WindowId,
    pub level: Tween,
}

impl Desktop {
    /// How focused window `w` looks right now, 0..=256 (animated across focus changes).
    pub(crate) fn focus_mix(&self, id: WindowId, focused: bool) -> u32 {
        match self.focus_mix.borrow().iter().find(|f| f.id == id) {
            Some(f) => (f.level.value().clamp(0.0, 1.0) * 256.0) as u32,
            None => {
                if focused {
                    256
                } else {
                    0
                }
            }
        }
    }

    /// True while `id`'s focus transition runs (it is redrawn every frame then).
    pub(crate) fn focus_busy(&self, id: WindowId) -> bool {
        self.focus_mix
            .borrow()
            .iter()
            .any(|f| f.id == id && !f.level.finished())
    }

    /// Track focus changes and advance their transitions. Returns whether any runs.
    pub(crate) fn step_focus(&mut self, dt: f32) -> bool {
        let focused = self.focused();
        let wins: Vec<WindowId> = self.wm.windows().iter().map(|w| w.id).collect();
        let mut mix = self.focus_mix.borrow_mut();
        mix.retain(|f| wins.contains(&f.id));
        for &id in &wins {
            let target = if focused == Some(id) { 1.0 } else { 0.0 };
            match mix.iter_mut().find(|f| f.id == id) {
                Some(f) => {
                    if f.level.target() != target {
                        f.level.retarget(target, FOCUS_SECS, curves::STANDARD);
                    }
                }
                None => mix.push(FocusMix {
                    id,
                    level: Tween::at(target),
                }),
            }
        }
        let mut busy = false;
        for f in mix.iter_mut() {
            busy |= f.level.step(dt);
        }
        busy
    }

    /// Draw window `win` (rectangle `r`, which may differ from `win.rect` while it
    /// animates): shadow, unified title bar, traffic lights, title, outline and the
    /// app's content.
    pub(crate) fn draw_window(
        &self,
        c: &mut Canvas,
        win: &Win,
        r: Rect,
        focused: bool,
        shadow: bool,
    ) {
        let p = theme::pal();
        let mix = self.focus_mix(win.id, focused);
        let th = TITLE_H;
        let radius = if win.maximized && win.zoom.is_none() {
            0
        } else {
            R_WINDOW.min(r.w / 2).min(r.h / 2)
        };

        if shadow && radius > 0 {
            let hole = Rect::new(r.x, r.y + radius, r.w, (r.h - 2 * radius).max(0));
            // A window being dragged keeps only its ambient layer: half the shadow cost.
            let layers = if self.drag.as_ref().is_some_and(|d| d.win == win.id) {
                1
            } else {
                2
            };
            let (hi, lo) = (window_shadow(true, 256), window_shadow(false, 256));
            for k in 0..layers {
                let l = |a: u32, b: u32| (b * (256 - mix) + a * mix) / 256;
                let sh = Shadow {
                    blur: ((hi[k].blur * mix as i32 + lo[k].blur * (256 - mix as i32)) / 256)
                        .max(1),
                    dy: (hi[k].dy * mix as i32 + lo[k].dy * (256 - mix as i32)) / 256,
                    alpha: l(hi[k].alpha, lo[k].alpha),
                };
                c.draw_shadow(r, sh, hole);
            }
        }

        // Bottom corners are repainted after the app's content so a square fill cannot
        // poke out of the rounded shape: remember what is behind them now.
        let corner = radius.max(1);
        let (bl, br) = (
            Rect::new(r.x, r.bottom() - corner, corner, corner),
            Rect::new(r.right() - corner, r.bottom() - corner, corner, corner),
        );
        let (mut saved_l, mut saved_r) = (Vec::new(), Vec::new());
        if radius > 0 {
            c.read_region(bl, &mut saved_l);
            c.read_region(br, &mut saved_r);
        }

        // Body, then the unified title bar over its top band.
        c.fill_rrect(r, radius, Corner::Circle, theme::window_body(), 256);
        let active_bg = theme::solid(p.window_bg);
        let inactive_bg = if theme::dark() {
            Color::rgb(0x27, 0x27, 0x2A)
        } else {
            Color::rgb(0xEC, 0xEC, 0xF0)
        };
        let bar_bg = inactive_bg.lerp(active_bg, (mix * 255 / 256) as u16);
        let band = Rect::new(r.x, r.y, r.w, th);
        let saved = c.set_clip(
            band.intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        );
        c.fill_rrect(r, radius, Corner::Circle, bar_bg, 256);
        c.restore_clip(saved);
        // Hairline under the bar.
        let (sc, sa) = theme::tint(p.separator);
        c.blend_rect(Rect::new(r.x, r.y + th - 1, r.w, 1), sc, sa);

        self.draw_lights(c, win, r, mix, p);
        // Title: centred on the window, clipped so it never reaches the lights.
        let lights_right = r.max_rect().right() + 14;
        let room = (r.w - 2 * (lights_right - r.x)).max(60).min(r.w - 24);
        let title = text::ellipsize(&win.app.title, BODY, Weight::Medium, room);
        let tw = text::measure(&title, BODY, Weight::Medium);
        let tx = (r.x + (r.w - tw) / 2).max(lights_right.min(r.right() - tw - 8));
        let fg =
            theme::solid(p.title_inactive).lerp(theme::solid(p.text), (mix * 255 / 256) as u16);
        let ty = text::center_y(r.y, th, BODY, Weight::Medium);
        text::draw(c, tx, ty, &title, BODY, Weight::Medium, fg);

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
            App::Gallery(g) => self.draw_gallery(c, r, g),
        }
        if let Some(cs) = saved_clip {
            c.restore_clip(cs);
        }
        // What this window cost to draw (the monitor's per-app CPU figure).
        win.app
            .cost
            .set(win.app.cost.get() + crate::io::rdtsc().wrapping_sub(cost_t0));

        if radius > 0 {
            c.restore_corner(bl, &saved_l, radius, false);
            c.restore_corner(br, &saved_r, radius, true);
        }
        // Outline last, over the content.
        let edge = if theme::dark() {
            0x55FF_FFFFu32
        } else {
            0x2E00_0000
        };
        let (ec, ea) = theme::tint(edge);
        if radius > 0 {
            c.stroke_rrect(r, radius, Corner::Circle, ec, ea.min(96));
        }
    }

    /// The three lights: coloured (red, amber, green) when the window is focused or
    /// the pointer is over them, grey otherwise; glyphs appear on hover.
    fn draw_lights(
        &self,
        c: &mut Canvas,
        win: &Win,
        r: Rect,
        mix: u32,
        p: &osjeff_core::style::Palette,
    ) {
        let over =
            self.hover == Some(win.id) && r.lights_rect().contains(self.cursor_x, self.cursor_y);
        let colored = mix > 128 || over;
        let grey = theme::solid(p.light_inactive);
        let discs = [
            (r.close_rect(), theme::CLOSE, true),
            (r.min_rect(), theme::MINIMIZE, true),
            (r.max_rect(), theme::MAXIMIZE, win.resizable),
        ];
        for (rect, col, enabled) in discs {
            let col = if colored && enabled { col } else { grey };
            c.fill_rrect(rect, rect.w / 2, Corner::Circle, col, 256);
            c.stroke_rrect(rect, rect.w / 2, Corner::Circle, Color::rgb(0, 0, 0), 38);
        }
        if !over {
            return;
        }
        let ink = Color::rgb(0x3A, 0x1A, 0x16);
        let stroke = |c: &mut Canvas, rect: Rect, g: iconart::Glyph| {
            ui::draw_glyph(c, g, rect.x - 2, rect.y - 2, 16, 0xFF00_0000 | pack(ink));
        };
        stroke(c, r.close_rect(), iconart::Glyph::Close);
        stroke(c, r.min_rect(), iconart::Glyph::Minus);
        if win.resizable {
            stroke(
                c,
                r.max_rect(),
                if win.maximized {
                    iconart::Glyph::Minus
                } else {
                    iconart::Glyph::Plus
                },
            );
        }
    }
}

fn pack(c: Color) -> u32 {
    ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32
}
