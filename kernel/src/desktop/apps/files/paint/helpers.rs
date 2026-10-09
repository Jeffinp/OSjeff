//! Small drawing helpers of Arquivos: colours, the inline rename field and the drag ghost.

use crate::desktop::kit::appui::{self};
use crate::desktop::*;
use crate::text::{self, BODY, CAPTION, Weight};
use kitsune_core::appart::Tool;
use kitsune_core::fileman::ui::{self as fui, Columns};

pub(super) fn argb(c: Color) -> u32 {
    appui::rgb_of(c)
}

/// `color` at `a` (0..=256) as a blend of text colour: used for dim text.
pub(super) fn alpha_of(a: u32, base: u32) -> u32 {
    (a * base / 256).min(256)
}

pub(super) fn tertiary() -> Color {
    theme::solid(theme::pal().text_tertiary)
}

/// Width of the size column's text area.
pub(super) fn ui_size_w(cols: &Columns) -> i32 {
    ((if cols.has_date() {
        cols.date_x
    } else {
        cols.right
    }) - cols.size_x
        - 12)
        .max(40)
}

/// How one item is to be drawn.
pub(super) struct ItemCtx {
    pub(super) sel: bool,
    pub(super) hover: u32,
    pub(super) cut: bool,
    pub(super) dropping: bool,
    pub(super) focused: bool,
    pub(super) alpha: u32,
    pub(super) editing: bool,
}

/// The inline rename field (a white field with an accent ring, the stem selected).
pub(super) fn draw_rename_field(c: &mut Canvas, r: Rect, e: &NameEdit) {
    // Opaque underneath: the field sits on a selected (accent) row in the list.
    c.fill_rrect(r, 6, Corner::Circle, theme::surface(), 256);
    let text = e.input.to_string_lossy();
    appui::field(
        c,
        r,
        &appui::FieldText {
            text: &text,
            caret: e.input.caret(),
            selection: e.input.selection(),
        },
        "",
        true,
        appui::caret_alpha(e.last_input),
        None,
        false,
    );
}

/// The card that follows the pointer while items are dragged.
pub(super) fn draw_drag_ghost(c: &mut Canvas, d: &DragState, ctrl: bool) {
    let p = theme::pal();
    let name_w = text::measure(&d.label, BODY, Weight::Regular).min(160);
    let w = 12 + 22 + 8 + name_w + 12 + if d.count > 1 { 26 } else { 0 };
    let r = Rect::new(d.pos.0 + 14, d.pos.1 + 10, w, 32);
    c.draw_shadow(
        r,
        Shadow {
            blur: 10,
            dy: 4,
            alpha: 90,
        },
        Rect::new(r.x, r.y + 6, r.w, r.h - 12),
    );
    let valid = d.op.is_some();
    c.fill_rrect(r, 8, Corner::Circle, theme::solid(p.window_bg), 240);
    ui::stroke_token(c, r, 8, p.control_border);
    kit::appart::blit_file(
        c,
        d.kind,
        r.x + 8,
        r.y + 5,
        22,
        if valid { 256 } else { 150 },
    );
    text::draw_ellipsis(
        c,
        r.x + 38,
        text::center_y(r.y, r.h, BODY, Weight::Regular),
        name_w,
        &d.label,
        BODY,
        Weight::Regular,
        if valid {
            theme::text()
        } else {
            theme::text_muted()
        },
    );
    if d.count > 1 {
        let b = Rect::new(r.right() - 26, r.y + 7, 18, 18);
        c.fill_rrect(b, 9, Corner::Circle, theme::accent(), 256);
        text::draw_centered(
            c,
            b,
            &alloc::format!("{}", d.count.min(99)),
            CAPTION,
            Weight::Semibold,
            theme::ACCENT_TEXT,
        );
    }
    // A copy gets a plus badge on the corner of the card.
    if ctrl && valid && d.op == Some(fui::DropOp::Copy) {
        let b = Rect::new(r.x - 6, r.y - 6, 16, 16);
        c.fill_rrect(b, 8, Corner::Circle, theme::ok(), 256);
        appui::blit_tool_dim(c, Tool::Plus, b.x + 3, b.y + 3, 10, 0xFFFFFF, 256);
    }
}
