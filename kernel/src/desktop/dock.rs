//! The app bar (dock): a floating translucent panel with the app icons,
//! magnification under the pointer (a spring per icon), running indicators,
//! tooltips, launch bounce and the right-click menu.
//!
//! The panel is a blit of the blurred wallpaper strip built when the wallpaper was
//! painted (see `widgets::paint_background`) with a tint over it; when a window is
//! under the panel it is a plain translucent tint instead (nothing is blurred per
//! frame). Icons are cached scaled surfaces; sizes are rounded to even pixels so
//! the cache stays small.

use super::shell::*;
use super::*;
use crate::text::{FOOTNOTE, Weight};
use osjeff_core::anim::{self, curves};
use osjeff_core::chrome::{self, DOCK_ICON, DOCK_REACH};
use osjeff_core::style::R_DOCK;

/// Seconds the pointer must rest on an icon before its label shows.
const TIP_DELAY: f32 = 0.35;
/// Peak height of the first launch hop.
const BOUNCE_PX: f32 = 18.0;

impl Kind {
    /// Position of this app among the app bar's app icons (the Apps button is slot 0 of
    /// [`DOCK_ITEMS`], so the item index is this plus one).
    pub(crate) fn dock_slot(self) -> Option<usize> {
        DOCK_ITEMS.iter().position(|e| *e == DockEntry::App(self))
    }
}

fn entry_icon(e: DockEntry) -> Icon {
    match e {
        DockEntry::Apps => Icon::Launchpad,
        DockEntry::App(k) => k.icon(),
    }
}

fn entry_label(e: DockEntry) -> &'static str {
    match e {
        DockEntry::Apps => "Apps",
        DockEntry::App(k) => k.label(),
    }
}

impl Desktop {
    fn dock_rest_layout(&self) -> (Rect, Vec<Rect>) {
        chrome::dock_rest(self.sw, self.sh, DOCK_ITEMS.len(), Some(0))
    }

    /// Screen rectangle of the (unmagnified) icon of `w`'s app: where a window flies to
    /// when minimised and from when restored.
    pub(crate) fn dock_target(&self, w: &Win) -> Option<Rect> {
        let slot = w.app.kind().dock_slot()?;
        self.dock_rest_layout().1.get(slot).copied()
    }

    /// Icon sizes now (springs), rounded to even pixels.
    fn dock_sizes(&self) -> Vec<i32> {
        self.shell
            .dock
            .sizes
            .iter()
            .map(|s| ((s.value() + 1.0) as i32 & !1).clamp(DOCK_ICON, chrome::DOCK_MAX))
            .collect()
    }

    /// The layout the dock is drawn in right now.
    pub(crate) fn dock_geometry(&self) -> (Rect, Vec<Rect>) {
        chrome::dock_layout(self.sw, self.sh, &self.dock_sizes(), Some(0))
    }

    /// Region the dock can paint in (including magnified icons and the tooltip).
    pub(crate) fn dock_paint_zone(&self) -> Rect {
        let (rest, _) = self.dock_rest_layout();
        let top = rest.y - (chrome::DOCK_MAX - DOCK_ICON) - 56;
        Rect::new(rest.x - 80, top, rest.w + 160, self.sh - top)
    }

    /// Does the dock need frames (it moves, a tooltip is pending or fading)?
    pub(crate) fn dock_animating(&self) -> bool {
        let d = &self.shell.dock;
        d.dirty
            || d.sizes.iter().any(|s| !s.at_rest())
            || !d.bounce.is_empty()
            || (d.hover.is_some() && d.rest < TIP_DELAY + 0.05)
            || !d.tip.finished()
    }

    /// Advance the dock's springs, tooltip timer and bounces.
    pub(crate) fn step_dock(&mut self, dt: f32) -> bool {
        let (_, rest) = self.dock_rest_layout();
        let targets = chrome::dock_magnify_scaled(
            &rest,
            self.shell.dock.pointer_x,
            crate::settings::get().dock_zoom,
        );
        let d = &mut self.shell.dock;
        let mut busy = false;
        for (s, t) in d.sizes.iter_mut().zip(targets) {
            s.set_target(t);
            busy |= s.step(dt);
        }
        if d.hover.is_some() {
            d.rest += dt;
            if d.rest >= TIP_DELAY && d.tip.target() < 1.0 {
                d.tip.retarget(1.0, 0.12, curves::ENTER);
            }
            busy |= d.rest < TIP_DELAY + 0.05;
        }
        busy |= d.tip.step(dt);
        d.bounce.retain_mut(|(_, t)| {
            *t += dt;
            anim::bounce(*t, BOUNCE_PX).1
        });
        busy |= !d.bounce.is_empty();
        if !busy {
            d.dirty = false;
        }
        busy || d.dirty
    }

    /// Start the launch bounce of `kind`'s icon.
    pub(crate) fn dock_bounce(&mut self, kind: Kind) {
        if let Some(slot) = kind.dock_slot() {
            let item = slot + 1;
            if !self.shell.dock.bounce.iter().any(|(i, _)| *i == item) {
                self.shell.dock.bounce.push((item, 0.0));
            }
        }
    }

    /// The pointer moved to `(x, y)`: magnify when it is on the dock, track the icon.
    pub(crate) fn dock_pointer(&mut self, x: i32, y: i32) {
        let (panel, icons) = self.dock_geometry();
        let over = panel.inflated(2).contains(x, y) || icons.iter().any(|r| r.contains(x, y));
        let d = &mut self.shell.dock;
        let new_x = over.then_some(x);
        let hover = if over {
            chrome::dock_icon_at(panel, &icons, x, y)
        } else {
            None
        };
        if hover != d.hover {
            d.hover = hover;
            d.rest = 0.0;
            if d.tip.target() > 0.0 {
                d.tip.retarget(0.0, 0.08, curves::EXIT);
            }
            d.dirty = true;
        }
        if new_x != d.pointer_x {
            d.pointer_x = new_x;
        }
        let _ = DOCK_REACH;
    }

    /// The app-bar item under `(x, y)`.
    pub(crate) fn dock_item_at(&self, x: i32, y: i32) -> Option<usize> {
        let (panel, icons) = self.dock_geometry();
        let over = panel.inflated(2).contains(x, y) || icons.iter().any(|r| r.contains(x, y));
        if over {
            chrome::dock_icon_at(panel, &icons, x, y)
        } else {
            None
        }
    }

    /// Left click on item `i`.
    pub(crate) fn dock_click(&mut self, i: usize) {
        match DOCK_ITEMS.get(i) {
            Some(DockEntry::Apps) => self.open_apps(),
            Some(DockEntry::App(k)) => {
                let k = *k;
                self.launch(k);
            }
            None => {}
        }
        self.shell.dock.dirty = true;
    }

    /// Right click on item `i` at `(x, y)`: open its menu.
    pub(crate) fn dock_context(&mut self, i: usize, x: i32, _y: i32) {
        let Some(DockEntry::App(k)) = DOCK_ITEMS.get(i).copied() else {
            return;
        };
        let running = self.wm.windows().iter().any(|w| w.app.kind() == k);
        let mut entries = alloc::vec![Entry::item(
            if running { "Mostrar" } else { "Abrir" },
            "",
            Cmd::Launch(k)
        )];
        if k.multi() {
            entries.push(Entry::item("Nova janela", "", Cmd::NewOf(k)));
        }
        if running {
            entries.push(Entry::sep());
            entries.push(Entry::item("Encerrar", "", Cmd::QuitOf(k)));
        }
        let rows = entries.len() as i32 * 24 + 12;
        self.open_menu(
            MenuOrigin::Context,
            entries,
            (x - 80, self.dock_rest_layout().0.y - rows - 8),
        );
    }

    /// Does any shown window sit under the panel (so a cached blur would be wrong)?
    fn dock_covered(&self, panel: Rect) -> bool {
        self.wm
            .windows()
            .iter()
            .any(|w| w.shown() && self.window_box(w).intersection(&panel).is_some())
    }

    /// Draw the dock: panel, separator, icons, indicators, tooltip.
    pub(crate) fn draw_dock(&self, c: &mut Canvas) {
        let p = theme::pal();
        let (panel, icons) = self.dock_geometry();
        self.shell.dock.layout.set((panel, {
            let mut a = [Rect::new(0, 0, 0, 0); 9];
            for (slot, r) in a.iter_mut().zip(&icons) {
                *slot = *r;
            }
            a
        }));
        // Shadow, glass, tint, edge.
        let hole = Rect::new(
            panel.x,
            panel.y + R_DOCK,
            panel.w,
            (panel.h - 2 * R_DOCK).max(0),
        );
        c.draw_shadow(
            panel,
            Shadow {
                blur: 12,
                dy: 6,
                alpha: 60,
            },
            hole,
        );
        let covered = self.dock_covered(panel);
        if !covered {
            widgets::dock_glass(c, panel, R_DOCK);
        }
        let (tc, ta) = theme::tint(p.dock_tint);
        c.fill_rrect(
            panel,
            R_DOCK,
            Corner::Circle,
            tc,
            if covered { (ta + 70).min(230) } else { ta },
        );
        let (ec, ea) = theme::tint(p.glass_edge);
        c.stroke_rrect(panel, R_DOCK, Corner::Circle, ec, ea);
        // Separator after the Apps button.
        if icons.len() > 1 {
            let x = icons[0].right() + (chrome::DOCK_GAP + chrome::DOCK_SEP) / 2;
            let (sc, sa) = theme::tint(p.separator);
            c.blend_rect(
                Rect::new(x, panel.y + 14, 1, panel.h - 28),
                sc,
                (sa * 2).min(256),
            );
        }
        let bounce_of = |item: usize| -> i32 {
            self.shell
                .dock
                .bounce
                .iter()
                .find(|(i, _)| *i == item)
                .map_or(0, |(_, t)| anim::bounce(*t, BOUNCE_PX).0 as i32)
        };
        for (i, (entry, r)) in DOCK_ITEMS.iter().zip(&icons).enumerate() {
            let up = bounce_of(i);
            icons::blit(c, entry_icon(*entry), r.x, r.y - up, r.w, 256);
            // Running indicator: a dot under the icon.
            if let DockEntry::App(k) = entry
                && self.wm.windows().iter().any(|w| w.app.kind() == *k)
            {
                let (dc, da) = theme::tint(p.bar_text);
                let dot = Rect::new(r.x + r.w / 2 - 2, panel.bottom() - 6, 4, 4);
                c.fill_rrect(dot, 2, Corner::Circle, dc, da.min(210));
            }
        }
        // Tooltip.
        let d = &self.shell.dock;
        if let Some(h) = d.hover
            && d.tip.value() > 0.5
            && let (Some(r), Some(e)) = (icons.get(h), DOCK_ITEMS.get(h))
        {
            let top = r.y - bounce_of(h);
            ui::tooltip(c, r.x + r.w / 2, top - 8, entry_label(*e));
            let _ = (FOOTNOTE, Weight::Regular);
        }
    }
}
