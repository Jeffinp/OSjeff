//! Drawing the browser window: the toolbar with the omnibox, the tab strip, the new-tab
//! page, error pages, and what floats over the page (suggestions, the security popover,
//! the find bar, the context menu, notices). The page itself is `browser_paint.rs`.
//!
//! The chrome follows the system appearance; the page area keeps the page's own colours.
//! Geometry comes from `kitsune_core::layout` (the same functions hit-test clicks), colours
//! from the palette, type from `text::*`, widgets and glyphs from the toolkit.

use super::logic::{SecurityTone, page_menu_geom, start_items};
use crate::desktop::BrowserHover as H;
use crate::desktop::kit::glass::panel;
use crate::desktop::*;
use crate::text::{self, BODY, CALLOUT, CAPTION, FOOTNOTE, TITLE2, TITLE3, Weight};
use kitsune_core::browser::errors::{self, ErrorArt};
use kitsune_core::browser::{Status, tabs as tabs_model};
use kitsune_core::iconart::Glyph;
use kitsune_core::layout as geo;
use kitsune_core::style::{R_MENU, R_POPOVER};
use kitsune_core::t;

/// Amber of a favourite.
const AMBER: Color = Color::rgb(0xF5, 0xA6, 0x23);

/// Straight ARGB of `c` with opacity `a` (0..=255).
fn argb(c: Color, a: u32) -> u32 {
    (a.min(255) << 24) | (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
}

/// The badge colour of a host or letter: a stable pick from the accent family.
pub(crate) fn badge_color(seed: &str) -> Color {
    const PALETTE: [(u8, u8, u8); 8] = [
        (0x5B, 0x5C, 0xF6),
        (0x14, 0xB8, 0xC4),
        (0xF5, 0x9E, 0x0B),
        (0xEC, 0x48, 0x99),
        (0x10, 0xB9, 0x81),
        (0xF9, 0x73, 0x16),
        (0x8B, 0x5C, 0xF6),
        (0x0E, 0xA5, 0xE9),
    ];
    let h = seed
        .bytes()
        .fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
    let (r, g, b) = PALETTE[(h % 8) as usize];
    Color::rgb(r, g, b)
}

/// A rounded badge with a letter: the favicon-less site mark.
fn letter_badge(c: &mut Canvas, r: Rect, letter: char, seed: &str, squircle: bool) {
    if letter == tabs_model::BRAND_BADGE {
        // The browser's own pages: the fox, like a favicon (its tile has the corners).
        c.blit_surface(icons::surface(Icon::Brand, r.w.min(r.h)), r.x, r.y, 256);
        return;
    }
    let style = if squircle {
        Corner::Squircle
    } else {
        Corner::Circle
    };
    let rad = if squircle { r.w * 28 / 100 } else { r.w / 2 };
    c.fill_rrect(r, rad, style, badge_color(seed), 256);
    let mut buf = [0u8; 4];
    let s = letter.encode_utf8(&mut buf);
    let px = if r.w >= 36 {
        TITLE3
    } else if r.w >= 22 {
        BODY
    } else {
        CAPTION
    };
    text::draw_centered(c, r, s, px, Weight::Semibold, theme::WHITE);
}

/// Flat icon button: a wash on hover.
fn icon_button(c: &mut Canvas, r: Rect, g: Glyph, enabled: bool, hover: bool) {
    let p = theme::pal();
    if hover && enabled {
        ui::fill_token(c, r, 8, p.hover);
    }
    let ink = if enabled {
        theme::solid(p.text)
    } else {
        theme::ink_dim()
    };
    ui::draw_glyph(
        c,
        g,
        r.x + (r.w - 16) / 2,
        r.y + (r.h - 16) / 2,
        16,
        argb(ink, 255),
    );
}

impl Desktop {
    pub(crate) fn draw_browser(&self, c: &mut Canvas, r: Rect, focused: bool, bs: &BrowserState) {
        let p = theme::pal();
        let ch = bs.chrome(r);
        let t = bs.tabs.active();
        let id = self.browser_id();
        let mix = id.map_or(if focused { 256 } else { 0 }, |i| {
            self.focus_mix(i, focused)
        });

        // ---- the toolbar: the same colour as the title bar above it (it covers its hairline)
        let active_bg = theme::solid(p.window_bg);
        let inactive_bg = if theme::dark() {
            Color::rgb(0x27, 0x27, 0x2A)
        } else {
            Color::rgb(0xEC, 0xEC, 0xF0)
        };
        let bar_bg = inactive_bg.lerp(active_bg, (mix * 255 / 256) as u16);
        let band = Rect::new(r.x, ch.toolbar.y - 1, r.w, ch.toolbar.h + 1);
        c.fill_rect(
            band.x.max(0) as usize,
            band.y.max(0) as usize,
            band.w.max(0) as usize,
            band.h.max(0) as usize,
            bar_bg,
        );
        self.draw_toolbar(c, &ch, bs, focused);
        let (sc, sa) = theme::tint(p.separator);
        if ch.strip.h == 0 {
            c.blend_rect(Rect::new(r.x, ch.toolbar.bottom() - 1, r.w, 1), sc, sa);
        } else {
            self.draw_tab_strip(c, &ch, bs);
            c.blend_rect(Rect::new(r.x, ch.strip.bottom() - 1, r.w, 1), sc, sa);
        }

        // ---- the page area ----
        let content = ch.content;
        let status = t.browser.status();
        if t.browser.is_home() {
            self.draw_start_page(c, content, bs, focused);
        } else if let Some(page) = &t.page {
            self.paint_web_page(c, page, content, t.scroll, bs);
            if crate::trace::ON && t.trace_t0.get() != 0 {
                crate::trace::note(
                    "load to first paint",
                    t.trace_t0.replace(0),
                    page.cmds.len() as u64,
                );
            }
            self.draw_page_banner(c, content, t);
            self.draw_page_scrollbar(c, content, page, t);
        } else if status == Status::Error {
            self.draw_error_page(c, content, bs);
        } else {
            // A page on its way: a blank sheet with a word, and the bar says the rest.
            let bg = theme::surface();
            c.fill_rect(
                content.x.max(0) as usize,
                content.y.max(0) as usize,
                content.w.max(0) as usize,
                content.h.max(0) as usize,
                bg,
            );
            if t.browser.is_loading() {
                text::draw_centered(
                    c,
                    Rect::new(content.x, content.y + content.h / 3, content.w, 24),
                    t!("web.loading"),
                    CALLOUT,
                    Weight::Regular,
                    theme::solid(p.text_tertiary),
                );
            }
        }

        // ---- over the page ----
        if t.find.is_open() {
            self.draw_page_find_bar(c, content, bs, focused);
        }
        self.draw_flashes(c, content, bs);
        if bs.popover {
            self.draw_security_popover(c, r, &ch, bs);
        }
        self.draw_suggestions(c, &ch, bs, focused);
        if let Some(m) = &bs.ctx {
            self.draw_page_menu(c, r, m, bs);
        }
    }

    // ---- toolbar ----

    fn draw_toolbar(
        &self,
        c: &mut Canvas,
        ch: &geo::BrowserChrome,
        bs: &BrowserState,
        focused: bool,
    ) {
        let p = theme::pal();
        let t = bs.tabs.active();
        let hov = bs.hover;
        icon_button(
            c,
            ch.back,
            Glyph::ChevronLeft,
            t.browser.can_back(),
            hov == H::Back,
        );
        icon_button(
            c,
            ch.forward,
            Glyph::ChevronRight,
            t.browser.can_forward(),
            hov == H::Forward,
        );
        let loading = t.browser.is_loading();
        icon_button(
            c,
            ch.reload,
            if loading { Glyph::Close } else { Glyph::Reload },
            loading || !t.browser.is_home(),
            hov == H::Reload,
        );
        icon_button(
            c,
            ch.newtab,
            Glyph::Plus,
            bs.tabs.can_open(),
            hov == H::NewTab,
        );

        // The omnibox.
        let bar = ch.bar;
        let editing = focused && t.browser.bar_focus();
        let rad = bar.h / 2;
        ui::fill_token(c, bar, rad, p.field_bg);
        if editing {
            let acc = theme::accent();
            c.stroke_rrect(bar.inflated(2), rad + 2, Corner::Circle, acc, 90);
            c.stroke_rrect(bar, rad, Corner::Circle, acc, 256);
        } else {
            ui::stroke_token(c, bar, rad, p.control_border);
        }

        // The security indicator, or a plain glyph on the start page and the browser's own pages.
        if let Some((label, glyph, tone)) = bs.security_badge() {
            let (col, wash) = match tone {
                SecurityTone::Good => (theme::ok(), 40u16),
                SecurityTone::Warn => (
                    if theme::dark() {
                        Color::rgb(0xFB, 0xBF, 0x24)
                    } else {
                        Color::rgb(0xB4, 0x5F, 0x06)
                    },
                    44,
                ),
                SecurityTone::Bad => (theme::danger(), 44),
            };
            let s = ch.shield;
            let hot = hov == H::Shield || bs.popover;
            c.fill_rrect(
                s,
                s.h / 2,
                Corner::Circle,
                col,
                if hot { wash * 2 } else { wash },
            );
            ui::draw_glyph(c, glyph, s.x + 8, s.y + (s.h - 16) / 2, 16, argb(col, 255));
            text::draw(
                c,
                s.x + 8 + 16 + 6,
                text::center_y(s.y, s.h, FOOTNOTE, Weight::Medium),
                label,
                FOOTNOTE,
                Weight::Medium,
                col,
            );
        } else {
            ui::draw_glyph(
                c,
                if t.browser.is_home() {
                    Glyph::Search
                } else {
                    Glyph::Globe
                },
                bar.x + 12,
                bar.y + (bar.h - 16) / 2,
                16,
                argb(theme::solid(p.text_tertiary), 255),
            );
        }

        // The star: an outline, with a filled one growing over it for a favourite.
        let star = ch.star;
        let can_star = !t.browser.is_home();
        if can_star {
            let ink = if hov == H::Star {
                theme::solid(p.text)
            } else {
                theme::solid(p.text_tertiary)
            };
            let v = t.star.value().clamp(0.0, 1.0);
            ui::draw_glyph(
                c,
                Glyph::Star,
                star.x + (star.w - 16) / 2,
                star.y + (star.h - 16) / 2,
                16,
                argb(if v > 0.5 { AMBER } else { ink }, 255),
            );
            if v > 0.02 {
                // Overshoots a little and settles: 6 px growing to 18 px, then 16.
                let grow = if v < 0.7 {
                    v / 0.7 * 1.12
                } else {
                    1.12 - (v - 0.7) / 0.3 * 0.12
                };
                let size = ((16.0 * grow) as i32).max(4);
                ui::draw_glyph(
                    c,
                    Glyph::StarFill,
                    star.x + (star.w - size) / 2,
                    star.y + (star.h - size) / 2,
                    size,
                    argb(AMBER, (v * 255.0) as u32),
                );
            }
        }

        // The address.
        let tx = ch.text_x();
        let right = if can_star {
            star.x - 4
        } else {
            bar.right() - 12
        };
        let room = (right - tx).max(8);
        let ty = text::center_y(bar.y, bar.h, BODY, Weight::Regular);
        let url = text::from_bytes(t.browser.url()).into_owned();
        let saved = c.set_clip(
            Rect::new(tx, bar.y, room, bar.h)
                .intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        );
        if url.is_empty() {
            text::draw(
                c,
                tx,
                ty,
                t!("web.omnibox"),
                BODY,
                Weight::Regular,
                theme::solid(p.text_tertiary),
            );
        } else if editing {
            // The tail of an address wider than the field, selected or with a caret.
            let mut start = 0usize;
            while start < url.len()
                && text::measure(&url[start..], BODY, Weight::Regular) > room - 2
            {
                start += url[start..].chars().next().map_or(1, char::len_utf8);
            }
            let shown = &url[start..];
            if t.browser.bar_selected() {
                let w = text::measure(shown, BODY, Weight::Regular);
                c.fill_rrect(
                    Rect::new(tx - 3, bar.y + 6, w + 6, bar.h - 12),
                    4,
                    Corner::Circle,
                    theme::accent(),
                    96,
                );
            }
            text::draw(
                c,
                tx,
                ty,
                shown,
                BODY,
                Weight::Regular,
                theme::solid(p.text),
            );
            let caret = t
                .browser
                .caret()
                .min(url.len())
                .saturating_sub(start)
                .min(shown.len());
            let cx = tx + text::measure(&shown[..caret.min(shown.len())], BODY, Weight::Regular);
            c.fill_rect(
                cx.max(0) as usize,
                (bar.y + 7).max(0) as usize,
                1,
                (bar.h - 14).max(0) as usize,
                theme::accent(),
            );
        } else {
            // At rest: the host stands out, the rest of the address is dim.
            let (hs, he) = tabs_model::host_range(&url);
            let shown = &url[hs..];
            let host = &url[hs..he];
            let hw = text::measure(host, BODY, Weight::Regular);
            text::draw(c, tx, ty, host, BODY, Weight::Regular, theme::solid(p.text));
            text::draw(
                c,
                tx + hw,
                ty,
                &shown[host.len()..],
                BODY,
                Weight::Regular,
                theme::solid(p.text_tertiary),
            );
        }
        c.restore_clip(saved);

        // The progress bar under the field.
        let a = t.load.alpha();
        if a > 0 {
            let pr = ch.progress;
            let w = (pr.w as i64 * i64::from(t.load.permille()) / 1000) as i32;
            if w > 0 {
                c.fill_rrect(
                    Rect::new(pr.x, pr.y, w.max(2), pr.h),
                    1,
                    Corner::Circle,
                    theme::accent(),
                    a as u16,
                );
            }
        }
    }

    // ---- tab strip ----

    fn draw_tab_strip(&self, c: &mut Canvas, ch: &geo::BrowserChrome, bs: &BrowserState) {
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

    // ---- new tab ----

    fn draw_start_page(&self, c: &mut Canvas, content: Rect, bs: &BrowserState, focused: bool) {
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

    // ---- error pages ----

    fn draw_error_page(&self, c: &mut Canvas, content: Rect, bs: &BrowserState) {
        let p = theme::pal();
        let t = bs.tabs.active();
        c.fill_rect(
            content.x.max(0) as usize,
            content.y.max(0) as usize,
            content.w.max(0) as usize,
            content.h.max(0) as usize,
            theme::surface(),
        );
        let reason = t.browser.fail_reason();
        let info = errors::describe(reason);
        let cert = t.browser.can_continue_insecure();
        let l = geo::browser_error_layout(content, cert);

        // The illustration: a tile with a big glyph and a soft halo.
        let danger = theme::danger();
        let amber = if theme::dark() {
            Color::rgb(0xFB, 0xBF, 0x24)
        } else {
            Color::rgb(0xD9, 0x82, 0x0A)
        };
        let (tint, glyph, ink) = match info.art {
            ErrorArt::Offline => (
                theme::solid(p.text_secondary),
                Glyph::NetworkOff,
                theme::solid(p.text_secondary),
            ),
            ErrorArt::NotFound => (theme::accent(), Glyph::Globe, theme::accent()),
            ErrorArt::Unreachable => (amber, Glyph::Warning, amber),
            ErrorArt::Certificate => (danger, Glyph::Lock, danger),
            ErrorArt::Blocked => (danger, Glyph::Warning, danger),
        };
        let a = l.art;
        c.fill_rrect(a.inflated(8), 36, Corner::Squircle, tint, 22);
        c.fill_rrect(a, 28, Corner::Squircle, tint, 40);
        ui::draw_glyph(
            c,
            glyph,
            a.x + (a.w - 48) / 2,
            a.y + (a.h - 48) / 2,
            48,
            argb(ink, 255),
        );
        if info.art == ErrorArt::Certificate || info.art == ErrorArt::Blocked {
            // A small slash over the glyph: the lock or the connection is not intact.
            ui::draw_glyph(
                c,
                Glyph::Close,
                a.right() - 30,
                a.y + 8,
                22,
                argb(theme::solid(p.text), 0),
            );
        }
        text::draw_centered(
            c,
            l.title,
            info.title,
            TITLE2,
            Weight::Semibold,
            theme::solid(p.text),
        );
        text::draw_centered(
            c,
            l.cause,
            info.cause,
            CALLOUT,
            Weight::Regular,
            theme::solid(p.text_secondary),
        );
        let retry_hot = bs.hover == H::Retry;
        ui::push_button(
            c,
            l.retry,
            t!("web.err.retry"),
            ui::ButtonKind::Primary,
            if retry_hot {
                ui::Control::Hover
            } else {
                ui::Control::Normal
            },
        );
        if cert {
            if let kitsune_core::browser::FailReason::Cert(e) = reason
                && e.clock_may_be_to_blame()
                && !crate::clock::confirmed()
            {
                text::draw_centered(
                    c,
                    Rect::new(l.cause.x, l.cause.bottom() + 2, l.cause.w, 20),
                    t!("web.err.clock_hint"),
                    FOOTNOTE,
                    Weight::Regular,
                    amber,
                );
            }
            ui::push_button(
                c,
                l.proceed,
                t!("web.err.proceed"),
                ui::ButtonKind::Destructive,
                if bs.hover == H::Proceed {
                    ui::Control::Hover
                } else {
                    ui::Control::Normal
                },
            );
            text::draw_centered(
                c,
                l.note,
                t!("web.err.proceed_note"),
                FOOTNOTE,
                Weight::Regular,
                theme::solid(p.text_tertiary),
            );
        }
    }

    // ---- over the page ----

    /// The slim bar at the bottom of a page that is only part of the document.
    fn draw_page_banner(&self, c: &mut Canvas, content: Rect, t: &TabData) {
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
    fn draw_page_scrollbar(
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

    fn draw_page_find_bar(&self, c: &mut Canvas, content: Rect, bs: &BrowserState, focused: bool) {
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
    fn draw_flashes(&self, c: &mut Canvas, content: Rect, bs: &BrowserState) {
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

    fn draw_suggestions(
        &self,
        c: &mut Canvas,
        ch: &geo::BrowserChrome,
        bs: &BrowserState,
        focused: bool,
    ) {
        let t = bs.tabs.active();
        let sugg = t.browser.suggestions();
        if sugg.is_empty() || !focused {
            bs.glass[0].clear();
            return;
        }
        let p = theme::pal();
        let rect = geo::browser_suggest_panel(ch.bar, sugg.len());
        panel(
            c,
            rect,
            R_POPOVER,
            &bs.glass[0],
            10,
            p.menu_tint,
            p.separator,
            Shadow {
                blur: 14,
                dy: 8,
                alpha: 80,
            },
            256,
        );
        let sel = t.browser.suggestion_selected();
        for (i, s) in sugg.iter().enumerate() {
            let row = geo::browser_suggestion_row(ch.bar, i);
            let selected = sel == Some(i);
            let hot = bs.hover == H::Suggestion(i);
            let fg = if selected {
                c.fill_rrect(row, R_MENU, Corner::Circle, theme::accent(), 256);
                theme::ACCENT_TEXT
            } else {
                if hot {
                    ui::fill_token(c, row, R_MENU, p.hover);
                }
                theme::solid(p.text)
            };
            let g = if s.bookmark {
                Glyph::StarFill
            } else {
                Glyph::Globe
            };
            let gc = if selected {
                theme::ACCENT_TEXT
            } else if s.bookmark {
                AMBER
            } else {
                theme::solid(p.text_tertiary)
            };
            ui::draw_glyph(
                c,
                g,
                row.x + 10,
                row.y + (row.h - 16) / 2,
                16,
                argb(gc, 255),
            );
            let tx = row.x + 36;
            let ty = text::center_y(row.y, row.h, BODY, Weight::Regular);
            let w = text::draw_ellipsis(c, tx, ty, row.w - 44, &s.label, BODY, Weight::Regular, fg);
            if s.bookmark && s.label != s.url {
                let ux = tx + w + 12;
                let room = row.right() - 10 - ux;
                if room > 40 {
                    let (hs, _) = tabs_model::host_range(&s.url);
                    text::draw_ellipsis(
                        c,
                        ux,
                        text::center_y(row.y, row.h, FOOTNOTE, Weight::Regular),
                        room,
                        &s.url[hs..],
                        FOOTNOTE,
                        Weight::Regular,
                        if selected {
                            theme::ACCENT_TEXT
                        } else {
                            theme::solid(p.text_tertiary)
                        },
                    );
                }
            }
        }
    }

    /// The popover under the security indicator: what is known about the connection.
    fn draw_security_popover(
        &self,
        c: &mut Canvas,
        win: Rect,
        ch: &geo::BrowserChrome,
        bs: &BrowserState,
    ) {
        let p = theme::pal();
        let t = bs.tabs.active();
        let Some((_, glyph, tone)) = bs.security_badge() else {
            return;
        };
        use kitsune_core::browser::Security;
        let sec = t.browser.security();
        let url = String::from_utf8_lossy(t.browser.nav_url()).into_owned();
        let host = String::from(tabs_model::host_of(&url));
        // Rows of "label: value".
        let mut rows: Vec<(&str, String)> = Vec::new();
        if let Some(ci) = &t.cert {
            if !ci.issuer.is_empty() {
                rows.push((t!("web.sec.issuer"), ci.issuer.clone()));
            }
            rows.push((
                t!("web.sec.valid_from"),
                kitsune_core::browser::cert::format_date(ci.not_before),
            ));
            rows.push((
                t!("web.sec.valid_to"),
                kitsune_core::browser::cert::format_date(ci.not_after),
            ));
        }
        let (verify, para): (&str, &str) = match sec {
            Security::HttpsVerified => (t!("web.sec.verified_how"), t!("web.sec.verified_note")),
            Security::HttpsInvalid => (t!("web.sec.unverified"), t!("web.sec.unverified_note")),
            _ => ("", t!("web.sec.http_note")),
        };
        if !verify.is_empty() {
            rows.push((t!("web.sec.verification"), String::from(verify)));
        }
        let inner_w = geo::browser_popover(ch.bar, win, 0).w - 32;
        let lines = text::wrap(para, FOOTNOTE, Weight::Regular, inner_w, 4).len() as i32;
        let h = 16
            + 36
            + 12
            + rows.len() as i32 * 24
            + if rows.is_empty() { 0 } else { 8 }
            + lines * 18
            + 12;
        let pr = geo::browser_popover(ch.bar, win, h);
        panel(
            c,
            pr,
            R_POPOVER,
            &bs.glass[1],
            10,
            p.menu_tint,
            p.separator,
            Shadow {
                blur: 14,
                dy: 8,
                alpha: 80,
            },
            256,
        );
        let col = match tone {
            SecurityTone::Good => theme::ok(),
            SecurityTone::Warn => {
                if theme::dark() {
                    Color::rgb(0xFB, 0xBF, 0x24)
                } else {
                    Color::rgb(0xB4, 0x5F, 0x06)
                }
            }
            SecurityTone::Bad => theme::danger(),
        };
        let icon = Rect::new(pr.x + 16, pr.y + 16, 36, 36);
        c.fill_rrect(icon, 18, Corner::Circle, col, 44);
        ui::draw_glyph(c, glyph, icon.x + 10, icon.y + 10, 16, argb(col, 255));
        let heading = match sec {
            Security::HttpsVerified => t!("web.sec.secure"),
            Security::HttpsInvalid => t!("web.sec.invalid"),
            _ => t!("web.sec.not_encrypted"),
        };
        text::draw(
            c,
            icon.right() + 12,
            pr.y + 18,
            heading,
            TITLE3,
            Weight::Semibold,
            theme::solid(p.text),
        );
        text::draw_ellipsis(
            c,
            icon.right() + 12,
            pr.y + 38,
            pr.w - 16 - 36 - 12 - 16,
            &host,
            FOOTNOTE,
            Weight::Regular,
            theme::solid(p.text_secondary),
        );
        let mut y = icon.bottom() + 12;
        let (sc, sa) = theme::tint(p.separator);
        c.blend_rect(Rect::new(pr.x + 16, y - 6, pr.w - 32, 1), sc, sa);
        for (k, v) in &rows {
            text::draw(
                c,
                pr.x + 16,
                text::center_y(y, 24, FOOTNOTE, Weight::Regular),
                k,
                FOOTNOTE,
                Weight::Regular,
                theme::solid(p.text_secondary),
            );
            text::draw_ellipsis(
                c,
                pr.x + 16 + 84,
                text::center_y(y, 24, FOOTNOTE, Weight::Medium),
                pr.w - 32 - 84,
                v,
                FOOTNOTE,
                Weight::Medium,
                theme::solid(p.text),
            );
            y += 24;
        }
        if !rows.is_empty() {
            y += 8;
        }
        for (i, rg) in text::wrap(para, FOOTNOTE, Weight::Regular, inner_w, 4)
            .iter()
            .enumerate()
        {
            text::draw(
                c,
                pr.x + 16,
                y + i as i32 * 18,
                &para[rg.0..rg.1],
                FOOTNOTE,
                Weight::Regular,
                theme::solid(p.text_secondary),
            );
        }
    }

    /// The page's context menu.
    fn draw_page_menu(&self, c: &mut Canvas, win: Rect, m: &PageMenu, bs: &BrowserState) {
        let p = theme::pal();
        let g = page_menu_geom(m, win);
        panel(
            c,
            g.rect,
            R_MENU,
            &bs.glass[3],
            10,
            p.menu_tint,
            p.separator,
            Shadow {
                blur: 12,
                dy: 8,
                alpha: 80,
            },
            256,
        );
        for (i, (row, (_, label, enabled))) in g.rows.iter().zip(&m.items).enumerate() {
            ui::menu_item(
                c,
                *row,
                label,
                "",
                bs.hover == H::MenuRow(i),
                *enabled,
                false,
            );
        }
    }
}
