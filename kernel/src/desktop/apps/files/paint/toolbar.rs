//! The toolbar: navigation, breadcrumbs, view and sort buttons, search.

use super::super::labels::crumbs_of;
use super::helpers::argb;
use super::helpers::tertiary;
use crate::desktop::kit::appui::{self};
use crate::desktop::*;
use crate::text::{self, BODY, Weight};
use kitsune_core::appart::Tool;
use kitsune_core::fileman::ui::{self as fui, Layout};
use kitsune_core::t;

impl Desktop {
    // ------------------------------------------------------------------ toolbar

    pub(super) fn draw_files_toolbar(
        &self,
        c: &mut Canvas,
        lay: &Layout,
        st: &FilesState,
        _focused: bool,
    ) {
        let hover_lvl = appui::level(&st.hover_t);
        let hv = |h: fui::Hit| -> u32 { if st.hover == Some(h) { hover_lvl } else { 0 } };
        appui::tool_button(
            c,
            lay.back,
            Tool::ChevronLeft,
            st.view.history.can_back(),
            hv(fui::Hit::Back),
            false,
            false,
        );
        appui::tool_button(
            c,
            lay.forward,
            Tool::ChevronRight,
            st.view.history.can_forward(),
            hv(fui::Hit::Forward),
            false,
            false,
        );
        // The path bar.
        if lay.path.w > 0 {
            appui::pill(c, lay.path);
            let (crumbs, labels, widths) = crumbs_of(&st.view.cwd);
            let cl = fui::crumb_layout(lay.path, &widths);
            let n = crumbs.len();
            let saved = c.set_clip(
                lay.path
                    .intersection(&c.clip_rect())
                    .unwrap_or(Rect::new(0, 0, 0, 0)),
            );
            let drop_crumb = match &st.gesture {
                Gesture::Drag(d) => match d.over {
                    DropHover::Crumb(i) => Some(i),
                    _ => None,
                },
                _ => None,
            };
            if let Some(f) = cl.fold {
                text::draw_centered(c, f, "…", BODY, Weight::Regular, theme::text_muted());
            }
            for (k, (i, rect)) in cl.spans.iter().enumerate() {
                let last = *i + 1 == n;
                let hovered = st.hover == Some(fui::Hit::Crumb(*i));
                if drop_crumb == Some(*i) {
                    c.fill_rrect(*rect, 5, Corner::Circle, theme::accent(), 80);
                } else if hovered && !last {
                    let (col, a) = theme::tint(theme::pal().hover);
                    c.fill_rrect(*rect, 5, Corner::Circle, col, a);
                }
                let mut x = rect.x + fui::CRUMB_PAD;
                if *i == 0 {
                    appui::blit_tool_dim(
                        c,
                        Tool::Disk,
                        x,
                        rect.y + (rect.h - 14) / 2,
                        14,
                        argb(theme::accent()),
                        256,
                    );
                    x += 20;
                }
                let (wt, col) = if last {
                    (Weight::Medium, theme::text())
                } else if hovered {
                    (Weight::Regular, theme::text())
                } else {
                    (Weight::Regular, theme::text_muted())
                };
                let room = (rect.right() - fui::CRUMB_PAD - x).max(0);
                text::draw_ellipsis(
                    c,
                    x,
                    text::center_y(rect.y, rect.h, BODY, wt),
                    room,
                    &labels[*i],
                    BODY,
                    wt,
                    col,
                );
                if k + 1 < cl.spans.len() {
                    appui::blit_tool_dim(
                        c,
                        Tool::ChevronRight,
                        rect.right() + (fui::CRUMB_GAP - 8) / 2,
                        rect.y + (rect.h - 8) / 2,
                        8,
                        argb(tertiary()),
                        256,
                    );
                }
            }
            c.restore_clip(saved);
        }
        if lay.view_switch.w > 0 {
            appui::tool_segmented(
                c,
                lay.view_switch,
                &[Tool::ViewList, Tool::ViewGrid],
                st.mode.index(),
            );
        }
        if lay.sort.w > 0 {
            appui::tool_button(
                c,
                lay.sort,
                Tool::Sort,
                true,
                hv(fui::Hit::SortButton),
                false,
                false,
            );
        }
        if lay.search_is_field() {
            let text = st.search.input.to_string_lossy();
            appui::field(
                c,
                lay.search,
                &appui::FieldText {
                    text: &text,
                    caret: st.search.input.caret(),
                    selection: st.search.input.selection(),
                },
                t!("files.search"),
                st.search.focused,
                if st.search.focused {
                    appui::caret_alpha(st.search.last_input)
                } else {
                    0
                },
                Some(Tool::Search),
                !text.is_empty(),
            );
        } else {
            appui::tool_button(
                c,
                lay.search,
                Tool::Search,
                true,
                hv(fui::Hit::Search),
                false,
                false,
            );
        }
        appui::tool_button(
            c,
            lay.preview_btn,
            Tool::Eye,
            true,
            hv(fui::Hit::PreviewButton),
            false,
            st.preview_open,
        );
    }
}
