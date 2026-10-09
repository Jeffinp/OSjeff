//! The "live" behaviour shared by the system apps (Tarefas):
//! hover tracking that repaints a window only when what the pointer is over changes, and the
//! animation clock for their glides. The apps own the details; this file is the one place the
//! compositor talks to.

use super::*;

impl Desktop {
    /// Pointer feedback for the system apps' windows. Returns whether anything needs a repaint.
    pub(crate) fn live_hover(&mut self, cx: i32, cy: i32, down: bool) -> bool {
        self.tarefas_hover(cx, cy, down)
    }

    /// Advance the system apps' animations by `dt` seconds.
    pub(crate) fn live_step(&mut self, dt: f32) {
        let ms = (dt * 1000.0) as u32;
        self.sysmon.age_ms = self.sysmon.age_ms.saturating_add(ms).min(60_000);
        self.tarefas_step(ms);
    }

    /// Does any system app still animate (so the compositor keeps rendering frames)?
    pub(crate) fn live_busy(&self) -> bool {
        self.wm.windows().iter().any(|w| self.tarefas_busy_one(w))
    }

    /// Is `w` kept out of the cached static layer because it animates by itself?
    pub(crate) fn live_dynamic(&self, w: &Win) -> bool {
        self.tarefas_busy_one(w)
    }

    /// The part of `w` that changes while it animates, when that is all that changes (no
    /// open / close / zoom, no drag, no other reason to be redrawn whole): the compositor
    /// then repaints only this rectangle.
    pub(crate) fn live_rect(&self, w: &Win) -> Option<Rect> {
        let own = matches!(w.app.app, App::Tarefas(_))
            && self.tarefas_busy_one(w)
            && w.anim.is_none()
            && w.zoom.is_none()
            && !self.drag.as_ref().is_some_and(|d| d.win == w.id)
            && !self.focus_busy(w.id);
        own.then(|| self.tarefas_live_rect(w))
    }
}
