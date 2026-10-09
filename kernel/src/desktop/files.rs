//! `Desktop` methods: files. Everything that reaches the filesystem from the
//! desktop goes through [`vfs`](super::vfs): the terminal commands (`shellhost`), the
//! editor's load and save (`edit`), and the file manager's commands (here).
//!
//! The file manager's state and decisions live in `osjeff_core::fileman` (tested
//! on the host: rows, sort, search, selection, the geometry and hit testing of
//! `fileman::ui`, the drag-and-drop rules); this module wires them to the VFS, the
//! window table, the shell's menus and the clipboard of paths, and runs long copies
//! in steps from `animate`.

use super::shell::{Cmd as ShellCmd, Entry, MenuOrigin};
use super::*;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicU64, Ordering};
use osjeff_core::fileman::apps::{self as fapps, AppAction, AppItem, AppKey};
use osjeff_core::fileman::ui::{
    self, Dir, DropOp, DropTarget, HitCtx, Layout, ViewMode, crumb_layout,
};
use osjeff_core::fileman::{
    self, APPS_PATH, Activation, Cmd, Crumb, FileClass, MenuCtx, Place, SortKey, TRASH_PATH,
};

/// Bytes copied per frame by a running copy job.
const JOB_CHUNK: usize = 128 * 1024;

/// A Unix time as local `dd/mm/aaaa hh:mm`.
pub(crate) fn local_time(t: u64) -> String {
    fileman::format_datetime(t, crate::rtc::tz_minutes() * 60)
}

static NOW_UNIX: AtomicU64 = AtomicU64::new(0);
static NOW_AT: AtomicU64 = AtomicU64::new(0);

/// The current Unix time, read from the clock chip at most every few seconds (a list shows
/// a date per row; each read is a handful of port accesses).
fn now_unix_cached() -> u64 {
    let t = appui::ticks();
    let at = NOW_AT.load(Ordering::Relaxed);
    if NOW_UNIX.load(Ordering::Relaxed) == 0 || t.saturating_sub(at) > 1250 {
        NOW_UNIX.store(crate::rtc::now_unix(), Ordering::Relaxed);
        NOW_AT.store(t, Ordering::Relaxed);
    }
    NOW_UNIX.load(Ordering::Relaxed)
}

/// A time for the list: `Hoje, 14:32`, `Ontem, 09:10`, else the date.
pub(crate) fn modified_label(t: u64) -> String {
    ui::format_modified(t, now_unix_cached(), crate::rtc::tz_minutes() * 60)
}

/// `1 item` / `3 itens`.
fn items(n: usize) -> String {
    if n == 1 {
        String::from("1 item")
    } else {
        alloc::format!("{n} itens")
    }
}

/// The crumbs of `cwd` with their display labels and measured widths (the last one is drawn
/// Medium, the others Regular).
pub(crate) fn crumbs_of(cwd: &[u8]) -> (Vec<Crumb>, Vec<String>, Vec<i32>) {
    let crumbs = fileman::breadcrumbs(cwd);
    let n = crumbs.len();
    let labels: Vec<String> = crumbs
        .iter()
        .map(|c| {
            if c.path == b"/home" {
                String::from("Início")
            } else {
                String::from_utf8_lossy(&c.label).into_owned()
            }
        })
        .collect();
    let widths: Vec<i32> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let w = if i + 1 == n {
                crate::text::Weight::Medium
            } else {
                crate::text::Weight::Regular
            };
            // The disk crumb carries a glyph in front.
            crate::text::measure(l, crate::text::BODY, w) + if i == 0 { 20 } else { 0 }
        })
        .collect();
    (crumbs, labels, widths)
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
    fn files_note(&mut self, id: WindowId, msg: &str, error: bool) {
        if let Some(f) = self.files_mut(id) {
            f.say(msg, error);
        }
    }

    /// Move the keyboard cursor to row `i` and scroll to it.
    fn files_reveal(&mut self, id: WindowId) {
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
    fn files_app_key(&mut self, id: WindowId, i: usize, key: AppKey) {
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
                        Some((String::from("Formato não suportado"), true))
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

    /// Open the context menu of window `id` at `(px, py)`.
    fn files_context_menu(&mut self, id: WindowId, px: i32, py: i32) {
        let Some(ctx) = self.files_menu_ctx(id) else {
            return;
        };
        let mut entries: Vec<Entry> = Vec::new();
        let mut last_group = None;
        for (cmd, label) in fileman::context_menu(ctx) {
            let g = cmd.group();
            if last_group.is_some_and(|l| l != g) {
                entries.push(Entry::sep());
            }
            last_group = Some(g);
            entries.push(Entry::item(label, cmd.shortcut(), ShellCmd::Files(cmd)));
        }
        self.open_menu(MenuOrigin::Context, entries, (px, py));
    }

    /// The sort menu, under the toolbar button.
    fn files_sort_menu(&mut self, id: WindowId, at: (i32, i32)) {
        let Some(App::Files(f)) = self.wm.get(id).map(|w| &w.app.app) else {
            return;
        };
        let sort = f.view.sort;
        let apps = f.view.in_apps();
        let trash = f.view.in_trash();
        let mut entries = Vec::new();
        for (key, label) in [
            (SortKey::Name, "Nome"),
            (SortKey::Size, "Tamanho"),
            (
                SortKey::Modified,
                if trash {
                    "Data da exclusão"
                } else if apps {
                    "Estado"
                } else {
                    "Última modificação"
                },
            ),
        ] {
            let mut e = Entry::item(label, "", ShellCmd::Files(Cmd::SortBy(key)));
            e.checked = sort.key == key;
            entries.push(e);
        }
        entries.push(Entry::sep());
        for (asc, label) in [(true, "Crescente"), (false, "Decrescente")] {
            let mut e = Entry::item(label, "", ShellCmd::Files(Cmd::SortDir(asc)));
            e.checked = sort.asc == asc;
            entries.push(e);
        }
        self.open_menu(MenuOrigin::Context, entries, at);
    }

    /// A menu entry of the file manager chosen: run it on the focused window.
    pub(crate) fn files_run_focused(&mut self, cmd: Cmd) {
        if let Some(id) = self.focused()
            && self.kind_of(id) == Some(Kind::Files)
        {
            self.files_cmd(id, cmd);
        }
    }

    // ---- mouse ----

    /// A press in file-manager window `id` at `(px, py)` (screen coordinates).
    /// `right` is the right button. Geometry is `fileman::ui::Layout`, the same the
    /// renderer uses.
    pub(crate) fn files_click(&mut self, id: WindowId, _rect: Rect, px: i32, py: i32, right: bool) {
        let ctrl = self.keymap.ctrl();
        let shift = self.keymap.shift();
        let Some((lay, hit)) = self.files_hit(id, px, py) else {
            return;
        };
        // A sheet takes the click first.
        if self.files_mut(id).is_some_and(|f| f.sheet_open()) {
            self.files_sheet_click(id, &lay, px, py);
            return;
        }
        // Clicking away from the inline name field commits it.
        if self.files_mut(id).is_some_and(|f| f.input.is_some()) {
            self.files_commit_input(id);
        }
        // Clicking away from the search field gives the keyboard back to the list.
        if !matches!(hit, ui::Hit::Search)
            && let Some(f) = self.files_mut(id)
            && f.search.focused
        {
            f.search.focused = false;
        }
        // The scrollbar sits over the right edge of the rows and wins the press there.
        if !right
            && matches!(hit, ui::Hit::Item(_) | ui::Hit::Blank)
            && self.files_scrollbar_press(id, &lay, px, py)
        {
            return;
        }
        let now = appui::ticks();
        match hit {
            ui::Hit::Back => self.files_history(id, 0),
            ui::Hit::Forward => self.files_history(id, 1),
            ui::Hit::Crumb(i) => {
                let target = self
                    .files_mut(id)
                    .map(|f| fileman::breadcrumbs(&f.view.cwd))
                    .and_then(|c| c.get(i).map(|c| c.path.clone()));
                if let Some(p) = target {
                    self.files_go(id, &p);
                }
            }
            ui::Hit::CrumbFold => {
                // Up to the parent of the first crumb that is shown.
                let cwd = self.files_mut(id).map(|f| f.view.cwd.clone());
                if let Some(cwd) = cwd {
                    let crumbs = fileman::breadcrumbs(&cwd);
                    let first = {
                        let (_, _, w) = crumbs_of(&cwd);
                        crumb_layout(lay.path, &w).first
                    };
                    if let Some(c) = crumbs.get(first.saturating_sub(1)) {
                        self.files_go(id, &c.path.clone());
                    }
                }
            }
            ui::Hit::PathBlank | ui::Hit::Dead | ui::Hit::PreviewPane => {}
            ui::Hit::View(m) => self.files_set_view(id, m),
            ui::Hit::SortButton => {
                let at = (lay.sort.x, lay.sort.bottom() + 4);
                self.files_sort_menu(id, at);
            }
            ui::Hit::Search => {
                let on_clear = self.files_mut(id).is_some_and(|f| {
                    !f.search.input.text().is_empty()
                        && lay.search_is_field()
                        && appui::field_clear_rect(lay.search).contains(px, py)
                });
                if on_clear {
                    self.files_set_search(id, b"");
                    if let Some(f) = self.files_mut(id) {
                        f.search.focused = false;
                    }
                } else if let Some(f) = self.files_mut(id) {
                    f.search.focused = true;
                    f.search.last_input = now;
                }
            }
            ui::Hit::PreviewButton => self.files_cmd(id, Cmd::TogglePreview),
            ui::Hit::Place(p) => {
                if right {
                    return;
                }
                self.files_go_place(id, p);
            }
            ui::Hit::Header(k) => {
                if let Some(f) = self.files_mut(id) {
                    f.view.click_header(k);
                }
            }
            ui::Hit::Item(i) => self.files_item_press(id, i, ctrl, shift, right, px, py),
            ui::Hit::Blank => {
                if right {
                    if let Some(f) = self.files_mut(id) {
                        f.view.sel.clear();
                    }
                    self.files_context_menu(id, px, py);
                    return;
                }
                if self.files_scrollbar_press(id, &lay, px, py) {
                    return;
                }
                let Some(f) = self.files_mut(id) else {
                    return;
                };
                let base = if ctrl {
                    f.view.sel.selected()
                } else {
                    Vec::new()
                };
                if !ctrl {
                    f.view.sel.clear();
                }
                let anchor = (px - lay.list.x, py - lay.list.y + f.scroller.pos());
                f.gesture = Gesture::Band {
                    anchor,
                    cur: (px, py),
                    base,
                    additive: ctrl,
                };
                self.drag = Some(Drag {
                    win: id,
                    mode: DragMode::Files,
                });
            }
        }
        self.files_sync_preview(id);
    }

    /// A press on the overlay scrollbar's track: start dragging the thumb. `true` when it
    /// was on the scrollbar.
    fn files_scrollbar_press(&mut self, id: WindowId, lay: &Layout, px: i32, py: i32) -> bool {
        let Some(f) = self.files_mut(id) else {
            return false;
        };
        let n = f.view.rows.len();
        let content = ui::content_height(f.mode, lay.list.w, n);
        if content <= lay.list.h || px < lay.list.right() - 14 {
            return false;
        }
        let (off, len) = osjeff_core::widgets::scroll_thumb(
            lay.list.h,
            content as usize,
            lay.list.h as usize,
            f.scroller.pos().max(0) as usize,
            28,
        );
        let thumb_top = lay.list.y + off;
        let grab = if py >= thumb_top && py < thumb_top + len {
            py - thumb_top
        } else {
            len / 2
        };
        f.gesture = Gesture::Thumb { grab };
        f.scroll_fade.touch(appui::now_ms());
        self.files_thumb_to(id, py);
        self.drag = Some(Drag {
            win: id,
            mode: DragMode::Files,
        });
        true
    }

    /// Scroll so the thumb follows pointer `py`.
    fn files_thumb_to(&mut self, id: WindowId, py: i32) {
        let Some(lay) = self.files_layout(id) else {
            return;
        };
        let Some(f) = self.files_mut(id) else {
            return;
        };
        let Gesture::Thumb { grab } = f.gesture else {
            return;
        };
        let n = f.view.rows.len();
        let content = ui::content_height(f.mode, lay.list.w, n);
        let (_, len) = osjeff_core::widgets::scroll_thumb(
            lay.list.h,
            content as usize,
            lay.list.h as usize,
            0,
            28,
        );
        let span = (lay.list.h - len).max(1) as i64;
        let rel = (py - grab - lay.list.y).clamp(0, span as i32) as i64;
        let max = ui::max_scroll(f.mode, lay.list.w, lay.list.h, n) as i64;
        f.scroller.jump((max * rel / span) as i32);
        f.scroll_fade.touch(appui::now_ms());
    }

    #[allow(clippy::too_many_arguments)]
    fn files_item_press(
        &mut self,
        id: WindowId,
        i: usize,
        ctrl: bool,
        shift: bool,
        right: bool,
        px: i32,
        py: i32,
    ) {
        if right {
            if let Some(f) = self.files_mut(id)
                && !f.view.sel.is_selected(i)
            {
                f.view.sel.only(i);
            }
            self.files_context_menu(id, px, py);
            return;
        }
        let double = !ctrl && !shift && self.clicks.press(crate::interrupts::ticks(), px, py, id);
        if double {
            if let Some(f) = self.files_mut(id) {
                f.gesture = Gesture::None;
                f.view.sel.only(i);
            }
            self.drag = None;
            self.files_activate(id, i);
            return;
        }
        let Some(f) = self.files_mut(id) else {
            return;
        };
        let was_selected = f.view.sel.is_selected(i);
        if !was_selected || ctrl || shift {
            f.view.sel.click(i, ctrl, shift);
        }
        f.gesture = Gesture::Press {
            item: i,
            at: (px, py),
            collapse: was_selected && !ctrl && !shift,
        };
        self.drag = Some(Drag {
            win: id,
            mode: DragMode::Files,
        });
    }

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
                let kind = first.map_or(osjeff_core::appart::FileKind::Generic, |r| {
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
                Ok(p) => DropTarget::Folder(p),
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
    fn files_band_update(&mut self, id: WindowId) {
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
                String::from("Disco")
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
                    None => {
                        self.files_note(id, &alloc::format!("{} na lixeira", items(done)), false)
                    }
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
                        &alloc::format!(
                            "{} {} para {}",
                            items(n),
                            if n == 1 { "movido" } else { "movidos" },
                            name(&dest)
                        ),
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
    fn files_start_copy(&mut self, id: WindowId, sources: &[Vec<u8>], dest: &[u8]) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        if f.job.is_some() {
            f.say("Já existe uma cópia em andamento", true);
            return;
        }
        match vfs::copy_plan(sources, dest) {
            Ok(job) => {
                if let Some(f) = self.files_mut(id) {
                    f.job = Some(Job {
                        copy: job,
                        label: "Copiando",
                        started: crate::interrupts::ticks(),
                    });
                    f.msg = None;
                }
            }
            Err(e) => self.files_note(id, e.message(), true),
        }
    }

    /// The pointer moved over a file manager (no button held): the item under it lights up.
    pub(crate) fn files_hover(&mut self, id: WindowId, px: i32, py: i32) -> bool {
        let hit = self.files_hit(id, px, py).map(|(_, h)| h);
        // The scrollbar wakes up when the pointer is near it.
        let Some(f) = self.files_mut(id) else {
            return false;
        };
        let hit = match hit {
            Some(ui::Hit::Blank | ui::Hit::Dead | ui::Hit::PathBlank | ui::Hit::PreviewPane) => {
                None
            }
            other => other,
        };
        if f.hover == hit {
            return false;
        }
        f.hover = hit;
        f.hover_t = osjeff_core::anim::Tween::at(0.0);
        f.hover_t
            .retarget(1.0, 0.12, osjeff_core::anim::curves::STANDARD);
        true
    }

    /// The pointer left window `id` (or moved to another window).
    pub(crate) fn files_unhover(&mut self, id: WindowId) {
        if let Some(f) = self.files_mut(id)
            && f.hover.is_some()
        {
            f.hover = None;
        }
    }

    /// Mouse wheel over a file manager: three rows per notch (`notches` > 0 scrolls down).
    pub(crate) fn files_wheel(&mut self, id: WindowId, notches: i32) {
        if let Some(f) = self.files_mut(id) {
            f.scroller.scroll_by(notches * ui::WHEEL_STEP);
            f.scroll_fade.touch(appui::now_ms());
        }
    }

    // ---- keys ----

    /// Ctrl+letter shortcuts of the file manager. `true` when consumed.
    pub(crate) fn files_ctrl(&mut self, id: WindowId, ch: u8) -> bool {
        if self
            .files_mut(id)
            .is_some_and(|f| f.sheet_open() || f.input.is_some())
        {
            return false;
        }
        if self.files_mut(id).is_some_and(|f| f.search.focused) && matches!(ch, b'a' | b'A') {
            if let Some(f) = self.files_mut(id) {
                f.search.input.select_all();
            }
            return true;
        }
        let cmd = match ch.to_ascii_lowercase() {
            b'a' => Cmd::SelectAll,
            b'c' => Cmd::Copy,
            b'x' => Cmd::Cut,
            b'v' => Cmd::Paste,
            b'r' => Cmd::Refresh,
            b'i' => Cmd::Properties,
            b'1' => Cmd::SetView(ViewMode::List),
            b'2' => Cmd::SetView(ViewMode::Icons),
            b'f' => {
                if let Some(f) = self.files_mut(id) {
                    f.search.focused = true;
                    f.search.last_input = appui::ticks();
                }
                return true;
            }
            _ => return false,
        };
        self.files_cmd(id, cmd);
        true
    }

    /// Keys of the file manager (after the global shortcuts).
    pub(crate) fn files_key(&mut self, id: WindowId, key: Key) {
        let shift = self.keymap.shift();
        let ctrl = self.keymap.ctrl();
        let Some(lay) = self.files_layout(id) else {
            return;
        };
        let now = appui::ticks();
        let Some(f) = self.files_mut(id) else {
            return;
        };
        // 1. a sheet.
        if f.sheet_open() {
            if f.confirm.is_some() {
                match key {
                    Key::Enter => self.files_confirmed(id),
                    Key::Esc => {
                        f.confirm = None;
                        f.say("Cancelado", false);
                    }
                    _ => {}
                }
            } else if f.props.is_some() {
                f.props = None;
            } else if key == Key::Esc {
                self.files_cancel_job(id);
            }
            return;
        }
        // 2. the inline name field.
        if let Some(edit) = f.input.as_mut() {
            edit.last_input = now;
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
        // 3. the search field.
        if f.search.focused {
            f.search.last_input = now;
            let before = f.search.input.text().to_vec();
            match key {
                Key::Esc => {
                    if f.search.input.text().is_empty() {
                        f.search.focused = false;
                    } else {
                        f.search.input.clear();
                    }
                }
                Key::Enter | Key::Down | Key::Tab => f.search.focused = false,
                Key::Backspace => f.search.input.backspace(),
                Key::Delete => f.search.input.delete(),
                Key::Left => f.search.input.left(),
                Key::Right => f.search.input.right(),
                Key::Home => f.search.input.home(),
                Key::End => f.search.input.end(),
                Key::Char(b) => f.search.input.insert(b),
                _ => {}
            }
            if f.search.input.text() != &before[..] {
                let text = f.search.input.text().to_vec();
                f.view.set_filter(&text);
                f.scroller.jump(0);
                f.view.select_first();
            }
            self.files_sync_preview(id);
            return;
        }
        // 4. normal keys.
        let (mode, vw) = (f.mode, lay.list.w);
        let n = f.view.rows.len();
        let in_trash = f.view.in_trash();
        let in_apps = f.view.in_apps();
        let cur = f.view.sel.cursor();
        let mut moved = false;
        match key {
            Key::Esc => {
                if f.view.sel.count() > 0 {
                    f.view.sel.clear();
                } else if !f.view.filter().is_empty() {
                    f.search.input.clear();
                    f.view.clear_filter();
                } else {
                    self.request_close(id);
                }
            }
            Key::Up if ctrl => self.files_history(id, 2),
            Key::Left if ctrl => self.files_history(id, 0),
            Key::Right if ctrl => self.files_history(id, 1),
            Key::Up | Key::Down | Key::Left | Key::Right
                if mode == ViewMode::Icons || matches!(key, Key::Up | Key::Down) =>
            {
                let dir = match key {
                    Key::Up => Dir::Up,
                    Key::Down => Dir::Down,
                    Key::Left => Dir::Left,
                    _ => Dir::Right,
                };
                let to = ui::step_index(mode, vw, cur, dir, n);
                f.view.sel.move_cursor(to as isize - cur as isize, shift);
                moved = true;
            }
            Key::Home => {
                f.view.sel.move_cursor(-(n as isize), shift);
                moved = true;
            }
            Key::End => {
                f.view.sel.move_cursor(n as isize, shift);
                moved = true;
            }
            Key::Left | Key::Backspace => self.files_history(id, 2),
            Key::Enter | Key::Right => {
                if n > 0 {
                    self.files_activate(id, cur);
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
            // Space shows or hides the preview pane.
            Key::Char(b' ') => self.files_cmd(id, Cmd::TogglePreview),
            // The Apps place: `I` installs the bundled package, `Del` removes the app.
            Key::Char(b'i') | Key::Char(b'I') if in_apps => {
                self.files_app_key(id, cur, AppKey::Install);
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
        if moved {
            self.files_reveal(id);
        }
        self.files_sync_preview(id);
    }

    /// F2 / F5 / PageUp / PageDown in a file manager. `true` when consumed.
    pub(crate) fn files_special(&mut self, id: WindowId, sp: Special) -> bool {
        let shift = self.keymap.shift();
        let Some(lay) = self.files_layout(id) else {
            return false;
        };
        if self
            .files_mut(id)
            .is_none_or(|f| f.sheet_open() || f.input.is_some() || f.search.focused)
        {
            return false;
        }
        match sp {
            Special::F2 => self.files_cmd(id, Cmd::Rename),
            Special::F5 => self.files_cmd(id, Cmd::Refresh),
            Special::PageUp | Special::PageDown => {
                if let Some(f) = self.files_mut(id) {
                    let d = ui::page_items(f.mode, lay.list.w, lay.list.h) as isize;
                    f.view
                        .sel
                        .move_cursor(if sp == Special::PageUp { -d } else { d }, shift);
                }
                self.files_reveal(id);
            }
            _ => return false,
        }
        self.files_sync_preview(id);
        true
    }

    // ---- view state ----

    /// Switch between the list and the icon grid.
    fn files_set_view(&mut self, id: WindowId, m: ViewMode) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        if f.mode == m {
            return;
        }
        f.mode = m;
        f.enter_t = osjeff_core::anim::Tween::at(0.0);
        f.enter_t
            .retarget(1.0, 0.18, osjeff_core::anim::curves::ENTER);
        f.scroller.jump(0);
        f.hover = None;
        self.files_reveal(id);
    }

    /// Replace the search text (and filter the rows).
    fn files_set_search(&mut self, id: WindowId, text: &[u8]) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        f.search.input.set(text);
        f.view.set_filter(text);
        f.scroller.jump(0);
        f.view.select_first();
    }

    // ---- commands ----

    /// Run a file-manager command on window `id`'s selection.
    pub(crate) fn files_cmd(&mut self, id: WindowId, cmd: Cmd) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        if let Cmd::SetView(m) = cmd {
            return self.files_set_view(id, m);
        }
        match cmd {
            Cmd::TogglePreview => {
                f.preview_open = !f.preview_open;
                f.preview = None;
                self.files_sync_preview(id);
                return;
            }
            Cmd::SortBy(k) => {
                if f.view.sort.key == k {
                    f.view.click_header(k);
                } else {
                    f.view.set_sort(k);
                }
                return;
            }
            Cmd::SortDir(asc) => {
                if f.view.sort.asc != asc {
                    let k = f.view.sort.key;
                    f.view.click_header(k);
                }
                return;
            }
            _ => {}
        }
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
                let base: &[u8] = if cmd == Cmd::NewFile {
                    "Novo arquivo.txt".as_bytes()
                } else {
                    "Nova pasta".as_bytes()
                };
                let name = vfs::unique_name_in(&cwd, base);
                let made = if cmd == Cmd::NewFile {
                    vfs::new_file(&cwd, &name)
                } else {
                    vfs::new_folder(&cwd, &name)
                };
                match made {
                    Ok(path) => {
                        // The new item exists at once; its name is being edited.
                        self.fs_changed();
                        if let Some(f) = self.files_mut(id) {
                            // A new item must show: drop a search that would hide it.
                            if !f.view.filter().is_empty() {
                                f.search.input.clear();
                                f.view.clear_filter();
                            }
                            f.view.select_name(vfs::base_name(&path));
                        }
                        self.files_reveal(id);
                        self.files_begin_rename(id, path, true);
                    }
                    Err(e) => self.files_note(id, e.message(), true),
                }
            }
            Cmd::Rename => {
                if in_trash || paths.len() != 1 {
                    return;
                }
                let _ = first_name;
                self.files_begin_rename(id, paths[0].clone(), false);
            }
            Cmd::Copy | Cmd::Cut => {
                if paths.is_empty() {
                    return;
                }
                let n = paths.len();
                let cut = cmd == Cmd::Cut;
                f.say(
                    &alloc::format!(
                        "{} {}",
                        items(n),
                        if n == 1 {
                            if cut { "recortado" } else { "copiado" }
                        } else if cut {
                            "recortados"
                        } else {
                            "copiados"
                        }
                    ),
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
                    None => {
                        self.files_note(id, &alloc::format!("{} na lixeira", items(done)), false)
                    }
                    Some(e) => self.files_note(id, e.message(), true),
                }
            }
            Cmd::DeletePermanent => {
                if in_trash {
                    if !ids.is_empty() {
                        f.confirm = Some(Confirm::PurgeTrash(ids));
                        f.open_sheet();
                    }
                } else if !paths.is_empty() {
                    f.confirm = Some(Confirm::Purge(paths));
                    f.open_sheet();
                }
            }
            Cmd::EmptyTrash => {
                f.confirm = Some(Confirm::EmptyTrash);
                f.open_sheet();
            }
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
                    None => self.files_note(
                        id,
                        &alloc::format!(
                            "{} {}",
                            items(done),
                            if done == 1 {
                                "restaurado"
                            } else {
                                "restaurados"
                            }
                        ),
                        false,
                    ),
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
            Cmd::SortBy(_) | Cmd::SortDir(_) | Cmd::SetView(_) | Cmd::TogglePreview => {}
        }
        self.files_sync_preview(id);
    }

    /// Start editing the name of the item at `path` (the stem selected, or the whole name for
    /// a new item).
    fn files_begin_rename(&mut self, id: WindowId, path: Vec<u8>, whole: bool) {
        let name = vfs::base_name(&path).to_vec();
        if let Some(f) = self.files_mut(id) {
            let mut input = fileman::TextInput::new(&name, vfs::MAX_NAME);
            if whole {
                input.select_all();
            } else {
                input.select_stem();
            }
            f.input = Some(NameEdit {
                input,
                purpose: EditPurpose::Rename(path),
                last_input: appui::ticks(),
            });
        }
    }

    /// Ctrl+V: a cut moves (instant); a copy becomes a job that runs in steps.
    fn files_paste(&mut self, id: WindowId) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        if f.view.in_trash() {
            f.say("Não é possível colar na lixeira", true);
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
                    self.files_note(
                        id,
                        &alloc::format!(
                            "{} {}",
                            items(n),
                            if n == 1 { "movido" } else { "movidos" }
                        ),
                        false,
                    );
                }
                Some(e) => self.files_note(id, e.message(), true),
            }
            self.fs_changed();
            self.files_select_paths(id, &rep.moved);
            return;
        }
        self.files_start_copy(id, &sources, &dest);
    }

    /// Select, in window `id`, the items of `paths` that live in its current folder.
    fn files_select_paths(&mut self, id: WindowId, paths: &[Vec<u8>]) {
        if let Some(f) = self.files_mut(id) {
            let names: Vec<Vec<u8>> = paths
                .iter()
                .filter(|p| vfs::parent(p) == f.view.cwd)
                .map(|p| vfs::base_name(p).to_vec())
                .collect();
            f.view.select_names(&names);
        }
        self.files_reveal(id);
    }

    /// Abort window `id`'s copy (Esc, Cancelar): the half-written file goes away.
    pub(crate) fn files_cancel_job(&mut self, id: WindowId) {
        if let Some(f) = self.files_mut(id)
            && let Some(mut job) = f.job.take()
        {
            vfs::copy_abort(&mut job.copy);
            f.say("Cópia cancelada", false);
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
                    Some((
                        alloc::format!(
                            "Cópia concluída ({})",
                            if n == 1 {
                                String::from("1 arquivo")
                            } else {
                                alloc::format!("{n} arquivos")
                            }
                        ),
                        false,
                    ))
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
    fn files_commit_input(&mut self, id: WindowId) {
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
    fn files_confirmed(&mut self, id: WindowId) {
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
            None => self.files_note(id, "Excluído", false),
            Some(e) => self.files_note(id, e.message(), true),
        }
    }

    /// A press while a sheet is up: its buttons.
    fn files_sheet_click(&mut self, id: WindowId, lay: &Layout, px: i32, py: i32) {
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
                f.say("Cancelado", false);
            }
            (SheetKind::Confirm, _) => self.files_confirmed(id),
            (SheetKind::Info, _) => f.props = None,
            (SheetKind::Copy, _) => self.files_cancel_job(id),
        }
    }

    /// Build the information sheet of the selection (or of the folder).
    fn files_properties(&mut self, id: WindowId, in_trash: bool, cwd: &[u8], paths: &[Vec<u8>]) {
        let mut lines: Vec<String> = Vec::new();
        let free = vfs::statfs();
        let show = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        if in_trash {
            lines.push(String::from("Local: Lixeira"));
            if let Some(f) = self.files_mut(id) {
                lines.push(alloc::format!("Itens: {}", f.view.rows.len()));
            }
        } else if paths.len() == 1 {
            let p = &paths[0];
            lines.push(alloc::format!("Nome: {}", show(vfs::base_name(p))));
            lines.push(alloc::format!("Local: {}", show(&vfs::parent(p))));
            match vfs::stat(p) {
                Ok(info) => {
                    if info.kind == vfs::EntryKind::Dir {
                        let t = vfs::with_backend(|b| osjeff_core::vfs::tree_size(b, p));
                        lines.push(String::from("Tipo: Pasta"));
                        if let Ok(Ok(t)) = t {
                            lines.push(alloc::format!(
                                "Conteúdo: {} arquivos, {} pastas",
                                t.files,
                                t.dirs.saturating_sub(1)
                            ));
                            lines
                                .push(alloc::format!("Tamanho: {}", fileman::format_size(t.bytes)));
                        }
                    } else {
                        lines.push(alloc::format!(
                            "Tipo: {}",
                            ui::kind_label(vfs::base_name(p), false)
                        ));
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
                Err(e) => lines.push(alloc::format!("Erro: {}", e.message())),
            }
        } else if paths.len() > 1 {
            lines.push(alloc::format!("Seleção: {} itens", paths.len()));
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
                lines.push(alloc::format!("Itens: {}", f.view.rows.len()));
            }
        }
        lines.push(alloc::format!(
            "Livre: {} de {}",
            fileman::format_size(free.free),
            fileman::format_size(free.total)
        ));
        if vfs::volume() == vfs::Volume::Memory {
            lines.push(String::from("Volume: memória (não persiste)"));
        }
        if let Some(f) = self.files_mut(id) {
            f.props = Some(lines);
            f.open_sheet();
        }
    }
}

/// Which sheet a file manager shows, and its size.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SheetKind {
    Confirm,
    Info,
    Copy,
}

impl SheetKind {
    /// Button labels, left to right (the last is the default).
    pub(crate) fn buttons(self) -> [&'static str; 2] {
        match self {
            SheetKind::Confirm => ["Cancelar", "Excluir"],
            SheetKind::Info => ["", "Concluído"],
            SheetKind::Copy => ["", "Cancelar"],
        }
    }
}

/// The sheet window state asks for now: kind and size.
pub(crate) fn files_sheet_kind(f: &FilesState) -> (SheetKind, (i32, i32)) {
    if f.confirm.is_some() {
        (SheetKind::Confirm, (400, 156))
    } else if let Some(lines) = &f.props {
        (SheetKind::Info, (460, 112 + lines.len() as i32 * 24))
    } else {
        (SheetKind::Copy, (400, 148))
    }
}

impl Desktop {
    /// Properties lines of a `.wasm` file: whether it is a valid app package and, if
    /// so, its manifest (permissions and limits) and whether it is installed.
    fn wasm_property_lines(&self, path: &[u8]) -> Vec<String> {
        let bytes = match vfs::read_range(path, 0, osjeff_core::appinstall::MAX_PACKAGE_BYTES + 1) {
            Ok(b) => b,
            Err(e) => return alloc::vec![alloc::format!("Erro: {}", e.message())],
        };
        match osjeff_core::appinstall::check(&bytes) {
            Ok(m) => {
                let mut v = alloc::vec![String::from("Pacote: pacote de app válido")];
                v.extend(fapps::manifest_lines(&m));
                let state = if self.apps.iter().any(|a| a.id == m.id) {
                    "Estado: instalado"
                } else {
                    "Estado: não instalado (Enter instala e abre)"
                };
                v.push(String::from(state));
                v
            }
            Err(e) => alloc::vec![alloc::format!("Pacote: inválido ({e})")],
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
            None => alloc::vec![String::from("Manifesto: indisponível")],
        };
        lines.push(alloc::format!(
            "Estado: {}",
            fapps::status_label(row.installed)
        ));
        lines.push(alloc::format!("Pacote: {}", fileman::format_size(row.size)));
        if row.installed {
            lines.push(alloc::format!("Arquivo: /apps/{app_id}.wasm"));
        } else {
            lines.push(String::from("Origem: embutido no sistema (I instala)"));
        }
        if let Some(f) = self.files_mut(id) {
            f.props = Some(lines);
            f.open_sheet();
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
                f.enter_t = osjeff_core::anim::Tween::at(0.0);
                f.enter_t
                    .retarget(1.0, 0.2, osjeff_core::anim::curves::ENTER);
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

    /// Rebuild the preview pane's content when the selection changed since it was made.
    pub(crate) fn files_sync_preview(&mut self, id: WindowId) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        if !f.preview_open {
            return;
        }
        // What the pane should show: nothing, one file, or "N itens".
        let key: Vec<u8> = match f.view.sel.count() {
            0 => Vec::new(),
            1 => f
                .view
                .selected_rows()
                .first()
                .map(|r| {
                    let mut k = f.view.cwd.clone();
                    k.push(0);
                    k.extend_from_slice(&r.name);
                    k.push(0);
                    k.extend_from_slice(&r.id);
                    k.extend_from_slice(&r.size.to_le_bytes());
                    k
                })
                .unwrap_or_default(),
            n => alloc::format!("#{n}").into_bytes(),
        };
        if f.preview.as_ref().map(|p| &p.path[..]) == Some(&key[..]) {
            return;
        }
        let data = self.build_preview(id, key);
        if let Some(f) = self.files_mut(id) {
            f.preview = Some(Box::new(data));
        }
    }

    fn build_preview(&mut self, id: WindowId, key: Vec<u8>) -> PreviewData {
        use osjeff_core::appart::FileKind;
        let empty = |key: Vec<u8>| PreviewData {
            path: key,
            name: String::new(),
            kind: FileKind::Generic,
            kind_label: String::new(),
            info: Vec::new(),
            image: None,
            lines: Vec::new(),
            note: None,
        };
        let Some(f) = self.files_mut(id) else {
            return empty(key);
        };
        let rows: Vec<fileman::Row> = f.view.selected_rows().into_iter().cloned().collect();
        let in_trash = f.view.in_trash();
        let in_apps = f.view.in_apps();
        let cwd = f.view.cwd.clone();
        if rows.is_empty() {
            return empty(key);
        }
        if rows.len() > 1 {
            let bytes: u64 = rows.iter().filter(|r| !r.is_dir()).map(|r| r.size).sum();
            let mut d = empty(key);
            d.name = alloc::format!("{} itens", rows.len());
            d.kind = FileKind::Folder;
            d.kind_label = String::from("Seleção");
            d.info
                .push((String::from("Tamanho"), fileman::format_size(bytes)));
            return d;
        }
        let row = &rows[0];
        let name = String::from_utf8_lossy(&row.name).into_owned();
        let pk = ui::preview_kind(&row.name, row.is_dir());
        let mut d = empty(key);
        d.name = name;
        d.kind = ui::icon_kind(&row.name, row.is_dir());
        d.kind_label = ui::kind_label(&row.name, row.is_dir());
        if in_apps {
            d.kind = FileKind::App;
            d.kind_label = String::from("Aplicativo");
            d.info.push((
                String::from("Estado"),
                String::from(fapps::status_label(row.installed)),
            ));
        }
        if !row.is_dir() {
            d.info.push((
                String::from(if in_apps { "Pacote" } else { "Tamanho" }),
                fileman::format_size(row.size),
            ));
        }
        if !in_apps {
            d.info.push((
                String::from(if in_trash { "Excluído" } else { "Modificado" }),
                modified_label(row.mtime),
            ));
        }
        if in_trash || in_apps {
            return d;
        }
        let path = vfs::join(&cwd, &row.name);
        match pk {
            ui::PreviewKind::Image => {
                if row.size > ui::PREVIEW_MAX_IMAGE {
                    d.note = Some(String::from("Imagem grande demais para pré-visualizar"));
                } else {
                    match vfs::read_file(&path)
                        .ok()
                        .and_then(|b| osjeff_core::image::decode(&b).ok())
                    {
                        Some(img) => {
                            d.info.insert(
                                1,
                                (
                                    String::from("Dimensões"),
                                    alloc::format!("{} × {} px", img.width(), img.height()),
                                ),
                            );
                            d.image = thumbnail(&img, 216, 156);
                        }
                        None => d.note = Some(String::from("Não foi possível abrir a imagem")),
                    }
                }
            }
            ui::PreviewKind::Text | ui::PreviewKind::Other => {
                match vfs::read_range(&path, 0, ui::PREVIEW_TEXT_BYTES) {
                    Ok(head) => {
                        d.lines = ui::text_preview(&head, 14, 34);
                        if d.lines.is_empty() && pk == ui::PreviewKind::Other {
                            d.note = Some(String::from("Sem pré-visualização"));
                        }
                    }
                    Err(e) => d.note = Some(String::from(e.message())),
                }
            }
            ui::PreviewKind::Folder => {
                if let Ok(list) = vfs::list(&path) {
                    d.info
                        .push((String::from("Itens"), list.len().to_string_lossy()));
                }
            }
            ui::PreviewKind::App => {}
        }
        d
    }
}

/// A picture scaled to fit `bw x bh`, as a premultiplied surface for the preview pane.
fn thumbnail(
    img: &osjeff_core::image::Image,
    bw: usize,
    bh: usize,
) -> Option<osjeff_core::raster::Surface> {
    use osjeff_core::image::Filter;
    let small = img.fit(bw, bh, false, Filter::Box).ok()?;
    let mut s = osjeff_core::raster::Surface::new(small.width(), small.height());
    for (d, &p) in s.px.iter_mut().zip(small.pixels()) {
        *d = osjeff_core::raster::premul(p);
    }
    Some(s)
}

/// `usize` to a string without importing `ToString` everywhere.
trait ToStringLossy {
    fn to_string_lossy(self) -> String;
}

impl ToStringLossy for usize {
    fn to_string_lossy(self) -> String {
        alloc::format!("{self}")
    }
}
