//! Editor window geometry and the Open / Save-as picker helpers.

use crate::desktop::apps::editor::state::MAX_OPEN;
use crate::desktop::kit::appui;
use crate::desktop::services::vfs;
use crate::desktop::*;
use crate::text::{self, BODY, Weight};
use kitsune_core::editor2::ui::{self as eui, FindLay, Lay, Metrics};
use kitsune_core::editor2::{
    CloseAsk, CloseChoice, Editor as Ed2, PickMode, PickRow, Picker, PromptKind,
};
use kitsune_core::settings::{FONT_MAX, FONT_MIN};
use kitsune_core::t;

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
    (w(t!("edit.replace")), w(t!("edit.all")))
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
    [
        w(CloseAsk::label(CloseChoice::Discard)),
        w(CloseAsk::label(CloseChoice::Cancel)),
        w(CloseAsk::label(CloseChoice::Save)),
    ]
}

/// The close question's buttons (Descartar, Cancelar, Salvar) in its panel.
pub(crate) fn close_rects(panel: Rect) -> [Rect; 3] {
    eui::close_buttons(panel, close_widths())
}

/// Labels of the picker's two buttons.
pub(crate) fn picker_labels(picker: &Picker) -> [&'static str; 2] {
    [
        t!("common.cancel"),
        if picker.asking().is_some() {
            t!("edit.replace")
        } else if picker.mode == PickMode::Open {
            t!("common.open")
        } else {
            t!("edit.save")
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
pub(super) fn picker_go(p: &mut Picker, dir: &str) {
    match listing(dir) {
        Ok(rows) => p.set_entries(dir, rows),
        Err(e) => p.set_error(e.message()),
    }
}

/// A file for the editor: not a folder, not absurdly large.
pub(super) fn read_for_editor(path: &[u8]) -> Result<Vec<u8>, vfs::VfsError> {
    let info = vfs::stat(path)?;
    if info.kind == vfs::EntryKind::Dir {
        return Err(vfs::VfsError::IsDir);
    }
    if info.size > MAX_OPEN {
        return Err(vfs::VfsError::TooBig);
    }
    vfs::read_file(path)
}
