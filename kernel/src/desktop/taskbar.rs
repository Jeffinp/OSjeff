//! The taskbar: a floating rounded bar at the bottom with the Apps button, the pinned apps (they
//! can be dragged to reorder), the apps that run without being pinned, running indicators
//! (a long pill for the focused app, a dot for the others), a tooltip, the launch hop, a context
//! menu with the app's windows, and a *Mostrar área de trabalho* sliver at the right end.
//!
//! Geometry and rules are pure (`osjeff_core::taskbar`). The bar is a plain translucent surface
//! (nothing is blurred) painted live over the cached scene; icons are cached scaled surfaces.
//! There is no magnification: an icon lifts a little under the pointer (a spring per icon) and a
//! dragged icon makes its neighbours slide (a spring per icon too).

use super::shell::*;
use super::*;
use crate::text::FOOTNOTE;
use osjeff_core::anim::{self, Spring, Tween, curves};
use osjeff_core::style::R_TASKBAR;
use osjeff_core::taskbar::{self as tb, Click, Hit, Indicator};

/// Seconds the pointer must rest on an icon before its label shows.
const TIP_DELAY: f32 = 0.35;
/// Peak height of the first launch hop.
const BOUNCE_PX: f32 = 12.0;

/// The apps the bar starts with, in order.
pub(crate) const DEFAULT_PINNED: [Kind; 8] = [
    Kind::Files,
    Kind::Browser,
    Kind::Terminal,
    Kind::Editor,
    Kind::Calculator,
    Kind::Viewer,
    Kind::TaskMgr,
    Kind::Settings,
];

/// A pinned icon being dragged: the item it started on, the pointer x and the slot it hovers.
pub(crate) struct IconDrag {
    pub from: usize,
    pub x: i32,
    pub slot: usize,
}

/// Live state of the bar.
pub(crate) struct TaskbarState {
    pub pinned: Vec<Kind>,
    pub hover: Option<Hit>,
    /// Seconds the pointer has rested on `hover` (the tooltip waits for [`TIP_DELAY`]).
    pub rest: f32,
    pub tip: Tween,
    /// Launch hops: `(kind, seconds since the launch)`.
    pub bounce: Vec<(Kind, f32)>,
    /// Lift of each icon under the pointer, and each icon's current x (reordering slides).
    pub lift: Vec<(Kind, Spring)>,
    pub xs: Vec<(Kind, Spring)>,
    /// The icon pressed (and where) while the button is down: a click if it was not dragged.
    pub press: Option<(usize, i32, i32)>,
    pub drag: Option<IconDrag>,
    /// Windows that *Mostrar área de trabalho* minimised, to bring them back.
    pub hidden: Vec<WindowId>,
    /// How many icons the last layout had (a change snaps the slides instead of animating).
    pub count: usize,
    /// Set when something changed that needs a repaint even without motion.
    pub dirty: bool,
}

impl TaskbarState {
    pub(crate) fn new() -> TaskbarState {
        TaskbarState {
            pinned: DEFAULT_PINNED.to_vec(),
            hover: None,
            rest: 0.0,
            tip: Tween::at(0.0),
            bounce: Vec::new(),
            lift: Vec::new(),
            xs: Vec::new(),
            press: None,
            drag: None,
            hidden: Vec::new(),
            count: 0,
            dirty: true,
        }
    }
}

impl Kind {
    /// The key the launcher's *Recentes* remembers a built-in app by.
    pub(crate) fn recent_key(self) -> String {
        alloc::format!("sys:{}", self.proc_name())
    }

    /// Can the app be pinned to the bar?
    pub(crate) fn pinnable(self) -> bool {
        !matches!(self, Kind::WasmApp | Kind::Gallery)
    }
}

fn spring_of(v: &mut Vec<(Kind, Spring)>, k: Kind, init: f32) -> &mut Spring {
    if let Some(i) = v.iter().position(|(kk, _)| *kk == k) {
        &mut v[i].1
    } else {
        v.push((k, Spring::pixels(init, 420.0, 30.0)));
        &mut v.last_mut().expect("just pushed").1
    }
}

impl Desktop {
    /// The icons on the bar, left to right: the pinned apps in their order, then the apps that
    /// are running without being pinned (by when they first opened).
    pub(crate) fn task_kinds(&self) -> Vec<Kind> {
        let mut v = self.shell.task.pinned.clone();
        let mut extra: Vec<(u32, Kind)> = Vec::new();
        for w in self.wm.windows() {
            let k = w.app.kind();
            if w.is_closing() || v.contains(&k) {
                continue;
            }
            match extra.iter_mut().find(|(_, kk)| *kk == k) {
                Some(e) => e.0 = e.0.min(w.id.raw()),
                None => extra.push((w.id.raw(), k)),
            }
        }
        extra.sort_by_key(|(id, _)| *id);
        v.extend(extra.into_iter().map(|(_, k)| k));
        v
    }

    fn task_layout(&self) -> tb::Layout {
        tb::layout(self.sw, self.sh, self.task_kinds().len())
    }

    /// Screen rectangle of the icon of `w`'s app: where a window flies to when minimised and from
    /// when restored.
    pub(crate) fn dock_target(&self, w: &Win) -> Option<Rect> {
        let l = self.task_layout();
        let i = self.task_kinds().iter().position(|k| *k == w.app.kind())?;
        l.items.get(i).copied()
    }

    /// Is any window of `kind` open (and not on its way out)?
    fn kind_running(&self, kind: Kind) -> bool {
        self.wm
            .windows()
            .iter()
            .any(|w| w.app.kind() == kind && !w.is_closing())
    }

    fn kind_all_minimized(&self, kind: Kind) -> bool {
        self.wm
            .windows()
            .iter()
            .filter(|w| w.app.kind() == kind && !w.is_closing())
            .all(|w| !w.shown())
    }

    fn kind_focused(&self, kind: Kind) -> bool {
        self.focused().and_then(|id| self.kind_of(id)) == Some(kind)
    }

    /// Region the bar can paint in (lifted icons, hops and the tooltip included).
    pub(crate) fn dock_paint_zone(&self) -> Rect {
        tb::paint_zone(&self.task_layout(), self.sw, self.sh)
    }

    /// Does the bar need frames (something slides, a tooltip is pending, an icon is dragged)?
    pub(crate) fn dock_animating(&self) -> bool {
        let d = &self.shell.task;
        d.dirty
            || d.drag.is_some()
            || d.lift.iter().any(|(_, s)| !s.at_rest())
            || d.xs.iter().any(|(_, s)| !s.at_rest())
            || !d.bounce.is_empty()
            || (d.hover.is_some() && d.rest < TIP_DELAY + 0.05)
            || !d.tip.finished()
    }

    /// Advance the bar's springs, tooltip timer and hops.
    pub(crate) fn step_dock(&mut self, dt: f32) -> bool {
        let kinds = self.task_kinds();
        let l = tb::layout(self.sw, self.sh, kinds.len());
        let hover_kind = match self.shell.task.hover {
            Some(Hit::Item(i)) => kinds.get(i).copied(),
            _ => None,
        };
        let d = &mut self.shell.task;
        let snap = d.count != kinds.len();
        d.count = kinds.len();
        let mut busy = false;
        let reduce = osjeff_core::anim::reduce_motion();
        for (i, k) in kinds.iter().enumerate() {
            // Where this icon wants to be (dragging makes room for the dragged one).
            let slot = match &d.drag {
                Some(dr) => tb::slot_while_dragging(i, dr.from, dr.slot),
                None => i,
            };
            let tx = l.items.get(slot).map_or(0, |r| r.x) as f32;
            let s = spring_of(&mut d.xs, *k, tx);
            if snap || reduce {
                *s = Spring::pixels(tx, 420.0, 30.0);
            } else {
                s.set_target(tx);
                busy |= s.step(dt);
            }
            let lift = spring_of(&mut d.lift, *k, 0.0);
            lift.set_target(if hover_kind == Some(*k) && d.drag.is_none() {
                tb::LIFT as f32
            } else {
                0.0
            });
            busy |= lift.step(dt);
        }
        d.xs.retain(|(k, _)| kinds.contains(k));
        d.lift.retain(|(k, _)| kinds.contains(k));
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
        if !busy && d.drag.is_none() {
            d.dirty = false;
        }
        busy || d.dirty || d.drag.is_some()
    }

    /// Start the launch hop of `kind`'s icon.
    pub(crate) fn dock_bounce(&mut self, kind: Kind) {
        if !self.shell.task.bounce.iter().any(|(k, _)| *k == kind) {
            self.shell.task.bounce.push((kind, 0.0));
        }
    }

    /// The part of the bar under `(x, y)`.
    pub(crate) fn dock_item_at(&self, x: i32, y: i32) -> Option<Hit> {
        tb::hit(&self.task_layout(), x, y)
    }

    /// The pointer moved to `(x, y)`: track the hovered icon, the tooltip and an icon drag.
    pub(crate) fn dock_pointer(&mut self, x: i32, y: i32) {
        let l = self.task_layout();
        let hover = tb::hit(&l, x, y);
        let pinned = self.shell.task.pinned.len();
        let d = &mut self.shell.task;
        if hover != d.hover {
            d.hover = hover;
            d.rest = 0.0;
            if d.tip.target() > 0.0 {
                d.tip.retarget(0.0, 0.08, curves::EXIT);
            }
            d.dirty = true;
        }
        // A press that travelled far enough starts dragging a pinned icon.
        if d.drag.is_none()
            && let Some((i, px, _)) = d.press
            && i < pinned
            && (x - px).abs() >= tb::DRAG_SLOP
        {
            d.drag = Some(IconDrag {
                from: i,
                x,
                slot: i,
            });
            d.tip.retarget(0.0, 0.05, curves::EXIT);
        }
        if let Some(dr) = d.drag.as_mut() {
            dr.x = x;
            let slot = tb::drop_slot(&l, x, pinned);
            if slot != dr.slot {
                dr.slot = slot;
            }
            d.dirty = true;
        }
    }

    /// A left press on part `hit` of the bar. The Apps button and the sliver act at once; an app
    /// icon waits for the release (so it can be dragged instead).
    pub(crate) fn dock_press(&mut self, hit: Hit, x: i32, y: i32) {
        match hit {
            Hit::Apps => self.open_apps(),
            Hit::Sliver => self.show_desktop(),
            Hit::Item(i) => self.shell.task.press = Some((i, x, y)),
        }
        self.shell.task.dirty = true;
    }

    /// The button went up: finish an icon drag (reorder) or complete a click.
    pub(crate) fn dock_release(&mut self, x: i32, y: i32) {
        let d = &mut self.shell.task;
        if let Some(dr) = d.drag.take() {
            d.press = None;
            tb::reorder(&mut d.pinned, dr.from, dr.slot);
            d.dirty = true;
            return;
        }
        let Some((i, _, _)) = d.press.take() else {
            return;
        };
        if self.dock_item_at(x, y) == Some(Hit::Item(i)) {
            self.task_click(i, self.keymap.shift());
        }
        self.shell.task.dirty = true;
    }

    /// Complete a click on icon `i`: open, bring forward or minimise; with Shift, a new window.
    fn task_click(&mut self, i: usize, new_window: bool) {
        let Some(kind) = self.task_kinds().get(i).copied() else {
            return;
        };
        if new_window && kind.multi() {
            self.new_window(kind);
            return;
        }
        match tb::click_action(self.kind_running(kind), self.kind_focused(kind)) {
            Click::Launch | Click::Focus => {
                if kind != Kind::WasmApp {
                    self.shell.recents.note(&kind.recent_key());
                }
                self.launch(kind);
            }
            Click::Minimize => {
                if let Some(id) = self.focused() {
                    self.wm.minimize(id);
                }
            }
        }
    }

    /// *Mostrar área de trabalho*: minimise every window, or bring back the ones it hid.
    pub(crate) fn show_desktop(&mut self) {
        let visible: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| w.active())
            .map(|w| w.id)
            .collect();
        if !visible.is_empty() {
            for &id in &visible {
                self.wm.minimize(id);
            }
            self.shell.task.hidden = visible;
        } else {
            for id in core::mem::take(&mut self.shell.task.hidden) {
                if self.wm.get(id).is_some_and(|w| w.minimized) {
                    self.wm.activate(id);
                }
            }
        }
        self.force_full = true;
    }

    /// Pin or unpin `kind`.
    pub(crate) fn set_pinned(&mut self, kind: Kind, pin: bool) {
        let t = &mut self.shell.task;
        if pin {
            if !t.pinned.contains(&kind) && kind.pinnable() {
                t.pinned.push(kind);
            }
        } else {
            t.pinned.retain(|k| *k != kind);
        }
        t.dirty = true;
        self.force_full = true;
    }

    /// Right click on part `hit` at `(x, y)`: open its menu (the app's windows, new window, pin).
    pub(crate) fn dock_context(&mut self, hit: Hit, x: i32, _y: i32) {
        let Hit::Item(i) = hit else {
            return;
        };
        let Some(k) = self.task_kinds().get(i).copied() else {
            return;
        };
        let focused = self.focused();
        let mut entries: Vec<Entry> = Vec::new();
        let wins: Vec<(WindowId, String)> = self
            .wm
            .windows()
            .iter()
            .filter(|w| w.app.kind() == k && !w.is_closing())
            .map(|w| (w.id, w.app.title.clone()))
            .collect();
        for (id, title) in wins.iter().take(8) {
            let mut e = Entry::item(title, "", Cmd::Activate(*id));
            e.checked = focused == Some(*id);
            entries.push(e);
        }
        if wins.is_empty() {
            entries.push(Entry::item(
                osjeff_core::t!("common.open"),
                "",
                Cmd::Launch(k),
            ));
        }
        entries.push(Entry::sep());
        if k.multi() {
            entries.push(Entry::item(
                osjeff_core::t!("taskbar.menu.new_window"),
                "",
                Cmd::NewOf(k),
            ));
        }
        if self.shell.task.pinned.contains(&k) {
            entries.push(Entry::item(
                osjeff_core::t!("taskbar.menu.unpin"),
                "",
                Cmd::Unpin(k),
            ));
        } else if k.pinnable() {
            entries.push(Entry::item(
                osjeff_core::t!("taskbar.menu.pin"),
                "",
                Cmd::Pin(k),
            ));
        }
        if !wins.is_empty() {
            entries.push(Entry::sep());
            entries.push(Entry::item(
                if wins.len() > 1 {
                    osjeff_core::t!("taskbar.menu.close_all")
                } else {
                    osjeff_core::t!("taskbar.menu.close_window")
                },
                "",
                Cmd::QuitOf(k),
            ));
        }
        let rows = entries.len() as i32 * 24 + 12 + 8;
        let top = self.task_layout().panel.y;
        self.open_menu(MenuOrigin::Context, entries, (x - 80, top - rows - 8));
    }

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
                        osjeff_core::t!("taskbar.apps"),
                    );
                }
                Some(Hit::Sliver) => {
                    ui::tooltip(
                        c,
                        l.sliver.x + l.sliver.w / 2,
                        panel.y - 6,
                        osjeff_core::t!("taskbar.show_desktop"),
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
                            osjeff_core::tp!("taskbar.tip", n, name = k.label())
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
