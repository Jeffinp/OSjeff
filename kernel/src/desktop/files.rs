//! `Desktop` methods: files. Everything that reaches the filesystem from the
//! desktop goes through [`vfs`](super::vfs): the terminal commands, the editor's
//! load and save, and the file manager's commands.
//!
//! The file manager's state and decisions live in `osjeff_core::fileman` (tested
//! on the host); this module wires them to the VFS, the window table and the
//! clipboard of paths, and runs long copies in steps from `animate`.

use super::*;
use osjeff_core::fileman::{self, Activation, Cmd, FileClass, MenuCtx, TRASH_PATH};
use osjeff_core::vfs::VfsError;

/// Bytes copied per frame by a running copy job.
const JOB_CHUNK: usize = 128 * 1024;
/// How much of a file the editor can ever show (its grid is 44 x 18 cells).
const EDITOR_READ: usize = 64 * 1024;

/// Serialize an editor buffer (lines joined by `\n`).
fn serialize_editor(editor: &Editor) -> Vec<u8> {
    let mut out = Vec::new();
    let rows = editor.rows();
    for i in 0..rows {
        out.extend_from_slice(editor.line(i));
        if i + 1 < rows {
            out.push(b'\n');
        }
    }
    out
}

/// A top-level path from a terminal file name.
fn root_path(f: FileName) -> Vec<u8> {
    vfs::join(b"/", f.as_bytes())
}

/// A Unix time as local `dd/mm/aaaa hh:mm`.
pub(crate) fn local_time(t: u64) -> String {
    fileman::format_datetime(t, crate::rtc::TZ_OFFSET_HOURS * 3600)
}

impl Desktop {
    // ---- terminal and editor ----

    /// Print `text` into terminal window `tid`, or — when `None` (the action
    /// came from an editor or the file manager) — into the most recently used
    /// terminal, if there is one.
    pub(crate) fn say(&mut self, tid: Option<WindowId>, text: &[u8]) {
        let target = tid.or_else(|| self.mru_of_kind(Kind::Terminal));
        if let Some(t) = target.and_then(|id| self.term_mut(id)) {
            t.println(text);
        }
    }

    /// Ctrl+S: save the focused editor's buffer to the file it was opened from.
    pub(crate) fn save_editor_file(&mut self) {
        let Some(top) = self.focused() else {
            return;
        };
        let Some(path) = self.editor_mut(top).map(|e| e.path.clone()) else {
            return;
        };
        self.fs_save_in(top, path, None);
    }

    /// Terminal `save <name>`: saves the most recently used editor's buffer as a
    /// top-level file (the shell has no current directory).
    pub(crate) fn fs_save(&mut self, tid: WindowId, f: FileName) {
        match self.mru_of_kind(Kind::Editor) {
            Some(eid) => self.fs_save_in(eid, root_path(f), Some(tid)),
            None => self.say(Some(tid), b"no editor open"),
        }
    }

    /// Write editor window `eid`'s buffer to `path`.
    pub(crate) fn fs_save_in(&mut self, eid: WindowId, path: Vec<u8>, tid: Option<WindowId>) {
        let data;
        {
            let Some(e) = self.editor_mut(eid) else {
                return;
            };
            // The file did not fit the editor grid when it was opened, so the buffer
            // holds only part of it: writing it back would silently destroy the rest.
            if e.editor.is_lossy() {
                self.say(tid, b"not saved: file is larger than the editor window");
                crate::klog!(Warn, "editor: refusing to save a truncated buffer");
                return;
            }
            data = serialize_editor(&e.editor);
        }
        match vfs::write_file(&path, &data) {
            Ok(()) => {
                if let Some(e) = self.editor_mut(eid) {
                    e.editor.mark_clean();
                    e.path = path.clone();
                }
                self.print_named(tid, b"Saved ", vfs::base_name(&path));
                self.fs_changed();
            }
            Err(e) => self.print_fs_err(tid, e),
        }
    }

    /// Terminal `load <name>`: a top-level file.
    pub(crate) fn fs_load(&mut self, tid: WindowId, f: FileName) {
        match self.fs_load_path(root_path(f)) {
            Ok(_) => self.print_named(Some(tid), b"Loaded ", f.as_bytes()),
            Err(VfsError::NotFound) => self.say(Some(tid), b"file not found"),
            Err(e) => self.print_fs_err(Some(tid), e),
        }
    }

    /// Open the file at `path` in an editor window: the one that already shows it,
    /// else a new one. Returns a warning for the caller to show when the editor
    /// can only hold part of the file (the buffer is then read-only in effect:
    /// saving is refused).
    pub(crate) fn fs_load_path(&mut self, path: Vec<u8>) -> Result<Option<&'static str>, VfsError> {
        let info = vfs::stat(&path)?;
        if info.kind == vfs::EntryKind::Dir {
            return Err(VfsError::IsDir);
        }
        let data = vfs::read_range(&path, 0, EDITOR_READ)?;
        // Already open (and possibly edited): just bring it forward.
        let open = self
            .wm
            .windows()
            .iter()
            .filter(|w| !w.is_closing())
            .find_map(|w| match &w.app.app {
                App::Editor(e) if e.path == path => Some(w.id),
                _ => None,
            });
        if let Some(id) = open {
            self.wm.activate(id);
            return Ok(None);
        }
        let id = self.open_new(Kind::Editor).ok_or(VfsError::Busy)?;
        let mut warn = None;
        if let Some(e) = self.editor_mut(id) {
            e.editor.set_text(&data);
            e.path = path;
            if e.editor.is_lossy() || info.size > data.len() as u64 {
                warn = Some("Arquivo truncado no editor: somente leitura");
            }
        }
        Ok(warn)
    }

    pub(crate) fn fs_cat(&mut self, tid: WindowId, f: FileName) {
        let data = match vfs::read_range(&root_path(f), 0, 16 * 1024) {
            Ok(d) => d,
            Err(VfsError::NotFound) => {
                self.say(Some(tid), b"file not found");
                return;
            }
            Err(e) => {
                self.print_fs_err(Some(tid), e);
                return;
            }
        };
        if data.is_empty() {
            self.say(Some(tid), b"(empty)");
            return;
        }
        let mut start = 0;
        for i in 0..data.len() {
            if data[i] == b'\n' {
                self.say(Some(tid), &data[start..i]);
                start = i + 1;
            }
        }
        if start < data.len() {
            self.say(Some(tid), &data[start..]);
        }
    }

    /// Terminal `remove <name>`: the file goes to the trash (restorable from the
    /// file manager).
    pub(crate) fn fs_remove(&mut self, tid: WindowId, f: FileName) {
        match vfs::remove(&root_path(f)) {
            Ok(()) => {
                self.print_named(Some(tid), b"Removed ", f.as_bytes());
                self.fs_changed();
            }
            Err(e) => self.print_fs_err(Some(tid), e),
        }
    }

    pub(crate) fn fs_list(&mut self, tid: WindowId) {
        let mut rows = match vfs::list(b"/") {
            Ok(r) => r,
            Err(e) => {
                self.print_fs_err(Some(tid), e);
                return;
            }
        };
        if rows.is_empty() {
            self.say(Some(tid), b"(no files)");
            return;
        }
        rows.sort_by(|a, b| fileman::natural_cmp(&a.name, &b.name));
        for r in rows {
            let mut line = [b' '; 28];
            let shown = fileman::display_ascii(&r.name);
            let n = shown.len().min(17);
            line[..n].copy_from_slice(&shown[..n]);
            if r.kind == vfs::EntryKind::Dir {
                line[n.min(17)] = b'/';
            } else {
                write_uint(&mut line, 18, 6, r.size.min(999_999) as u32);
            }
            self.say(Some(tid), &line);
        }
    }

    pub(crate) fn print_named(&mut self, tid: Option<WindowId>, prefix: &[u8], name: &[u8]) {
        let mut line = [b' '; 40];
        let mut p = 0;
        for &b in prefix.iter().chain(fileman::display_ascii(name).iter()) {
            if p < line.len() {
                line[p] = b;
                p += 1;
            }
        }
        self.say(tid, &line[..p]);
    }

    pub(crate) fn print_fs_err(&mut self, tid: Option<WindowId>, e: VfsError) {
        let mut line = [b' '; 40];
        let msg = e.message().as_bytes();
        let n = msg.len().min(line.len());
        line[..n].copy_from_slice(&msg[..n]);
        self.say(tid, &line[..n]);
    }

    // ---- file manager ----

    /// Reload window `id`'s folder from the filesystem.
    pub(crate) fn files_refresh(&mut self, id: WindowId) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        match vfs::with_backend(|b| f.view.refresh(b)) {
            Ok(Ok(())) => {}
            Ok(Err(e)) | Err(e) => f.say(e.message(), true),
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
        if let Activation::Open(path, class) = act {
            self.open_path(id, &path, class);
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
                let target: &[u8] = if in_trash { b"/" } else { TRASH_PATH };
                self.files_go(id, target);
            }
            Key::Delete => {
                let cmd = if shift || in_trash {
                    Cmd::DeletePermanent
                } else {
                    Cmd::Delete
                };
                self.files_cmd(id, cmd);
            }
            Key::Char(b'n') | Key::Char(b'N') if !in_trash => self.files_cmd(id, Cmd::NewFolder),
            Key::Char(b'f') | Key::Char(b'F') if !in_trash => self.files_cmd(id, Cmd::NewFile),
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

/// Width of the file manager's context menu.
pub(crate) const MENU_W_FILES: i32 = 230;
/// Height of one context-menu row.
pub(crate) const MENU_ROW_H: i32 = 26;

/// Total height of a context menu with `n` rows.
pub(crate) fn menu_height(n: usize) -> i32 {
    8 + n as i32 * MENU_ROW_H
}
