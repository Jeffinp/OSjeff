//! Suggestions, the security popover and the page context menu.

use super::helpers::AMBER;
use super::helpers::argb;
use crate::desktop::BrowserHover as H;
use crate::desktop::apps::browser::SecurityTone;
use crate::desktop::apps::browser::hover::page_menu_geom;
use crate::desktop::kit::glass::panel;
use crate::desktop::*;
use crate::text::{self, BODY, FOOTNOTE, TITLE3, Weight};
use kitsune_core::browser::tabs as tabs_model;
use kitsune_core::iconart::Glyph;
use kitsune_core::layout as geo;
use kitsune_core::style::{R_MENU, R_POPOVER};
use kitsune_core::t;

impl Desktop {
    pub(super) fn draw_suggestions(
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
    pub(super) fn draw_security_popover(
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
    pub(super) fn draw_page_menu(
        &self,
        c: &mut Canvas,
        win: Rect,
        m: &PageMenu,
        bs: &BrowserState,
    ) {
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
