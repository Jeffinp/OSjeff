//! Drawing the Ajustes window.

use super::builder::Ui;
use super::pages::page;
use super::state::A_SEC;
use super::state::DOWN;
use super::state::SECTIONS;
use super::state::SIDE_W;
use super::state::pane_of;
use super::state::side_rect;
use super::state::*;
use crate::desktop::kit;
use crate::desktop::kit::ui::{self};
use crate::desktop::*;
use crate::text::{self, BODY, Weight};
use kitsune_core::i18n::{self};

impl Desktop {
    pub(crate) fn draw_settings(&self, c: &mut Canvas, r: Rect, st: &SettingsState) {
        let p = theme::pal();
        let body = r.body();
        // The sidebar.
        ui::fill(
            c,
            Rect::new(body.x, body.y, SIDE_W, body.h),
            theme::sidebar(),
        );
        ui::separator(c, Rect::new(body.x + SIDE_W - 1, body.y, 1, body.h));
        let hv = st.hover.get() & !DOWN;
        for (i, (name, glyph, rgb)) in SECTIONS.iter().enumerate() {
            let rr = side_rect(r, i);
            let selected = st.section as usize == i;
            let fg = if selected {
                c.fill_rrect(rr, 8, Corner::Circle, theme::accent(), 256);
                theme::ACCENT_TEXT
            } else {
                if hv == A_SEC + i as u32 {
                    ui::fill_token(c, rr, 8, p.hover);
                }
                kit::ink()
            };
            let badge = Rect::new(rr.x + 8, rr.y + 5, 22, 22);
            let col = Color::rgb((*rgb >> 16) as u8, (*rgb >> 8) as u8, *rgb as u8);
            c.fill_rrect(badge, 6, Corner::Circle, col, 256);
            ui::draw_glyph(c, *glyph, badge.x + 3, badge.y + 3, 16, 0xFFFF_FFFF);
            text::draw_left(
                c,
                Rect::new(badge.right() + 10, rr.y, rr.w - 50, rr.h),
                i18n::tr(name),
                BODY,
                if selected {
                    Weight::Medium
                } else {
                    Weight::Regular
                },
                fg,
            );
        }
        // The page.
        let pane = pane_of(r);
        let saved = kit::clip_to(c, pane);
        let mut ui = Ui::new(Some(c), pane, st, None);
        page(&mut ui, self);
        let c = ui.c.take().expect("canvas");
        c.restore_clip(saved);
        let total = st.content_h.get().max(1);
        let rows = pane.h.max(1);
        ui::overlay_scrollbar(
            c,
            Rect::new(pane.right() - 10, pane.y + 4, 10, pane.h - 8),
            st.scroll.value().max(0) as usize,
            total as usize,
            rows as usize,
            st.sb.alpha(crate::desktop::shell::toasts::now_ms()),
        );
    }
}
