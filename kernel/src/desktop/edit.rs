//! The text editor window: `Desktop` methods around [`EditorState`].
//!
//! The text engine is `osjeff_core::editor2` (gap buffer, UTF-8, selection,
//! undo/redo, find/replace, line numbers) and the questions it does not own (the
//! Open / Save-as picker, "save changes?") are `editor2::dialog`; all of it is
//! tested on the host, and so is the window geometry (`editor2::ui`). This file feeds
//! keys and the mouse in, keeps the animation state (eased caret, selection, sheets,
//! scrollbar) and reaches files only through [`vfs`](super::vfs); the pixels are in
//! `edit_ui.rs`.
//!
//! A window with unsaved changes never closes silently: every way to close it
//! (title-bar button, Ctrl+Q, the task manager, the terminal's `kill`, power
//! actions) goes through [`Desktop::request_close`], which asks first.

use super::appui;
use super::sysstore::VfsStore;
use super::vfs;
use super::*;
use crate::text::{self, BODY, Weight};
use osjeff_core::anim::{Spring, Tween, curves};
use osjeff_core::editor2::ui::{self as eui, FindHit, FindLay, Lay, Metrics};
use osjeff_core::editor2::{
    CloseAsk, CloseChoice, Editor as Ed2, Event as EdEvent, PickEvent, PickMode, PickRow, Picker,
    PromptKind,
};
use osjeff_core::input::{KeyCode, KeyEvent};
use osjeff_core::settings::{FONT_MAX, FONT_MIN, font_step};
use osjeff_core::sysif::SettingsStore;
use osjeff_core::vfs::VfsError;
use osjeff_core::widgets::ScrollbarFade;

/// Largest file the editor opens (the gap buffer, undo and a copy for saving all live in the heap).
pub(crate) const MAX_OPEN: u64 = 16 * 1024 * 1024;

/// Size of the "save changes?" sheet.
pub(crate) const CLOSE_SIZE: (i32, i32) = (430, 168);

/// A question or dialog over the text.
pub(crate) enum EdModal {
    Open(Picker),
    SaveAs { picker: Picker, then_close: bool },
    Close(CloseAsk),
}

/// What the pointer is over, for the hover wash.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum EdHit {
    Bar(FindHit),
    /// A button of the open sheet, left to right.
    Sheet(usize),
    Place(usize),
}

/// An editor window.
pub(crate) struct EditorState {
    pub ed: Ed2,
    /// The file the buffer belongs to; `None` for a new, unnamed document.
    pub path: Option<Vec<u8>>,
    pub modal: Option<EdModal>,
    /// Last result (saved, error), shown in the status bar until the next key.
    pub msg: Option<(String, bool)>,
    /// "Descartar" was chosen: the next close does not ask again.
    pub force_close: bool,
    /// Last text click: tick, byte offset and how many in a row (double / triple click).
    last_click: (u64, usize, u8),
    /// Where the last text press happened: a "drag" that has not moved leaves a word or line
    /// selection (double / triple click) alone.
    press_at: (i32, i32),
    /// Tick of the last key or click: the caret holds still, then blinks (and rests again).
    pub last_input: u64,
    /// Where the caret is drawn: pixels right of the first text column. It glides along a row.
    pub caret_x: Spring,
    /// Top line, left column and screen row the caret target was last computed for: a change
    /// of any of them jumps instead of gliding.
    caret_seen: (usize, usize, usize),
    /// The selection fading in.
    pub sel_t: Tween,
    had_sel: bool,
    /// The sheet sliding down.
    pub sheet_t: Tween,
    pub hover: Option<EdHit>,
    pub hover_t: Tween,
    pub scroll_fade: ScrollbarFade,
    seen_top: usize,
}

impl EditorState {
    pub(crate) fn new() -> Self {
        let mut ed = Ed2::new();
        ed.set_line_numbers(true);
        Self {
            ed,
            path: None,
            modal: None,
            msg: None,
            force_close: false,
            last_click: (0, 0, 0),
            press_at: (-1, -1),
            last_input: 0,
            caret_x: Spring::pixels(0.0, 1100.0, 66.0),
            caret_seen: (0, 0, 0),
            sel_t: Tween::at(1.0),
            had_sel: false,
            sheet_t: Tween::at(1.0),
            hover: None,
            hover_t: Tween::at(1.0),
            scroll_fade: ScrollbarFade::new(),
            seen_top: 0,
        }
    }

    /// Replace the buffer with `data` from `path`.
    fn load(&mut self, path: Vec<u8>, data: &[u8]) {
        self.ed.set_text(data);
        self.path = Some(path);
        self.modal = None;
        self.msg = None;
    }

    /// Put a dialog or question over the text, sliding in.
    fn raise(&mut self, m: EdModal) {
        self.modal = Some(m);
        self.sheet_t = Tween::at(0.0);
        self.sheet_t.retarget(1.0, 0.24, curves::ENTER);
        self.hover = None;
    }

    /// Count a click at `pos` (a byte offset, or a list row) at `ticks`: 1, then 2
    /// and 3 when it repeats the same spot within half a second.
    fn click_count(&mut self, ticks: u64, pos: usize) -> u8 {
        let (t0, p0, n0) = self.last_click;
        let count = if n0 > 0 && p0 == pos && ticks.saturating_sub(t0) <= 125 {
            (n0 % 3) + 1
        } else {
            1
        };
        self.last_click = (ticks, pos, count);
        count
    }

    /// Nothing typed and nothing opened: a new file can reuse this window.
    fn pristine(&self) -> bool {
        self.path.is_none() && self.ed.is_empty() && !self.ed.is_modified()
    }

    /// File name for the title and messages.
    pub(crate) fn name(&self) -> String {
        match &self.path {
            Some(p) => String::from_utf8_lossy(vfs::base_name(p)).into_owned(),
            None => String::from("sem nome"),
        }
    }

    /// Whether the window needs frames: a blinking or gliding caret, a selection fading in, a
    /// sheet sliding, a fading scrollbar. `focused` windows blink their caret, others rest.
    pub(crate) fn animating(&self, focused: bool) -> bool {
        (focused && self.modal.is_none() && appui::caret_animating(self.last_input))
            || !self.caret_x.at_rest()
            || !self.sel_t.finished()
            || !self.sheet_t.finished()
            || !self.hover_t.finished()
            || self.scroll_fade.active(appui::now_ms())
    }
}

// ---- geometry ----------------------------------------------------------------

/// The text size in pixels (a setting shared by all editors).
pub(crate) fn font_px() -> u16 {
    crate::settings::get().editor_font.clamp(FONT_MIN, FONT_MAX) as u16
}

/// The character cell: the face's pitch and a line a little taller than its natural height.
pub(crate) fn metrics() -> Metrics {
    let (cw, lh) = text::mono_cell_px(font_px());
    Metrics { cw, lh: lh + 3 }
}

/// Height of the find bar the engine's prompt needs (0 with none open).
fn bar_height(ed: &Ed2) -> i32 {
    match ed.prompt().map(|p| p.kind) {
        Some(PromptKind::Replace) => eui::REPLACE_H,
        Some(_) => eui::FIND_H,
        None => 0,
    }
}

/// The geometry of window `r` for the state of the engine.
pub(crate) fn geom(r: Rect, ed: &Ed2) -> Lay {
    eui::layout(r, metrics(), bar_height(ed), ed.gutter_width())
}

/// Widths of the two buttons of the replace row.
fn replace_widths() -> (i32, i32) {
    let w = |s: &str| text::measure(s, BODY, Weight::Medium) + 28;
    (w("Substituir"), w("Todos"))
}

/// The controls of the find bar, if one is open.
pub(crate) fn find_lay(lay: &Lay, ed: &Ed2) -> Option<FindLay> {
    let bar = lay.bar?;
    let replace = ed.prompt().is_some_and(|p| p.kind == PromptKind::Replace);
    let (one, all) = replace_widths();
    Some(eui::find_layout(bar, replace, one, all))
}

/// Widths of the buttons of the close question, in the order Descartar, Cancelar, Salvar.
fn close_widths() -> [i32; 3] {
    let w = |s: &str| (text::measure(s, BODY, Weight::Medium) + 32).max(76);
    [w("Descartar"), w("Cancelar"), w("Salvar")]
}

/// The close question's buttons (Descartar, Cancelar, Salvar) in its panel.
pub(crate) fn close_rects(panel: Rect) -> [Rect; 3] {
    eui::close_buttons(panel, close_widths())
}

/// Labels of the picker's two buttons.
pub(crate) fn picker_labels(picker: &Picker) -> [&'static str; 2] {
    [
        "Cancelar",
        if picker.asking().is_some() {
            "Substituir"
        } else if picker.mode == PickMode::Open {
            "Abrir"
        } else {
            "Salvar"
        },
    ]
}

/// The panel of the dialog over window `r`.
pub(crate) fn picker_panel(r: Rect, picker: &Picker) -> Rect {
    appui::sheet_rect(r, eui::picker_size(r, picker.mode == PickMode::SaveAs))
}

/// The picker's two buttons in its panel.
pub(crate) fn picker_buttons(panel: Rect, picker: &Picker) -> Vec<Rect> {
    appui::button_row(
        panel.right() - appui::SHEET_PAD,
        panel.bottom() - appui::SHEET_PAD - appui::BUTTON_H,
        &picker_labels(picker),
    )
}

fn listing(dir: &str) -> Result<Vec<PickRow>, vfs::VfsError> {
    Ok(vfs::list(dir.as_bytes())?
        .into_iter()
        .map(|e| PickRow {
            name: String::from_utf8_lossy(&e.name).into_owned(),
            dir: e.kind == vfs::EntryKind::Dir,
            size: e.size,
        })
        .collect())
}

/// Show `dir` in the picker (or why it cannot be shown).
fn picker_go(p: &mut Picker, dir: &str) {
    match listing(dir) {
        Ok(rows) => p.set_entries(dir, rows),
        Err(e) => p.set_error(e.message()),
    }
}

/// A file for the editor: not a folder, not absurdly large.
fn read_for_editor(path: &[u8]) -> Result<Vec<u8>, vfs::VfsError> {
    let info = vfs::stat(path)?;
    if info.kind == vfs::EntryKind::Dir {
        return Err(vfs::VfsError::IsDir);
    }
    if info.size > MAX_OPEN {
        return Err(vfs::VfsError::TooBig);
    }
    vfs::read_file(path)
}

impl Desktop {
    pub(crate) fn editor_mut(&mut self, id: WindowId) -> Option<&mut EditorState> {
        match self.app_mut(id) {
            Some(App::Editor(e)) => Some(e),
            _ => None,
        }
    }

    fn rect_of(&self, id: WindowId) -> Option<Rect> {
        self.wm.get(id).map(|w| w.rect)
    }

    /// Make the engine's window size match the window (and the find bar), and the dialog's
    /// list the sheet.
    pub(crate) fn sync_editor(&mut self, id: WindowId) {
        let Some(rect) = self.rect_of(id) else { return };
        let Some(e) = self.editor_mut(id) else { return };
        let lay = geom(rect, &e.ed);
        if e.ed.viewport() != (lay.rows, lay.cols) {
            e.ed.resize(lay.rows, lay.cols);
        }
        if let Some(EdModal::Open(p) | EdModal::SaveAs { picker: p, .. }) = &mut e.modal {
            let rows = eui::picker_layout(picker_panel(rect, p), p.mode == PickMode::SaveAs).rows;
            p.set_visible(rows);
        }
    }

    /// Re-fit every editor to its window (a resize or maximize changes the grid).
    pub(crate) fn sync_text_windows(&mut self) {
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Editor(_)))
            .map(|w| w.id)
            .collect();
        for id in ids {
            self.sync_editor(id);
        }
    }

    /// Title-bar text: `Editor — name`, with a dot after the name while there are unsaved
    /// changes.
    pub(crate) fn refresh_editor_title(&mut self, id: WindowId) {
        let Some(w) = self.wm.get(id) else { return };
        let App::Editor(e) = &w.app.app else { return };
        let mut t = base_title(Kind::Editor, w.app.index);
        t.push_str(" \u{2014} ");
        t.push_str(&e.name());
        if e.ed.is_modified() {
            t.push_str(" \u{2022}");
        }
        if w.app.title != t
            && let Some(w) = self.wm.get_mut(id)
        {
            w.app.title = t;
        }
    }

    // ---- keys ----

    pub(crate) fn editor_key(&mut self, id: WindowId, key: Key) -> bool {
        let ev = KeyEvent::from_key(key, self.mods());
        self.editor_event(id, ev)
    }

    /// Give a key to editor `id`. `true` when the window needs a repaint.
    pub(crate) fn editor_event(&mut self, id: WindowId, ev: KeyEvent) -> bool {
        self.sync_editor(id);
        let Some(e) = self.editor_mut(id) else {
            return false;
        };
        if let Some(modal) = e.modal.take() {
            self.editor_modal_key(id, modal, ev);
            self.refresh_editor_title(id);
            self.editor_track(id);
            return true;
        }
        e.msg = None;
        let ctrl = ev.mods.ctrl && !ev.mods.alt;
        if ctrl && let KeyCode::Char(c) = ev.code {
            match c.to_ascii_lowercase() {
                'o' => {
                    self.editor_open_dialog(id);
                    return true;
                }
                's' if ev.mods.shift => {
                    self.editor_save_as_dialog(id, false);
                    return true;
                }
                '+' | '=' => {
                    self.editor_zoom(1);
                    return true;
                }
                '-' | '_' => {
                    self.editor_zoom(-1);
                    return true;
                }
                '0' => {
                    self.editor_zoom(0);
                    return true;
                }
                _ => {}
            }
        }
        let event = {
            let Desktop { wm, clipboard, .. } = self;
            let Some(App::Editor(e)) = wm.get_mut(id).map(|w| &mut w.app.app) else {
                return false;
            };
            e.ed.handle_key(ev, clipboard)
        };
        // A find bar opening or closing changes how many rows fit.
        self.sync_editor(id);
        match event {
            EdEvent::SaveRequested => self.editor_save(id, false),
            EdEvent::QuitRequested => self.request_close(id),
            EdEvent::Handled | EdEvent::Ignored => {}
        }
        self.refresh_editor_title(id);
        self.editor_track(id);
        true
    }

    /// Ctrl +, Ctrl - and Ctrl 0: the text size of every editor, kept in the settings file.
    fn editor_zoom(&mut self, dir: i32) {
        let mut s = crate::settings::get();
        let n = font_step(s.editor_font, dir);
        if n == s.editor_font {
            return;
        }
        s.editor_font = n;
        crate::settings::set(s);
        let _ = VfsStore.save(&s.to_text());
        self.sync_text_windows();
        // Every editor redraws at the new size; the carets do not glide to it.
        self.force_full = true;
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Editor(_)))
            .map(|w| w.id)
            .collect();
        for other in ids {
            if let Some(e) = self.editor_mut(other) {
                e.caret_seen = (usize::MAX, 0, 0);
            }
            self.editor_track(other);
        }
    }

    /// After input: restart the blink, aim the caret, start the selection fade and show the
    /// scrollbar when the text moved.
    pub(crate) fn editor_track(&mut self, id: WindowId) {
        let Some(rect) = self.rect_of(id) else { return };
        let Some(e) = self.editor_mut(id) else { return };
        let lay = geom(rect, &e.ed);
        e.last_input = appui::ticks();
        let top = e.ed.top_line();
        if top != e.seen_top {
            e.seen_top = top;
            e.scroll_fade.touch(appui::now_ms());
        }
        if let Some((row, col)) = e.ed.cursor_screen() {
            let (x, _) = lay.cell_xy(row, col);
            let rel = (x - lay.text_x) as f32;
            let seen = (top, e.ed.left_col(), row);
            e.caret_x.set_target(rel);
            if seen != e.caret_seen {
                e.caret_x.jump(rel);
                e.caret_seen = seen;
            }
        }
        let has = e.ed.has_selection();
        if has && !e.had_sel {
            e.sel_t = Tween::at(0.0);
            e.sel_t.retarget(1.0, 0.14, curves::STANDARD);
        } else if !has {
            e.sel_t = Tween::at(1.0);
        }
        e.had_sel = has;
    }

    /// A key while a dialog is open. `modal` was taken out of the state; put it
    /// back unless the dialog is over.
    fn editor_modal_key(&mut self, id: WindowId, modal: EdModal, ev: KeyEvent) {
        match modal {
            EdModal::Close(mut ask) => match ask.key(ev) {
                None => self.set_modal(id, Some(EdModal::Close(ask))),
                Some(CloseChoice::Save) => self.editor_save(id, true),
                Some(CloseChoice::Discard) => {
                    if let Some(e) = self.editor_mut(id) {
                        e.force_close = true;
                    }
                    self.request_close(id);
                }
                Some(CloseChoice::Cancel) => {}
            },
            EdModal::Open(mut p) => match p.key(ev) {
                PickEvent::None | PickEvent::Redraw | PickEvent::Overwrite(_) => {
                    self.set_modal(id, Some(EdModal::Open(p)));
                }
                PickEvent::Navigate(d) => {
                    picker_go(&mut p, &d);
                    self.set_modal(id, Some(EdModal::Open(p)));
                }
                PickEvent::Choose(path) => self.editor_open_chosen(id, p, path),
                PickEvent::Cancel => {}
            },
            EdModal::SaveAs {
                mut picker,
                then_close,
            } => match picker.key(ev) {
                PickEvent::None | PickEvent::Redraw => {
                    self.set_modal(id, Some(EdModal::SaveAs { picker, then_close }));
                }
                PickEvent::Navigate(d) => {
                    picker_go(&mut picker, &d);
                    self.set_modal(id, Some(EdModal::SaveAs { picker, then_close }));
                }
                PickEvent::Choose(path) => {
                    self.editor_save_chosen(id, picker, path, then_close, false)
                }
                PickEvent::Overwrite(path) => {
                    self.editor_save_chosen(id, picker, path, then_close, true);
                }
                PickEvent::Cancel => {}
            },
        }
    }

    fn set_modal(&mut self, id: WindowId, m: Option<EdModal>) {
        if let Some(e) = self.editor_mut(id) {
            e.modal = m;
        }
    }

    // ---- open ----

    /// Ctrl+O.
    fn editor_open_dialog(&mut self, id: WindowId) {
        let Some(e) = self.editor_mut(id) else { return };
        let dir = e.path.as_deref().map_or_else(
            || String::from("/"),
            |p| String::from_utf8_lossy(&vfs::parent(p)).into_owned(),
        );
        let mut p = Picker::new(PickMode::Open, &dir, "");
        picker_go(&mut p, &dir);
        e.raise(EdModal::Open(p));
    }

    /// The user picked `path` in the Open dialog `p`.
    fn editor_open_chosen(&mut self, id: WindowId, mut p: Picker, path: String) {
        let bytes = path.into_bytes();
        let here = self.editor_mut(id).is_some_and(|e| e.pristine());
        if here {
            match read_for_editor(&bytes) {
                Ok(data) => {
                    if let Some(e) = self.editor_mut(id) {
                        e.load(bytes, &data);
                    }
                }
                Err(err) => {
                    p.set_error(err.message());
                    self.set_modal(id, Some(EdModal::Open(p)));
                }
            }
            return;
        }
        // The window holds a document: the chosen file gets its own window (or the
        // one already showing it), so nothing is replaced.
        if let Err(err) = self.fs_load_path(bytes) {
            p.set_error(err.message());
            self.set_modal(id, Some(EdModal::Open(p)));
        }
    }

    /// Open `path` in an editor window: the one that already shows it, else a new
    /// one. Used by the file manager and by `edit`. The `Option` is a warning for
    /// the caller to show (none today: files are opened whole).
    pub(crate) fn fs_load_path(&mut self, path: Vec<u8>) -> Result<Option<&'static str>, VfsError> {
        let open = self
            .wm
            .windows()
            .iter()
            .filter(|w| !w.is_closing())
            .find_map(|w| match &w.app.app {
                App::Editor(e) if e.path.as_deref() == Some(path.as_slice()) => Some(w.id),
                _ => None,
            });
        if let Some(id) = open {
            self.wm.activate(id);
            return Ok(None);
        }
        let data = read_for_editor(&path)?;
        let id = self.open_new(Kind::Editor).ok_or(VfsError::Busy)?;
        if let Some(e) = self.editor_mut(id) {
            e.load(path, &data);
        }
        self.sync_editor(id);
        self.refresh_editor_title(id);
        Ok(None)
    }

    /// `edit [FILE]` from the terminal: open `path` (an empty document that
    /// will be created on save when it does not exist yet), or a blank editor.
    pub(crate) fn open_editor_for(&mut self, path: Option<String>) {
        let Some(path) = path else {
            self.open_new(Kind::Editor);
            return;
        };
        let bytes = path.into_bytes();
        if !vfs::exists(&bytes) {
            let Some(id) = self.open_new(Kind::Editor) else {
                return;
            };
            if let Some(e) = self.editor_mut(id) {
                e.path = Some(bytes);
                e.msg = Some((String::from("Novo arquivo"), false));
            }
            self.refresh_editor_title(id);
            return;
        }
        if let Err(e) = self.fs_load_path(bytes) {
            crate::notify!(Warn, "Editor: {}", e.message());
        }
    }

    // ---- save ----

    /// Ctrl+S (and "Salvar" in the close question).
    pub(crate) fn editor_save(&mut self, id: WindowId, then_close: bool) {
        let Some(path) = self.editor_mut(id).map(|e| e.path.clone()) else {
            return;
        };
        match path {
            None => self.editor_save_as_dialog(id, then_close),
            Some(p) => {
                if let Err(err) = self.editor_write(id, &p) {
                    if let Some(e) = self.editor_mut(id) {
                        e.msg = Some((String::from(err.message()), true));
                    }
                } else if then_close {
                    self.finish_close(id);
                }
            }
        }
        self.refresh_editor_title(id);
    }

    /// Write the buffer to `path`; on success it becomes the document's file.
    fn editor_write(&mut self, id: WindowId, path: &[u8]) -> Result<(), VfsError> {
        let Some(e) = self.editor_mut(id) else {
            return Err(VfsError::NotFound);
        };
        let data = e.ed.to_bytes();
        vfs::write_file(path, &data)?;
        e.ed.mark_saved();
        e.path = Some(path.to_vec());
        e.msg = Some((
            alloc::format!("Salvo: {}", String::from_utf8_lossy(vfs::base_name(path))),
            false,
        ));
        self.fs_changed();
        Ok(())
    }

    fn finish_close(&mut self, id: WindowId) {
        if let Some(e) = self.editor_mut(id) {
            e.force_close = true;
        }
        self.request_close(id);
    }

    /// Ctrl+Shift+S, and Ctrl+S on a document without a file.
    fn editor_save_as_dialog(&mut self, id: WindowId, then_close: bool) {
        let Some(e) = self.editor_mut(id) else { return };
        let (dir, name) = match &e.path {
            Some(p) => (
                String::from_utf8_lossy(&vfs::parent(p)).into_owned(),
                String::from_utf8_lossy(vfs::base_name(p)).into_owned(),
            ),
            None => (String::from("/"), String::from("sem-nome.txt")),
        };
        let mut picker = Picker::new(PickMode::SaveAs, &dir, &name);
        picker_go(&mut picker, &dir);
        e.raise(EdModal::SaveAs { picker, then_close });
    }

    /// The user chose `path` in the Save-as dialog. An existing file other than the
    /// document's own is replaced only after "Substituir?" is confirmed.
    fn editor_save_chosen(
        &mut self,
        id: WindowId,
        mut picker: Picker,
        path: String,
        then_close: bool,
        confirmed: bool,
    ) {
        let bytes = path.clone().into_bytes();
        let own = self
            .editor_mut(id)
            .is_some_and(|e| e.path.as_deref() == Some(bytes.as_slice()));
        if !confirmed && !own && vfs::exists(&bytes) {
            picker.confirm_overwrite(&path);
            self.set_modal(id, Some(EdModal::SaveAs { picker, then_close }));
            return;
        }
        match self.editor_write(id, &bytes) {
            Ok(()) => {
                if then_close {
                    self.finish_close(id);
                }
            }
            Err(err) => {
                picker.set_error(err.message());
                self.set_modal(id, Some(EdModal::SaveAs { picker, then_close }));
            }
        }
    }

    // ---- closing ----

    /// An editor with unsaved changes is asked about first. `true` when the close
    /// was held back (the question is now on screen).
    pub(crate) fn editor_holds_close(&mut self, id: WindowId) -> bool {
        let Some(w) = self.wm.get_mut(id) else {
            return false;
        };
        if w.is_closing() {
            return false;
        }
        let App::Editor(e) = &mut w.app.app else {
            return false;
        };
        if !e.ed.is_modified() || e.force_close {
            return false;
        }
        if !matches!(e.modal, Some(EdModal::Close(_))) {
            e.raise(EdModal::Close(CloseAsk::new()));
        }
        self.wm.activate(id);
        true
    }

    /// Before a reboot or shutdown: if some editor holds unsaved changes, ask
    /// about it instead and cancel the power action. `true` = blocked.
    pub(crate) fn guard_unsaved(&mut self) -> bool {
        let dirty = self
            .wm
            .windows()
            .iter()
            .find(|w| !w.is_closing() && matches!(&w.app.app, App::Editor(e) if e.ed.is_modified()))
            .map(|w| w.id);
        match dirty {
            Some(id) => self.editor_holds_close(id),
            None => false,
        }
    }

    // ---- mouse ----

    fn editor_ref(&self, id: WindowId) -> Option<&EditorState> {
        match self.wm.get(id).map(|w| &w.app.app) {
            Some(App::Editor(e)) => Some(e),
            _ => None,
        }
    }

    /// Press in editor `id` at `(px, py)`.
    pub(crate) fn editor_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        self.sync_editor(id);
        let shift = self.keymap.shift();
        let ticks = crate::interrupts::ticks();
        let Some(e) = self.editor_mut(id) else { return };
        match e.modal.take() {
            Some(EdModal::Close(mut ask)) => {
                let btns = close_rects(appui::sheet_rect(rect, CLOSE_SIZE));
                let choice = btns
                    .iter()
                    .position(|b| b.contains(px, py))
                    .map(|i| [CloseChoice::Discard, CloseChoice::Cancel, CloseChoice::Save][i]);
                match choice {
                    Some(ch) => {
                        ask.select(ch);
                        // A click answers: replay it as Enter on that button.
                        self.editor_modal_key(
                            id,
                            EdModal::Close(ask),
                            KeyEvent::plain(KeyCode::Enter),
                        );
                    }
                    None => self.set_modal(id, Some(EdModal::Close(ask))),
                }
            }
            Some(m @ (EdModal::Open(_) | EdModal::SaveAs { .. })) => {
                self.editor_picker_click(id, rect, m, px, py);
            }
            _ => {
                let lay = geom(rect, &e.ed);
                if lay.bar.is_some_and(|b| b.contains(px, py)) {
                    self.editor_bar_click(id, rect, px, py);
                } else if lay.in_text(px, py) {
                    let (row, col) = lay.cell_at(px, py);
                    let pos = e.ed.pos_at_screen(row, col);
                    // Same spot within half a second: double (word), triple (line).
                    let count = e.click_count(ticks, pos);
                    e.ed.mouse_down(row, col, count, shift);
                    e.press_at = (px, py);
                    self.drag = Some(Drag {
                        win: id,
                        mode: DragMode::Select,
                    });
                }
            }
        }
        self.editor_track(id);
    }

    /// A press while the Open / Save-as sheet is up.
    fn editor_picker_click(&mut self, id: WindowId, rect: Rect, modal: EdModal, px: i32, py: i32) {
        let ticks = crate::interrupts::ticks();
        let (mut picker, then_close, save_as) = match modal {
            EdModal::Open(p) => (p, false, false),
            EdModal::SaveAs { picker, then_close } => (picker, then_close, true),
            m => {
                self.set_modal(id, Some(m));
                return;
            }
        };
        let rebuild = |p: Picker| {
            if save_as {
                EdModal::SaveAs {
                    picker: p,
                    then_close,
                }
            } else {
                EdModal::Open(p)
            }
        };
        let panel = picker_panel(rect, &picker);
        let lay = eui::picker_layout(panel, save_as);
        let btns = picker_buttons(panel, &picker);
        if btns[0].contains(px, py) {
            return;
        }
        if btns[1].contains(px, py) {
            self.editor_modal_key(id, rebuild(picker), KeyEvent::plain(KeyCode::Enter));
            return;
        }
        if let Some(i) = lay.place_at(px, py) {
            picker_go(&mut picker, eui::PLACES[i].1);
            self.set_modal(id, Some(rebuild(picker)));
            return;
        }
        let ev = match lay
            .row_at(picker.scroll(), px, py)
            .filter(|&i| i < picker.rows().len())
        {
            Some(i) => {
                let double = self
                    .editor_mut(id)
                    .is_some_and(|e| e.click_count(ticks, i) >= 2);
                picker.click(i, double)
            }
            None => PickEvent::None,
        };
        self.after_picker_click(id, rebuild(picker), ev);
    }

    /// A press inside the find bar.
    fn editor_bar_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        let Some(e) = self.editor_mut(id) else { return };
        let lay = geom(rect, &e.ed);
        let Some(fl) = find_lay(&lay, &e.ed) else {
            return;
        };
        let goto = e.ed.prompt().is_some_and(|p| p.kind == PromptKind::Goto);
        match fl.hit(px, py, goto) {
            Some(FindHit::Find) => e.ed.prompt_focus(0),
            Some(FindHit::Replace) => e.ed.prompt_focus(1),
            Some(FindHit::Prev) => {
                e.ed.find_prev();
            }
            Some(FindHit::Next) => {
                e.ed.find_next();
            }
            Some(FindHit::Case) => {
                let on = e.ed.config().case_sensitive;
                e.ed.set_case_sensitive(!on);
            }
            Some(FindHit::Close) => e.ed.close_prompt(),
            Some(FindHit::ReplaceOne) => {
                e.ed.replace_current();
            }
            Some(FindHit::ReplaceAll) => {
                e.ed.replace_all();
            }
            None => {}
        }
        self.sync_editor(id);
        self.refresh_editor_title(id);
    }

    /// What a click inside a dialog led to.
    fn after_picker_click(&mut self, id: WindowId, modal: EdModal, ev: PickEvent) {
        match ev {
            PickEvent::None | PickEvent::Redraw => self.set_modal(id, Some(modal)),
            PickEvent::Navigate(d) => {
                let modal = match modal {
                    EdModal::Open(mut p) => {
                        picker_go(&mut p, &d);
                        EdModal::Open(p)
                    }
                    EdModal::SaveAs {
                        mut picker,
                        then_close,
                    } => {
                        picker_go(&mut picker, &d);
                        EdModal::SaveAs { picker, then_close }
                    }
                    m => m,
                };
                self.set_modal(id, Some(modal));
            }
            PickEvent::Choose(path) => match modal {
                EdModal::Open(p) => self.editor_open_chosen(id, p, path),
                EdModal::SaveAs { picker, then_close } => {
                    self.editor_save_chosen(id, picker, path, then_close, false);
                }
                m => self.set_modal(id, Some(m)),
            },
            PickEvent::Overwrite(_) | PickEvent::Cancel => {}
        }
        self.refresh_editor_title(id);
    }

    /// The pointer moved with the button held after a text click: extend the selection.
    pub(crate) fn editor_drag(&mut self, id: WindowId, px: i32, py: i32) {
        let Some(rect) = self.rect_of(id) else { return };
        let Some(e) = self.editor_mut(id) else { return };
        if e.modal.is_some() || (px, py) == e.press_at {
            return;
        }
        let lay = geom(rect, &e.ed);
        // Dragging above or below the text scrolls it.
        if py < lay.top {
            e.ed.scroll_by(-1);
        } else if py >= lay.top + lay.rows as i32 * lay.m.lh {
            e.ed.scroll_by(1);
        }
        let (row, col) = lay.cell_at(px, py);
        e.ed.mouse_drag(row, col);
        self.editor_track(id);
    }

    /// Wheel over editor `id` (`notches` > 0 = up).
    pub(crate) fn editor_wheel(&mut self, id: WindowId, notches: i32) {
        let Some(e) = self.editor_mut(id) else { return };
        match &mut e.modal {
            Some(EdModal::Open(p)) | Some(EdModal::SaveAs { picker: p, .. }) => {
                p.scroll_by(-(notches as isize) * 3);
            }
            Some(EdModal::Close(_)) => {}
            None => {
                e.ed.scroll_by(-(notches as isize) * 3);
                e.scroll_fade.touch(appui::now_ms());
                e.seen_top = e.ed.top_line();
            }
        }
    }

    /// What the pointer at `(px, py)` is over in editor `id`.
    fn editor_hit_at(&self, id: WindowId, px: i32, py: i32) -> Option<EdHit> {
        let rect = self.rect_of(id)?;
        let e = self.editor_ref(id)?;
        match &e.modal {
            Some(EdModal::Close(_)) => close_rects(appui::sheet_rect(rect, CLOSE_SIZE))
                .iter()
                .position(|b| b.contains(px, py))
                .map(EdHit::Sheet),
            Some(EdModal::Open(p)) | Some(EdModal::SaveAs { picker: p, .. }) => {
                let panel = picker_panel(rect, p);
                let btns = picker_buttons(panel, p);
                btns.iter()
                    .position(|b| b.contains(px, py))
                    .map(EdHit::Sheet)
                    .or_else(|| {
                        eui::picker_layout(panel, p.mode == PickMode::SaveAs)
                            .place_at(px, py)
                            .map(EdHit::Place)
                    })
            }
            None => {
                let lay = geom(rect, &e.ed);
                let fl = find_lay(&lay, &e.ed)?;
                let goto = e.ed.prompt().is_some_and(|p| p.kind == PromptKind::Goto);
                fl.hit(px, py, goto).map(EdHit::Bar)
            }
        }
    }

    /// The pointer moved over editor `id`. `true` when something under it changed.
    pub(crate) fn editor_hover(&mut self, id: WindowId, px: i32, py: i32) -> bool {
        let hit = self.editor_hit_at(id, px, py);
        let Some(e) = self.editor_mut(id) else {
            return false;
        };
        if e.hover == hit {
            return false;
        }
        e.hover = hit;
        e.hover_t = Tween::at(0.0);
        e.hover_t.retarget(1.0, 0.12, curves::STANDARD);
        true
    }

    pub(crate) fn editor_unhover(&mut self, id: WindowId) {
        if let Some(e) = self.editor_mut(id) {
            e.hover = None;
        }
    }

    /// Whether the pointer at `(cx, cy)` is over the text of editor `id` (it shows an I-beam).
    pub(crate) fn editor_text_at(&self, id: WindowId, cx: i32, cy: i32) -> bool {
        let (Some(rect), Some(e)) = (self.rect_of(id), self.editor_ref(id)) else {
            return false;
        };
        e.modal.is_none() && geom(rect, &e.ed).in_text(cx, cy)
    }

    // ---- per-frame state ----

    /// Advance the animations of every editor by `dt`. Returns whether any still needs frames.
    pub(crate) fn step_editors(&mut self, dt: f32) -> bool {
        let focus = self.focused();
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Editor(_)) && w.shown())
            .map(|w| w.id)
            .collect();
        let mut busy = false;
        for id in ids {
            if let Some(e) = self.editor_mut(id) {
                e.caret_x.step(dt);
                e.sel_t.step(dt);
                e.sheet_t.step(dt);
                e.hover_t.step(dt);
                busy |= e.animating(focus == Some(id));
            }
        }
        busy
    }
}
