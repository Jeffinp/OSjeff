//! Pointer handling of the editor: text, find bar and sheets.

use super::geometry::picker_go;
use crate::desktop::apps::editor::geometry::close_rects;
use crate::desktop::apps::editor::geometry::find_lay;
use crate::desktop::apps::editor::geometry::geom;
use crate::desktop::apps::editor::geometry::picker_buttons;
use crate::desktop::apps::editor::geometry::picker_panel;
use crate::desktop::apps::editor::state::CLOSE_SIZE;
use crate::desktop::apps::editor::state::EdHit;
use crate::desktop::apps::editor::state::EdModal;
use crate::desktop::kit::appui;
use crate::desktop::*;
use kitsune_core::anim::{Tween, curves};
use kitsune_core::editor2::ui::{self as eui, FindHit};
use kitsune_core::editor2::{CloseChoice, PickEvent, PickMode, Picker, PromptKind};
use kitsune_core::input::{KeyCode, KeyEvent};

impl Desktop {
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
}
