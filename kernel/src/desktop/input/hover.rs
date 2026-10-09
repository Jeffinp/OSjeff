//! Hover: which window, file item and title-bar button the pointer is over.

use crate::desktop::*;

impl Desktop {
    /// Update the hover state for a pointer at `(cx, cy)`. Returns whether the scene changed.
    pub(super) fn mouse_hover(&mut self, cx: i32, cy: i32, cursor_moved: bool) -> bool {
        let mut scene = false;
        // Hover: the window under the cursor shows its minimize / maximize
        // buttons. Only an enter / leave changes pixels; skipped while dragging.
        if self.drag.is_none() {
            let hov = self.topmost_at(cx, cy);
            // The item or button under the pointer in a file manager or viewer lights up.
            if let Some(h) = hov
                && cursor_moved
                && !self.overlay_open()
            {
                let changed = match self.kind_of(h) {
                    Some(Kind::Files) => self.files_hover(h, cx, cy),
                    Some(Kind::Viewer) => self.viewer_hover(h, cx, cy),
                    Some(Kind::Editor) => self.editor_hover(h, cx, cy),
                    _ => false,
                };
                if changed && let Some(win) = self.wm.get(h) {
                    let b = self.window_box(win);
                    self.mark_dirty(b);
                }
            }
            if hov != self.hover {
                match self.hover.and_then(|o| self.kind_of(o).map(|k| (o, k))) {
                    Some((o, Kind::Files)) => self.files_unhover(o),
                    Some((o, Kind::Viewer)) => self.viewer_unhover(o),
                    Some((o, Kind::Editor)) => self.editor_unhover(o),
                    _ => {}
                }
                for id in [self.hover, hov].into_iter().flatten() {
                    if let Some(r) = self.wm.get(id).map(|w| self.window_box(w)) {
                        self.mark_dirty(r);
                    }
                }
                self.hover = hov;
                scene = true;
            }
            // Which title-bar button the pointer is on (its hover fill).
            let btn = hov
                .and_then(|id| self.wm.get(id))
                .and_then(|w| {
                    w.rect
                        .title_button_at(w.resizable, true, cx, cy)
                        .map(|b| (w.id, b))
                })
                .filter(|_| !self.overlay_open());
            if btn != self.title_hover {
                for id in [self.title_hover.map(|t| t.0), btn.map(|t| t.0)]
                    .into_iter()
                    .flatten()
                {
                    if let Some(r) = self.wm.get(id).map(|w| self.window_box(w)) {
                        self.mark_dirty(r);
                    }
                }
                self.title_hover = btn;
                scene = true;
            }
        }
        scene
    }
}
