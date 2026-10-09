//! What is drawn over the page: banner, scrollbar, find bar and flashes.

use super::helpers::argb;
use super::helpers::icon_button;
use crate::desktop::BrowserHover as H;
use crate::desktop::kit::glass::panel;
use crate::desktop::*;
use crate::text::{self, BODY, FOOTNOTE, Weight};
use kitsune_core::iconart::Glyph;
use kitsune_core::layout as geo;
use kitsune_core::style::R_POPOVER;
use kitsune_core::t;

impl Desktop {
    // ---- over the page ----

    /// The slim bar at the bottom of a page that is only part of the document.
    pub(super) fn draw_page_banner(&self, c: &mut Canvas, content: Rect, t: &TabData) {
        let Some(note) = t.browser.note() else { return };
        if content.h <= 40 {
            return;
        }
        let h = 32;
        let r = Rect::new(content.x, content.bottom() - h, content.w, h);
        c.fill_rect(
            r.x.max(0) as usize,
            r.y.max(0) as usize,
            r.w.max(0) as usize,
            r.h.max(0) as usize,
            Color::rgb(0xFF, 0xF1, 0xC7),
        );
        c.blend_rect(
            Rect::new(r.x, r.y, r.w, 1),
            Color::rgb(0x8A, 0x5A, 0x00),
            60,
        );
        ui::draw_glyph(
            c,
            Glyph::Info,
            r.x + 14,
            r.y + (h - 14) / 2,
            14,
            argb(Color::rgb(0x7A, 0x4B, 0x00), 255),
        );
        text::draw_ellipsis(
            c,
            r.x + 36,
            text::center_y(r.y, h, FOOTNOTE, Weight::Medium),
            r.w - 52,
            note.label(),
            FOOTNOTE,
            Weight::Medium,
            Color::rgb(0x6B, 0x3F, 0x00),
        );
    }

    /// An overlay scrollbar that fades after the page stops moving; dark over a light page,
    /// light over a dark one.
    pub(super) fn draw_page_scrollbar(
        &self,
        c: &mut Canvas,
        content: Rect,
        page: &kitsune_core::web::Page,
        t: &TabData,
    ) {
        let alpha = t.scroll_bar.alpha(crate::desktop::shell::toasts::now_ms());
        if alpha == 0 || page.height <= content.h || content.h <= 0 {
            return;
        }
        let (off, len) = wlogic::scroll_thumb(
            content.h - 8,
            page.height as usize,
            content.h as usize,
            t.scroll as usize,
            32,
        );
        let w = 6;
        let thumb = Rect::new(content.right() - w - 3, content.y + 4 + off, w, len);
        let bg = page.background;
        let luma = (u32::from(bg.0) * 30 + u32::from(bg.1) * 59 + u32::from(bg.2) * 11) / 100;
        let col = if luma > 140 {
            Color::rgb(0, 0, 0)
        } else {
            Color::rgb(0xFF, 0xFF, 0xFF)
        };
        c.fill_rrect(
            thumb,
            w / 2,
            Corner::Circle,
            col,
            (alpha * 120 / 256) as u16,
        );
    }

    pub(super) fn draw_page_find_bar(
        &self,
        c: &mut Canvas,
        content: Rect,
        bs: &BrowserState,
        focused: bool,
    ) {
        let p = theme::pal();
        let t = bs.tabs.active();
        let f = geo::browser_find_layout(content);
        panel(
            c,
            f.bar,
            R_POPOVER,
            &bs.glass[2],
            10,
            p.menu_tint,
            p.separator,
            Shadow {
                blur: 12,
                dy: 6,
                alpha: 70,
            },
            256,
        );
        ui::text_field_frame(c, f.field, 8, focused);
        let q = t.find.query();
        let ty = text::center_y(f.field.y, f.field.h, BODY, Weight::Regular);
        if q.is_empty() {
            text::draw(
                c,
                f.field.x + 10,
                ty,
                t!("web.find.placeholder"),
                BODY,
                Weight::Regular,
                theme::solid(p.text_tertiary),
            );
        } else {
            let w = text::draw_ellipsis(
                c,
                f.field.x + 10,
                ty,
                f.field.w - 20,
                q,
                BODY,
                Weight::Regular,
                theme::solid(p.text),
            );
            if focused {
                c.fill_rect(
                    (f.field.x + 11 + w).max(0) as usize,
                    (f.field.y + 6).max(0) as usize,
                    1,
                    (f.field.h - 12).max(0) as usize,
                    theme::accent(),
                );
            }
        }
        if !q.is_empty() {
            let label = if t.find.count() == 0 {
                String::from(t!("web.find.none"))
            } else {
                t!(
                    "web.find.count",
                    n = t.find.position(),
                    total = t.find.count()
                )
            };
            let col = if t.find.count() == 0 {
                theme::danger()
            } else {
                theme::solid(p.text_secondary)
            };
            text::draw_centered(c, f.count, &label, FOOTNOTE, Weight::Regular, col);
        }
        let can = t.find.count() > 0;
        icon_button(c, f.prev, Glyph::ChevronLeft, can, bs.hover == H::FindPrev);
        icon_button(c, f.next, Glyph::ChevronRight, can, bs.hover == H::FindNext);
        icon_button(c, f.close, Glyph::Close, true, bs.hover == H::FindClose);
    }

    /// The zoom pill and the notice, fading.
    pub(super) fn draw_flashes(&self, c: &mut Canvas, content: Rect, bs: &BrowserState) {
        let p = theme::pal();
        let t = bs.tabs.active();
        let pill = |c: &mut Canvas, r: Rect, label: &str, a: u32| {
            if a == 0 {
                return;
            }
            let (bg, ba) = theme::tint(p.tooltip_bg);
            c.fill_rrect(
                r,
                r.h / 2,
                Corner::Circle,
                bg,
                (u32::from(ba) * a / 256) as u16,
            );
            text::draw_centered_a(
                c,
                r,
                label,
                FOOTNOTE,
                Weight::Medium,
                theme::solid(p.tooltip_text),
                a as u16,
            );
        };
        let za = bs.zoom_flash.alpha();
        if za > 0 {
            let label = alloc::format!("{} %", t.zoom);
            pill(c, geo::browser_zoom_pill(content), &label, za);
        }
        let na = bs.notice_flash.alpha();
        if na > 0
            && let Some(msg) = &bs.notice
        {
            let w = text::measure(msg, FOOTNOTE, Weight::Medium) + 32;
            let r = Rect::new(
                content.x + (content.w - w) / 2,
                content.bottom() - 16 - 32,
                w,
                32,
            );
            pill(c, r, msg, na);
        }
    }
}
