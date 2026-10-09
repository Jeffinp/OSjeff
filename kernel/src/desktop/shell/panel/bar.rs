//! The panel itself (Apps button, Busca, clock, status pill) and the notification history.

use super::helpers::pack;
use crate::desktop::shell::*;
use crate::desktop::*;
use crate::text::{self, BODY, Weight};
use kitsune_core::chrome::PANEL_PAD;
use kitsune_core::chrome::{self, panel_layout};
use kitsune_core::i18n::{self, Civil, DateStyle};
use kitsune_core::iconart::Glyph;
use kitsune_core::style::{PANEL_H, R_CONTROL};
use kitsune_core::t;

/// Room kept left of the clock text for the unread-notifications dot.
const CLOCK_DOT_W: i32 = 14;
/// Most notifications kept for the centre.
const NOTIF_MAX: usize = 24;

/// One entry of the notification centre's history.
pub(crate) struct Notif {
    pub level: crate::klog::Level,
    pub text: String,
    pub ms: u32,
}

pub(super) fn level_title(l: crate::klog::Level) -> &'static str {
    kitsune_core::i18n::tr(l.title_key())
}

pub(super) fn level_color(l: crate::klog::Level) -> Color {
    use crate::klog::Level;
    match l {
        Level::Warn => Color::rgb(0xF5, 0xA6, 0x23),
        Level::Error => theme::CLOSE,
        Level::Fatal => Color::rgb(0xFF, 0x4D, 0x9D),
        _ => theme::accent(),
    }
}

/// "agora", "3 min", "2 h": how long ago a notification arrived.
pub(super) fn age_text(now_ms: u32, ms: u32) -> String {
    let secs = now_ms.wrapping_sub(ms) / 1000;
    match secs {
        0..=44 => String::from(t!("time.ago.now")),
        45..=3599 => t!("time.ago.min", n = (secs + 30) / 60),
        _ => t!("time.ago.hour", n = secs / 3600),
    }
}

impl Desktop {
    /// The panel's rectangle (its glass is baked into the cached wallpaper).
    pub fn panel_rect(&self) -> Rect {
        Rect::new(0, 0, self.sw, PANEL_H)
    }

    /// The text of the clock item (`qui 8 out  18:09` / `Thu Oct 8  6:09 PM`).
    fn clock_text(&self, time: Time) -> String {
        let (year, month, day) = self.today.get();
        let civil = Civil {
            year,
            month,
            day,
            weekday: self.weekday.get(),
            hour: time.h,
            minute: time.m,
            second: time.s,
        };
        i18n::format_date(civil, DateStyle::Panel, crate::settings::clock24())
    }

    fn clock_width(&self) -> i32 {
        let t = if crate::settings::clock24() {
            t!("panel.clock_template_24")
        } else {
            t!("panel.clock_template_12")
        };
        text::measure(t, BODY, Weight::Medium) + CLOCK_DOT_W
    }

    /// Every panel item with its rectangle.
    pub(crate) fn panel_items(&self) -> Vec<(PanelItem, Rect)> {
        let apps_w = 16 + 8 + text::measure(t!("panel.apps"), BODY, Weight::Medium);
        let ws_w = chrome::workspace_width(self.wm.visible_workspaces());
        let left = [apps_w, 16, ws_w];
        let right = [chrome::pill_width(3)];
        let g = panel_layout(self.sw, &left, self.clock_width(), &right);
        alloc::vec![
            (PanelItem::Apps, g.left[0]),
            (PanelItem::Search, g.left[1]),
            (PanelItem::Workspaces, g.left[2]),
            (PanelItem::Clock, g.center),
            (PanelItem::Tray, g.right[0]),
        ]
    }

    /// The item under `(x, y)`.
    pub(crate) fn panel_item_at(&self, x: i32, y: i32) -> Option<(PanelItem, Rect)> {
        if y >= PANEL_H {
            return None;
        }
        self.panel_items()
            .into_iter()
            .find(|(_, r)| r.contains(x, y))
    }

    /// Screen rectangle of the clock item (what the per-second tick repaints).
    pub fn clock_rect(&self) -> Rect {
        self.panel_items()
            .iter()
            .find(|(i, _)| *i == PanelItem::Clock)
            .map_or(Rect::new(0, 0, 0, 0), |(_, r)| *r)
    }

    /// Panel content over the strip baked into the wallpaper.
    pub(crate) fn draw_panel(&self, c: &mut Canvas, time: Time) {
        for (item, rect) in self.panel_items() {
            self.draw_panel_item(c, item, rect, time);
        }
    }

    fn panel_item_active(&self, item: PanelItem) -> bool {
        let sh = &self.shell;
        sh.pop.as_ref().is_some_and(|p| {
            !p.closing
                && matches!(
                    (p.kind, item),
                    (PopKind::Quick, PanelItem::Tray) | (PopKind::Centre, PanelItem::Clock)
                )
        }) || (item == PanelItem::Search && sh.search.as_ref().is_some_and(|s| !s.closing))
            || (item == PanelItem::Apps && sh.apps.as_ref().is_some_and(|a| !a.closing))
    }

    fn draw_panel_item(&self, c: &mut Canvas, item: PanelItem, rect: Rect, time: Time) {
        let p = theme::pal();
        let fg = theme::solid(p.bar_text);
        let active = self.panel_item_active(item);
        let hover = self.shell.panel_hover == Some(item);
        let pill = Rect::new(rect.x + 1, rect.y + 3, rect.w - 2, rect.h - 6);
        let (hc, ha) = theme::tint(p.hover);
        if item == PanelItem::Tray {
            // The status pill is always a pill; it darkens with hover and while its popover is open.
            let a = if active {
                ha * 3
            } else if hover {
                ha * 2
            } else {
                ha
            };
            c.fill_rrect(pill, pill.h / 2, Corner::Circle, hc, a.min(256));
        } else if active || hover {
            c.fill_rrect(
                pill,
                R_CONTROL,
                Corner::Circle,
                hc,
                if active { (ha * 2).min(256) } else { ha },
            );
        }
        let argb = 0xFF00_0000 | pack(fg);
        let ty = text::center_y(rect.y, rect.h, BODY, Weight::Medium);
        match item {
            PanelItem::Apps => {
                ui::draw_glyph(c, Glyph::Brand, rect.x + PANEL_PAD, rect.y + 7, 16, argb);
                text::draw(
                    c,
                    rect.x + PANEL_PAD + 16 + 8,
                    ty,
                    t!("panel.apps"),
                    BODY,
                    Weight::Medium,
                    fg,
                );
            }
            PanelItem::Search => {
                ui::draw_glyph(c, Glyph::Search, rect.x + PANEL_PAD, rect.y + 7, 16, argb)
            }
            PanelItem::Workspaces => {
                let (n, cur) = (self.wm.visible_workspaces(), self.wm.workspace());
                for i in 0..n {
                    let d = chrome::workspace_dot(rect, i, cur);
                    let (dc, da) = theme::tint(p.bar_text);
                    if i == cur {
                        c.fill_rrect(d, d.h / 2, Corner::Circle, theme::accent(), 256);
                    } else {
                        // A workspace with windows is a stronger dot than an empty one.
                        let a = if self.wm.windows_on(i) > 0 {
                            da.min(170)
                        } else {
                            da.min(80)
                        };
                        c.fill_rrect(d, d.h / 2, Corner::Circle, dc, a);
                    }
                }
            }
            PanelItem::Clock => {
                let t = self.clock_text(time);
                let w = text::measure(&t, BODY, Weight::Medium);
                let dot = self.shell.notif_unread > 0;
                let room = rect.w - 2 * PANEL_PAD - CLOCK_DOT_W;
                let x = rect.x + PANEL_PAD + CLOCK_DOT_W + (room - w) / 2;
                if dot {
                    let d = Rect::new(x - 12, rect.y + (rect.h - 6) / 2, 6, 6);
                    c.fill_rrect(d, 3, Corner::Circle, theme::accent(), 256);
                }
                text::draw(c, x, ty, &t, BODY, Weight::Medium, fg);
            }
            PanelItem::Tray => {
                let up = crate::netd::stats().link_up;
                let net = if up {
                    Glyph::Network
                } else {
                    Glyph::NetworkOff
                };
                let look = if theme::dark() {
                    Glyph::Moon
                } else {
                    Glyph::Sun
                };
                for (i, g) in [net, look, Glyph::Power].into_iter().enumerate() {
                    let r = chrome::pill_icon(rect, i);
                    ui::draw_glyph(c, g, r.x, r.y, r.w, argb);
                }
            }
        }
    }

    // ---- notification history ----

    /// Collect new warnings and errors of the system log into the notification centre's list.
    pub(crate) fn refresh_notifs(&mut self) {
        let mut warns = [None; 4];
        let n = crate::klog::take_warnings(&mut self.shell.notif_seen, &mut warns);
        let now = crate::klog::ticks_to_ms_now();
        for w in warns.iter().take(n).flatten() {
            let open = self
                .shell
                .pop
                .as_ref()
                .is_some_and(|p| p.kind == PopKind::Centre && !p.closing);
            let text = String::from(&*crate::text::from_bytes(w.text()));
            if self.shell.notifs.len() >= NOTIF_MAX {
                self.shell.notifs.remove(0);
            }
            self.shell.notifs.push(Notif {
                level: w.level,
                text,
                ms: now,
            });
            if !open {
                self.shell.notif_unread += 1;
            }
            self.force_full |= open;
        }
    }
}
