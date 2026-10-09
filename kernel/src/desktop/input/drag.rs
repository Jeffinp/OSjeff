//! The end of a press and the drag in progress: moving, resizing, panning, selecting and dropping.

use crate::desktop::*;

impl Desktop {
    /// The button went up at `(cx, cy)`: finish the drag, if any. Returns whether the scene changed.
    pub(super) fn mouse_release(&mut self, cx: i32, cy: i32) -> bool {
        let mut scene = false;
        self.dock_release(cx, cy);
        if let Some(d) = self.drag.take() {
            if matches!(d.mode, DragMode::Ui) {
                self.live_drop(d.win);
            }
            // A resized browser lays its page out again for the new width.
            if matches!(d.mode, DragMode::Resize { .. }) {
                self.relayout_browser(d.win);
                self.relayout_viewer(d.win);
            }
            // Dropped on an edge: tile the window where the preview showed.
            if let Some(p) = self.shell.snap.take() {
                self.snap_window(d.win, p.zone);
                self.force_full = true;
            }
            // A press in a file manager ends: a click, a drop or the end of a band.
            if matches!(d.mode, DragMode::Files) {
                self.files_release(d.win, cx, cy);
                scene = true;
            }
            // A dragged picture glides on after the button is released.
            if matches!(d.mode, DragMode::Pan { .. }) {
                self.viewer_release(d.win);
            }
        }
        scene
    }

    /// Advance the drag in progress for a pointer at `(cx, cy)` with the left button `left`.
    pub(super) fn mouse_drag(&mut self, left: bool, cx: i32, cy: i32) {
        if let Some(d) = &self.drag {
            if left {
                let (w, mode) = (d.win, d.mode);
                let (sw, sh) = (self.sw, self.sh);
                match mode {
                    DragMode::Pan { last_x, last_y } => {
                        self.viewer_pan(w, cx - last_x, cy - last_y);
                        self.drag = Some(Drag {
                            win: w,
                            mode: DragMode::Pan {
                                last_x: cx,
                                last_y: cy,
                            },
                        });
                    }
                    DragMode::Select => {
                        if self.kind_of(w) == Some(Kind::Terminal) {
                            self.term_drag(w, cx, cy);
                        } else {
                            self.editor_drag(w, cx, cy);
                        }
                    }
                    DragMode::Move { grab_dx, grab_dy } => {
                        self.wm.move_to(w, cx - grab_dx, cy - grab_dy, sw, sh);
                        self.update_snap_preview(w, cx, cy);
                    }
                    DragMode::Unsnap { ox, oy } => {
                        if (cx - ox).abs() >= 4 || (cy - oy).abs() >= 4 {
                            if let Some((gx, gy)) = self.wm.restore_for_drag(w, cx, cy) {
                                self.drag = Some(Drag {
                                    win: w,
                                    mode: DragMode::Move {
                                        grab_dx: gx,
                                        grab_dy: gy,
                                    },
                                });
                                self.geometry_changed(w);
                                self.wm.move_to(w, cx - gx, cy - gy, sw, sh);
                            } else {
                                self.drag = None;
                            }
                        }
                    }
                    DragMode::Resize {
                        edge,
                        start,
                        ox,
                        oy,
                    } => {
                        self.wm.resize(w, edge, start, (cx - ox, cy - oy), (sw, sh));
                    }
                    DragMode::PageSelect => self.browser_select_drag(w),
                    DragMode::Ui => self.live_drag(w, cx, cy),
                    DragMode::Files => self.files_drag(w, cx, cy),
                }
                // NOT scene_dirty: a drag is driven by the per-frame damage path
                // (keyed on `cursor_moved`), which repaints only the window's
                // old+new rect. Marking the whole scene dirty would force a full
                // recompose + 8 MiB blit every mouse step — the very cost we're
                // removing. The dragged window is kept out of the static layer
                // (see `is_dynamic`), so moving it never touches the others.
            } else {
                self.drag = None;
                self.shell.snap = None;
            }
        }
    }
}
