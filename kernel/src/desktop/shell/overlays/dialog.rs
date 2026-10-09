//! The confirmation sheet.

use crate::desktop::shell::*;
use crate::desktop::*;
use crate::text::{self, BODY, TITLE3, Weight};

/// Corner radius of the Busca panel and of the confirmation sheet.
pub(super) const R_PANEL: i32 = 12;

/// Confirmation sheet geometry: panel and the two buttons.
pub(super) fn sheet_geom(sw: i32, sh: i32) -> (Rect, Rect, Rect) {
    let panel = Rect::new(sw / 2 - 190, sh / 2 - 110, 380, 220);
    let ok = Rect::new(panel.right() - 20 - 120, panel.bottom() - 20 - 32, 120, 32);
    let cancel = Rect::new(ok.x - 10 - 120, ok.y, 120, 32);
    (panel, cancel, ok)
}

impl Desktop {
    // ---- confirmation sheet ----

    pub(super) fn draw_dialog(&self, c: &mut Canvas, d: &Dialog) {
        let p = theme::pal();
        let fade = level(&d.t);
        let full = Rect::new(0, 0, self.sw, self.sh);
        c.blend_rect(full, Color::rgb(0, 0, 0), (110 * fade / 256) as u16);
        let (r, cancel, ok) = sheet_geom(self.sw, self.sh);
        let r = Rect::new(r.x, r.y - ((256 - fade) as i32 * 10) / 256, r.w, r.h);
        let dy = r.y - sheet_geom(self.sw, self.sh).0.y;
        let hole = Rect::new(r.x, r.y + R_PANEL, r.w, r.h - 2 * R_PANEL);
        c.draw_shadow(
            r,
            Shadow {
                blur: 28,
                dy: 16,
                alpha: (110 * fade / 256).min(255),
            },
            hole,
        );
        c.fill_rrect(
            r,
            R_PANEL,
            Corner::Circle,
            theme::solid(p.window_bg),
            fade as u16,
        );
        ui::stroke_token(c, r, R_PANEL, p.separator);
        if fade < 150 {
            return;
        }
        c.blit_surface(icons::surface(Icon::Brand, 56), r.x + 24, r.y + 22, 256);
        text::draw(
            c,
            r.x + 96,
            r.y + 26,
            &d.title,
            TITLE3,
            Weight::Semibold,
            theme::solid(p.text),
        );
        for (i, (a, b)) in text::wrap(&d.body, BODY, Weight::Regular, r.w - 96 - 24, 3)
            .into_iter()
            .enumerate()
        {
            text::draw(
                c,
                r.x + 96,
                r.y + 54 + i as i32 * 19,
                &d.body[a..b],
                BODY,
                Weight::Regular,
                theme::solid(p.text_secondary),
            );
        }
        let st = |i: usize| {
            if d.focus == i {
                ui::Control::Hover
            } else {
                ui::Control::Normal
            }
        };
        let off = |rc: Rect| Rect::new(rc.x, rc.y + dy, rc.w, rc.h);
        ui::push_button(
            c,
            off(cancel),
            kitsune_core::t!("common.cancel"),
            ui::ButtonKind::Secondary,
            st(0),
        );
        ui::push_button(
            c,
            off(ok),
            &d.ok,
            if d.cmd == Cmd::Shutdown {
                ui::ButtonKind::Destructive
            } else {
                ui::ButtonKind::Primary
            },
            st(1),
        );
    }

    pub(super) fn dialog_key(&mut self, key: Key) {
        match key {
            Key::Esc => self.close_dialog(),
            Key::Left | Key::Right | Key::Tab => {
                if let Some(d) = self.shell.dialog.as_mut() {
                    d.focus = 1 - d.focus;
                }
            }
            Key::Enter => {
                let go = self.shell.dialog.as_ref().map(|d| (d.focus == 1, d.cmd));
                self.close_dialog();
                if let Some((true, cmd)) = go {
                    self.power_now(cmd);
                }
            }
            _ => {}
        }
        self.force_full = true;
    }
}
