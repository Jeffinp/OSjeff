//! Keeping the editor in step with its window: size, title and language.

use crate::desktop::apps::editor::geometry::geom;
use crate::desktop::apps::editor::geometry::picker_panel;
use crate::desktop::apps::editor::state::EdModal;
use crate::desktop::*;
use kitsune_core::editor2::PickMode;
use kitsune_core::editor2::ui::{self as eui};

impl Desktop {
    pub(crate) fn editor_mut(&mut self, id: WindowId) -> Option<&mut EditorState> {
        match self.app_mut(id) {
            Some(App::Editor(e)) => Some(e),
            _ => None,
        }
    }

    pub(super) fn rect_of(&self, id: WindowId) -> Option<Rect> {
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

    /// The language changed: drop every editor's status message and dialog error (written in the
    /// old language) and rebuild the title (the name of an unnamed document is a word).
    pub(crate) fn editor_language_changed_all(&mut self) {
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Editor(_)))
            .map(|w| w.id)
            .collect();
        for id in ids {
            self.editor_language_changed(id);
        }
    }

    fn editor_language_changed(&mut self, id: WindowId) {
        if let Some(e) = self.editor_mut(id) {
            e.msg = None;
            if let Some(EdModal::Open(p) | EdModal::SaveAs { picker: p, .. }) = e.modal.as_mut() {
                p.clear_error();
            }
        }
        self.refresh_editor_title(id);
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
}
