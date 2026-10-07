//! `Desktop` methods: files. Everything that reaches the filesystem from the
//! desktop goes through [`vfs`](super::vfs): the terminal commands (`shellhost`), the
//! editor's load and save (`edit`), and the file manager's commands (here).
//!
//! The file manager's state and decisions live in `osjeff_core::fileman` (tested
//! on the host); this module wires them to the VFS, the window table and the
//! clipboard of paths, and runs long copies in steps from `animate`.

use super::*;
use osjeff_core::fileman::apps::{self as fapps, AppAction, AppItem, AppKey};
use osjeff_core::fileman::{self, APPS_PATH, Activation, Cmd, FileClass, MenuCtx, TRASH_PATH};
use osjeff_core::vfs::VfsError;

/// Bytes copied per frame by a running copy job.
const JOB_CHUNK: usize = 128 * 1024;
/// A Unix time as local `dd/mm/aaaa hh:mm`.
pub(crate) fn local_time(t: u64) -> String {
    fileman::format_datetime(t, crate::rtc::TZ_OFFSET_HOURS * 3600)
}

impl Desktop {
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

    fn files_rect(&self, id: WindowId) -> Option<Rect> {
        self.wm.get(id).map(|w| w.rect)
    }

    fn files_visible(&self, id: WindowId) -> usize {
        self.files_rect(id)
            .map_or(10, |r| fileman::Layout::of(r).visible_rows())
    }

    /// Run `f` on window `id`'s state, then say `ok` or the error.
    fn files_note(&mut self, id: WindowId, msg: &str, error: bool) {
        if let Some(f) = self.files_mut(id) {
            f.say(msg, error);
        }
    }

    /// Go to `path` (a folder, or `/.trash`).
    pub(crate) fn files_go(&mut self, id: WindowId, path: &[u8]) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        f.menu = None;
        f.msg = None;
        match vfs::with_backend(|b| f.view.navigate(b, path)) {
            Ok(Ok(())) => {}
            Ok(Err(e)) | Err(e) => f.say(e.message(), true),
        }
        self.files_load_apps(id);
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
        f.menu = None;
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
    }

    /// A key of the Apps place on row `i`: run, install or remove the app, and say
    /// the outcome (or why it does not apply) in the status line.
    fn files_app_key(&mut self, id: WindowId, i: usize, key: AppKey) {
        let Some(row) = self.files_mut(id).and_then(|f| f.view.rows.get(i).cloned()) else {
            return;
        };
        let action = match fapps::app_action(&row, key) {
            Ok(a) => a,
            Err(m) => return self.files_note(id, m, true),
        };
        let name = String::from_utf8_lossy(&fileman::display_ascii(&row.name)).into_owned();
        let result: Result<String, String> = match action {
            AppAction::Launch(app) => {
                self.launch_wasm_app(&app);
                Ok(alloc::format!("{name} aberto"))
            }
            AppAction::InstallAndLaunch(app) => self.install_bundled(&app).map(|()| {
                self.launch_wasm_app(&app);
                alloc::format!("{name} instalado e aberto")
            }),
            AppAction::Install(app) => self
                .install_bundled(&app)
                .map(|()| alloc::format!("{name} instalado")),
            AppAction::Remove(app) => self
                .remove_app(&app)
                .map(|()| alloc::format!("{name} removido")),
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
                        Some((String::from("Formato nao suportado"), true))
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

    /// Context needed to build the context menu for a click on row `row`.
    fn files_menu_ctx(&self, id: WindowId) -> Option<MenuCtx> {
        let Some(App::Files(f)) = self.wm.get(id).map(|w| &w.app.app) else {
            return None;
        };
        let rows = f.view.selected_rows();
        Some(MenuCtx {
            in_trash: f.view.in_trash(),
            in_apps: f.view.in_apps(),
            app_installed: rows.len() == 1 && rows[0].installed,
            selected: rows.len(),
            image: rows.len() == 1 && fileman::is_image(&rows[0].name) && !rows[0].is_dir(),
            clip_has_items: !self.pathclip.is_empty(),
        })
    }

    /// A press in file-manager window `id` at `(px, py)` (screen coordinates).
    /// `right` is the right button. Geometry is `fileman::Layout`, the same the
    /// renderer uses.
    pub(crate) fn files_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32, right: bool) {
        let ctrl = self.keymap.ctrl();
        let shift = self.keymap.shift();
        let lay = fileman::Layout::of(rect);

        // An open context menu takes the click first.
        let menu = self.files_mut(id).and_then(|f| f.menu.take());
        if let Some(m) = menu {
            let h = menu_height(m.items.len());
            let inside = px >= m.x && px < m.x + MENU_W_FILES && py >= m.y && py < m.y + h;
            if inside {
                let i = ((py - m.y - 4) / MENU_ROW_H).max(0) as usize;
                if let Some(&(cmd, _)) = m.items.get(i) {
                    self.files_cmd(id, cmd);
                }
            }
            return;
        }
        // A modal panel: a click dismisses the properties, the rest wait for keys.
        if let Some(f) = self.files_mut(id) {
            if f.props.is_some() {
                f.props = None;
                return;
            }
            if f.modal() {
                return;
            }
        }

        let (labels, scroll) = match self.wm.get(id).map(|w| &w.app.app) {
            Some(App::Files(f)) => (
                fileman::breadcrumbs(&f.view.cwd)
                    .iter()
                    .map(|c| fileman::display_ascii(&c.label).len())
                    .collect::<Vec<_>>(),
                f.view.scroll,
            ),
            _ => return,
        };
        let hit = lay.hit(px, py, scroll, &labels);
        match hit {
            Some(fileman::Hit::Back) => self.files_history(id, 0),
            Some(fileman::Hit::Forward) => self.files_history(id, 1),
            Some(fileman::Hit::Up) => self.files_history(id, 2),
            Some(fileman::Hit::Crumb(i)) => {
                let target = self
                    .files_mut(id)
                    .map(|f| fileman::breadcrumbs(&f.view.cwd))
                    .and_then(|c| c.get(i).map(|c| c.path.clone()));
                if let Some(p) = target {
                    self.files_go(id, &p);
                }
            }
            Some(fileman::Hit::Place(p)) => {
                let path: &[u8] = match p {
                    fileman::Place::Root | fileman::Place::Disk => b"/",
                    fileman::Place::Documents => b"/Documentos",
                    fileman::Place::Apps => APPS_PATH,
                    fileman::Place::Trash => TRASH_PATH,
                };
                self.files_go(id, path);
            }
            Some(fileman::Hit::Header(k)) => {
                if let Some(f) = self.files_mut(id) {
                    f.view.click_header(k);
                }
            }
            Some(fileman::Hit::Scroll(pm)) => {
                if let Some(f) = self.files_mut(id) {
                    f.view.scroll = lay.scroll_for(pm, f.view.rows.len());
                }
            }
            Some(fileman::Hit::Row(i)) => self.files_row_click(id, i, ctrl, shift, right, px, py),
            Some(fileman::Hit::Address) | None => {}
            Some(fileman::Hit::Blank) => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn files_row_click(
        &mut self,
        id: WindowId,
        i: usize,
        ctrl: bool,
        shift: bool,
        right: bool,
        px: i32,
        py: i32,
    ) {
        let rows = self.files_mut(id).map_or(0, |f| f.view.rows.len());
        let on_row = i < rows;
        if right {
            if let Some(f) = self.files_mut(id) {
                if on_row && !f.view.sel.is_selected(i) {
                    f.view.sel.only(i);
                } else if !on_row {
                    f.view.sel.clear();
                }
            }
            let Some(ctx) = self.files_menu_ctx(id) else {
                return;
            };
            let items = fileman::context_menu(ctx);
            let h = menu_height(items.len());
            let (mx, my) = (
                px.min(self.sw - MENU_W_FILES - 4).max(0),
                py.min(self.sh - h - 4).max(0),
            );
            if let Some(f) = self.files_mut(id) {
                f.menu = Some(CtxMenu {
                    x: mx,
                    y: my,
                    items,
                });
            }
            return;
        }
        if !on_row {
            if let Some(f) = self.files_mut(id) {
                f.view.sel.clear();
            }
            return;
        }
        let double = !ctrl && !shift && self.clicks.press(crate::interrupts::ticks(), px, py, id);
        if let Some(f) = self.files_mut(id) {
            f.view.sel.click(i, ctrl, shift);
        }
        if double {
            self.files_activate(id, i);
        }
    }

    /// Mouse wheel over a file manager: three rows per notch (`notches` > 0 scrolls down).
    pub(crate) fn files_wheel(&mut self, id: WindowId, notches: i32) {
        let vis = self.files_visible(id);
        if let Some(f) = self.files_mut(id) {
            f.view.scroll_by(3 * notches as isize, vis);
        }
    }

    /// Ctrl+letter shortcuts of the file manager. `true` when consumed.
    pub(crate) fn files_ctrl(&mut self, id: WindowId, ch: u8) -> bool {
        if self.files_mut(id).is_some_and(|f| f.modal()) {
            return false;
        }
        let cmd = match ch.to_ascii_lowercase() {
            b'a' => Cmd::SelectAll,
            b'c' => Cmd::Copy,
            b'x' => Cmd::Cut,
            b'v' => Cmd::Paste,
            b'r' => Cmd::Refresh,
            _ => return false,
        };
        self.files_cmd(id, cmd);
        true
    }

    /// Keys of the file manager (after the global shortcuts).
    pub(crate) fn files_key(&mut self, id: WindowId, key: Key) {
        let shift = self.keymap.shift();
        let ctrl = self.keymap.ctrl();
        let vis = self.files_visible(id);
        let Some(f) = self.files_mut(id) else {
            return;
        };
        // 1. context menu: any key closes it.
        if f.menu.take().is_some() {
            return;
        }
        // 2. properties panel.
        if f.props.is_some() {
            f.props = None;
            return;
        }
        // 3. a question.
        if f.confirm.is_some() {
            match key {
                Key::Enter => self.files_confirmed(id),
                Key::Esc => {
                    f.confirm = None;
                    f.say("Cancelado", false);
                }
                _ => {}
            }
            return;
        }
        // 4. the inline name field.
        if let Some(edit) = f.input.as_mut() {
            match key {
                Key::Esc => f.input = None,
                Key::Enter => self.files_commit_input(id),
                Key::Backspace => edit.input.backspace(),
                Key::Delete => edit.input.delete(),
                Key::Left => edit.input.left(),
                Key::Right => edit.input.right(),
                Key::Home => edit.input.home(),
                Key::End => edit.input.end(),
                Key::Char(b) => edit.input.insert(b),
                _ => {}
            }
            return;
        }
        // 5. a copy in progress: Esc cancels it.
        if f.job.is_some() {
            if key == Key::Esc {
                self.files_cancel_job(id);
            }
            return;
        }
        // 6. normal keys.
        let in_trash = f.view.in_trash();
        let in_apps = f.view.in_apps();
        match key {
            Key::Esc => {
                if f.view.sel.count() > 0 {
                    f.view.sel.clear();
                } else {
                    self.request_close(id);
                }
            }
            Key::Up if ctrl => self.files_history(id, 2),
            Key::Left if ctrl => self.files_history(id, 0),
            Key::Right if ctrl => self.files_history(id, 1),
            Key::Up => {
                f.view.sel.move_cursor(-1, shift);
                f.view.ensure_visible(vis);
            }
            Key::Down => {
                f.view.sel.move_cursor(1, shift);
                f.view.ensure_visible(vis);
            }
            Key::Home => {
                f.view.sel.move_cursor(-(f.view.rows.len() as isize), shift);
                f.view.ensure_visible(vis);
            }
            Key::End => {
                f.view.sel.move_cursor(f.view.rows.len() as isize, shift);
                f.view.ensure_visible(vis);
            }
            Key::Left | Key::Backspace => self.files_history(id, 2),
            Key::Enter | Key::Right => {
                let c = f.view.sel.cursor();
                if !f.view.rows.is_empty() {
                    self.files_activate(id, c);
                }
            }
            Key::Tab => {
                let target: &[u8] = if in_trash || in_apps {
                    b"/"
                } else {
                    TRASH_PATH
                };
                self.files_go(id, target);
            }
            // The Apps place: `I` installs the bundled package, `Del` removes the app.
            Key::Char(b'i') | Key::Char(b'I') if in_apps => {
                let c = f.view.sel.cursor();
                self.files_app_key(id, c, AppKey::Install);
            }
            Key::Char(b'a') | Key::Char(b'A') if !in_trash && !in_apps => {
                self.files_go(id, APPS_PATH);
            }
            Key::Delete => {
                let cmd = if shift || in_trash {
                    Cmd::DeletePermanent
                } else {
                    Cmd::Delete
                };
                self.files_cmd(id, cmd);
            }
            Key::Char(b'n') | Key::Char(b'N') if !in_trash && !in_apps => {
                self.files_cmd(id, Cmd::NewFolder)
            }
            Key::Char(b'f') | Key::Char(b'F') if !in_trash && !in_apps => {
                self.files_cmd(id, Cmd::NewFile)
            }
            _ => {}
        }
    }

    /// F2 / F5 / PageUp / PageDown in a file manager. `true` when consumed.
    pub(crate) fn files_special(&mut self, id: WindowId, sp: Special) -> bool {
        let vis = self.files_visible(id);
        let shift = self.keymap.shift();
        if self
            .files_mut(id)
            .is_none_or(|f| f.modal() || f.menu.is_some())
        {
            return false;
        }
        match sp {
            Special::F2 => self.files_cmd(id, Cmd::Rename),
            Special::F5 => self.files_cmd(id, Cmd::Refresh),
            Special::PageUp | Special::PageDown => {
                let d = vis.saturating_sub(1).max(1) as isize;
                if let Some(f) = self.files_mut(id) {
                    f.view
                        .sel
                        .move_cursor(if sp == Special::PageUp { -d } else { d }, shift);
                    f.view.ensure_visible(vis);
                }
            }
            _ => return false,
        }
        true
    }

    /// Run a file-manager command on window `id`'s selection.
    pub(crate) fn files_cmd(&mut self, id: WindowId, cmd: Cmd) {
        let vis = self.files_visible(id);
        let Some(f) = self.files_mut(id) else {
            return;
        };
        f.menu = None;
        if f.view.in_apps() {
            let c = f.view.sel.cursor();
            return match cmd {
                Cmd::Open => self.files_app_key(id, c, AppKey::Enter),
                Cmd::InstallApp => self.files_app_key(id, c, AppKey::Install),
                // Delete and Shift+Delete both mean "remove the app".
                Cmd::RemoveApp | Cmd::Delete | Cmd::DeletePermanent => {
                    self.files_app_key(id, c, AppKey::Remove)
                }
                Cmd::Properties => self.files_app_properties(id),
                Cmd::SelectAll => f.view.sel.select_all(),
                Cmd::Refresh => {
                    self.files_refresh(id);
                    self.files_note(id, "Atualizado", false);
                }
                _ => {}
            };
        }
        let in_trash = f.view.in_trash();
        let cwd = f.view.cwd.clone();
        let paths = f.view.selected_paths();
        let ids: Vec<Vec<u8>> = f
            .view
            .selected_rows()
            .iter()
            .map(|r| r.id.clone())
            .collect();
        let first_name = f.view.selected_rows().first().map(|r| r.name.clone());
        match cmd {
            Cmd::Open => {
                let c = f.view.sel.cursor();
                self.files_activate(id, c);
            }
            Cmd::SelectAll => f.view.sel.select_all(),
            Cmd::Refresh => {
                self.files_refresh(id);
                self.files_note(id, "Atualizado", false);
            }
            Cmd::NewFile | Cmd::NewFolder => {
                if in_trash {
                    return;
                }
                let (base, purpose): (&[u8], EditPurpose) = if cmd == Cmd::NewFile {
                    (b"Novo arquivo.txt", EditPurpose::NewFile)
                } else {
                    (b"Nova pasta", EditPurpose::NewFolder)
                };
                let name = vfs::unique_name_in(&cwd, base);
                f.input = Some(NameEdit {
                    input: fileman::TextInput::new(&name, vfs::MAX_NAME),
                    purpose,
                });
            }
            Cmd::Rename => {
                if in_trash || paths.len() != 1 {
                    return;
                }
                let name = first_name.unwrap_or_default();
                f.input = Some(NameEdit {
                    input: fileman::TextInput::new(&name, vfs::MAX_NAME),
                    purpose: EditPurpose::Rename(paths[0].clone()),
                });
            }
            Cmd::Copy | Cmd::Cut => {
                if paths.is_empty() {
                    return;
                }
                let n = paths.len();
                let cut = cmd == Cmd::Cut;
                f.say(
                    &alloc::format!("{} {}", n, if cut { "recortado(s)" } else { "copiado(s)" }),
                    false,
                );
                self.pathclip.set(paths, cut);
            }
            Cmd::Paste => self.files_paste(id),
            Cmd::Delete => {
                if in_trash {
                    return self.files_cmd(id, Cmd::DeletePermanent);
                }
                if paths.is_empty() {
                    return;
                }
                let mut done = 0;
                let mut err = None;
                for p in &paths {
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
                    None => self.files_note(id, &alloc::format!("{done} na lixeira"), false),
                    Some(e) => self.files_note(id, e.message(), true),
                }
            }
            Cmd::DeletePermanent => {
                if in_trash {
                    if !ids.is_empty() {
                        f.confirm = Some(Confirm::PurgeTrash(ids));
                    }
                } else if !paths.is_empty() {
                    f.confirm = Some(Confirm::Purge(paths));
                }
            }
            Cmd::EmptyTrash => f.confirm = Some(Confirm::EmptyTrash),
            Cmd::Restore => {
                if !in_trash || ids.is_empty() {
                    return;
                }
                let mut done = 0;
                let mut err = None;
                for t in &ids {
                    match vfs::restore(t) {
                        Ok(_) => done += 1,
                        Err(e) => err = Some(e),
                    }
                }
                self.fs_changed();
                match err {
                    None => self.files_note(id, &alloc::format!("{done} restaurado(s)"), false),
                    Some(e) => self.files_note(id, e.message(), true),
                }
            }
            Cmd::Properties => self.files_properties(id, in_trash, &cwd, &paths),
            // The Apps place handles these above; they do not exist on a volume path.
            Cmd::InstallApp | Cmd::RemoveApp => {}
            Cmd::SetWallpaper => {
                if let Some(p) = paths.first() {
                    match self.set_wallpaper_path(p) {
                        Some(m) => self.files_note(id, &m, true),
                        None => self.files_note(id, "Papel de parede aplicado", false),
                    }
                }
            }
        }
        let _ = vis;
    }

    /// Ctrl+V: a cut moves (instant); a copy becomes a job that runs in steps.
    fn files_paste(&mut self, id: WindowId) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        if f.view.in_trash() {
            f.say("Nao e possivel colar na lixeira", true);
            return;
        }
        if f.job.is_some() {
            return;
        }
        let dest = f.view.cwd.clone();
        if self.pathclip.is_empty() {
            self.files_note(id, "Nada para colar", true);
            return;
        }
        let sources: Vec<Vec<u8>> = self.pathclip.paths().to_vec();
        if self.pathclip.is_cut() {
            let rep = vfs::move_to(&sources, &dest);
            let n = rep.moved.len();
            match rep.error {
                None => {
                    self.pathclip.after_paste();
                    self.files_note(id, &alloc::format!("{n} movido(s)"), false);
                }
                Some(e) => self.files_note(id, e.message(), true),
            }
            self.fs_changed();
            self.files_select_paths(id, &rep.moved);
            return;
        }
        match vfs::copy_plan(&sources, &dest) {
            Ok(job) => {
                if let Some(f) = self.files_mut(id) {
                    f.job = Some(Job {
                        copy: job,
                        label: "Copiando",
                    });
                    f.msg = None;
                }
            }
            Err(e) => self.files_note(id, e.message(), true),
        }
    }

    /// Select, in window `id`, the items of `paths` that live in its current folder.
    fn files_select_paths(&mut self, id: WindowId, paths: &[Vec<u8>]) {
        let vis = self.files_visible(id);
        if let Some(f) = self.files_mut(id) {
            let names: Vec<Vec<u8>> = paths
                .iter()
                .filter(|p| vfs::parent(p) == f.view.cwd)
                .map(|p| vfs::base_name(p).to_vec())
                .collect();
            f.view.select_names(&names, vis);
        }
    }

    /// Abort window `id`'s copy (Esc): the half-written file goes away.
    pub(crate) fn files_cancel_job(&mut self, id: WindowId) {
        if let Some(f) = self.files_mut(id)
            && let Some(mut job) = f.job.take()
        {
            vfs::copy_abort(&mut job.copy);
            f.say("Copia cancelada", false);
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
                    Some((alloc::format!("Copia concluida ({n} arquivo(s))"), false))
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

    /// Enter in the inline name field: create or rename.
    fn files_commit_input(&mut self, id: WindowId) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        let Some(edit) = f.input.take() else {
            return;
        };
        let dir = f.view.cwd.clone();
        let name = edit.input.text().to_vec();
        let r = match &edit.purpose {
            EditPurpose::NewFile => vfs::new_file(&dir, &name),
            EditPurpose::NewFolder => vfs::new_folder(&dir, &name),
            EditPurpose::Rename(p) => vfs::rename(p, &name),
        };
        match r {
            Ok(path) => {
                self.fs_changed();
                let vis = self.files_visible(id);
                if let Some(f) = self.files_mut(id) {
                    f.view.select_name(vfs::base_name(&path), vis);
                    f.msg = None;
                }
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
    fn files_confirmed(&mut self, id: WindowId) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        let Some(q) = f.confirm.take() else {
            return;
        };
        let mut done = 0;
        let mut err = None;
        match q {
            Confirm::Purge(paths) => {
                for p in &paths {
                    match vfs::purge(p) {
                        Ok(()) => done += 1,
                        Err(e) => {
                            err = Some(e);
                            break;
                        }
                    }
                }
            }
            Confirm::PurgeTrash(ids) => {
                for t in &ids {
                    match vfs::trash_purge(t) {
                        Ok(()) => done += 1,
                        Err(e) => {
                            err = Some(e);
                            break;
                        }
                    }
                }
            }
            Confirm::EmptyTrash => match vfs::empty_trash() {
                Ok(()) => done = 1,
                Err(e) => err = Some(e),
            },
        }
        self.fs_changed();
        match err {
            None => self.files_note(id, "Excluido", false),
            Some(e) => self.files_note(id, e.message(), true),
        }
        let _ = done;
    }

    /// Build the properties panel of the selection (or of the folder).
    fn files_properties(&mut self, id: WindowId, in_trash: bool, cwd: &[u8], paths: &[Vec<u8>]) {
        let mut lines: Vec<String> = Vec::new();
        let free = vfs::statfs();
        let show = |b: &[u8]| String::from_utf8_lossy(&fileman::display_ascii(b)).into_owned();
        if in_trash {
            lines.push(String::from("Lixeira"));
            if let Some(f) = self.files_mut(id) {
                lines.push(alloc::format!("{} itens", f.view.rows.len()));
            }
        } else if paths.len() == 1 {
            let p = &paths[0];
            lines.push(alloc::format!("Nome: {}", show(vfs::base_name(p))));
            lines.push(alloc::format!("Caminho: {}", show(p)));
            match vfs::stat(p) {
                Ok(info) => {
                    if info.kind == vfs::EntryKind::Dir {
                        let t = vfs::with_backend(|b| osjeff_core::vfs::tree_size(b, p));
                        lines.push(String::from("Tipo: pasta"));
                        if let Ok(Ok(t)) = t {
                            lines.push(alloc::format!(
                                "Conteudo: {} arquivos, {} pastas",
                                t.files,
                                t.dirs.saturating_sub(1)
                            ));
                            lines
                                .push(alloc::format!("Tamanho: {}", fileman::format_size(t.bytes)));
                        }
                    } else {
                        lines.push(String::from("Tipo: arquivo"));
                        lines.push(alloc::format!(
                            "Tamanho: {} ({} bytes)",
                            fileman::format_size(info.size),
                            info.size
                        ));
                    }
                    lines.push(alloc::format!("Criado: {}", local_time(info.ctime)));
                    lines.push(alloc::format!("Modificado: {}", local_time(info.mtime)));
                    if info.kind == vfs::EntryKind::File
                        && fileman::classify(vfs::base_name(p)) == FileClass::Wasm
                    {
                        lines.extend(self.wasm_property_lines(p));
                    }
                }
                Err(e) => lines.push(String::from(e.message())),
            }
        } else if paths.len() > 1 {
            lines.push(alloc::format!("{} itens selecionados", paths.len()));
            let mut bytes = 0u64;
            for p in paths {
                if let Ok(Ok(t)) = vfs::with_backend(|b| osjeff_core::vfs::tree_size(b, p)) {
                    bytes += t.bytes;
                }
            }
            lines.push(alloc::format!(
                "Tamanho total: {}",
                fileman::format_size(bytes)
            ));
        } else {
            lines.push(alloc::format!("Pasta: {}", show(cwd)));
            if let Some(f) = self.files_mut(id) {
                lines.push(alloc::format!("{} itens", f.view.rows.len()));
            }
        }
        lines.push(alloc::format!(
            "Livre: {} de {}",
            fileman::format_size(free.free),
            fileman::format_size(free.total)
        ));
        if vfs::volume() == vfs::Volume::Memory {
            lines.push(String::from("Volume: memoria (nao persiste)"));
        }
        if let Some(f) = self.files_mut(id) {
            f.props = Some(lines);
        }
    }
}

impl Desktop {
    /// Properties lines of a `.wasm` file: whether it is a valid app package and, if
    /// so, its manifest (permissions and limits) and whether it is installed.
    fn wasm_property_lines(&self, path: &[u8]) -> Vec<String> {
        let bytes = match vfs::read_range(path, 0, osjeff_core::appinstall::MAX_PACKAGE_BYTES + 1) {
            Ok(b) => b,
            Err(e) => return alloc::vec![String::from(e.message())],
        };
        match osjeff_core::appinstall::check(&bytes) {
            Ok(m) => {
                let mut v = alloc::vec![String::from("Pacote de app valido")];
                v.extend(fapps::manifest_lines(&m));
                let state = if self.apps.iter().any(|a| a.id == m.id) {
                    "Estado: instalado"
                } else {
                    "Estado: nao instalado (Enter instala e abre)"
                };
                v.push(String::from(state));
                v
            }
            Err(e) => alloc::vec![alloc::format!("Pacote invalido: {e}")],
        }
    }

    /// Properties of the selected app in the Apps place: manifest, state and package.
    fn files_app_properties(&mut self, id: WindowId) {
        let Some(row) = self
            .files_mut(id)
            .and_then(|f| f.view.selected_rows().first().map(|r| (*r).clone()))
        else {
            return;
        };
        let app_id = String::from_utf8_lossy(&row.id).into_owned();
        let mut lines = match self.app_manifest(&app_id) {
            Some(m) => fapps::manifest_lines(&m),
            None => alloc::vec![String::from("Manifesto indisponivel")],
        };
        lines.push(alloc::format!(
            "Estado: {}   Pacote: {}",
            fapps::status_label(row.installed),
            fileman::format_size(row.size)
        ));
        if row.installed {
            lines.push(alloc::format!("Arquivo: /apps/{app_id}.wasm"));
        } else {
            lines.push(String::from("Pacote embutido no sistema (I instala)"));
        }
        if let Some(f) = self.files_mut(id) {
            f.props = Some(lines);
        }
    }
}

/// Width of the file manager's context menu.
pub(crate) const MENU_W_FILES: i32 = 230;
/// Height of one context-menu row.
pub(crate) const MENU_ROW_H: i32 = 26;

/// Total height of a context menu with `n` rows.
pub(crate) fn menu_height(n: usize) -> i32 {
    8 + n as i32 * MENU_ROW_H
}
