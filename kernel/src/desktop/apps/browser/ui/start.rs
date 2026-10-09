//! The new-tab page.

use super::helpers::argb;
use super::helpers::letter_badge;
use crate::desktop::BrowserHover as H;
use crate::desktop::apps::browser::hover::start_items;
use crate::desktop::*;
use crate::text::{self, BODY, CALLOUT, FOOTNOTE, Weight};
use kitsune_core::browser::tabs as tabs_model;
use kitsune_core::iconart::Glyph;
use kitsune_core::layout as geo;
use kitsune_core::t;

impl Desktop {
    // ---- new tab ----

    pub(super) fn draw_start_page(
        &self,
        c: &mut Canvas,
        content: Rect,
        bs: &BrowserState,
        focused: bool,
    ) {
        let p = theme::pal();
        let t = bs.tabs.active();
        c.fill_rect(
            content.x.max(0) as usize,
            content.y.max(0) as usize,
            content.w.max(0) as usize,
            content.h.max(0) as usize,
            theme::surface(),
        );
        let (tiles, recents) = start_items(bs);
        let l = geo::browser_start_layout(content, tiles.len(), recents.len());

        // The big search field mirrors the omnibox text.
        let s = l.search;
        let editing = focused && t.browser.bar_focus();
        ui::fill_token(c, s, s.h / 2, p.sidebar_bg);
        if editing {
            c.stroke_rrect(
                s.inflated(2),
                s.h / 2 + 2,
                Corner::Circle,
                theme::accent(),
                80,
            );
            c.stroke_rrect(s, s.h / 2, Corner::Circle, theme::accent(), 256);
        } else if bs.hover == H::Bar {
            ui::stroke_token(c, s, s.h / 2, p.control_border);
        }
        ui::draw_glyph(
            c,
            Glyph::Search,
            s.x + 18,
            s.y + (s.h - 18) / 2,
            18,
            argb(theme::solid(p.text_tertiary), 255),
        );
        let url = text::from_bytes(t.browser.url()).into_owned();
        let ty = text::center_y(s.y, s.h, CALLOUT, Weight::Regular);
        let tx = s.x + 18 + 18 + 12;
        let room = s.w - (tx - s.x) - 20;
        if url.is_empty() {
            text::draw_ellipsis(
                c,
                tx,
                ty,
                room,
                t!("web.omnibox"),
                CALLOUT,
                Weight::Regular,
                theme::solid(p.text_tertiary),
            );
        } else {
            let mut start = 0usize;
            while start < url.len() && text::measure(&url[start..], CALLOUT, Weight::Regular) > room
            {
                start += url[start..].chars().next().map_or(1, char::len_utf8);
            }
            let shown = &url[start..];
            let w = text::draw(
                c,
                tx,
                ty,
                shown,
                CALLOUT,
                Weight::Regular,
                theme::solid(p.text),
            );
            if editing {
                c.fill_rect(
                    (tx + w + 1).max(0) as usize,
                    (s.y + 12).max(0) as usize,
                    1,
                    (s.h - 24).max(0) as usize,
                    theme::accent(),
                );
            }
        }

        // Favourites (or suggestions when there are none).
        if !l.tiles.is_empty() {
            let has_bm = !t.browser.bookmarks().is_empty();
            text::draw(
                c,
                l.tiles_heading.x,
                text::center_y(
                    l.tiles_heading.y,
                    l.tiles_heading.h,
                    FOOTNOTE,
                    Weight::Semibold,
                ),
                if has_bm {
                    t!("web.start.bookmarks")
                } else {
                    t!("web.start.suggestions")
                },
                FOOTNOTE,
                Weight::Semibold,
                theme::solid(p.text_secondary),
            );
        }
        for (i, r) in l.tiles.iter().enumerate() {
            let (label, url) = &tiles[i];
            let hot = bs.hover == H::Tile(i);
            if hot {
                c.draw_shadow(
                    *r,
                    Shadow {
                        blur: 8,
                        dy: 3,
                        alpha: if theme::dark() { 90 } else { 34 },
                    },
                    Rect::new(r.x, r.y + 12, r.w, (r.h - 24).max(0)),
                );
                ui::fill_token(c, *r, 12, p.sidebar_bg);
                ui::stroke_token(c, *r, 12, p.separator);
            }
            let chip = Rect::new(r.x + (r.w - 40) / 2, r.y + 14, 40, 40);
            let host = tabs_model::host_of(url);
            letter_badge(c, chip, tabs_model::tab_badge(label, url), host, true);
            text::draw_centered(
                c,
                Rect::new(r.x + 6, r.y + 62, r.w - 12, 22),
                label,
                FOOTNOTE,
                Weight::Medium,
                theme::solid(p.text),
            );
        }

        // Recently visited.
        if !l.recents.is_empty() {
            text::draw(
                c,
                l.recent_heading.x,
                text::center_y(
                    l.recent_heading.y,
                    l.recent_heading.h,
                    FOOTNOTE,
                    Weight::Semibold,
                ),
                t!("web.start.recent"),
                FOOTNOTE,
                Weight::Semibold,
                theme::solid(p.text_secondary),
            );
        }
        for (i, r) in l.recents.iter().enumerate() {
            let url = &recents[i];
            if bs.hover == H::Recent(i) {
                ui::fill_token(c, *r, 8, p.hover);
            }
            let host = tabs_model::host_of(url);
            let b = Rect::new(r.x + 8, r.y + (r.h - 24) / 2, 24, 24);
            letter_badge(c, b, tabs_model::tab_badge("", url), host, false);
            let (hs, _) = tabs_model::host_range(url);
            let rest = &url[hs..];
            let tx = b.right() + 12;
            let w = text::draw_ellipsis(
                c,
                tx,
                text::center_y(r.y, r.h, BODY, Weight::Medium),
                r.w - (tx - r.x) - 8,
                host,
                BODY,
                Weight::Medium,
                theme::solid(p.text),
            );
            let tail_x = tx + w + 10;
            let tail_room = r.right() - 8 - tail_x;
            if tail_room > 24 && rest.len() > host.len() {
                text::draw_ellipsis(
                    c,
                    tail_x,
                    text::center_y(r.y, r.h, FOOTNOTE, Weight::Regular),
                    tail_room,
                    &rest[host.len()..],
                    FOOTNOTE,
                    Weight::Regular,
                    theme::solid(p.text_tertiary),
                );
            }
        }
    }
}
