//! Per-frame state of the editors.

use crate::desktop::*;

impl Desktop {
    // ---- per-frame state ----

    /// Advance the animations of every editor by `dt`. Returns whether any still needs frames.
    pub(crate) fn step_editors(&mut self, dt: f32) -> bool {
        let focus = self.focused();
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Editor(_)) && w.shown())
            .map(|w| w.id)
            .collect();
        let mut busy = false;
        for id in ids {
            if let Some(e) = self.editor_mut(id) {
                e.caret_x.step(dt);
                e.sel_t.step(dt);
                e.sheet_t.step(dt);
                e.hover_t.step(dt);
                busy |= e.animating(focus == Some(id));
            }
        }
        busy
    }
}
