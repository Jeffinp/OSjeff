//! The Open / Save-as and "save changes?" sheets attached to an editor window.

use super::paint::secondary;
use super::paint::tertiary;
use crate::desktop::apps::editor::geometry::close_rects;
use crate::desktop::apps::editor::geometry::picker_buttons;
use crate::desktop::apps::editor::geometry::picker_labels;
use crate::desktop::apps::editor::geometry::picker_panel;
use crate::desktop::apps::editor::state::CLOSE_SIZE;
use crate::desktop::apps::editor::state::EdHit;
use crate::desktop::kit::appui::{self, EmptyIcon, level};
use crate::desktop::kit::ui::ButtonKind;
use crate::desktop::*;
use crate::text::{self, BODY, CALLOUT, CAPTION, FOOTNOTE, Weight};
use kitsune_core::appart::{FileKind, Tool};
use kitsune_core::editor2::ui::{self as eui};
use kitsune_core::editor2::{CloseAsk, CloseChoice, Picker};
use kitsune_core::fileman;
use kitsune_core::fileman::ui as fui;
use kitsune_core::t;

impl Desktop {
    // ---- sheets ----

    pub(super) fn draw_close_sheet(
        &self,
        c: &mut Canvas,
        r: Rect,
        e: &EditorState,
        ask: &CloseAsk,
    ) {
        let panel = appui::sheet(c, r, CLOSE_SIZE, level(&e.sheet_t));
        let saved = c.set_clip(
            panel
                .intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        );
        let inner = panel.w - 2 * appui::SHEET_PAD;
        let ty = panel.y + appui::SHEET_PAD;
        text::draw(
            c,
            panel.x + appui::SHEET_PAD,
            ty,
            t!("edit.close.title"),
            text::TITLE3,
            Weight::Semibold,
            theme::text(),
        );
        let msg = t!("edit.close.body", name = &e.name());
        let mut y = ty + text::line_height(text::TITLE3) + 8;
        for (a, b) in text::wrap(&msg, BODY, Weight::Regular, inner, 3) {
            text::draw(
                c,
                panel.x + appui::SHEET_PAD,
                y,
                &msg[a..b],
                BODY,
                Weight::Regular,
                secondary(),
            );
            y += text::line_height(BODY) + 2;
        }
        let btns = close_rects(panel);
        let sel = ask.selected();
        let items = [
            (CloseAsk::label(CloseChoice::Discard), CloseChoice::Discard),
            (CloseAsk::label(CloseChoice::Cancel), CloseChoice::Cancel),
            (CloseAsk::label(CloseChoice::Save), CloseChoice::Save),
        ];
        for (i, (label, choice)) in items.into_iter().enumerate() {
            let kind = if choice == sel {
                if choice == CloseChoice::Discard {
                    ButtonKind::Destructive
                } else {
                    ButtonKind::Primary
                }
            } else {
                ButtonKind::Secondary
            };
            appui::sheet_button(
                c,
                btns[i],
                label,
                kind,
                e.hover == Some(EdHit::Sheet(i)),
                false,
            );
        }
        c.restore_clip(saved);
    }

    pub(super) fn draw_picker_sheet(
        &self,
        c: &mut Canvas,
        r: Rect,
        e: &EditorState,
        p: &Picker,
        title: &str,
    ) {
        let save = p.mode == kitsune_core::editor2::PickMode::SaveAs;
        let size = eui::picker_size(r, save);
        let panel = appui::sheet(c, r, size, level(&e.sheet_t));
        let saved = c.set_clip(
            panel
                .intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        );
        let lay = eui::picker_layout(panel, save);
        let pal = theme::pal();
        text::draw(
            c,
            lay.title.x,
            lay.title.y,
            title,
            text::TITLE3,
            Weight::Semibold,
            theme::text(),
        );
        // The sidebar of places.
        text::draw(
            c,
            lay.sidebar.x + 8,
            lay.sidebar.y + 2,
            t!("edit.dir.favorites"),
            CAPTION,
            Weight::Semibold,
            tertiary(),
        );
        let here = eui::place_of(p.dir());
        let tools = [Tool::Home, Tool::Folder, Tool::Photo, Tool::Disk];
        for (i, tool) in tools.iter().enumerate() {
            let label = eui::place_label(i);
            let rr = lay.place_rect(i);
            let active = here == Some(i);
            if active {
                let (col, a) = theme::tint(0x30_00_00_00 | appui::rgb_of(theme::accent()));
                c.fill_rrect(rr, 6, Corner::Circle, col, a);
            } else if e.hover == Some(EdHit::Place(i)) {
                let (col, a) = theme::tint(pal.hover);
                c.fill_rrect(rr, 6, Corner::Circle, col, a);
            }
            let col = if active { theme::accent() } else { secondary() };
            appui::blit_tool_dim(
                c,
                *tool,
                rr.x + 8,
                rr.y + (rr.h - 16) / 2,
                16,
                appui::rgb_of(col),
                256,
            );
            let tcol = if active {
                theme::accent()
            } else {
                theme::text()
            };
            let ty = text::center_y(rr.y, rr.h, BODY, Weight::Regular);
            text::draw(
                c,
                rr.x + 32,
                ty,
                label,
                BODY,
                if active {
                    Weight::Medium
                } else {
                    Weight::Regular
                },
                tcol,
            );
        }
        // The folder shown.
        let dir = text::ellipsize_middle(p.dir(), FOOTNOTE, Weight::Medium, lay.path.w);
        let ty = text::center_y(lay.path.y, lay.path.h, FOOTNOTE, Weight::Medium);
        text::draw(
            c,
            lay.path.x,
            ty,
            &dir,
            FOOTNOTE,
            Weight::Medium,
            secondary(),
        );
        // The list.
        ui::fill_token(c, lay.list, 8, pal.field_bg);
        ui::stroke_token(c, lay.list, 8, pal.control_border);
        let list_clip = c.set_clip(
            lay.list
                .intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        );
        let first = p.scroll();
        for (k, row) in p.rows().iter().skip(first).take(lay.rows).enumerate() {
            let rr = Rect::new(
                lay.list.x + 4,
                lay.list.y + k as i32 * eui::PICK_ROW_H,
                lay.list.w - 8,
                eui::PICK_ROW_H,
            );
            let sel = first + k == p.selected();
            if sel {
                c.fill_rrect(
                    Rect::new(rr.x, rr.y + 1, rr.w, rr.h - 2),
                    6,
                    Corner::Circle,
                    theme::accent(),
                    256,
                );
            }
            let kind = if row.name == ".." {
                FileKind::Folder
            } else {
                fui::icon_kind(row.name.as_bytes(), row.dir)
            };
            kit::appart::blit_file(c, kind, rr.x + 6, rr.y + (rr.h - 18) / 2, 18, 256);
            let name_col = if sel {
                theme::ACCENT_TEXT
            } else {
                theme::text()
            };
            let size = if row.dir {
                String::new()
            } else {
                fileman::format_size(row.size)
            };
            let sw = if size.is_empty() {
                0
            } else {
                text::measure(&size, FOOTNOTE, Weight::Regular) + 8
            };
            let ty = text::center_y(rr.y, rr.h, BODY, Weight::Regular);
            text::draw_ellipsis(
                c,
                rr.x + 32,
                ty,
                (rr.w - 32 - sw - 12).max(0),
                &row.name,
                BODY,
                Weight::Regular,
                name_col,
            );
            if !size.is_empty() {
                let col = if sel { theme::ACCENT_TEXT } else { tertiary() };
                text::draw_right(
                    c,
                    Rect::new(rr.x, rr.y, rr.w - 8, rr.h),
                    &size,
                    FOOTNOTE,
                    Weight::Regular,
                    col,
                );
            }
        }
        if p.rows().is_empty() {
            appui::empty_state(
                c,
                lay.list,
                EmptyIcon::File(FileKind::Folder),
                t!("edit.dir.empty"),
                "",
            );
        }
        ui::overlay_scrollbar(c, lay.list, first, p.rows().len(), lay.rows, 256);
        c.restore_clip(list_clip);
        // The name field.
        if save {
            let (txt, caret) = p.field();
            let byte = txt.char_indices().nth(caret).map_or(txt.len(), |(i, _)| i);
            appui::field(
                c,
                lay.field,
                &appui::FieldText {
                    text: &txt,
                    caret: byte,
                    selection: p.field_fresh().then_some((0, txt.len())),
                },
                t!("edit.dir.file_name"),
                true,
                appui::caret_alpha(e.last_input),
                None,
                false,
            );
        }
        // The overwrite question or the reason something failed.
        let hint: Option<(String, Color)> = if let Some(path) = p.asking() {
            Some((
                t!(
                    "edit.dir.overwrite",
                    name = &String::from_utf8_lossy(vfs::base_name(path.as_bytes())).into_owned()
                ),
                theme::danger(),
            ))
        } else {
            p.error().map(|m| (String::from(m), theme::danger()))
        };
        if let Some((msg, col)) = hint {
            let t = text::ellipsize(&msg, FOOTNOTE, Weight::Regular, lay.hint.w);
            let ty = text::center_y(lay.hint.y, lay.hint.h, FOOTNOTE, Weight::Regular);
            text::draw(c, lay.hint.x, ty, &t, FOOTNOTE, Weight::Regular, col);
        }
        // Buttons.
        let btns = picker_buttons(panel, p);
        let labels = picker_labels(p);
        appui::sheet_button(
            c,
            btns[0],
            labels[0],
            ButtonKind::Secondary,
            e.hover == Some(EdHit::Sheet(0)),
            false,
        );
        appui::sheet_button(
            c,
            btns[1],
            labels[1],
            if p.asking().is_some() {
                ButtonKind::Destructive
            } else {
                ButtonKind::Primary
            },
            e.hover == Some(EdHit::Sheet(1)),
            false,
        );
        let _ = (CALLOUT, picker_panel);
        c.restore_clip(saved);
    }
}
