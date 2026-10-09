//! Which icons the bar shows, where, and its animations.

use super::state::BOUNCE_PX;
use super::state::TIP_DELAY;
use super::state::spring_of;
use crate::desktop::*;
use kitsune_core::anim::{self, Spring, curves};
use kitsune_core::taskbar::{self as tb, Hit};

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

    pub(super) fn task_layout(&self) -> tb::Layout {
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
    pub(super) fn kind_running(&self, kind: Kind) -> bool {
        self.wm
            .windows()
            .iter()
            .any(|w| w.app.kind() == kind && !w.is_closing())
    }

    pub(super) fn kind_all_minimized(&self, kind: Kind) -> bool {
        self.wm
            .windows()
            .iter()
            .filter(|w| w.app.kind() == kind && !w.is_closing())
            .all(|w| !w.shown())
    }

    pub(super) fn kind_focused(&self, kind: Kind) -> bool {
        self.focused().and_then(|id| self.kind_of(id)) == Some(kind)
    }

    /// Region the bar can paint in. While it is at rest (nothing hovered, dragged, lifted,
    /// hopping or showing a tooltip) that is the bar and its shadow; otherwise the whole zone
    /// above it that lifted icons, hops and the tooltip can reach.
    pub(crate) fn dock_paint_zone(&self) -> Rect {
        let l = self.task_layout();
        let d = &self.shell.task;
        let at_rest = d.hover.is_none()
            && d.drag.is_none()
            && d.press.is_none()
            && d.bounce.is_empty()
            && d.tip.finished()
            && d.tip.value() <= 0.0
            && d.lift.iter().all(|(_, s)| s.at_rest())
            && d.xs.iter().all(|(_, s)| s.at_rest());
        if at_rest {
            l.panel.inflated(24).clamped_to(self.sw, self.sh)
        } else {
            tb::paint_zone(&l, self.sw, self.sh)
        }
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
        let reduce = kitsune_core::anim::reduce_motion();
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
}
