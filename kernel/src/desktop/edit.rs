//! The text editor window: `Desktop` methods around [`EditorState`].
//!
//! The text engine is `osjeff_core::editor2` (gap buffer, UTF-8, selection,
//! undo/redo, find/replace, line numbers) and the questions it does not own (the
//! Open / Save-as picker, "save changes?") are `editor2::dialog`; all of it is
//! tested on the host. This file feeds keys and the mouse in, paints what the
//! engine exposes, and reaches files only through [`vfs`](super::vfs).
//!
//! A window with unsaved changes never closes silently: every way to close it
//! (title-bar button, Ctrl+Q, the task manager, the terminal's `kill`, power
//! actions) goes through [`Desktop::request_close`], which asks first.

use super::term::latin1;
use super::*;
use super::{ui, vfs};
use osjeff_core::editor2::{
    CloseAsk, CloseChoice, Editor as Ed2, Event as EdEvent, Notice, PickEvent, PickMode, PickRow,
    Picker, PromptKind, PromptView, status_line,
};
use osjeff_core::fileman;
use osjeff_core::input::{KeyCode, KeyEvent};
use osjeff_core::vfs::VfsError;

/// Largest file the editor opens (the gap buffer, undo and a copy for saving all live in the heap).
pub(crate) const MAX_OPEN: u64 = 16 * 1024 * 1024;

const GUTTER: Color = Color::rgb(0xEE, 0xF1, 0xF8);
const ERR: Color = Color::rgb(0xC0, 0x2B, 0x2B);
const OKC: Color = Color::rgb(0x1B, 0x7F, 0x5F);
const BLUE: Color = Color::rgb(0x25, 0x63, 0xEB);

const PAD: i32 = 10;
const CELL_W: i32 = 12;
const LINE_H: i32 = 18;
const STATUS_H: i32 = 24;
const PROMPT_H: i32 = 26;
const TOP: i32 = TITLE_H + 6;

/// A question or dialog over the text.
pub(crate) enum EdModal {
    Open(Picker),
    SaveAs { picker: Picker, then_close: bool },
    Close(CloseAsk),
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
        }
    }

    /// Replace the buffer with `data` from `path`.
    fn load(&mut self, path: Vec<u8>, data: &[u8]) {
        self.ed.set_text(data);
        self.path = Some(path);
        self.modal = None;
        self.msg = None;
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
    fn name(&self) -> String {
        match &self.path {
            Some(p) => String::from_utf8_lossy(vfs::base_name(p)).into_owned(),
            None => String::from("sem nome"),
        }
    }
}

// ---- geometry ----------------------------------------------------------------

struct Lay {
    /// The text area (a whole number of cells).
    text: Rect,
    prompt: Rect,
    status: Rect,
    rows: usize,
    cols: usize,
}

fn editor_layout(r: Rect, prompt: bool) -> Lay {
    let prompt_h = if prompt { PROMPT_H } else { 0 };
    let avail_h = r.h - TOP - STATUS_H - prompt_h - 4;
    let rows = (avail_h / LINE_H).max(1);
    let cols = ((r.w - 2 * PAD) / CELL_W).max(2);
    Lay {
        text: Rect::new(r.x + PAD, r.y + TOP, cols * CELL_W, rows * LINE_H),
        prompt: Rect::new(
            r.x + PAD,
            r.y + r.h - STATUS_H - prompt_h,
            r.w - 2 * PAD,
            prompt_h,
        ),
        status: Rect::new(r.x, r.y + r.h - STATUS_H, r.w, STATUS_H),
        rows: rows as usize,
        cols: cols as usize,
    }
}

/// Geometry of the Open / Save-as dialog inside the window `r`.
struct PickLay {
    frame: Rect,
    path: Rect,
    list: Rect,
    field: Rect,
    hint: Rect,
    rows: usize,
}

const PICK_ROW_H: i32 = 20;

fn picker_layout(r: Rect) -> PickLay {
    let w = (r.w - 24).clamp(260, 560);
    let h = (r.h - TITLE_H - 16).clamp(180, 340);
    let x = r.x + (r.w - w) / 2;
    let y = r.y + TITLE_H + 8;
    let rows = ((h - 58 - 74) / PICK_ROW_H).max(1);
    PickLay {
        frame: Rect::new(x, y, w, h),
        path: Rect::new(x + 12, y + 32, w - 24, 20),
        list: Rect::new(x + 12, y + 58, w - 24, rows * PICK_ROW_H),
        field: Rect::new(x + 12, y + h - 66, w - 24, 28),
        hint: Rect::new(x + 12, y + h - 30, w - 24, 20),
        rows: rows as usize,
    }
}

/// The three buttons of the close question inside the window `r`.
fn close_layout(r: Rect) -> (Rect, [Rect; 3]) {
    let w = (r.w - 30).clamp(260, 420);
    let h = 120;
    let x = r.x + (r.w - w) / 2;
    let y = r.y + TITLE_H + (r.h - TITLE_H - h).max(0) / 2;
    let bw = (w - 28 - 16) / 3;
    let by = y + h - 44;
    let btn = |i: i32| Rect::new(x + 14 + i * (bw + 8), by, bw, 32);
    (Rect::new(x, y, w, h), [btn(0), btn(1), btn(2)])
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

    /// Make the engine's window size match the window (and the find bar).
    pub(crate) fn sync_editor(&mut self, id: WindowId) {
        let Some(rect) = self.rect_of(id) else { return };
        let Some(e) = self.editor_mut(id) else { return };
        let lay = editor_layout(rect, e.ed.is_prompt_open());
        if e.ed.viewport() != (lay.rows, lay.cols) {
            e.ed.resize(lay.rows, lay.cols);
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

    /// Title-bar text: `OSJEFF EDIT - name *` (the star marks unsaved changes).
    pub(crate) fn refresh_editor_title(&mut self, id: WindowId) {
        let Some(w) = self.wm.get(id) else { return };
        let App::Editor(e) = &w.app.app else { return };
        let mut t = base_title(Kind::Editor, w.app.index);
        t.push_str(" - ");
        t.push_str(&e.name());
        if e.ed.is_modified() {
            t.push_str(" *");
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
        true
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
        e.modal = Some(EdModal::Open(p));
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
        e.modal = Some(EdModal::SaveAs { picker, then_close });
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
            e.modal = Some(EdModal::Close(CloseAsk::new()));
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

    /// Press in editor `id` at `(px, py)`.
    pub(crate) fn editor_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        self.sync_editor(id);
        let shift = self.keymap.shift();
        let ticks = crate::interrupts::ticks();
        let Some(e) = self.editor_mut(id) else { return };
        match e.modal.take() {
            Some(EdModal::Close(mut ask)) => {
                let (_, btns) = close_layout(rect);
                let hit = btns.iter().position(|b| b.contains(px, py));
                match hit {
                    Some(i) => {
                        let choice = CloseAsk::LABELS[i].0;
                        ask.select(choice);
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
            Some(EdModal::Open(mut p)) => {
                let ev = match picker_row_at(&p, rect, px, py) {
                    Some(i) => {
                        let double = e.click_count(ticks, i) >= 2;
                        p.click(i, double)
                    }
                    None => PickEvent::None,
                };
                self.after_picker_click(id, EdModal::Open(p), ev);
            }
            Some(EdModal::SaveAs {
                mut picker,
                then_close,
            }) => {
                let ev = match picker_row_at(&picker, rect, px, py) {
                    Some(i) => {
                        let double = e.click_count(ticks, i) >= 2;
                        picker.click(i, double)
                    }
                    None => PickEvent::None,
                };
                self.after_picker_click(id, EdModal::SaveAs { picker, then_close }, ev);
            }
            None => {
                let lay = editor_layout(rect, e.ed.is_prompt_open());
                if !lay.text.contains(px, py) {
                    return;
                }
                let row = ((py - lay.text.y) / LINE_H) as usize;
                let col = ((px - lay.text.x) / CELL_W) as usize;
                let pos = e.ed.pos_at_screen(row, col);
                // Same spot within half a second: double (word), triple (line).
                let count = e.click_count(ticks, pos);
                e.ed.mouse_down(row, col, count, shift);
                self.drag = Some(Drag {
                    win: id,
                    mode: DragMode::Select,
                });
            }
        }
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
        if e.modal.is_some() {
            return;
        }
        let lay = editor_layout(rect, e.ed.is_prompt_open());
        // Dragging above or below the text scrolls it.
        if py < lay.text.y {
            e.ed.scroll_by(-1);
        } else if py >= lay.text.y + lay.text.h {
            e.ed.scroll_by(1);
        }
        let row = ((py - lay.text.y).max(0) / LINE_H).min(lay.rows as i32 - 1) as usize;
        let col = ((px - lay.text.x).max(0) / CELL_W).min(lay.cols as i32 - 1) as usize;
        e.ed.mouse_drag(row, col);
    }

    /// Wheel over editor `id` (`notches` > 0 = up).
    pub(crate) fn editor_wheel(&mut self, id: WindowId, notches: i32) {
        let Some(e) = self.editor_mut(id) else { return };
        match &mut e.modal {
            Some(EdModal::Open(p)) | Some(EdModal::SaveAs { picker: p, .. }) => {
                p.scroll_by(-(notches as isize) * 3);
            }
            Some(EdModal::Close(_)) => {}
            None => e.ed.scroll_by(-(notches as isize) * 3),
        }
    }

    // ---- drawing ----

    pub(crate) fn draw_editor(&self, c: &mut Canvas, r: Rect, e: &EditorState, focused: bool) {
        let ed = &e.ed;
        let prompt = ed.prompt();
        let lay = editor_layout(r, prompt.is_some());
        // The engine's grid is fitted on the next tick after a resize; never paint outside the window.
        let (vrows, vcols) = ed.viewport();
        let rows = vrows.min(lay.rows);
        let cols = vcols.min(lay.cols);
        let gutter = ed.gutter_width().min(cols.saturating_sub(1));
        let tx = lay.text.x.max(0) as usize;
        let ty = lay.text.y.max(0) as usize;
        let sel_bg = theme::accent().lerp(theme::WHITE, 150);
        if gutter > 0 {
            ui::fill(
                c,
                Rect::new(
                    lay.text.x - 4,
                    lay.text.y,
                    (gutter as i32) * CELL_W + 2,
                    rows as i32 * LINE_H,
                ),
                GUTTER,
            );
        }
        for (i, row) in ed.visible_rows().enumerate().take(rows) {
            let y = ty + i * LINE_H as usize;
            if let Some(n) = row.line_number {
                let s = alloc::format!("{n}");
                let w = s.len().min(gutter.saturating_sub(1));
                font::draw_bytes(
                    c,
                    tx + (gutter - 1 - w) * CELL_W as usize,
                    y + 2,
                    &s.as_bytes()[s.len() - w..],
                    theme::TEXT_MUTED,
                    2,
                );
            }
            let mut x = tx + gutter * CELL_W as usize;
            for cell in row.cells().take(cols - gutter) {
                if cell.selected {
                    c.fill_rect(x, y, CELL_W as usize, LINE_H as usize, sel_bg);
                }
                if cell.ch != ' ' {
                    font::draw_char(c, x, y + 2, latin1(cell.ch), theme::TEXT, 2);
                }
                x += CELL_W as usize;
            }
        }
        if focused
            && e.modal.is_none()
            && let Some((row, col)) = ed.cursor_screen()
            && row < rows
            && col < cols
        {
            c.fill_rect(
                tx + col * CELL_W as usize,
                ty + row * LINE_H as usize + 1,
                2,
                16,
                theme::accent(),
            );
        }
        if let Some(p) = &prompt {
            self.draw_find_bar(c, lay.prompt, p);
        }
        self.draw_editor_status(c, lay.status, e);
        match &e.modal {
            Some(EdModal::Close(ask)) => self.draw_close_ask(c, r, e, ask),
            Some(EdModal::Open(p)) => self.draw_picker(c, r, p, "Abrir arquivo"),
            Some(EdModal::SaveAs { picker, .. }) => self.draw_picker(c, r, picker, "Salvar como"),
            None => {}
        }
    }

    fn draw_editor_status(&self, c: &mut Canvas, st: Rect, e: &EditorState) {
        ui::fill(c, st, GUTTER);
        let (text, color): (String, Color) = match &e.msg {
            Some((m, err)) => (m.clone(), if *err { ERR } else { OKC }),
            None => (status_line(&e.ed.status(), false), theme::TEXT_MUTED),
        };
        let bytes: Vec<u8> = text.chars().map(latin1).collect();
        let room = (st.w - 2 * PAD).max(0);
        // Scale 2 when it fits, else the small font, else cut.
        let (scale, cell) = if bytes.len() as i32 * CELL_W <= room {
            (2, CELL_W)
        } else {
            (1, 6)
        };
        let n = bytes.len().min((room / cell).max(0) as usize);
        let h = if scale == 2 { 14 } else { 7 };
        font::draw_bytes(
            c,
            (st.x + PAD).max(0) as usize,
            (st.y + (st.h - h) / 2).max(0) as usize,
            &bytes[..n],
            color,
            scale,
        );
    }

    fn draw_find_bar(&self, c: &mut Canvas, r: Rect, p: &PromptView<'_>) {
        ui::fill_round(c, r, 6, GUTTER);
        let y = r.y + (r.h - 14) / 2;
        let mut x = r.x + 8;
        // The fields share what the labels and the notice (right edge) leave.
        let (nfields, labels) = match p.kind {
            PromptKind::Replace => (2, "Buscar:Trocar:".len()),
            PromptKind::Find => (1, "Buscar:".len()),
            PromptKind::Goto => (1, "Linha:".len()),
        };
        let notice_w = 22 * 6;
        let avail = r.w - 16 - notice_w - labels as i32 * CELL_W - nfields * 12;
        let field_w = (avail / nfields).max(CELL_W * 4);
        let label = |active: bool, s: &str, x: &mut i32, c: &mut Canvas, v: &str| {
            ui::text(c, *x, y, 200, s.as_bytes(), theme::TEXT_MUTED);
            *x += s.len() as i32 * CELL_W;
            let w = field_w;
            let box_r = Rect::new(*x - 2, r.y + 2, w + 4, r.h - 4);
            ui::fill_round(
                c,
                box_r,
                5,
                if active { theme::accent() } else { ui::BORDER },
            );
            ui::fill_round(
                c,
                Rect::new(box_r.x + 1, box_r.y + 1, box_r.w - 2, box_r.h - 2),
                4,
                theme::WHITE,
            );
            let chars = ((w / CELL_W) as usize).max(1);
            let bytes: Vec<u8> = v.chars().map(latin1).collect();
            let tail = &bytes[bytes.len().saturating_sub(chars)..];
            font::draw_bytes(
                c,
                (*x).max(0) as usize,
                y.max(0) as usize,
                tail,
                theme::TEXT,
                2,
            );
            if active {
                let cx = *x + tail.len() as i32 * CELL_W;
                ui::fill(c, Rect::new(cx, y, 2, 14), theme::accent());
            }
            *x += w + 12;
        };
        match p.kind {
            PromptKind::Goto => label(true, "Linha:", &mut x, c, p.text),
            PromptKind::Find => label(true, "Buscar:", &mut x, c, p.text),
            PromptKind::Replace => {
                label(p.active == 0, "Buscar:", &mut x, c, p.text);
                label(p.active == 1, "Trocar:", &mut x, c, p.text2.unwrap_or(""));
            }
        }
        let note = match p.notice {
            Notice::None => "",
            Notice::NotFound => "nao encontrado",
            Notice::Wrapped => "recomecou do inicio",
            Notice::Replaced(_) => "substituido",
            Notice::InvalidLine => "linha invalida",
        };
        let mut tail = String::new();
        if let Notice::Replaced(n) = p.notice {
            tail = alloc::format!("{n} troca(s)");
        } else if !note.is_empty() {
            tail.push_str(note);
        } else if p.case_sensitive {
            tail.push_str("Aa");
        }
        let color = if p.notice == Notice::NotFound || p.notice == Notice::InvalidLine {
            ERR
        } else {
            theme::TEXT_MUTED
        };
        let w = tail.len() as i32 * 6;
        font::draw_text(
            c,
            (r.right() - w - 8).max(x).max(0) as usize,
            (r.y + (r.h - 7) / 2).max(0) as usize,
            &tail,
            color,
            1,
        );
    }

    fn draw_close_ask(&self, c: &mut Canvas, r: Rect, e: &EditorState, ask: &CloseAsk) {
        let (frame, btns) = close_layout(r);
        ui::fill_round(
            c,
            Rect::new(frame.x - 3, frame.y - 3, frame.w + 6, frame.h + 6),
            12,
            ERR,
        );
        ui::fill_round(c, frame, 10, theme::WINDOW_BODY);
        ui::text(
            c,
            frame.x + 14,
            frame.y + 12,
            frame.w - 28,
            b"Salvar alteracoes?",
            theme::TEXT,
        );
        let mut name: Vec<u8> = e.name().chars().map(latin1).collect();
        // "Ha alteracoes em " takes 17 cells of the line.
        name = fileman::ellipsize(
            &name,
            (((frame.w - 28) / CELL_W) as usize).saturating_sub(17),
        );
        let mut line = Vec::from(&b"Ha alteracoes em "[..]);
        line.extend_from_slice(&name);
        ui::text(
            c,
            frame.x + 14,
            frame.y + 40,
            frame.w - 28,
            &line,
            theme::TEXT_MUTED,
        );
        for (i, (choice, label)) in CloseAsk::LABELS.iter().enumerate() {
            let state = if *choice == ask.selected() {
                ui::Btn::On
            } else {
                ui::Btn::Normal
            };
            ui::button(c, btns[i], label.as_bytes(), state);
        }
    }

    fn draw_picker(&self, c: &mut Canvas, r: Rect, p: &Picker, title: &str) {
        let lay = picker_layout(r);
        let f = lay.frame;
        ui::fill_round(
            c,
            Rect::new(f.x - 3, f.y - 3, f.w + 6, f.h + 6),
            12,
            ui::BORDER,
        );
        ui::fill_round(c, f, 10, theme::WINDOW_BODY);
        ui::text(
            c,
            f.x + 12,
            f.y + 10,
            f.w - 24,
            title.as_bytes(),
            theme::TEXT,
        );
        // Folder shown, cut from the left when long.
        let dir: Vec<u8> = p.dir().chars().map(latin1).collect();
        let room = (lay.path.w / CELL_W) as usize;
        let shown = if dir.len() > room {
            let mut v = Vec::from(&b"..."[..]);
            v.extend_from_slice(&dir[dir.len() - (room.saturating_sub(3))..]);
            v
        } else {
            dir
        };
        ui::text(
            c,
            lay.path.x,
            lay.path.y,
            lay.path.w,
            &shown,
            theme::TEXT_MUTED,
        );
        // The list.
        ui::fill_round(
            c,
            Rect::new(
                lay.list.x - 2,
                lay.list.y - 2,
                lay.list.w + 4,
                lay.list.h + 4,
            ),
            6,
            ui::BORDER,
        );
        ui::fill_round(c, lay.list, 5, theme::WHITE);
        let first = p.scroll();
        for (k, row) in p.rows().iter().skip(first).take(lay.rows).enumerate() {
            let y = lay.list.y + k as i32 * PICK_ROW_H;
            if first + k == p.selected() {
                ui::fill(
                    c,
                    Rect::new(lay.list.x, y, lay.list.w, PICK_ROW_H),
                    theme::accent().lerp(theme::WHITE, 150),
                );
            }
            let mut name: Vec<u8> = row.name.chars().map(latin1).collect();
            if row.dir && row.name != ".." {
                name.push(b'/');
            }
            let size = if row.dir {
                Vec::new()
            } else {
                fileman::format_size(row.size).into_bytes()
            };
            let room = ((lay.list.w - 12) / CELL_W) as usize;
            let name = fileman::ellipsize(&name, room.saturating_sub(size.len() + 1));
            ui::text(
                c,
                lay.list.x + 6,
                y + 3,
                lay.list.w,
                &name,
                if row.dir { BLUE } else { theme::TEXT },
            );
            ui::text_right(
                c,
                Rect::new(lay.list.x, y, lay.list.w - 6, PICK_ROW_H - 4),
                &size,
                theme::TEXT_MUTED,
            );
        }
        if p.rows().is_empty() {
            ui::text(
                c,
                lay.list.x + 6,
                lay.list.y + 3,
                lay.list.w,
                b"(pasta vazia)",
                theme::TEXT_MUTED,
            );
        }
        if p.rows().len() > lay.rows {
            let track = Rect::new(lay.list.right() - 5, lay.list.y + 2, 3, lay.list.h - 4);
            ui::fill_round(c, track, 1, ui::BORDER);
            let total = p.rows().len();
            let th = ((track.h as i64 * lay.rows as i64) / total as i64).max(10) as i32;
            let ty =
                track.y + ((track.h - th) as i64 * first as i64 / (total - lay.rows) as i64) as i32;
            ui::fill_round(c, Rect::new(track.x, ty, 3, th), 1, theme::accent());
        }
        // The name field.
        let (text, caret) = p.field();
        let bytes: Vec<u8> = text.chars().map(latin1).collect();
        ui::input_box(c, lay.field, &bytes, b"nome ou caminho", true);
        let _ = caret;
        // Hint, error or the overwrite question.
        let (msg, color): (Vec<u8>, Color) = if let Some(path) = p.asking() {
            let mut m = Vec::from(&b"Substituir "[..]);
            m.extend(fileman::ellipsize(
                &path.chars().map(latin1).collect::<Vec<u8>>(),
                ((lay.hint.w / CELL_W) as usize).saturating_sub(24),
            ));
            m.extend_from_slice(b"? Enter sim  Esc nao");
            (m, ERR)
        } else if let Some(err) = p.error() {
            (err.chars().map(latin1).collect(), ERR)
        } else {
            (
                Vec::from(&b"Enter confirma  Tab completa  Esc cancela"[..]),
                theme::TEXT_MUTED,
            )
        };
        ui::text(c, lay.hint.x, lay.hint.y, lay.hint.w, &msg, color);
    }
}

/// The list row under `(px, py)` in the dialog of window `r`, if any.
fn picker_row_at(p: &Picker, r: Rect, px: i32, py: i32) -> Option<usize> {
    let lay = picker_layout(r);
    if !lay.list.contains(px, py) {
        return None;
    }
    Some(p.scroll() + ((py - lay.list.y) / PICK_ROW_H) as usize)
}
