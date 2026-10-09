//! A mouse button going down: the right button opens context menus, the left one presses shell items, toasts and windows.

use crate::desktop::*;

impl Desktop {
    /// A right press at `(cx, cy)`: an app-bar icon's menu, the file manager's own menu, or the
    /// desktop menu on empty space. Returns whether the scene changed.
    pub(super) fn mouse_right_press(&mut self, cx: i32, cy: i32) -> bool {
        let mut scene = false;
        // Right click: an app-bar icon's menu, the file manager's own menu, or the
        // desktop menu on empty space.
        let files_right = !self.overlay_open()
            && self.topmost_at(cx, cy).is_some_and(|w| {
                self.kind_of(w) == Some(Kind::Files)
                    && self.wm.get(w).is_some_and(|win| !win.rect.on_title(cx, cy))
            });
        if let Some((item, _)) = self.panel_item_at(cx, cy).filter(|_| !self.modal_open()) {
            self.close_transients();
            self.panel_context(item);
            scene = true;
        } else if let Some(h) = self.dock_item_at(cx, cy).filter(|_| !self.modal_open()) {
            self.close_transients();
            self.dock_context(h, cx, cy);
            scene = true;
        } else if self.overlay_open() {
            self.close_transients();
            scene = true;
        } else if files_right && let Some(w) = self.topmost_at(cx, cy) {
            self.wm.raise(w);
            if let Some(rect) = self.wm.get(w).map(|win| win.rect) {
                self.files_click(w, rect, cx, cy, true);
            }
            scene = true;
        } else if let Some(w) = self
            .topmost_at(cx, cy)
            .filter(|&w| self.kind_of(w) == Some(Kind::Browser))
        {
            self.wm.raise(w);
            self.browser_rclick(w, cx, cy);
            scene = true;
        } else if self.wasm_pointer_target(cx, cy).is_none()
            && self.topmost_at(cx, cy).is_none()
            && cy >= MENUBAR_H
        {
            self.desktop_context(cx, cy);
            scene = true;
        }
        scene
    }

    /// A left press at `(cx, cy)`: a toast, the shell layers, the app bar, then the window under
    /// the pointer. Returns whether the scene changed.
    pub(super) fn mouse_left_press(&mut self, cx: i32, cy: i32) -> bool {
        let mut scene = false;
        if self
            .toasts
            .click(cx, cy, self.sw, self.sh, shell::toasts::now_ms())
        {
            // A click on a toast only dismisses it.
            self.toast_dirty = true;
        } else if self.shell_click(cx, cy) {
            scene = true;
        } else if let Some(h) = self.dock_item_at(cx, cy) {
            self.dock_press(h, cx, cy);
            scene = true;
        } else if let Some(w) = self.topmost_at(cx, cy) {
            self.click_window(w, cx, cy);
            scene = true;
        }
        scene
    }
}
