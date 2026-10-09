//! Keyboard, wheel, pan and click handling of the viewer.

use crate::desktop::kit::appui::{self};
use crate::desktop::*;
use kitsune_core::anim::{Tween, curves};
use kitsune_core::t;
use kitsune_core::viewer as vw;
use kitsune_core::viewer::ui::{self as vui, FitMode, Hit, Layout};

impl Desktop {
    /// Keys of the viewer (after the global shortcuts).
    pub(crate) fn viewer_key(&mut self, id: WindowId, key: Key) {
        let shift = self.keymap.shift();
        let (vpw, vph) = self.viewer_vp(id);
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        // The save-as sheet owns the keys while it is open.
        if let Some(input) = v.save.as_mut() {
            match key {
                Key::Esc => v.save = None,
                Key::Enter => self.viewer_save_commit(id),
                Key::Backspace => input.backspace(),
                Key::Delete => input.delete(),
                Key::Left => input.left(),
                Key::Right => input.right(),
                Key::Home => input.home(),
                Key::End => input.end(),
                Key::Char(b) => input.insert(b),
                _ => {}
            }
            return;
        }
        v.msg = None;
        let dims = v.image.as_ref().map(|i| (i.width(), i.height()));
        // Any key but the slideshow's own stops it; Esc then only stops it.
        let was_running = v.slideshow.running();
        if was_running && !matches!(key, Key::Char(b' ')) {
            v.slideshow.stop();
        }
        v.inertia.stop();
        match key {
            Key::Esc if was_running => {}
            Key::Esc => self.request_close(id),
            Key::Char(b' ') => self.viewer_toggle_slideshow(id),
            Key::Char(b'+') | Key::Char(b'=') => {
                if let Some((iw, ih)) = dims {
                    v.view.zoom_in(iw, ih, vpw, vph);
                }
            }
            Key::Char(b'-') | Key::Char(b'_') => {
                if let Some((iw, ih)) = dims {
                    v.view.zoom_out(iw, ih, vpw, vph);
                }
            }
            Key::Char(b'1') => self.viewer_set_mode(id, FitMode::Actual),
            Key::Char(b'0') => self.viewer_set_mode(id, FitMode::Fit),
            Key::Char(b'9') => self.viewer_set_mode(id, FitMode::Fill),
            Key::Char(b'r') | Key::Char(b'R') => self.viewer_rotate(id, !shift),
            Key::Char(b'h') | Key::Char(b'H') => {
                if let Some(img) = v.image.as_mut() {
                    img.flip_horizontal();
                    v.scaled = None;
                    v.enter_t = Tween::at(0.5);
                    v.enter_t.retarget(1.0, 0.2, curves::ENTER);
                }
            }
            Key::Char(b'v') | Key::Char(b'V') => {
                if let Some(img) = v.image.as_mut() {
                    img.flip_vertical();
                    v.scaled = None;
                    v.enter_t = Tween::at(0.5);
                    v.enter_t.retarget(1.0, 0.2, curves::ENTER);
                }
            }
            Key::Char(b'i') | Key::Char(b'I') => self.viewer_toggle_info(id),
            Key::Char(b's') | Key::Char(b'S') => self.viewer_save_prompt(id),
            Key::Char(b'w') | Key::Char(b'W') => {
                let path = v.path.clone();
                let r = self.set_wallpaper_path(&path);
                if let Some(v) = self.viewer_mut(id) {
                    v.msg = Some(match r {
                        Some(m) => (m, true),
                        None => (String::from(t!("viewer.wallpaper")), false),
                    });
                }
            }
            Key::Left if shift => {
                if let Some((iw, ih)) = dims {
                    v.view.pan_by(vw::PAN_STEP, 0, iw, ih, vpw, vph);
                }
            }
            Key::Right if shift => {
                if let Some((iw, ih)) = dims {
                    v.view.pan_by(-vw::PAN_STEP, 0, iw, ih, vpw, vph);
                }
            }
            Key::Up => {
                if let Some((iw, ih)) = dims {
                    v.view.pan_by(0, vw::PAN_STEP, iw, ih, vpw, vph);
                }
            }
            Key::Down => {
                if let Some((iw, ih)) = dims {
                    v.view.pan_by(0, -vw::PAN_STEP, iw, ih, vpw, vph);
                }
            }
            Key::Home => self.viewer_goto(id, 0),
            Key::End => {
                let last = self
                    .viewer_mut(id)
                    .map_or(0, |v| v.list.len().saturating_sub(1));
                self.viewer_goto(id, last);
            }
            Key::Left => self.viewer_step(id, false),
            Key::Right => self.viewer_step(id, true),
            _ => {}
        }
        self.viewer_sync(id);
    }

    /// PageUp / PageDown / numpad +/-: previous / next image, zoom.
    pub(crate) fn viewer_special(&mut self, id: WindowId, sp: Special) -> bool {
        match sp {
            Special::PageUp => self.viewer_step(id, false),
            Special::PageDown => self.viewer_step(id, true),
            Special::KpPlus => self.viewer_key(id, Key::Char(b'+')),
            Special::KpMinus => self.viewer_key(id, Key::Char(b'-')),
            _ => return false,
        }
        true
    }

    /// Mouse wheel over the viewer: zoom around the cursor.
    pub(crate) fn viewer_wheel(&mut self, id: WindowId, notches: i32) {
        let Some(lay) = self.viewer_layout(id) else {
            return;
        };
        let (vpw, vph) = (lay.canvas.w, lay.canvas.h);
        // Over the filmstrip the wheel scrolls it.
        if lay
            .strip
            .is_some_and(|s| s.contains(self.cursor_x, self.cursor_y))
        {
            if let Some(v) = self.viewer_mut(id) {
                let max = (vui::strip_content_w(v.list.len()) - lay.strip.map_or(0, |s| s.w)).max(0)
                    as f32;
                let t = (v.strip_scroll.target() + notches as f32 * 64.0).clamp(0.0, max);
                v.strip_scroll.set_target(t);
            }
            return;
        }
        let ax = self.cursor_x - (lay.canvas.x + vpw / 2);
        let ay = self.cursor_y - (lay.canvas.y + vph / 2);
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        let Some((iw, ih)) = v.image.as_ref().map(|i| (i.width(), i.height())) else {
            return;
        };
        v.inertia.stop();
        v.slideshow.stop();
        for _ in 0..notches.unsigned_abs().min(8) {
            let z = if notches > 0 {
                vw::next_zoom(v.view.zoom)
            } else {
                vw::prev_zoom(v.view.zoom)
            };
            v.view.set_zoom_at(z, (ax, ay), iw, ih, vpw, vph);
        }
        self.viewer_sync(id);
    }

    /// Pan by a mouse drag step (the picture follows the pointer exactly; the speed feeds the
    /// glide that starts when the button is released).
    pub(crate) fn viewer_pan(&mut self, id: WindowId, dx: i32, dy: i32) {
        let (vpw, vph) = self.viewer_vp(id);
        let now = appui::ticks();
        if let Some(v) = self.viewer_mut(id)
            && let Some((iw, ih)) = v.image.as_ref().map(|i| (i.width(), i.height()))
        {
            let dt = now.saturating_sub(v.drag_tick).max(1) as f32 / 250.0;
            v.drag_tick = now;
            let before = (v.view.pan_x, v.view.pan_y);
            v.view.pan_by(dx, dy, iw, ih, vpw, vph);
            // Only the movement that took effect counts towards the speed.
            v.inertia
                .push(v.view.pan_x - before.0, v.view.pan_y - before.1, dt);
            v.pan_x_s.jump(v.view.pan_x as f32);
            v.pan_y_s.jump(v.view.pan_y as f32);
        }
    }

    /// The left button was released after a drag on the picture: let it glide.
    pub(crate) fn viewer_release(&mut self, id: WindowId) {
        let now = appui::ticks();
        if let Some(v) = self.viewer_mut(id) {
            let idle = now.saturating_sub(v.drag_tick) as f32 / 250.0;
            v.inertia.release(idle);
        }
    }

    /// A left press inside the viewer window (content area).
    pub(crate) fn viewer_click(&mut self, id: WindowId, _rect: Rect, px: i32, py: i32) {
        let Some(lay) = self.viewer_layout(id) else {
            return;
        };
        let (count, scroll, saving) = match self.viewer_mut(id) {
            Some(v) => (
                v.list.len(),
                v.strip_scroll.value() as i32,
                v.save.is_some(),
            ),
            None => return,
        };
        if saving {
            self.viewer_sheet_click(id, &lay, px, py);
            return;
        }
        let Some(hit) = lay.hit(px, py, scroll, count) else {
            return;
        };
        let has_image = self.viewer_mut(id).is_some_and(|v| v.image.is_some());
        match hit {
            Hit::RotateLeft if has_image => self.viewer_rotate(id, false),
            Hit::RotateRight if has_image => self.viewer_rotate(id, true),
            Hit::Flip if has_image => self.viewer_key(id, Key::Char(b'h')),
            Hit::Mode(m) if has_image => self.viewer_set_mode(id, m),
            Hit::Slideshow => self.viewer_toggle_slideshow(id),
            Hit::Info if has_image => self.viewer_toggle_info(id),
            Hit::Save if has_image => self.viewer_save_prompt(id),
            Hit::Thumb(i) => {
                if let Some(v) = self.viewer_mut(id) {
                    v.slideshow.stop();
                }
                self.viewer_goto(id, i);
            }
            Hit::Canvas => {
                let double = self.clicks.press(crate::interrupts::ticks(), px, py, id);
                if double {
                    let fit = self.viewer_mut(id).is_some_and(|v| v.view.fit);
                    self.viewer_set_mode(id, if fit { FitMode::Actual } else { FitMode::Fit });
                    return;
                }
                if let Some(v) = self.viewer_mut(id) {
                    v.inertia.grab();
                    v.drag_tick = appui::ticks();
                    v.slideshow.stop();
                }
                self.drag = Some(Drag {
                    win: id,
                    mode: DragMode::Pan {
                        last_x: px,
                        last_y: py,
                    },
                });
            }
            _ => {}
        }
        self.viewer_sync(id);
    }

    /// A press while the save sheet is up: its buttons.
    fn viewer_sheet_click(&mut self, id: WindowId, lay: &Layout, px: i32, py: i32) {
        let panel = appui::sheet_rect(lay.window, (420, 176));
        let btns = appui::button_row(
            panel.right() - appui::SHEET_PAD,
            panel.bottom() - appui::SHEET_PAD - appui::BUTTON_H,
            &[t!("common.cancel"), t!("viewer.save")],
        );
        match btns.iter().position(|b| b.contains(px, py)) {
            Some(0) => {
                if let Some(v) = self.viewer_mut(id) {
                    v.save = None;
                }
            }
            Some(_) => self.viewer_save_commit(id),
            None => {}
        }
    }

    /// The pointer moved over viewer `id`.
    pub(crate) fn viewer_hover(&mut self, id: WindowId, px: i32, py: i32) -> bool {
        let Some(lay) = self.viewer_layout(id) else {
            return false;
        };
        let Some(v) = self.viewer_mut(id) else {
            return false;
        };
        let hit = lay
            .hit(px, py, v.strip_scroll.value() as i32, v.list.len())
            .filter(|h| !matches!(h, Hit::Canvas | Hit::Dead | Hit::InfoPanel));
        if v.hover == hit {
            return false;
        }
        v.hover = hit;
        v.hover_t = Tween::at(0.0);
        v.hover_t.retarget(1.0, 0.12, curves::STANDARD);
        true
    }

    pub(crate) fn viewer_unhover(&mut self, id: WindowId) {
        if let Some(v) = self.viewer_mut(id) {
            v.hover = None;
        }
    }

    /// Refit viewers after a resize or maximize.
    pub(crate) fn relayout_viewer(&mut self, id: WindowId) {
        if self.kind_of(id) == Some(Kind::Viewer) {
            self.viewer_sync(id);
            self.viewer_aim_strip(id, false);
        }
    }
}
