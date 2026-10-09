//! `Desktop::draw_files`: the Arquivos window. Pure drawing: the state is `FilesState` (rows
//! already loaded: no disk access happens while painting) and the geometry is
//! `osjeff_core::fileman::ui`, shared with the mouse handler.
//!
//! Layout: a translucent-looking sidebar (Favoritos, Locais, the disk with its usage bar), a
//! toolbar (back and forward, the breadcrumb path bar, the view switch, sort, search and the
//! preview toggle), the list or the icon grid on the content colour with overlay scrollbar,
//! an optional preview pane, a status bar and sheets attached to the window. Text is
//! measured, never counted in columns.

use super::appui::{self, EmptyIcon};
use super::files::{SheetKind, crumbs_of, files_sheet_kind, modified_label};
use super::ui::ButtonKind;
use super::*;
use crate::text::{self, BODY, CALLOUT, CAPTION, FOOTNOTE, Weight};
use osjeff_core::appart::{FileKind, Tool};
use osjeff_core::fileman::ui::{self as fui, Columns, Layout, SideLayout, ViewMode};
use osjeff_core::fileman::{self, Place, SortKey};
use osjeff_core::{t, tp};

fn argb(c: Color) -> u32 {
    appui::rgb_of(c)
}

/// `color` at `a` (0..=256) as a blend of text colour: used for dim text.
fn alpha_of(a: u32, base: u32) -> u32 {
    (a * base / 256).min(256)
}

fn tertiary() -> Color {
    theme::solid(theme::pal().text_tertiary)
}

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

    // ------------------------------------------------------------------ sidebar

    fn draw_files_sidebar(&self, c: &mut Canvas, lay: &Layout, st: &FilesState, focused: bool) {
        let p = theme::pal();
        let sb = lay.sidebar;
        // A veil of the accent that fades into the sidebar colour: the stand-in for a live
        // blur (a window is composited opaque, so there is no backdrop to show through).
        let base = theme::sidebar();
        let top = base.lerp(theme::accent(), if theme::dark() { 26 } else { 30 });
        c.fill_rrect_vgrad(sb, 0, Corner::Circle, top, base, 256);
        ui::fill_token(c, Rect::new(sb.right() - 1, sb.y, 1, sb.h), 0, p.separator);
        let side = SideLayout::of(sb);
        for (title, r) in [
            (t!("files.side.favorites"), side.favorites_title),
            (t!("files.side.places"), side.places_title),
        ] {
            text::draw_left(c, r, title, FOOTNOTE, Weight::Semibold, tertiary());
        }
        let current = Place::of_path(&st.view.cwd);
        let hover_place = match st.hover {
            Some(fui::Hit::Place(pl)) => Some(pl),
            _ => None,
        };
        let drop_place = match &st.gesture {
            Gesture::Drag(d) => match d.over {
                DropHover::Place(pl) => Some(pl),
                _ => None,
            },
            _ => None,
        };
        for (place, rect) in side.items {
            let is_disk = place == Place::Disk;
            let row = if is_disk {
                Rect::new(rect.x, rect.y, rect.w, 28)
            } else {
                rect
            };
            let active = place == current;
            let acc = theme::accent();
            if active {
                let (col, a) = if focused {
                    (acc, 44u16)
                } else {
                    (theme::text_muted(), 36)
                };
                c.fill_rrect(row, 6, Corner::Circle, col, a);
            } else if hover_place == Some(place) {
                let (col, a) = theme::tint(p.hover);
                let a = (a as u32 * appui::level(&st.hover_t) / 256) as u16;
                c.fill_rrect(row, 6, Corner::Circle, col, a);
            }
            if drop_place == Some(place) {
                c.fill_rrect(row, 6, Corner::Circle, acc, 70);
                c.stroke_rrect(row, 6, Corner::Circle, acc, 256);
            }
            let tool = match place {
                Place::Home => Tool::Home,
                Place::Documents => Tool::Folder,
                Place::Images => Tool::Photo,
                Place::Apps => Tool::Apps,
                Place::Trash => Tool::Trash,
                Place::Disk => Tool::Disk,
            };
            let icon_col = argb(acc);
            appui::blit_tool_dim(
                c,
                tool,
                row.x + 10,
                row.y + (row.h - 16) / 2,
                16,
                icon_col,
                if active { 256 } else { 210 },
            );
            let label = match place {
                Place::Home => t!("files.place.home"),
                Place::Documents => t!("files.place.documents"),
                Place::Images => t!("files.place.images"),
                Place::Apps => t!("files.place.apps"),
                Place::Trash => t!("files.place.trash"),
                Place::Disk => {
                    if vfs::volume() == vfs::Volume::Memory {
                        t!("files.place.memory")
                    } else {
                        t!("files.place.disk")
                    }
                }
            };
            let tcol = if active {
                theme::text()
            } else {
                theme::solid(p.text)
            };
            text::draw_left(
                c,
                Rect::new(row.x + 34, row.y, row.w - 40, row.h),
                label,
                BODY,
                if active {
                    Weight::Medium
                } else {
                    Weight::Regular
                },
                tcol,
            );
            if is_disk {
                let used = st.usage.used_permille();
                let bar = Rect::new(rect.x + 12, rect.y + 34, rect.w - 24, 5);
                let col = if used > 900 {
                    theme::danger()
                } else {
                    theme::accent()
                };
                ui::fill_token(
                    c,
                    bar,
                    3,
                    if theme::dark() {
                        0x33FF_FFFF
                    } else {
                        0x1F00_0000
                    },
                );
                let w = (bar.w as i64 * used as i64 / 1000) as i32;
                if w > 0 {
                    c.fill_rrect(
                        Rect::new(bar.x, bar.y, w.max(5), bar.h),
                        3,
                        Corner::Circle,
                        col,
                        256,
                    );
                }
                let free = t!(
                    "files.side.free",
                    size = &fileman::format_size(st.usage.free)
                );
                text::draw_ellipsis(
                    c,
                    rect.x + 12,
                    rect.y + 41,
                    rect.w - 24,
                    &free,
                    FOOTNOTE,
                    Weight::Regular,
                    theme::text_muted(),
                );
            }
        }
    }

    // ------------------------------------------------------------------ toolbar

    fn draw_files_toolbar(&self, c: &mut Canvas, lay: &Layout, st: &FilesState, _focused: bool) {
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

    // ------------------------------------------------------------------- header

    fn draw_files_header(&self, c: &mut Canvas, lay: &Layout, st: &FilesState) {
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

    // -------------------------------------------------------------------- items

    fn draw_files_items(&self, c: &mut Canvas, lay: &Layout, st: &FilesState, focused: bool) {
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
                appart::blit_file(c, FileKind::App, ix, iy, 20, a);
            }
        } else if st.view.in_apps() {
            appart::blit_file(c, FileKind::App, ix, iy, 20, a / 2);
        } else {
            appart::blit_file(c, fui::icon_kind(&row.name, row.is_dir()), ix, iy, 20, a);
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
                appart::blit_file(c, FileKind::App, cx - 24, rect.y + 8, 48, a);
            }
        } else {
            let kind = if st.view.in_apps() {
                FileKind::App
            } else {
                kind
            };
            appart::blit_file(c, kind, cx - 24, rect.y + 8, 48, a);
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

    // ------------------------------------------------------------------ preview

    fn draw_files_preview(&self, c: &mut Canvas, pane: Rect, st: &FilesState) {
        let p = theme::pal();
        ui::fill(c, pane, theme::sidebar());
        ui::fill_token(c, Rect::new(pane.x, pane.y, 1, pane.h), 0, p.separator);
        let saved = c.set_clip(
            pane.intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        );
        let inner = Rect::new(pane.x + 16, pane.y + 16, pane.w - 32, pane.h - 32);
        let Some(d) = st.preview.as_ref() else {
            c.restore_clip(saved);
            return;
        };
        if d.name.is_empty() {
            text::draw_centered(
                c,
                Rect::new(pane.x, pane.y, pane.w, pane.h),
                t!("files.preview.select"),
                BODY,
                Weight::Regular,
                tertiary(),
            );
            c.restore_clip(saved);
            return;
        }
        let mut y = inner.y;
        // Picture area.
        let box_h = 156;
        let card = Rect::new(inner.x, y, inner.w, box_h);
        if let Some(img) = &d.image {
            ui::fill_token(c, card, 8, p.content_bg);
            let ix = card.x + (card.w - img.w as i32) / 2;
            let iy = card.y + (card.h - img.h as i32) / 2;
            let clip = Rect::new(ix, iy, img.w as i32, img.h as i32);
            let saved2 = c.set_clip(
                clip.intersection(&c.clip_rect())
                    .unwrap_or(Rect::new(0, 0, 0, 0)),
            );
            c.blit_surface(img, ix, iy, 256);
            c.restore_clip(saved2);
            ui::stroke_token(c, card, 8, p.separator);
        } else if !d.lines.is_empty() {
            ui::fill_token(c, card, 8, p.content_bg);
            ui::stroke_token(c, card, 8, p.separator);
            let saved2 = c.set_clip(
                Rect::new(card.x + 1, card.y + 1, card.w - 2, card.h - 2)
                    .intersection(&c.clip_rect())
                    .unwrap_or(Rect::new(0, 0, 0, 0)),
            );
            let lh = 11;
            for (k, l) in d.lines.iter().take(12).enumerate() {
                text::draw_mono(
                    c,
                    card.x + 8,
                    card.y + 8 + k as i32 * lh,
                    l,
                    9,
                    theme::text_muted(),
                );
            }
            c.restore_clip(saved2);
        } else {
            appart::blit_file(
                c,
                d.kind,
                card.x + (card.w - 64) / 2,
                card.y + (card.h - 64) / 2,
                64,
                256,
            );
        }
        y = card.bottom() + 14;
        // Title and kind.
        let lines = text::wrap(&d.name, CALLOUT, Weight::Semibold, inner.w, 2);
        for (s, e) in &lines {
            let piece = d.name[*s..*e].trim_end();
            let tw = text::measure(piece, CALLOUT, Weight::Semibold);
            text::draw(
                c,
                inner.x + (inner.w - tw.min(inner.w)) / 2,
                y,
                &text::ellipsize(piece, CALLOUT, Weight::Semibold, inner.w),
                CALLOUT,
                Weight::Semibold,
                theme::text(),
            );
            y += text::line_height(CALLOUT) + 1;
        }
        let kw = text::measure(&d.kind_label, FOOTNOTE, Weight::Regular);
        text::draw(
            c,
            inner.x + (inner.w - kw.min(inner.w)) / 2,
            y + 2,
            &text::ellipsize(&d.kind_label, FOOTNOTE, Weight::Regular, inner.w),
            FOOTNOTE,
            Weight::Regular,
            theme::text_muted(),
        );
        y += 26;
        if let Some(note) = &d.note {
            text::draw_ellipsis(
                c,
                inner.x,
                y,
                inner.w,
                note,
                FOOTNOTE,
                Weight::Regular,
                tertiary(),
            );
            y += 22;
        }
        appui::hairline(c, inner.x, y, inner.w);
        y += 8;
        for (label, value) in &d.info {
            let lw = text::measure(label, FOOTNOTE, Weight::Regular) + 8;
            text::draw(c, inner.x, y, label, FOOTNOTE, Weight::Regular, tertiary());
            let room = (inner.w - lw).max(10);
            let v = text::ellipsize(value, FOOTNOTE, Weight::Regular, room);
            let vw = text::measure(&v, FOOTNOTE, Weight::Regular);
            text::draw(
                c,
                inner.right() - vw,
                y,
                &v,
                FOOTNOTE,
                Weight::Regular,
                theme::text(),
            );
            y += 22;
        }
        c.restore_clip(saved);
    }

    // ------------------------------------------------------------------- status

    fn draw_files_status(&self, c: &mut Canvas, lay: &Layout, st: &FilesState) {
        let s = lay.status;
        appui::hairline(c, s.x, s.y, s.w);
        let ty = text::center_y(s.y, s.h, FOOTNOTE, Weight::Regular);
        let mut summary = st.view.summary();
        if !st.view.filter().is_empty() {
            summary = tp!(
                "files.filtered",
                st.view.total_rows(),
                shown = st.view.rows.len()
            );
        }
        let w = text::draw(
            c,
            s.x + 16,
            ty,
            &summary,
            FOOTNOTE,
            Weight::Regular,
            theme::text_muted(),
        );
        if let Some((m, err)) = &st.msg {
            let x = s.x + 16 + w + 14;
            let col = if *err { theme::danger() } else { theme::ok() };
            text::draw_ellipsis(
                c,
                x,
                ty,
                (s.right() - 16 - x).max(0),
                m,
                FOOTNOTE,
                Weight::Medium,
                col,
            );
        } else if st.view.in_apps() {
            let hint = t!("files.hint.apps");
            let x = s.x + 16 + w + 14;
            text::draw_ellipsis(
                c,
                x,
                ty,
                (s.right() - 16 - x).max(0),
                hint,
                FOOTNOTE,
                Weight::Regular,
                tertiary(),
            );
        }
    }

    // -------------------------------------------------------------------- sheets

    fn draw_files_sheet(&self, c: &mut Canvas, r: Rect, st: &FilesState) {
        let (kind, size) = files_sheet_kind(st);
        let t = appui::level(&st.sheet_t);
        let panel = appui::sheet(c, r, size, t);
        let labels = kind.buttons();
        let btns = appui::button_row(
            panel.right() - appui::SHEET_PAD,
            panel.bottom() - appui::SHEET_PAD - appui::BUTTON_H,
            &labels,
        );
        let hover = |b: &Rect| b.contains(self.cursor_x, self.cursor_y);
        let saved = c.set_clip(
            panel
                .intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        );
        match kind {
            SheetKind::Confirm => {
                let what = |n: usize| tp!("files.confirm.purge_body", n);
                let (title, msg) = match &st.confirm {
                    Some(Confirm::Purge(p)) => (t!("files.confirm.purge_title"), what(p.len())),
                    Some(Confirm::PurgeTrash(p)) => {
                        (t!("files.confirm.purge_trash_title"), what(p.len()))
                    }
                    _ => (
                        t!("files.confirm.empty_title"),
                        String::from(t!("files.confirm.empty_body")),
                    ),
                };
                appui::sheet_text(c, panel, title, &msg, false);
                appui::sheet_button(
                    c,
                    btns[0],
                    labels[0],
                    ButtonKind::Secondary,
                    hover(&btns[0]),
                    false,
                );
                appui::sheet_button(
                    c,
                    btns[1],
                    labels[1],
                    ButtonKind::Destructive,
                    hover(&btns[1]),
                    false,
                );
            }
            SheetKind::Info => {
                let lines = st.props.as_deref().unwrap_or(&[]);
                let p = theme::pal();
                let tw = text::TITLE3;
                text::draw(
                    c,
                    panel.x + appui::SHEET_PAD,
                    panel.y + appui::SHEET_PAD,
                    t!("files.sheet.properties"),
                    tw,
                    Weight::Semibold,
                    theme::text(),
                );
                let mut y = panel.y + appui::SHEET_PAD + text::line_height(tw) + 10;
                // The widest label sets the column (English and Portuguese labels differ).
                let label_w = lines
                    .iter()
                    .filter_map(|l| l.split_once(": "))
                    .map(|(k, _)| text::measure(k, BODY, Weight::Regular))
                    .max()
                    .unwrap_or(0)
                    .clamp(64, panel.w / 2);
                for l in lines {
                    let (k, v) = match l.split_once(": ") {
                        Some((k, v)) => (k, v),
                        None => ("", l.as_str()),
                    };
                    let row = Rect::new(
                        panel.x + appui::SHEET_PAD,
                        y,
                        panel.w - 2 * appui::SHEET_PAD,
                        24,
                    );
                    text::draw_right(
                        c,
                        Rect::new(row.x, row.y, label_w, row.h),
                        k,
                        BODY,
                        Weight::Regular,
                        theme::solid(p.text_secondary),
                    );
                    let vx = row.x + label_w + 12;
                    let shown = text::ellipsize_middle(v, BODY, Weight::Regular, row.right() - vx);
                    text::draw(
                        c,
                        vx,
                        text::center_y(row.y, row.h, BODY, Weight::Regular),
                        &shown,
                        BODY,
                        Weight::Regular,
                        theme::text(),
                    );
                    y += 24;
                }
                appui::sheet_button(
                    c,
                    btns[1],
                    labels[1],
                    ButtonKind::Primary,
                    hover(&btns[1]),
                    false,
                );
            }
            SheetKind::Copy => {
                let (title, name, pm) = match &st.job {
                    Some(j) => {
                        let (n, _) = j.copy.files();
                        let label = if n > 1 {
                            tp!("files.job.copying_n", n)
                        } else {
                            String::from(osjeff_core::i18n::tr(j.label))
                        };
                        (
                            label,
                            String::from_utf8_lossy(j.copy.current_name()).into_owned(),
                            j.copy.permille(),
                        )
                    }
                    None => (String::new(), String::new(), 0),
                };
                text::draw(
                    c,
                    panel.x + appui::SHEET_PAD,
                    panel.y + appui::SHEET_PAD,
                    &title,
                    text::TITLE3,
                    Weight::Semibold,
                    theme::text(),
                );
                let y = panel.y + appui::SHEET_PAD + text::line_height(text::TITLE3) + 8;
                text::draw_ellipsis(
                    c,
                    panel.x + appui::SHEET_PAD,
                    y,
                    panel.w - 2 * appui::SHEET_PAD,
                    &name,
                    BODY,
                    Weight::Regular,
                    theme::text_muted(),
                );
                let bar = Rect::new(
                    panel.x + appui::SHEET_PAD,
                    y + 28,
                    panel.w - 2 * appui::SHEET_PAD - 44,
                    6,
                );
                ui::progress(c, bar, pm);
                let pct = alloc::format!("{}%", pm / 10);
                text::draw_right(
                    c,
                    Rect::new(bar.right() + 8, bar.y - 6, 36, 18),
                    &pct,
                    FOOTNOTE,
                    Weight::Medium,
                    theme::text_muted(),
                );
                appui::sheet_button(
                    c,
                    btns[1],
                    labels[1],
                    ButtonKind::Secondary,
                    hover(&btns[1]),
                    false,
                );
            }
        }
        c.restore_clip(saved);
    }
}

/// Width of the size column's text area.
fn ui_size_w(cols: &Columns) -> i32 {
    ((if cols.has_date() {
        cols.date_x
    } else {
        cols.right
    }) - cols.size_x
        - 12)
        .max(40)
}

/// How one item is to be drawn.
struct ItemCtx {
    sel: bool,
    hover: u32,
    cut: bool,
    dropping: bool,
    focused: bool,
    alpha: u32,
    editing: bool,
}

/// The inline rename field (a white field with an accent ring, the stem selected).
fn draw_rename_field(c: &mut Canvas, r: Rect, e: &NameEdit) {
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
fn draw_drag_ghost(c: &mut Canvas, d: &DragState, ctrl: bool) {
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
    appart::blit_file(
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
