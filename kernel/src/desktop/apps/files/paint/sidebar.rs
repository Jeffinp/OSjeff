//! The places sidebar.

use super::helpers::argb;
use super::helpers::tertiary;
use crate::desktop::kit::appui::{self};
use crate::desktop::*;
use crate::text::{self, BODY, FOOTNOTE, Weight};
use kitsune_core::appart::Tool;
use kitsune_core::fileman::ui::{self as fui, Layout, SideLayout};
use kitsune_core::fileman::{self, Place};
use kitsune_core::t;

impl Desktop {
    // ------------------------------------------------------------------ sidebar

    pub(super) fn draw_files_sidebar(
        &self,
        c: &mut Canvas,
        lay: &Layout,
        st: &FilesState,
        focused: bool,
    ) {
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
}
