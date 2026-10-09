//! Window chrome: the flat title bar (app icon and left-aligned title, menu button and the
//! minimise / maximise / close buttons at the right), the hairline outline, the shadow and
//! the focus transition. The window's content is drawn by the app inside the rectangle below
//! the title bar. See `docs/design/ui-identity.md`.

use super::*;
use crate::text::{self, BODY, Weight};
use osjeff_core::anim::{Tween, curves};
use osjeff_core::style::{R_CONTROL, R_WINDOW};
use osjeff_core::window::TitleBtn;

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

        // Focus signature: a 2 px accent line along the top edge, following the corners.
        if mix > 0 {
            let line = Rect::new(r.x, r.y, r.w, 2)
                .intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0));
            let saved = c.set_clip(line);
            c.fill_rrect(
                r,
                radius,
                Corner::Circle,
                theme::accent(),
                (mix * 230 / 256) as u16,
            );
            c.restore_clip(saved);
        }
        self.draw_title(c, win, r, mix, p);

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
            App::Tarefas(t) => self.draw_tarefas(c, r, t),
            App::Calculator(k) => self.draw_calculator(c, r, k),
            App::Browser(b) => self.draw_browser(c, r, focused, b),
            App::Wasm(w) => self.draw_wasm(c, r, w),
            App::Files(f) => self.draw_files(c, r, f, focused),
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
            // A clear inner highlight just inside the crisp 1 px border.
            let inner = if theme::dark() { 26u16 } else { 170 };
            c.stroke_rrect(
                r.inflated(-1),
                (radius - 1).max(0),
                Corner::Circle,
                theme::WHITE,
                inner,
            );
        }
    }

    /// The title bar's contents: the app icon, the title (left-aligned, Medium, cut before
    /// the buttons), the menu button and the three window buttons.
    fn draw_title(
        &self,
        c: &mut Canvas,
        win: &Win,
        r: Rect,
        mix: u32,
        p: &osjeff_core::style::Palette,
    ) {
        let lay = r.title_layout(win.resizable, true);
        let a = (mix * 255 / 256) as u16;
        icons::blit(
            c,
            win.app.kind().icon(),
            lay.icon.x,
            lay.icon.y,
            lay.icon.w,
            150 + mix * 106 / 256,
        );
        let fg = theme::solid(p.title_inactive).lerp(theme::solid(p.text), a);
        let room = (lay.title_right - lay.title_x).max(0);
        let title = text::ellipsize(&win.app.title, BODY, Weight::Medium, room);
        let ty = text::center_y(r.y, TITLE_H, BODY, Weight::Medium);
        text::draw(c, lay.title_x, ty, &title, BODY, Weight::Medium, fg);

        let hover = self
            .title_hover
            .filter(|(id, _)| *id == win.id)
            .map(|(_, b)| b);
        draw_title_buttons(c, &lay, hover, win.maximized || win.snap.is_some(), a, p);
    }

    /// The pointer of a window drag is at `(cx, cy)`: show, move or hide the snap preview.
    pub(crate) fn update_snap_preview(&mut self, id: WindowId, cx: i32, cy: i32) {
        use osjeff_core::snap;
        let zone = self
            .wm
            .get(id)
            .filter(|w| w.resizable)
            .and_then(|_| snap::zone_at(cx, cy, self.sw, self.sh));
        let Some(zone) = zone else {
            self.shell.snap = None;
            return;
        };
        if self.shell.snap.as_ref().is_some_and(|p| p.zone == zone) {
            return;
        }
        let Some((rect, min_w, min_h)) = self.wm.get(id).map(|w| (w.rect, w.min_w, w.min_h)) else {
            return;
        };
        let to = snap::zone_rect_min(zone, self.work_area(), min_w, min_h);
        // Start from where the previous preview was (or the window itself).
        let from = self.snap_preview_rect().unwrap_or(rect);
        let mut t = Tween::at(0.0);
        t.retarget(1.0, 0.18, curves::ENTER);
        self.shell.snap = Some(super::shell::SnapPreview { zone, from, to, t });
    }

    /// Where the snap preview is right now (it grows from the window to the zone).
    pub(crate) fn snap_preview_rect(&self) -> Option<Rect> {
        let p = self.shell.snap.as_ref()?;
        let t = (p.t.value().clamp(0.0, 1.0) * 256.0) as i32;
        Some(osjeff_core::snap::lerp_rect(p.from, p.to, t))
    }

    /// The translucent outline previewing where a dragged window will snap, travelling from the
    /// window to the zone while it fades in.
    pub(crate) fn draw_snap_preview(&self, c: &mut Canvas) {
        let Some(r) = self.snap_preview_rect() else {
            return;
        };
        let t = self
            .shell
            .snap
            .as_ref()
            .map_or(1.0, |p| p.t.value().clamp(0.0, 1.0));
        let a = (t * 256.0) as u32;
        let acc = theme::accent();
        c.fill_rrect(r, R_WINDOW, Corner::Circle, acc, (a * 46 / 256) as u16);
        c.stroke_rrect(r, R_WINDOW, Corner::Circle, acc, (a * 230 / 256) as u16);
        c.stroke_rrect(
            r.inflated(-1),
            (R_WINDOW - 1).max(0),
            Corner::Circle,
            acc,
            (a * 120 / 256) as u16,
        );
    }
}

/// The menu button and the minimise, maximise / restore and close buttons of a title bar laid
/// out as `lay`: flat glyphs, a rounded fill under the pointer (red for close), dimmed by `a`
/// (0..=255) when the window is not focused. Shared with the component gallery.
pub(super) fn draw_title_buttons(
    c: &mut Canvas,
    lay: &osjeff_core::window::TitleLayout,
    hover: Option<TitleBtn>,
    restore_glyph: bool,
    a: u16,
    p: &osjeff_core::style::Palette,
) {
    let ink = theme::solid(p.title_inactive).lerp(theme::solid(p.text_secondary), a);
    let cell = |c: &mut Canvas, rect: Rect, btn: TitleBtn| -> Color {
        let over = hover == Some(btn);
        if over {
            let pill = Rect::new(rect.x + 2, rect.y + 3, rect.w - 4, rect.h - 6);
            if btn == TitleBtn::Close {
                c.fill_rrect(pill, R_CONTROL, Corner::Circle, CLOSE_HOVER, 256);
            } else {
                let (hc, ha) = theme::tint(p.hover);
                c.fill_rrect(pill, R_CONTROL, Corner::Circle, hc, (ha * 2).min(256));
            }
        }
        match (over, btn) {
            (true, TitleBtn::Close) => theme::WHITE,
            (true, _) => theme::solid(p.text),
            _ => ink,
        }
    };
    if let Some(m) = lay.menu {
        let col = cell(c, m, TitleBtn::Menu);
        let (cx, cy) = (m.x + m.w / 2, m.y + m.h / 2);
        for dy in [-4, 0, 4] {
            c.blend_rect(Rect::new(cx - 6, cy + dy, 12, 1), col, 256);
        }
    }
    let col = cell(c, lay.min, TitleBtn::Minimize);
    let (cx, cy) = (lay.min.x + lay.min.w / 2, lay.min.y + lay.min.h / 2);
    c.blend_rect(Rect::new(cx - 5, cy + 3, 10, 1), col, 256);
    if let Some(m) = lay.max {
        let col = cell(c, m, TitleBtn::Maximize);
        let (cx, cy) = (m.x + m.w / 2, m.y + m.h / 2);
        if restore_glyph {
            // Restore: a square in front of the corner of a second one.
            let front = Rect::new(cx - 5, cy - 3, 8, 8);
            outline(c, front, col);
            c.blend_rect(Rect::new(cx - 3, cy - 5, 8, 1), col, 256);
            c.blend_rect(Rect::new(cx + 4, cy - 5, 1, 8), col, 256);
        } else {
            outline(c, Rect::new(cx - 5, cy - 5, 10, 10), col);
        }
    }
    let col = cell(c, lay.close, TitleBtn::Close);
    let (cx, cy) = (lay.close.x + lay.close.w / 2, lay.close.y + lay.close.h / 2);
    ui::draw_glyph(
        c,
        iconart::Glyph::Close,
        cx - 8,
        cy - 8,
        16,
        0xFF00_0000 | pack(col),
    );
}

/// A crisp 1 px square outline.
fn outline(c: &mut Canvas, r: Rect, col: Color) {
    c.blend_rect(Rect::new(r.x, r.y, r.w, 1), col, 256);
    c.blend_rect(Rect::new(r.x, r.bottom() - 1, r.w, 1), col, 256);
    c.blend_rect(Rect::new(r.x, r.y + 1, 1, r.h - 2), col, 256);
    c.blend_rect(Rect::new(r.right() - 1, r.y + 1, 1, r.h - 2), col, 256);
}

/// Fill of the close button under the pointer.
const CLOSE_HOVER: Color = Color::rgb(0xE5, 0x48, 0x4D);

fn pack(c: Color) -> u32 {
    ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32
}
