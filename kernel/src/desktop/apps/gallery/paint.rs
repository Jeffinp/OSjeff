//! Drawing the gallery's controls page.

use super::sections::gallery_colors;
use super::sections::gallery_icons;
use super::sections::gallery_type;
use super::state::Layout;
use super::state::TABS;
use super::state::layout;
use crate::desktop::apps::gallery::GalleryState;
use crate::desktop::kit::ui::{ButtonKind, Control};
use crate::desktop::*;
use crate::text::{self, BODY, CAPTION, FOOTNOTE, Weight};
use kitsune_core::style::Palette;
use kitsune_core::t;

impl Desktop {
    pub(crate) fn draw_gallery(&self, c: &mut Canvas, r: Rect, g: &GalleryState) {
        let p = theme::pal();
        let body = r.body();
        c.fill_rect(
            body.x.max(0) as usize,
            body.y.max(0) as usize,
            body.w.max(0) as usize,
            body.h.max(0) as usize,
            theme::solid(p.window_bg),
        );
        let l = layout(body);
        let tabs = TABS.map(kitsune_core::i18n::tr);
        ui::segmented(c, l.tabs, &tabs, g.tab);
        let pad = 24;
        match g.tab {
            0 => self.gallery_controls(c, body, g, &l, p),
            1 => gallery_type(c, body, pad, p),
            2 => gallery_colors(c, body, pad, p),
            3 => gallery_icons(c, body, pad, p),
            _ => self.gallery_shell(c, body, pad, p),
        }
    }

    fn gallery_controls(
        &self,
        c: &mut Canvas,
        body: Rect,
        g: &GalleryState,
        l: &Layout,
        p: &Palette,
    ) {
        let x = body.x + 24;
        let mut by = body.y + 60;
        // Buttons in their states.
        for (i, (label, kind, st)) in [
            (
                t!("kit.btn.default"),
                ButtonKind::Secondary,
                Control::Normal,
            ),
            (t!("kit.btn.primary"), ButtonKind::Primary, Control::Normal),
            (
                t!("kit.btn.destructive"),
                ButtonKind::Destructive,
                Control::Normal,
            ),
            (
                t!("kit.btn.pressed"),
                ButtonKind::Secondary,
                Control::Pressed,
            ),
            (
                t!("kit.state.disabled"),
                ButtonKind::Primary,
                Control::Disabled,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let bw = if i == 3 || i == 4 { 108 } else { 84 };
            let off = [0, 92, 184, 276, 392][i];
            ui::push_button(c, Rect::new(x + off, by, bw, 28), label, kind, st);
        }
        by += 40;
        let _ = by;
        ui::segmented(
            c,
            l.segmented,
            &[t!("kit.seg.day"), t!("kit.seg.week"), t!("kit.seg.month")],
            g.segment,
        );
        ui::switch(c, l.switch, if g.switch_on { 256 } else { 0 }, true);
        ui::slider(c, l.slider, g.slider, 0, 100, true);
        let v = alloc::format!("{}", g.slider);
        text::draw_left(
            c,
            Rect::new(l.slider.right() + 12, l.slider.y, 40, l.slider.h),
            &v,
            BODY,
            Weight::Regular,
            theme::solid(p.text_secondary),
        );
        for (i, (cr, label)) in l
            .checks
            .iter()
            .zip([t!("kit.check.remember"), t!("kit.check.notify")])
            .enumerate()
        {
            ui::checkbox(c, cr.x, cr.y + 2, g.checks[i]);
            text::draw_left(
                c,
                Rect::new(cr.x + 24, cr.y, cr.w - 24, cr.h),
                label,
                BODY,
                Weight::Regular,
                theme::solid(p.text),
            );
        }
        for (i, (rr, label)) in l
            .radios
            .iter()
            .zip([
                t!("kit.radio.first"),
                t!("kit.radio.second"),
                t!("kit.radio.third"),
            ])
            .enumerate()
        {
            ui::radio(c, rr.x, rr.y + 2, g.radio == i);
            text::draw_left(
                c,
                Rect::new(rr.x + 24, rr.y, rr.w - 24, rr.h),
                label,
                BODY,
                Weight::Regular,
                theme::solid(p.text),
            );
        }
        ui::text_field(
            c,
            l.field,
            &g.field,
            t!("kit.field"),
            g.field_focus,
            g.field_focus,
        );
        // Progress, below the field.
        let py = l.field.bottom() + 24;
        ui::progress(c, Rect::new(x, py, 300, 6), 640);
        // Right column: a grouped list, a menu preview and a tooltip.
        ui::group_box(c, l.list);
        for (i, label) in [
            t!("launcher.recents"),
            t!("kit.list.documents"),
            t!("kit.list.images"),
            t!("kit.list.downloads"),
        ]
        .iter()
        .enumerate()
        {
            let row = Rect::new(l.list.x + 4, l.list.y + 6 + i as i32 * 30, l.list.w - 8, 30);
            ui::list_row(c, row, g.list_sel == i, false, label);
        }
        let mx = l.list.x;
        let my = l.list.bottom() + 20;
        let menu = Rect::new(mx, my, 220, 24 * 3 + 12);
        c.draw_shadow(
            menu,
            Shadow {
                blur: 12,
                dy: 8,
                alpha: 70,
            },
            Rect::new(menu.x, menu.y + 8, menu.w, menu.h - 16),
        );
        ui::fill_token(c, menu, kitsune_core::style::R_MENU + 2, p.menu_tint);
        ui::stroke_token(c, menu, kitsune_core::style::R_MENU + 2, p.separator);
        ui::menu_item(
            c,
            Rect::new(menu.x + 6, menu.y + 6, menu.w - 12, 24),
            t!("menu.file.new_window"),
            "Ctrl+N",
            true,
            true,
            false,
        );
        ui::menu_item(
            c,
            Rect::new(menu.x + 6, menu.y + 30, menu.w - 12, 24),
            t!("kit.menu.show_bar"),
            "",
            false,
            true,
            true,
        );
        ui::menu_item(
            c,
            Rect::new(menu.x + 6, menu.y + 54, menu.w - 12, 24),
            t!("kit.state.disabled"),
            "",
            false,
            false,
            false,
        );
        ui::tooltip(c, mx + 60, my + menu.h + 44, t!("kit.tooltip"));
        let _ = (CAPTION, FOOTNOTE);
    }
}
