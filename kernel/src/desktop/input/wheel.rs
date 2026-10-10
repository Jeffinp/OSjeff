//! The mouse wheel.

use crate::desktop::*;

impl Desktop {
    /// A mouse-wheel step (`dz` > 0 = wheel toward the user = scroll down). As on desktop
    /// systems the window *under the pointer* gets it, focused or not, and the wheel does not
    /// change focus. Returns whether anything changed (the caller repaints). Ignored while a menu,
    /// the Apps launcher or the Alt+Tab switcher is up, or a window is being dragged.
    pub fn handle_wheel(&mut self, dz: i32) -> bool {
        if dz != 0 && self.shell.apps.is_some() {
            return self.shell_wheel(dz);
        }
        if dz == 0 || self.overlay_open() || self.drag.is_some() {
            return false;
        }
        let Some(w) = self.topmost_at(self.cursor_x, self.cursor_y) else {
            return false;
        };
        let Some(kind) = self.kind_of(w) else {
            return false;
        };
        let notches = dz.clamp(-8, 8);
        let changed = match kind {
            Kind::Browser => {
                if self.browser_wheel(w, notches) {
                    self.client_dirty = Some(w);
                }
                false
            }
            Kind::TaskMgr => {
                self.tarefas_wheel(w, notches);
                true
            }
            Kind::Files => {
                self.files_wheel(w, notches);
                true
            }
            Kind::Viewer => {
                // Wheel away from the user zooms in.
                self.viewer_wheel(w, -notches);
                true
            }
            Kind::Editor => {
                self.editor_wheel(w, -notches);
                true
            }
            Kind::Terminal => {
                // Wheel away from the user scrolls back in time.
                self.term_wheel(w, -notches);
                true
            }
            // Nothing to scroll (the WASM guest has no wheel ABI).
            Kind::LogViewer => {
                self.log_wheel(w, notches);
                true
            }
            Kind::Settings => {
                self.settings_wheel(w, notches);
                true
            }
            Kind::Calculator | Kind::WasmApp | Kind::Gallery => false,
        };
        if changed && let Some(r) = self.wm.get(w).map(|win| self.window_box(win)) {
            // The target may not be the focused window: make sure it is uploaded.
            self.mark_dirty(r);
        }
        changed
    }
}
