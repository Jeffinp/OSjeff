//! The tab strip.

use super::helpers::argb;
use super::helpers::letter_badge;
use crate::desktop::BrowserHover as H;
use crate::desktop::*;
use crate::text::{self, BODY, Weight};
use kitsune_core::browser::tabs as tabs_model;
use kitsune_core::iconart::Glyph;
use kitsune_core::layout as geo;
use kitsune_core::t;

impl Desktop {
    // ---- tab strip ----

    pub(super) fn draw_tab_strip(
        &self,
        c: &mut Canvas,
        ch: &geo::BrowserChrome,
        bs: &BrowserState,
    ) {
        let p = theme::pal();
        let strip = ch.strip;
        if strip.h < 8 {
            return;
        }
        let saved = c.set_clip(
            strip
                .intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        );
        c.fill_rect(
            strip.x.max(0) as usize,
            strip.y.max(0) as usize,
            strip.w.max(0) as usize,
            strip.h.max(0) as usize,
            theme::solid(p.sidebar_bg),
        );
        let rects = geo::browser_tab_rects(strip, &bs.strip_weights());
        let active = bs.tabs.active_index();
        let slots = bs.strip_tab_slots();
        for (ei, e) in bs.strip.iter().enumerate() {
            let r = rects[ei];
            if r.w < 12 {
                continue;
            }
            let ti = slots.iter().find(|(x, _)| *x == ei).map(|(_, t)| *t);
            let is_active = ti == Some(active);
            let hovered =
                ti.is_some_and(|ti| bs.hover == H::Tab(ti) || bs.hover == H::TabClose(ti));
            // Where the tab sits vertically follows the strip while it grows.
            let tab_saved = c.set_clip(
                r.intersection(&c.clip_rect())
                    .unwrap_or(Rect::new(0, 0, 0, 0)),
            );
            if is_active {
                c.draw_shadow(
                    r,
                    Shadow {
                        blur: 4,
                        dy: 1,
                        alpha: if theme::dark() { 90 } else { 40 },
                    },
                    Rect::new(r.x, r.y + 4, r.w, (r.h - 8).max(0)),
                );
                c.fill_rrect(r, 8, Corner::Circle, theme::button_bg(), 256);
                ui::stroke_token(c, r, 8, p.control_border);
            } else if hovered {
                ui::fill_token(c, r, 8, p.hover);
            }
            let (title, badge, seed) = match ti.and_then(|i| bs.tabs.get(i)) {
                Some(t) => {
                    let url = String::from_utf8_lossy(t.browser.nav_url()).into_owned();
                    let url = if t.browser.is_home() {
                        String::new()
                    } else {
                        url
                    };
                    (
                        tabs_model::tab_title(t.browser.page_title(), &url),
                        tabs_model::tab_badge(t.browser.page_title(), &url),
                        String::from(tabs_model::host_of(&url)),
                    )
                }
                None => (e.title.clone(), e.badge, String::new()),
            };
            let fresh = badge == '\u{2022}' || title == t!("web.tab.new");
            let bx = Rect::new(r.x + 8, r.y + (r.h - 16) / 2, 16, 16);
            if fresh {
                ui::draw_glyph(
                    c,
                    Glyph::Plus,
                    bx.x,
                    bx.y,
                    16,
                    argb(theme::solid(p.text_tertiary), 255),
                );
            } else {
                letter_badge(c, bx, badge, &seed, false);
            }
            let show_close = is_active || hovered;
            let close_w = if show_close { 26 } else { 8 };
            let tx = bx.right() + 8;
            let tw = (r.right() - close_w - tx).max(0);
            if tw > 8 {
                text::draw_ellipsis(
                    c,
                    tx,
                    text::center_y(r.y, r.h, BODY, Weight::Medium),
                    tw,
                    &title,
                    BODY,
                    if is_active {
                        Weight::Medium
                    } else {
                        Weight::Regular
                    },
                    if is_active {
                        theme::solid(p.text)
                    } else {
                        theme::solid(p.text_secondary)
                    },
                );
            }
            if show_close && ti.is_some() {
                let x = geo::browser_tab_close(r);
                if ti.is_some_and(|ti| bs.hover == H::TabClose(ti)) {
                    ui::fill_token(c, x, 6, p.hover);
                }
                ui::draw_glyph(
                    c,
                    Glyph::Close,
                    x.x + 5,
                    x.y + 5,
                    10,
                    argb(theme::solid(p.text_secondary), 255),
                );
            }
            c.restore_clip(tab_saved);
        }
        c.restore_clip(saved);
    }
}
