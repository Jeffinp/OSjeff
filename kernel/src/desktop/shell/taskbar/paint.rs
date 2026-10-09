//! Drawing the bar: surface, icons, indicators, the sliver and the tooltip.

use super::state::BOUNCE_PX;
use crate::desktop::*;
use crate::text::FOOTNOTE;
use kitsune_core::anim::{self};
use kitsune_core::style::R_TASKBAR;
use kitsune_core::taskbar::{self as tb, Hit, Indicator};

impl Desktop {
    /// Draw the bar: shadow, surface, the Apps button, icons, indicators, the sliver, the tooltip.
    pub(crate) fn draw_dock(&self, c: &mut Canvas) {
        let p = theme::pal();
        let kinds = self.task_kinds();
        let l = tb::layout(self.sw, self.sh, kinds.len());
        let panel = l.panel;
        let d = &self.shell.task;
        // Shadow, surface, hairline and inner highlight.
        let hole = Rect::new(
            panel.x,
            panel.y + R_TASKBAR,
            panel.w,
            (panel.h - 2 * R_TASKBAR).max(0),
        );
        c.draw_shadow(
            panel,
            Shadow {
                blur: 10,
                dy: 4,
                alpha: 56,
            },
            hole,
        );
        let (tc, ta) = theme::tint(p.dock_tint);
        c.fill_rrect(panel, R_TASKBAR, Corner::Circle, tc, ta);
        let (ec, ea) = theme::tint(p.glass_edge);
        c.stroke_rrect(panel, R_TASKBAR, Corner::Circle, ec, ea);
        c.stroke_rrect(
            panel.inflated(1),
            R_TASKBAR + 1,
            Corner::Circle,
            Color::rgb(0, 0, 0),
            if theme::dark() { 70 } else { 22 },
        );
        // Separators.
        let (sc, sa) = theme::tint(p.separator);
        for x in [l.sep_apps, l.sep_sliver] {
            c.blend_rect(
                Rect::new(x, panel.y + 12, 1, panel.h - 24),
                sc,
                (sa * 2).min(256),
            );
        }
        // The Apps button.
        let hot = |h: Hit| d.hover == Some(h) && d.drag.is_none();
        if hot(Hit::Apps) || self.shell.apps.as_ref().is_some_and(|a| !a.closing) {
            ui::fill_token(c, l.apps.inflated(3), 10, p.hover);
        }
        icons::blit(c, Icon::Launchpad, l.apps.x, l.apps.y, tb::ICON, 256);
        // The sliver.
        let sl = Rect::new(
            l.sliver.x + 3,
            l.sliver.y + 2,
            l.sliver.w - 6,
            l.sliver.h - 4,
        );
        let (hc, ha) = theme::tint(p.hover);
        c.fill_rrect(
            sl,
            2,
            Corner::Circle,
            hc,
            if hot(Hit::Sliver) {
                (ha * 3).min(256)
            } else {
                ha
            },
        );
        let bounce_of = |k: Kind| -> i32 {
            d.bounce
                .iter()
                .find(|(kk, _)| *kk == k)
                .map_or(0, |(_, t)| anim::bounce(*t, BOUNCE_PX).0 as i32)
        };
        let dragging = d.drag.as_ref().map(|dr| dr.from);
        let mut dragged: Option<(Kind, Rect)> = None;
        let mut tip: Option<(Rect, Kind)> = None;
        for (i, k) in kinds.iter().enumerate() {
            let rest = l.items[i];
            let x =
                d.xs.iter()
                    .find(|(kk, _)| kk == k)
                    .map_or(rest.x, |(_, s)| (s.value() + 0.5) as i32);
            let lift = d
                .lift
                .iter()
                .find(|(kk, _)| kk == k)
                .map_or(0, |(_, s)| (s.value() + 0.5) as i32);
            let r = Rect::new(x, rest.y - lift - bounce_of(*k), tb::ICON, tb::ICON);
            if dragging == Some(i) {
                dragged = Some((*k, r));
                continue;
            }
            let focused = self.kind_focused(*k);
            let hover = d.hover == Some(Hit::Item(i)) && d.drag.is_none();
            if focused {
                let (ac, _) = (theme::accent(), 0);
                c.fill_rrect(r.inflated(3), 10, Corner::Circle, ac, 44);
            } else if hover {
                ui::fill_token(c, r.inflated(3), 10, p.hover);
            }
            icons::blit(c, k.icon(), r.x, r.y, r.w, 256);
            self.draw_indicator(c, *k, Rect::new(x, rest.y, tb::ICON, tb::ICON), panel);
            if hover {
                tip = Some((r, *k));
            }
        }
        if let Some((k, r)) = dragged {
            // The dragged icon follows the pointer a little above the bar.
            let x = d.drag.as_ref().map_or(r.x, |dr| dr.x - tb::ICON / 2);
            let x = x.clamp(panel.x + tb::PAD_X, panel.right() - tb::PAD_X - tb::ICON);
            let dr = Rect::new(x, panel.y + tb::PAD_Y - 8, tb::ICON, tb::ICON);
            c.draw_shadow(
                dr,
                Shadow {
                    blur: 8,
                    dy: 4,
                    alpha: 90,
                },
                Rect::new(dr.x, dr.y + 8, dr.w, dr.h - 16),
            );
            icons::blit(c, k.icon(), dr.x, dr.y, dr.w, 256);
        }
        // Tooltip (after the pointer rested a moment).
        if d.tip.value() > 0.5 && d.drag.is_none() {
            match d.hover {
                Some(Hit::Apps) => {
                    ui::tooltip(
                        c,
                        l.apps.x + l.apps.w / 2,
                        panel.y - 6,
                        kitsune_core::t!("taskbar.apps"),
                    );
                }
                Some(Hit::Sliver) => {
                    ui::tooltip(
                        c,
                        l.sliver.x + l.sliver.w / 2,
                        panel.y - 6,
                        kitsune_core::t!("taskbar.show_desktop"),
                    );
                }
                _ => {
                    if let Some((r, k)) = tip {
                        let n = self
                            .wm
                            .windows()
                            .iter()
                            .filter(|w| w.app.kind() == k && !w.is_closing())
                            .count();
                        let label = if n > 1 {
                            kitsune_core::tp!("taskbar.tip", n, name = k.label())
                        } else {
                            String::from(k.label())
                        };
                        ui::tooltip(c, r.x + r.w / 2, panel.y - 6, &label);
                    }
                }
            }
        }
        let _ = FOOTNOTE;
    }

    /// The running indicator under the icon at `r`: a long accent pill for the focused app, a
    /// dot for the others (dimmer when all their windows are minimised).
    fn draw_indicator(&self, c: &mut Canvas, k: Kind, r: Rect, panel: Rect) {
        let p = theme::pal();
        let ind = tb::indicator(
            self.kind_running(k),
            self.kind_focused(k),
            self.kind_all_minimized(k),
        );
        let y = panel.bottom() - 6;
        match ind {
            Indicator::None => {}
            Indicator::Pill => {
                let pill = Rect::new(r.x + r.w / 2 - 8, y, 16, 3);
                c.fill_rrect(pill, 1, Corner::Circle, theme::accent(), 256);
            }
            Indicator::Dot { dim } => {
                let (dc, da) = theme::tint(p.bar_text);
                let dot = Rect::new(r.x + r.w / 2 - 2, y - 1, 4, 4);
                c.fill_rrect(
                    dot,
                    2,
                    Corner::Circle,
                    dc,
                    if dim { da / 3 } else { da.min(200) },
                );
            }
        }
    }
}
