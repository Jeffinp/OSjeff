//! The items: list rows, icon cells and the empty state.

use super::super::labels::modified_label;
use super::helpers::ItemCtx;
use super::helpers::alpha_of;
use super::helpers::draw_rename_field;
use super::helpers::ui_size_w;
use crate::desktop::kit::appui::{self, EmptyIcon};
use crate::desktop::*;
use crate::text::{self, BODY, FOOTNOTE, Weight};
use kitsune_core::appart::{FileKind, Tool};
use kitsune_core::fileman::ui::{self as fui, Columns, Layout, ViewMode};
use kitsune_core::fileman::{self};
use kitsune_core::t;

impl Desktop {
    // -------------------------------------------------------------------- items

    pub(super) fn draw_files_items(
        &self,
        c: &mut Canvas,
        lay: &Layout,
        st: &FilesState,
        focused: bool,
    ) {
        let rows = &st.view.rows;
        let n = rows.len();
        if n == 0 {
            self.draw_files_empty(c, lay, st);
            return;
        }
        let scroll = st.scroller.pos();
        let (first, end) = fui::visible_range(st.mode, lay.list.w, lay.list.h, scroll, n);
        let enter = appui::level(&st.enter_t);
        let slide = ((256 - enter) as i32 * 8) / 256;
        let cols = lay.columns();
        let hover_lvl = appui::level(&st.hover_t);
        let hovered = match st.hover {
            Some(fui::Hit::Item(i)) => Some(i),
            _ => None,
        };
        let drop_item = match &st.gesture {
            Gesture::Drag(d) => match d.over {
                DropHover::Item(i) => Some(i),
                _ => None,
            },
            _ => None,
        };
        let renaming = st.input.as_ref().and_then(|e| {
            let EditPurpose::Rename(p) = &e.purpose;
            let name = vfs::base_name(p);
            rows.iter().position(|r| r.name == name)
        });
        for (i, row) in rows.iter().enumerate().take(end).skip(first) {
            let base = fui::item_rect(st.mode, lay.list.w, i);
            let rect = Rect::new(
                lay.list.x + base.x,
                lay.list.y + base.y - scroll + slide,
                base.w,
                base.h,
            );
            let sel = st.view.sel.is_selected(i);
            let hov = hovered == Some(i) && !sel;
            let cut = !st.view.in_trash()
                && !st.view.in_apps()
                && self
                    .pathclip
                    .is_cut_path(&vfs::join(&st.view.cwd, &row.name));
            let dropping = drop_item == Some(i);
            let ctx = ItemCtx {
                sel,
                hover: if hov { hover_lvl } else { 0 },
                cut,
                dropping,
                focused,
                alpha: enter,
                editing: renaming == Some(i),
            };
            match st.mode {
                ViewMode::List => self.draw_list_row(c, rect, &cols, row, &ctx, st),
                ViewMode::Icons => self.draw_icon_cell(c, rect, row, &ctx, st),
            }
        }
        // The rubber band.
        if let Gesture::Band { anchor, cur, .. } = &st.gesture {
            let a = (lay.list.x + anchor.0, lay.list.y + anchor.1 - scroll);
            let b = (cur.0, cur.1);
            let r = fui::band_rect(a, b);
            let acc = theme::accent();
            c.fill_rrect(r, 3, Corner::Circle, acc, 34);
            c.stroke_rrect(r, 3, Corner::Circle, acc, 200);
        }
    }

    fn draw_files_empty(&self, c: &mut Canvas, lay: &Layout, st: &FilesState) {
        let q = st.view.filter();
        let (icon, title, sub): (EmptyIcon, &str, String) = if !q.is_empty() {
            (
                EmptyIcon::Tool(Tool::Search),
                t!("files.empty.no_results"),
                t!(
                    "files.empty.nothing_for",
                    query = &String::from_utf8_lossy(q).into_owned()
                ),
            )
        } else if st.view.in_trash() {
            (
                EmptyIcon::Tool(Tool::Trash),
                t!("files.empty.trash"),
                String::new(),
            )
        } else if st.view.in_apps() {
            (
                EmptyIcon::File(FileKind::App),
                t!("files.empty.apps"),
                String::new(),
            )
        } else {
            (
                EmptyIcon::File(FileKind::Folder),
                t!("files.empty.folder"),
                String::from(t!("files.empty.drag_here")),
            )
        };
        appui::empty_state(c, lay.list, icon, title, &sub);
    }

    fn draw_list_row(
        &self,
        c: &mut Canvas,
        rect: Rect,
        cols: &Columns,
        row: &fileman::Row,
        ctx: &ItemCtx,
        st: &FilesState,
    ) {
        let p = theme::pal();
        let pill = Rect::new(rect.x, rect.y + 1, rect.w, rect.h - 2);
        if ctx.sel {
            c.fill_rrect(
                pill,
                6,
                Corner::Circle,
                appui::selection_fill(ctx.focused),
                256,
            );
        } else if ctx.hover > 0 {
            let (col, a) = theme::tint(p.hover);
            c.fill_rrect(
                pill,
                6,
                Corner::Circle,
                col,
                (a as u32 * ctx.hover / 256) as u16,
            );
        }
        if ctx.dropping {
            c.fill_rrect(pill, 6, Corner::Circle, theme::accent(), 60);
            c.stroke_rrect(pill, 6, Corner::Circle, theme::accent(), 256);
        }
        let dim = if ctx.cut { 110 } else { 256 };
        let a = alpha_of(ctx.alpha, dim);
        let (fg, fg2) = if ctx.sel {
            let t = appui::selection_text(ctx.focused);
            (t, t)
        } else {
            (theme::text(), theme::text_muted())
        };
        // Icon: an installed app shows its own, the rest the kind's icon.
        let ix = cols.name_x;
        let iy = rect.y + (rect.h - 20) / 2;
        if st.view.in_apps() && row.installed {
            let rgba = self
                .apps
                .iter()
                .find(|app| app.id.as_bytes() == &row.id[..])
                .and_then(|app| app.icon.as_deref());
            if let Some(px) = rgba {
                let tile = icons::app_tile(Some(px), 20);
                c.blit_surface(&tile, ix, iy, a);
            } else {
                kit::appart::blit_file(c, FileKind::App, ix, iy, 20, a);
            }
        } else if st.view.in_apps() {
            kit::appart::blit_file(c, FileKind::App, ix, iy, 20, a / 2);
        } else {
            kit::appart::blit_file(c, fui::icon_kind(&row.name, row.is_dir()), ix, iy, 20, a);
        }
        // Name.
        let name_x = ix + 28;
        let name_w = (cols.size_x - name_x - 12).max(20);
        if ctx.editing {
            if let Some(e) = &st.input {
                draw_rename_field(
                    c,
                    Rect::new(name_x - 4, rect.y + 2, name_w + 4, rect.h - 4),
                    e,
                );
            }
        } else {
            let name = String::from_utf8_lossy(&row.name);
            let shown = text::ellipsize_middle(&name, BODY, Weight::Regular, name_w);
            text::draw_a(
                c,
                name_x,
                text::center_y(rect.y, rect.h, BODY, Weight::Regular),
                &shown,
                BODY,
                Weight::Regular,
                fg,
                a as u16,
            );
        }
        // Size and date.
        let size: String = if row.is_dir() {
            String::from("—")
        } else {
            fileman::format_size(row.size)
        };
        let size_w = ui_size_w(cols);
        let sw = text::measure(&size, BODY, Weight::Regular);
        text::draw_a(
            c,
            cols.size_x + size_w - sw,
            text::center_y(rect.y, rect.h, BODY, Weight::Regular),
            &size,
            BODY,
            Weight::Regular,
            fg2,
            a as u16,
        );
        if cols.has_date() {
            let (label, col) = if st.view.in_apps() {
                let l = fileman::apps::status_label(row.installed);
                (
                    String::from(l),
                    if row.installed && !ctx.sel {
                        theme::ok()
                    } else {
                        fg2
                    },
                )
            } else {
                (modified_label(row.mtime), fg2)
            };
            appui::draw_ellipsis_a(
                c,
                cols.date_x,
                text::center_y(rect.y, rect.h, BODY, Weight::Regular),
                cols.right - cols.date_x - 12,
                &label,
                BODY,
                Weight::Regular,
                col,
                a as u16,
            );
        }
    }

    fn draw_icon_cell(
        &self,
        c: &mut Canvas,
        rect: Rect,
        row: &fileman::Row,
        ctx: &ItemCtx,
        st: &FilesState,
    ) {
        let p = theme::pal();
        let cx = rect.x + rect.w / 2;
        let tile = Rect::new(cx - 30, rect.y + 2, 60, 60);
        let acc = theme::accent();
        if ctx.sel {
            let col = if ctx.focused {
                acc
            } else {
                theme::text_muted()
            };
            c.fill_rrect(tile, 10, Corner::Circle, col, 52);
        } else if ctx.hover > 0 {
            let (col, a) = theme::tint(p.hover);
            c.fill_rrect(
                tile,
                10,
                Corner::Circle,
                col,
                (a as u32 * ctx.hover / 256) as u16,
            );
        }
        if ctx.dropping {
            c.fill_rrect(tile, 10, Corner::Circle, acc, 70);
            c.stroke_rrect(tile, 10, Corner::Circle, acc, 256);
        }
        let dim = if ctx.cut { 110 } else { 256 };
        let a = alpha_of(ctx.alpha, dim);
        let kind = fui::icon_kind(&row.name, row.is_dir());
        if st.view.in_apps() && row.installed {
            let rgba = self
                .apps
                .iter()
                .find(|app| app.id.as_bytes() == &row.id[..])
                .and_then(|app| app.icon.as_deref());
            if let Some(px) = rgba {
                let t = icons::app_tile(Some(px), 48);
                c.blit_surface(&t, cx - 24, rect.y + 8, a);
            } else {
                kit::appart::blit_file(c, FileKind::App, cx - 24, rect.y + 8, 48, a);
            }
        } else {
            let kind = if st.view.in_apps() {
                FileKind::App
            } else {
                kind
            };
            kit::appart::blit_file(c, kind, cx - 24, rect.y + 8, 48, a);
        }
        // Label: up to two lines, centred; a selection pill behind it.
        let name = String::from_utf8_lossy(&row.name).into_owned();
        let max_w = rect.w - 4;
        if ctx.editing {
            if let Some(e) = &st.input {
                draw_rename_field(c, Rect::new(rect.x - 4, rect.y + 62, rect.w + 8, 24), e);
            }
            return;
        }
        // A name that fits is one line; one with spaces wraps to two (the second cut with an
        // ellipsis); a long name without spaces is cut in the middle so the extension stays.
        let mut pieces: Vec<String> = Vec::new();
        if text::measure(&name, FOOTNOTE, Weight::Regular) <= max_w {
            pieces.push(name.clone());
        } else if name.contains(' ') {
            let lines = text::wrap(&name, FOOTNOTE, Weight::Regular, max_w, 2);
            let n = lines.len();
            for (k, (s, e)) in lines.iter().enumerate() {
                if k + 1 == n && *e < name.len() {
                    pieces.push(text::ellipsize(
                        &name[*s..],
                        FOOTNOTE,
                        Weight::Regular,
                        max_w,
                    ));
                } else {
                    pieces.push(String::from(name[*s..*e].trim_end()));
                }
            }
        } else {
            pieces.push(text::ellipsize_middle(
                &name,
                FOOTNOTE,
                Weight::Regular,
                max_w,
            ));
        }
        let mut ly = rect.y + 64;
        let lh = text::line_height(FOOTNOTE);
        for piece in &pieces {
            let tw = text::measure(piece, FOOTNOTE, Weight::Regular);
            if ctx.sel {
                let pr = Rect::new(cx - tw / 2 - 4, ly - 1, tw + 8, lh + 2);
                c.fill_rrect(
                    pr,
                    4,
                    Corner::Circle,
                    appui::selection_fill(ctx.focused),
                    256,
                );
            }
            let col = if ctx.sel {
                appui::selection_text(ctx.focused)
            } else {
                theme::text()
            };
            text::draw_a(
                c,
                cx - tw / 2,
                ly,
                piece,
                FOOTNOTE,
                Weight::Regular,
                col,
                a as u16,
            );
            ly += lh + 1;
        }
    }
}
