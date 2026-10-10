//! Dragging in Arquivos: moving the selection, drop targets, the rubber band and the copy a drop starts.

use crate::desktop::*;
use alloc::boxed::Box;
use kitsune_core::fileman::ui::{self, DropOp, DropTarget};
use kitsune_core::fileman::{self, APPS_PATH, TRASH_PATH};
use kitsune_core::{t, tk, tp};

impl Desktop {
    /// The pointer moved with the left button held after a press in window `id`.
    pub(crate) fn files_drag(&mut self, id: WindowId, px: i32, py: i32) {
        let ctrl = self.keymap.ctrl();
        let Some(lay) = self.files_layout(id) else {
            return;
        };
        let Some(f) = self.files_mut(id) else {
            return;
        };
        match &mut f.gesture {
            Gesture::None => {}
            Gesture::Thumb { .. } => self.files_thumb_to(id, py),
            Gesture::Press { item, at, .. } => {
                if !ui::drag_started(*at, (px, py)) {
                    return;
                }
                let item = *item;
                let sources = f.view.selected_paths();
                if sources.is_empty() || !f.view.sel.is_selected(item) {
                    // Nothing to carry (the trash, the Apps place, a toggled-off item).
                    f.gesture = Gesture::None;
                    return;
                }
                let first = f.view.rows.get(item);
                let label = first
                    .map(|r| String::from_utf8_lossy(&r.name).into_owned())
                    .unwrap_or_default();
                let kind = first.map_or(kitsune_core::appart::FileKind::Generic, |r| {
                    ui::icon_kind(&r.name, r.is_dir())
                });
                f.gesture = Gesture::Drag(Box::new(DragState {
                    count: sources.len(),
                    sources,
                    label,
                    kind,
                    over: DropHover::None,
                    op: None,
                    pos: (px, py),
                }));
                self.files_drag_target(id, px, py, ctrl);
            }
            Gesture::Drag(_) => self.files_drag_target(id, px, py, ctrl),
            Gesture::Band { cur, .. } => {
                *cur = (px, py);
                self.files_band_update(id);
            }
        }
        let _ = lay;
    }

    /// While dragging items: find what is under the pointer and what a drop would do.
    fn files_drag_target(&mut self, id: WindowId, px: i32, py: i32, copy: bool) {
        let hit = self.files_hit(id, px, py).map(|(_, h)| h);
        let Some(f) = self.files_mut(id) else {
            return;
        };
        let Gesture::Drag(d) = &mut f.gesture else {
            return;
        };
        d.pos = (px, py);
        let cwd = f.view.cwd.clone();
        // Resolve the target to a folder path (or the bin) and a highlight.
        let (over, dest): (DropHover, Option<Result<Vec<u8>, ()>>) = match hit {
            Some(ui::Hit::Place(p)) => match ui::place_target(p) {
                DropTarget::Folder(path) => (DropHover::Place(p), Some(Ok(path.to_vec()))),
                DropTarget::Trash => (DropHover::Place(p), Some(Err(()))),
                DropTarget::None => (DropHover::None, None),
            },
            Some(ui::Hit::Crumb(i)) => {
                let crumbs = fileman::breadcrumbs(&cwd);
                match crumbs.get(i) {
                    Some(c) if c.path != TRASH_PATH && c.path != APPS_PATH => {
                        (DropHover::Crumb(i), Some(Ok(c.path.clone())))
                    }
                    _ => (DropHover::None, None),
                }
            }
            Some(ui::Hit::Item(j)) => match f.view.rows.get(j) {
                Some(r) if r.is_dir() && !f.view.sel.is_selected(j) => {
                    (DropHover::Item(j), Some(Ok(vfs::join(&cwd, &r.name))))
                }
                _ => (DropHover::None, None),
            },
            _ => (DropHover::None, None),
        };
        let Gesture::Drag(d) = &mut f.gesture else {
            return;
        };
        d.over = DropHover::None;
        d.op = None;
        if let Some(dest) = dest {
            let target = match &dest {
                Ok(p) => DropTarget::Folder(alloc::borrow::Cow::Borrowed(p.as_slice())),
                Err(()) => DropTarget::Trash,
            };
            d.op = ui::plan_drop(&d.sources, target, copy);
            if d.op.is_some() {
                d.over = over;
            }
        }
        // Dragging near the top or bottom edge scrolls the list.
        if let Some(lay) = self.files_layout(id) {
            let dy = ui::edge_scroll(py, lay.list.y, lay.list.bottom());
            if dy != 0
                && lay.list.contains(px, py)
                && let Some(f) = self.files_mut(id)
            {
                f.scroller.jump(f.scroller.pos() + dy);
            }
        }
    }

    /// Update the rubber band selection from the pointer position (and auto-scroll).
    pub(super) fn files_band_update(&mut self, id: WindowId) {
        let Some(lay) = self.files_layout(id) else {
            return;
        };
        let Some(f) = self.files_mut(id) else {
            return;
        };
        let n = f.view.rows.len();
        let (mode, vw) = (f.mode, lay.list.w);
        let Gesture::Band {
            anchor,
            cur,
            base,
            additive,
        } = &f.gesture
        else {
            return;
        };
        let dy = ui::edge_scroll(cur.1, lay.list.y, lay.list.bottom());
        let (anchor, cur, additive) = (*anchor, *cur, *additive);
        let base = base.clone();
        if dy != 0 {
            f.scroller.jump(f.scroller.pos() + dy);
        }
        let scroll = f.scroller.pos();
        let here = (cur.0 - lay.list.x, cur.1 - lay.list.y + scroll);
        let band = ui::band_rect(anchor, here);
        let touched = ui::items_in_rect(mode, vw, n, band);
        let sel = ui::band_selection(&base, &touched, additive);
        if sel.is_empty() {
            f.view.sel.clear();
        } else {
            f.view.sel.select_set(&sel);
        }
        f.scroll_fade.touch(appui::now_ms());
    }

    /// The left button was released after a press or drag in window `id`.
    pub(crate) fn files_release(&mut self, id: WindowId, px: i32, py: i32) {
        let ctrl = self.keymap.ctrl();
        let Some(f) = self.files_mut(id) else {
            return;
        };
        let g = core::mem::replace(&mut f.gesture, Gesture::None);
        match g {
            Gesture::Press { item, collapse, .. } => {
                if collapse {
                    f.view.sel.only(item);
                }
            }
            Gesture::Drag(d) => {
                // Refresh the target for the final pointer position, then drop.
                f.gesture = Gesture::Drag(d);
                self.files_drag_target(id, px, py, ctrl);
                let Some(f) = self.files_mut(id) else {
                    return;
                };
                let Gesture::Drag(d) = core::mem::replace(&mut f.gesture, Gesture::None) else {
                    return;
                };
                self.files_drop(id, *d);
            }
            Gesture::Band { .. } | Gesture::Thumb { .. } | Gesture::None => {}
        }
        self.files_sync_preview(id);
    }

    /// Carry out a drop.
    fn files_drop(&mut self, id: WindowId, d: DragState) {
        let Some(op) = d.op else {
            return;
        };
        let cwd = self
            .files_mut(id)
            .map(|f| f.view.cwd.clone())
            .unwrap_or_default();
        let dest: Option<Vec<u8>> = match d.over {
            DropHover::Place(p) => match ui::place_target(p) {
                DropTarget::Folder(path) => Some(path.to_vec()),
                _ => None,
            },
            DropHover::Crumb(i) => fileman::breadcrumbs(&cwd).get(i).map(|c| c.path.clone()),
            DropHover::Item(j) => self
                .files_mut(id)
                .and_then(|f| f.view.rows.get(j).map(|r| vfs::join(&cwd, &r.name))),
            DropHover::None => None,
        };
        let name = |p: &[u8]| {
            if p == b"/" {
                String::from(t!("files.place.disk"))
            } else {
                String::from_utf8_lossy(vfs::base_name(p)).into_owned()
            }
        };
        match op {
            DropOp::Trash => {
                let mut done = 0;
                let mut err = None;
                for p in &d.sources {
                    match vfs::remove(p) {
                        Ok(()) => done += 1,
                        Err(e) => {
                            err = Some(e);
                            break;
                        }
                    }
                }
                self.fs_changed();
                match err {
                    None => self.files_note(id, &tp!("files.msg.trashed", done), false),
                    Some(e) => self.files_note(id, e.message(), true),
                }
            }
            DropOp::Move => {
                let Some(dest) = dest else {
                    return;
                };
                let rep = vfs::move_to(&d.sources, &dest);
                let n = rep.moved.len();
                self.fs_changed();
                match rep.error {
                    None => self.files_note(
                        id,
                        &tp!("files.msg.moved_to", n, dest = &name(&dest)),
                        false,
                    ),
                    Some(e) => self.files_note(id, e.message(), true),
                }
            }
            DropOp::Copy => {
                let Some(dest) = dest else {
                    return;
                };
                self.files_start_copy(id, &d.sources, &dest);
            }
        }
    }

    /// Plan a copy of `sources` into `dest` and run it as a job of window `id`.
    pub(super) fn files_start_copy(&mut self, id: WindowId, sources: &[Vec<u8>], dest: &[u8]) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        if f.job.is_some() {
            f.say(t!("files.msg.copy_running"), true);
            return;
        }
        match vfs::copy_plan(sources, dest) {
            Ok(job) => {
                if let Some(f) = self.files_mut(id) {
                    f.job = Some(Job {
                        copy: job,
                        label: tk!("files.job.copying"),
                        started: crate::interrupts::ticks(),
                    });
                    f.msg = None;
                }
            }
            Err(e) => self.files_note(id, e.message(), true),
        }
    }
}
