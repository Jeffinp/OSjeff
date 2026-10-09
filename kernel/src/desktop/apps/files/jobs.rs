//! Long copies in steps, the confirmation sheet and the per-frame state.

use crate::desktop::apps::files::props::SheetKind;
use crate::desktop::apps::files::props::files_sheet_kind;
use crate::desktop::*;
use kitsune_core::fileman::ui::{self, Layout};
use kitsune_core::{t, tp};

/// Bytes copied per frame by a running copy job.
const JOB_CHUNK: usize = 128 * 1024;

impl Desktop {
    /// Abort window `id`'s copy (Esc, Cancelar): the half-written file goes away.
    pub(crate) fn files_cancel_job(&mut self, id: WindowId) {
        if let Some(f) = self.files_mut(id)
            && let Some(mut job) = f.job.take()
        {
            vfs::copy_abort(&mut job.copy);
            f.say(t!("files.msg.copy_cancelled"), false);
        }
        self.fs_changed();
    }

    /// One bounded step of every running copy (called each frame from `animate`).
    pub(crate) fn step_file_jobs(&mut self) {
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(&w.app.app, App::Files(f) if f.job.is_some()))
            .map(|w| w.id)
            .collect();
        for id in ids {
            let Some(f) = self.files_mut(id) else {
                continue;
            };
            let Some(job) = f.job.as_mut() else {
                continue;
            };
            let finished = match vfs::copy_step(&mut job.copy, JOB_CHUNK) {
                Ok(vfs::Progress::Running) => None,
                Ok(vfs::Progress::Done) => {
                    let (n, _) = job.copy.files();
                    Some((tp!("files.msg.copy_done", n), false))
                }
                Err(e) => Some((String::from(e.message()), true)),
            };
            if let Some((m, err)) = finished {
                let results: Vec<Vec<u8>> = f
                    .job
                    .as_ref()
                    .map(|j| j.copy.results().to_vec())
                    .unwrap_or_default();
                f.job = None;
                f.say(&m, err);
                self.fs_changed();
                if !err {
                    self.files_select_paths(id, &results);
                }
            }
        }
    }

    /// Enter in the inline name field: rename.
    pub(super) fn files_commit_input(&mut self, id: WindowId) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        let Some(edit) = f.input.take() else {
            return;
        };
        let name = edit.input.text().to_vec();
        let EditPurpose::Rename(path) = &edit.purpose;
        if vfs::base_name(path) == &name[..] {
            return;
        }
        match vfs::rename(path, &name) {
            Ok(new_path) => {
                self.fs_changed();
                if let Some(f) = self.files_mut(id) {
                    f.view.select_name(vfs::base_name(&new_path));
                    f.msg = None;
                }
                self.files_reveal(id);
            }
            Err(e) => {
                // Keep the field open so the name can be fixed.
                if let Some(f) = self.files_mut(id) {
                    f.input = Some(edit);
                    f.say(e.message(), true);
                }
            }
        }
    }

    /// Enter on a confirmation: do the permanent delete.
    pub(super) fn files_confirmed(&mut self, id: WindowId) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        let Some(q) = f.confirm.take() else {
            return;
        };
        let mut err = None;
        match q {
            Confirm::Purge(paths) => {
                for p in &paths {
                    if let Err(e) = vfs::purge(p) {
                        err = Some(e);
                        break;
                    }
                }
            }
            Confirm::PurgeTrash(ids) => {
                for t in &ids {
                    if let Err(e) = vfs::trash_purge(t) {
                        err = Some(e);
                        break;
                    }
                }
            }
            Confirm::EmptyTrash => {
                if let Err(e) = vfs::empty_trash() {
                    err = Some(e);
                }
            }
        }
        self.fs_changed();
        match err {
            None => self.files_note(id, t!("files.msg.deleted"), false),
            Some(e) => self.files_note(id, e.message(), true),
        }
    }

    /// A press while a sheet is up: its buttons.
    pub(super) fn files_sheet_click(&mut self, id: WindowId, lay: &Layout, px: i32, py: i32) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        let (kind, size) = files_sheet_kind(f);
        let panel = appui::sheet_rect(lay.window, size);
        let labels = kind.buttons();
        let btns = appui::button_row(
            panel.right() - appui::SHEET_PAD,
            panel.bottom() - appui::SHEET_PAD - appui::BUTTON_H,
            &labels,
        );
        let hit = btns.iter().position(|b| b.contains(px, py));
        let Some(i) = hit else {
            return;
        };
        match (kind, i) {
            (SheetKind::Confirm, 0) => {
                f.confirm = None;
                f.say(t!("files.msg.cancelled"), false);
            }
            (SheetKind::Confirm, _) => self.files_confirmed(id),
            (SheetKind::Info, _) => f.props = None,
            (SheetKind::Copy, _) => self.files_cancel_job(id),
        }
    }

    // ---- per-frame state ----

    /// Advance every file manager's animations by `dt` and keep its scroll range and
    /// navigation state in line with the window. Returns whether anything still moves.
    pub(crate) fn step_files(&mut self, dt: f32) -> bool {
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Files(_)))
            .map(|w| w.id)
            .collect();
        let mut busy = false;
        for id in ids {
            let Some(lay) = self.files_layout(id) else {
                continue;
            };
            let (cx, cy) = (self.cursor_x, self.cursor_y);
            let band = self
                .files_mut(id)
                .is_some_and(|f| matches!(f.gesture, Gesture::Band { .. }));
            if band {
                // A band held still at an edge keeps scrolling.
                if let Some(f) = self.files_mut(id)
                    && let Gesture::Band { cur, .. } = &mut f.gesture
                {
                    *cur = (cx, cy);
                }
                self.files_band_update(id);
            }
            let Some(f) = self.files_mut(id) else {
                continue;
            };
            let n = f.view.rows.len();
            f.scroller
                .set_max(ui::max_scroll(f.mode, lay.list.w, lay.list.h, n));
            if f.view.nav_gen != f.seen_nav {
                f.seen_nav = f.view.nav_gen;
                f.scroller.jump(0);
                f.hover = None;
                f.search.input.clear();
                f.enter_t = kitsune_core::anim::Tween::at(0.0);
                f.enter_t
                    .retarget(1.0, 0.2, kitsune_core::anim::curves::ENTER);
            }
            if f.copy_sheet() && f.sheet_t.target() < 1.0 {
                f.open_sheet();
            }
            busy |= f.scroller.step(dt);
            busy |= f.hover_t.step(dt);
            busy |= f.sheet_t.step(dt);
            busy |= f.enter_t.step(dt);
            busy |= f.animating();
        }
        busy
    }
}
