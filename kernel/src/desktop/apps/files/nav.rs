//! Navigation: loading folders, places, history, activating items and following the
//! filesystem generation.

use crate::desktop::apps::files::labels::crumbs_of;
use crate::desktop::*;
use kitsune_core::fileman::apps::{self as fapps, AppAction, AppItem, AppKey};
use kitsune_core::fileman::ui::{self, HitCtx, Layout, crumb_layout};
use kitsune_core::fileman::{self, Activation, Cmd, FileClass, Place};
use kitsune_core::t;

impl Desktop {
    /// The interface language changed: every file manager holds text built with the old one
    /// (status message, preview pane, information sheet). Drop or rebuild it; what is drawn from
    /// the catalog each frame needs nothing.
    pub(crate) fn files_language_changed_all(&mut self) {
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Files(_)))
            .map(|w| w.id)
            .collect();
        for id in ids {
            self.files_language_changed(id);
        }
    }

    fn files_language_changed(&mut self, id: WindowId) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        f.msg = None;
        f.preview = None;
        let props_open = f.props.is_some();
        let apps = f.view.in_apps();
        let in_trash = f.view.in_trash();
        let cwd = f.view.cwd.clone();
        let paths = f.view.selected_paths();
        if props_open {
            if apps {
                self.files_app_properties(id);
            } else {
                self.files_properties(id, in_trash, &cwd, &paths);
            }
            // The sheet stays up as it was: no slide-in again.
            if let Some(f) = self.files_mut(id) {
                f.sheet_t = kitsune_core::anim::Tween::at(1.0);
            }
        }
        self.files_sync_preview(id);
    }

    // ---- file manager ----

    /// Reload window `id`'s folder from the filesystem.
    pub(crate) fn files_refresh(&mut self, id: WindowId) {
        let items = self.apps_items_if_shown(id);
        let Some(f) = self.files_mut(id) else {
            return;
        };
        match vfs::with_backend(|b| f.view.refresh(b)) {
            Ok(Ok(())) => {}
            Ok(Err(e)) | Err(e) => f.say(e.message(), true),
        }
        if let Some(items) = items {
            f.view.set_apps(&items);
        }
        f.usage = vfs::statfs();
        if f.msg.is_none()
            && let Some(n) = vfs::notice()
        {
            f.say(n, true);
        }
        self.files_sync_preview(id);
    }

    /// Something changed on disk: every file manager reloads, the next frame repaints.
    pub(crate) fn fs_changed(&mut self) {
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Files(_)))
            .map(|w| w.id)
            .collect();
        for id in ids {
            self.files_refresh(id);
        }
        self.fs_gen = vfs::generation();
    }

    /// Once per frame: when something other than the desktop itself changed the
    /// volume (an app writing through the app filesystem), reload the file
    /// managers, at most every 100 ms so a chatty app cannot make them thrash.
    pub(crate) fn poll_fs_changes(&mut self) {
        let g = vfs::generation();
        if g == self.fs_gen {
            return;
        }
        let now = crate::interrupts::ticks();
        if now.saturating_sub(self.fs_gen_tick) < 25 {
            return;
        }
        self.fs_gen_tick = now;
        self.fs_changed();
    }

    /// The Apps place's rows (installed apps, then the bundled packages not installed).
    fn app_items(&self) -> Vec<AppItem> {
        self.app_rows()
            .into_iter()
            .map(|r| AppItem {
                id: r.id,
                name: r.name,
                installed: r.installed,
                size: r.size,
            })
            .collect()
    }

    /// The app list when window `id` is showing the Apps place (it is then loaded
    /// into the view by the caller), else `None`.
    fn apps_items_if_shown(&self, id: WindowId) -> Option<Vec<AppItem>> {
        match self.wm.get(id).map(|w| &w.app.app) {
            Some(App::Files(f)) if f.view.in_apps() => Some(self.app_items()),
            _ => None,
        }
    }

    /// The catalog changed (install, remove): every file manager on the Apps place
    /// reloads its rows.
    pub(crate) fn refresh_apps_views(&mut self) {
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(&w.app.app, App::Files(f) if f.view.in_apps()))
            .map(|w| w.id)
            .collect();
        if ids.is_empty() {
            return;
        }
        let items = self.app_items();
        for id in ids {
            if let Some(f) = self.files_mut(id) {
                f.view.set_apps(&items);
            }
        }
    }

    /// Geometry of window `id` as the renderer sees it.
    pub(crate) fn files_layout(&self, id: WindowId) -> Option<Layout> {
        let w = self.wm.get(id)?;
        let App::Files(f) = &w.app.app else {
            return None;
        };
        Some(Layout::of(
            w.rect,
            f.mode,
            f.preview_open,
            f.search_is_open(),
        ))
    }

    /// What `(px, py)` lands on in window `id`.
    pub(crate) fn files_hit(&self, id: WindowId, px: i32, py: i32) -> Option<(Layout, ui::Hit)> {
        let w = self.wm.get(id)?;
        let App::Files(f) = &w.app.app else {
            return None;
        };
        let lay = Layout::of(w.rect, f.mode, f.preview_open, f.search_is_open());
        let (_, _, widths) = crumbs_of(&f.view.cwd);
        let crumbs = crumb_layout(lay.path, &widths);
        let hit = lay.hit(
            px,
            py,
            &HitCtx {
                mode: f.mode,
                scroll: f.scroller.pos(),
                count: f.view.rows.len(),
                crumbs: &crumbs,
            },
        )?;
        Some((lay, hit))
    }

    /// Run `f` on window `id`'s state, then say `ok` or the error.
    pub(super) fn files_note(&mut self, id: WindowId, msg: &str, error: bool) {
        if let Some(f) = self.files_mut(id) {
            f.say(msg, error);
        }
    }

    /// Move the keyboard cursor to row `i` and scroll to it.
    pub(super) fn files_reveal(&mut self, id: WindowId) {
        let Some(lay) = self.files_layout(id) else {
            return;
        };
        let Some(f) = self.files_mut(id) else {
            return;
        };
        let n = f.view.rows.len();
        if n == 0 {
            return;
        }
        let to = ui::reveal(
            f.mode,
            lay.list.w,
            lay.list.h,
            f.scroller.target(),
            f.view.sel.cursor().min(n - 1),
            n,
        );
        f.scroller
            .set_max(ui::max_scroll(f.mode, lay.list.w, lay.list.h, n));
        f.scroller.scroll_to(to);
        f.scroll_fade.touch(appui::now_ms());
    }

    /// Go to `path` (a folder, or `/.trash`).
    pub(crate) fn files_go(&mut self, id: WindowId, path: &[u8]) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        f.msg = None;
        f.input = None;
        match vfs::with_backend(|b| f.view.navigate(b, path)) {
            Ok(Ok(())) => {}
            Ok(Err(e)) | Err(e) => f.say(e.message(), true),
        }
        self.files_load_apps(id);
        self.files_sync_preview(id);
    }

    /// Open a sidebar place; a favourite folder that does not exist yet is created.
    pub(crate) fn files_go_place(&mut self, id: WindowId, p: Place) {
        if p.is_folder() && !vfs::exists(p.path()) {
            let _ = vfs::mkdir(p.path());
            self.fs_changed();
        }
        self.files_go(id, p.path());
    }

    /// When window `id` is on the Apps place, fill its rows from the catalog.
    fn files_load_apps(&mut self, id: WindowId) {
        if let Some(items) = self.apps_items_if_shown(id)
            && let Some(f) = self.files_mut(id)
        {
            f.view.set_apps(&items);
        }
    }

    /// Back, forward or up.
    pub(crate) fn files_history(&mut self, id: WindowId, which: u8) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        f.input = None;
        let r = vfs::with_backend(|b| match which {
            0 => f.view.go_back(b),
            1 => f.view.go_forward(b),
            _ => f.view.go_up(b),
        });
        match r {
            Ok(Ok(())) => f.msg = None,
            Ok(Err(e)) | Err(e) => f.say(e.message(), true),
        }
        self.files_load_apps(id);
        self.files_sync_preview(id);
    }

    /// Enter / double click on row `i`.
    pub(crate) fn files_activate(&mut self, id: WindowId, i: usize) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        if f.view.in_trash() {
            self.files_cmd(id, Cmd::Restore);
            return;
        }
        let act = match vfs::with_backend(|b| f.view.activate(b, i)) {
            Ok(a) => a,
            Err(e) => {
                f.say(e.message(), true);
                return;
            }
        };
        f.msg = None;
        match act {
            Activation::Open(path, class) => self.open_path(id, &path, class),
            Activation::App { .. } => self.files_app_key(id, i, AppKey::Enter),
            Activation::Entered | Activation::None => {}
        }
        self.files_sync_preview(id);
    }

    /// A key of the Apps place on row `i`: run, install or remove the app, and say
    /// the outcome (or why it does not apply) in the status line.
    pub(super) fn files_app_key(&mut self, id: WindowId, i: usize, key: AppKey) {
        let Some(row) = self.files_mut(id).and_then(|f| f.view.rows.get(i).cloned()) else {
            return;
        };
        let action = match fapps::app_action(&row, key) {
            Ok(a) => a,
            Err(m) => return self.files_note(id, m, true),
        };
        let name = String::from_utf8_lossy(&row.name).into_owned();
        let result: Result<String, String> = match action {
            AppAction::Launch(app) => {
                self.launch_wasm_app(&app);
                Ok(t!("files.msg.opened", name = &name))
            }
            AppAction::InstallAndLaunch(app) => self.install_bundled(&app).map(|()| {
                self.launch_wasm_app(&app);
                t!("files.msg.installed_opened", name = &name)
            }),
            AppAction::Install(app) => self
                .install_bundled(&app)
                .map(|()| t!("files.msg.installed", name = &name)),
            AppAction::Remove(app) => self
                .remove_app(&app)
                .map(|()| t!("files.msg.removed", name = &name)),
        };
        match result {
            Ok(m) => self.files_note(id, &m, false),
            Err(e) => self.files_note(id, &e, true),
        }
    }

    /// Open `path` the way its type asks; failures show in window `from`.
    pub(crate) fn open_path(&mut self, from: WindowId, path: &[u8], class: FileClass) {
        let note = match class {
            FileClass::Image => self.open_viewer(path).map(|e| (e, true)),
            FileClass::Wasm => {
                self.open_wasm_path(path);
                None
            }
            FileClass::Text | FileClass::Other => {
                let sniff = vfs::read_range(path, 0, 4096);
                match sniff {
                    Err(e) => Some((String::from(e.message()), true)),
                    Ok(head) if class == FileClass::Other && !fileman::looks_like_text(&head) => {
                        Some((String::from(t!("files.msg.unsupported")), true))
                    }
                    Ok(_) => match self.fs_load_path(path.to_vec()) {
                        Ok(Some(w)) => Some((String::from(w), true)),
                        Ok(None) => None,
                        Err(e) => Some((String::from(e.message()), true)),
                    },
                }
            }
        };
        if let Some((m, err)) = note {
            self.files_note(from, &m, err);
        }
    }
}
