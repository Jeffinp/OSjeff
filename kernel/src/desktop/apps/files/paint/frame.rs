//! Drawing a file manager window: the sidebar, toolbar, items, preview, status bar and sheet.

use super::helpers::draw_drag_ghost;
use crate::desktop::kit::appui::{self};
use crate::desktop::*;
use kitsune_core::fileman::ui::{self as fui, Layout};

impl Desktop {
    pub(crate) fn draw_files(&self, c: &mut Canvas, r: Rect, st: &FilesState, focused: bool) {
        let lay = Layout::of(r, st.mode, st.preview_open, st.search_is_open());
        let now_ms = appui::now_ms();
        self.draw_files_sidebar(c, &lay, st, focused);
        self.draw_files_toolbar(c, &lay, st, focused);
        let list_clip = lay
            .list
            .intersection(&c.clip_rect())
            .unwrap_or(Rect::new(0, 0, 0, 0));
        // The content colour under the header and the list.
        ui::fill(
            c,
            Rect::new(
                lay.main.x,
                lay.toolbar.bottom(),
                lay.main.w - lay.preview.map_or(0, |p| p.w),
                (lay.status.y - lay.toolbar.bottom()).max(0),
            ),
            theme::surface(),
        );
        appui::hairline(c, lay.main.x, lay.toolbar.bottom() - 1, lay.main.w);
        if lay.header.h > 0 {
            self.draw_files_header(c, &lay, st);
        }
        let saved = c.set_clip(list_clip);
        self.draw_files_items(c, &lay, st, focused);
        c.restore_clip(saved);
        // Overlay scrollbar.
        let n = st.view.rows.len();
        let content = fui::content_height(st.mode, lay.list.w, n);
        if content > lay.list.h {
            let alpha = if matches!(st.gesture, Gesture::Thumb { .. }) {
                256
            } else {
                st.scroll_fade.alpha(now_ms)
            };
            let track = Rect::new(lay.list.right() - 12, lay.list.y, 12, lay.list.h);
            ui::overlay_scrollbar(
                c,
                track,
                st.scroller.pos().max(0) as usize,
                content as usize,
                lay.list.h as usize,
                alpha,
            );
        }
        if let Some(pane) = lay.preview {
            self.draw_files_preview(c, pane, st);
        }
        self.draw_files_status(c, &lay, st);
        // The ghost of dragged items follows the pointer inside the window.
        if let Gesture::Drag(d) = &st.gesture {
            let saved = c.set_clip(
                r.intersection(&c.clip_rect())
                    .unwrap_or(Rect::new(0, 0, 0, 0)),
            );
            draw_drag_ghost(c, d, self.keymap.ctrl());
            c.restore_clip(saved);
        }
        if st.sheet_open() {
            self.draw_files_sheet(c, r, st);
        }
    }
}
