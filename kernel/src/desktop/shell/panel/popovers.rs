//! Popovers: opening, drawing, clicks, and the drawing entry points of the menu and popover layers.

use super::helpers::tween_at;
use crate::desktop::kit::glass::panel as glass_panel;
use crate::desktop::shell::*;
use crate::desktop::*;
use crate::text::{self, TITLE3, Weight};
use kitsune_core::chrome::{
    self, QuickTile, centre_geom, popover_centered, popover_rect, quick_geom,
};
use kitsune_core::iconart::Glyph;
use kitsune_core::style::R_POPOVER;
use kitsune_core::t;

impl Desktop {
    // ---- popovers ----

    /// Open the popover of `kind` under its panel item.
    pub(crate) fn open_popover(&mut self, kind: PopKind) {
        let item = match kind {
            PopKind::Quick => PanelItem::Tray,
            PopKind::Centre => PanelItem::Clock,
        };
        let Some((_, anchor)) = self.panel_items().into_iter().find(|(i, _)| *i == item) else {
            return;
        };
        let rect = match kind {
            PopKind::Quick => popover_rect(anchor, chrome::QUICK_W, chrome::QUICK_H, self.sw),
            PopKind::Centre => {
                popover_centered(anchor, chrome::CENTRE_W, chrome::CENTRE_H, self.sw)
            }
        };
        self.shell.menu = None;
        self.shell.pop = Some(Popover {
            kind,
            rect,
            t: fade_in(MENU_FADE),
            closing: false,
            glass: Default::default(),
            month_off: 0,
        });
        self.shell.knobs = [
            tween_at(crate::settings::get().reduce_motion),
            tween_at(crate::settings::get().clock24),
            tween_at(!crate::settings::get().toasts),
        ];
        if kind == PopKind::Centre {
            self.shell.notif_unread = 0;
        }
        self.force_full = true;
    }

    fn draw_popover(&self, c: &mut Canvas, pop: &Popover) {
        let p = theme::pal();
        let fade = level(&pop.t);
        let mut r = pop.rect;
        r.y -= ((256 - fade) as i32 * 6) / 256;
        glass_panel(
            c,
            r,
            R_POPOVER,
            &pop.glass,
            10,
            p.menu_tint,
            p.separator,
            Shadow {
                blur: 14,
                dy: 8,
                alpha: 80,
            },
            fade,
        );
        if fade < 150 {
            return;
        }
        match pop.kind {
            PopKind::Quick => self.draw_quick(c, r),
            PopKind::Centre => self.draw_centre(c, r, pop.month_off),
        }
    }

    /// Quick Settings: a tile grid, the accent swatches and the power buttons.
    fn draw_quick(&self, c: &mut Canvas, r: Rect) {
        let p = theme::pal();
        let g = quick_geom(r);
        let s = crate::settings::get();
        text::draw_left(
            c,
            g.title,
            t!("quick.title"),
            TITLE3,
            Weight::Semibold,
            theme::solid(p.text),
        );
        let st = crate::netd::stats();
        let up = st.link_up;
        for (rect, tile) in g.tiles.iter().zip(chrome::QUICK_ORDER) {
            let (glyph, label, sub, on): (Glyph, &str, String, bool) = match tile {
                QuickTile::Network => (
                    if up {
                        Glyph::Network
                    } else {
                        Glyph::NetworkOff
                    },
                    t!("quick.network"),
                    match (&st.config, up) {
                        (Some(cfg), true) => alloc::format!("{}", cfg.ip),
                        (None, true) => String::from(t!("quick.network.connecting")),
                        _ => String::from(t!("quick.network.offline")),
                    },
                    up,
                ),
                QuickTile::Appearance => (
                    if theme::dark() {
                        Glyph::Moon
                    } else {
                        Glyph::Sun
                    },
                    t!("quick.appearance"),
                    String::from(match s.appearance {
                        kitsune_core::style::AppearanceSetting::Auto => t!("quick.appearance.auto"),
                        kitsune_core::style::AppearanceSetting::Light => {
                            t!("quick.appearance.light")
                        }
                        kitsune_core::style::AppearanceSetting::Dark => t!("quick.appearance.dark"),
                    }),
                    theme::dark(),
                ),
                QuickTile::ReduceMotion => (
                    Glyph::Wave,
                    t!("quick.motion"),
                    String::from(if s.reduce_motion {
                        t!("quick.motion.reduced")
                    } else {
                        t!("quick.motion.full")
                    }),
                    s.reduce_motion,
                ),
                QuickTile::DoNotDisturb => (
                    Glyph::Bell,
                    t!("quick.dnd"),
                    String::from(if s.toasts {
                        t!("quick.dnd.off")
                    } else {
                        t!("quick.dnd.on")
                    }),
                    !s.toasts,
                ),
                QuickTile::Clock24 => (
                    Glyph::Clock,
                    t!("quick.clock24"),
                    String::from(if s.clock24 {
                        t!("quick.clock24.on")
                    } else {
                        t!("quick.clock24.off")
                    }),
                    s.clock24,
                ),
                QuickTile::Settings => (
                    Glyph::Control,
                    t!("quick.settings"),
                    String::from(t!("common.open")),
                    false,
                ),
            };
            let hover = rect.contains(self.cursor_x, self.cursor_y);
            self.draw_tile(c, *rect, glyph, label, &sub, on, hover);
        }
        ui::caption(c, g.accent_label.x, g.accent_label.y, t!("quick.accent"));
        for (i, sw) in g.swatches.iter().enumerate() {
            let rgb = kitsune_core::settings::ACCENTS[i];
            let col = Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
            if s.accent as usize == i {
                c.stroke_rrect(sw.inflated(3), 9, Corner::Circle, theme::solid(p.text), 200);
            }
            c.fill_rrect(*sw, 7, Corner::Circle, col, 256);
        }
        let hov = |r: Rect| {
            if r.contains(self.cursor_x, self.cursor_y) {
                ui::Control::Hover
            } else {
                ui::Control::Normal
            }
        };
        ui::push_button(
            c,
            g.restart,
            t!("power.restart"),
            ui::ButtonKind::Secondary,
            hov(g.restart),
        );
        ui::push_button(
            c,
            g.shutdown,
            t!("power.shutdown"),
            ui::ButtonKind::Secondary,
            hov(g.shutdown),
        );
    }

    /// A click inside the open popover. Returns true when it was handled.
    pub(crate) fn popover_click(&mut self, x: i32, y: i32) -> bool {
        let Some(pop) = self.shell.pop.as_ref() else {
            return false;
        };
        if !pop.rect.contains(x, y) {
            return false;
        }
        match pop.kind {
            PopKind::Quick => {
                let g = quick_geom(pop.rect);
                let mut s = crate::settings::get();
                if let Some(tile) = chrome::quick_tile_at(&g, x, y) {
                    match tile {
                        QuickTile::Network | QuickTile::Settings => {
                            self.close_transients();
                            self.open_settings(0);
                            return true;
                        }
                        QuickTile::Appearance => s.appearance = s.appearance.next(),
                        QuickTile::ReduceMotion => s.reduce_motion = !s.reduce_motion,
                        QuickTile::DoNotDisturb => s.toasts = !s.toasts,
                        QuickTile::Clock24 => s.set_clock24(!s.clock24),
                    }
                } else if let Some(i) = g.swatches.iter().position(|r| r.inflated(3).contains(x, y))
                {
                    s.accent = i as u8;
                } else if g.restart.contains(x, y) {
                    self.close_transients();
                    self.execute(Cmd::Reboot);
                    return true;
                } else if g.shutdown.contains(x, y) {
                    self.close_transients();
                    self.execute(Cmd::Shutdown);
                    return true;
                } else {
                    return true;
                }
                let look = s.appearance != crate::settings::get().appearance;
                let _ = self.settings_apply(s);
                if look {
                    // The look changed: the popover's blurred backdrop is stale, so it comes up again.
                    self.open_popover(PopKind::Quick);
                }
                true
            }
            PopKind::Centre => {
                let g = centre_geom(pop.rect);
                let cal = chrome::calendar_geom(g.calendar);
                if g.clear.contains(x, y) {
                    self.shell.notifs.clear();
                    self.shell.notif_unread = 0;
                } else if g.dnd_label.contains(x, y) || g.dnd_switch.contains(x, y) {
                    let mut s = crate::settings::get();
                    s.toasts = !s.toasts;
                    self.shell.knobs[2].retarget(
                        if s.toasts { 0.0 } else { 1.0 },
                        0.18,
                        kitsune_core::anim::curves::ENTER,
                    );
                    let _ = self.settings_apply(s);
                } else if cal.prev.contains(x, y) {
                    if let Some(p) = self.shell.pop.as_mut() {
                        p.month_off -= 1;
                    }
                } else if cal.next.contains(x, y)
                    && let Some(p) = self.shell.pop.as_mut()
                {
                    p.month_off += 1;
                }
                true
            }
        }
    }

    /// Draw the open menu (a context menu, the system menu, the window menu).
    pub(crate) fn draw_menu_layer(&self, c: &mut Canvas) {
        if let Some(m) = &self.shell.menu {
            self.draw_open_menu(c, m);
        }
    }

    /// Draw the open popover (Quick Settings, the notification centre).
    pub(crate) fn draw_popover_layer(&self, c: &mut Canvas) {
        if let Some(p) = &self.shell.pop {
            self.draw_popover(c, p);
        }
    }
}
