//! Keyboard handling of the editor, including the modal sheets.

use super::geometry::picker_go;
use crate::desktop::apps::editor::geometry::geom;
use crate::desktop::apps::editor::state::EdModal;
use crate::desktop::kit::appui;
use crate::desktop::services::sysstore::VfsStore;
use crate::desktop::*;
use kitsune_core::anim::{Tween, curves};
use kitsune_core::editor2::{CloseChoice, Event as EdEvent, PickEvent};
use kitsune_core::input::{KeyCode, KeyEvent};
use kitsune_core::settings::font_step;
use kitsune_core::sysif::SettingsStore;

impl Desktop {
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
    pub(super) fn editor_modal_key(&mut self, id: WindowId, modal: EdModal, ev: KeyEvent) {
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

    pub(super) fn set_modal(&mut self, id: WindowId, m: Option<EdModal>) {
        if let Some(e) = self.editor_mut(id) {
            e.modal = m;
        }
    }
}
