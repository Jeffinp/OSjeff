//! The column header of the list view.

use super::helpers::argb;
use super::helpers::ui_size_w;
use crate::desktop::kit::appui::{self};
use crate::desktop::*;
use crate::text::{self, FOOTNOTE, Weight};
use kitsune_core::appart::Tool;
use kitsune_core::fileman::SortKey;
use kitsune_core::fileman::ui::Layout;
use kitsune_core::t;

impl Desktop {
    // ------------------------------------------------------------------- header

    pub(super) fn draw_files_header(&self, c: &mut Canvas, lay: &Layout, st: &FilesState) {
        let h = lay.header;
        let cols = lay.columns();
        let date_title = if st.view.in_trash() {
            t!("files.col.deleted")
        } else if st.view.in_apps() {
            t!("files.col.state")
        } else {
            t!("files.col.modified")
        };
        appui::hairline(c, h.x, h.bottom() - 1, h.w);
        let sort = st.view.sort;
        let draw_title =
            |c: &mut Canvas, x: i32, w: i32, title: &str, key: SortKey, right: bool| {
                let active = sort.key == key;
                let wt = if active {
                    Weight::Medium
                } else {
                    Weight::Regular
                };
                let col = if active {
                    theme::text()
                } else {
                    theme::text_muted()
                };
                let tw = text::measure(title, FOOTNOTE, wt);
                let arrow = if active { 14 } else { 0 };
                let tx = if right { x + w - tw - arrow } else { x };
                text::draw(
                    c,
                    tx,
                    text::center_y(h.y, h.h - 1, FOOTNOTE, wt),
                    title,
                    FOOTNOTE,
                    wt,
                    col,
                );
                if active {
                    appui::blit_tool_dim(
                        c,
                        if sort.asc {
                            Tool::ChevronUp
                        } else {
                            Tool::ChevronDown
                        },
                        tx + tw + 4,
                        h.y + (h.h - 1 - 8) / 2,
                        8,
                        argb(theme::accent()),
                        256,
                    );
                }
            };
        draw_title(
            c,
            cols.name_x + 28,
            cols.size_x - cols.name_x - 28,
            t!("files.col.name"),
            SortKey::Name,
            false,
        );
        draw_title(
            c,
            cols.size_x,
            ui_size_w(&cols),
            t!("files.col.size"),
            SortKey::Size,
            true,
        );
        if cols.has_date() {
            draw_title(
                c,
                cols.date_x,
                cols.right - cols.date_x - 12,
                date_title,
                SortKey::Modified,
                false,
            );
        }
    }
}
