//! Window geometry actions: maximise, tile, workspaces and snapping.

use crate::desktop::*;

impl Desktop {
    /// Maximize / restore window `id` and make the next frame repaint the whole
    /// screen (the window and everything it uncovers change).
    pub(crate) fn toggle_maximize(&mut self, id: WindowId) {
        let work = self.work_area();
        if self.wm.toggle_maximize(id, work) {
            self.geometry_changed(id);
        }
    }

    /// A window's rectangle changed by more than a drag step (maximise, snap, restore): repaint
    /// the screen and lay out the apps whose content depends on their width.
    pub(crate) fn geometry_changed(&mut self, id: WindowId) {
        self.force_full = true;
        self.relayout_browser(id);
        self.relayout_viewer(id);
    }

    /// Show workspace `to` (windows slide) and put overlays away.
    pub(crate) fn go_workspace(&mut self, to: u8) {
        self.close_transients();
        self.shell.snap = None;
        if self.wm.switch_workspace(to) {
            self.drag = None;
            self.title_hover = None;
            self.force_full = true;
        }
    }

    /// Move window `id` to workspace `to` and go there with it.
    pub(crate) fn move_window_to_workspace(&mut self, id: WindowId, to: u8) {
        if self.wm.move_to_workspace(id, to) {
            self.force_full = true;
            self.go_workspace(to);
            self.wm.activate(id);
        }
    }

    /// Tile window `id` to `zone` of the work area (animated).
    pub(crate) fn snap_window(&mut self, id: WindowId, zone: kitsune_core::snap::SnapZone) {
        let work = self.work_area();
        if self.wm.snap_to(id, zone, work) {
            self.geometry_changed(id);
        }
    }

    /// `Alt+arrow` on the focused window. `true` when there was a window to act on.
    pub(crate) fn snap_key(&mut self, arrow: kitsune_core::snap::Arrow) -> bool {
        use kitsune_core::snap::{SnapAct, key_action};
        let Some(id) = self.focused() else {
            return false;
        };
        let Some(state) = self.wm.get(id).map(|w| w.snap_state()) else {
            return false;
        };
        match key_action(state, arrow) {
            SnapAct::Zone(z) => self.snap_window(id, z),
            SnapAct::Restore => {
                if self.wm.unmaximize(id) {
                    self.geometry_changed(id);
                }
            }
            SnapAct::Minimize => {
                self.wm.minimize(id);
                self.force_full = true;
            }
            SnapAct::Stay => {}
        }
        true
    }
}
