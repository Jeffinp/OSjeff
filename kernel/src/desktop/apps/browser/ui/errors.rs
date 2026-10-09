//! Error pages.

use super::helpers::argb;
use crate::desktop::BrowserHover as H;
use crate::desktop::*;
use crate::text::{self, CALLOUT, FOOTNOTE, TITLE2, Weight};
use kitsune_core::browser::errors::{self, ErrorArt};
use kitsune_core::iconart::Glyph;
use kitsune_core::layout as geo;
use kitsune_core::t;

impl Desktop {
    // ---- error pages ----

    pub(super) fn draw_error_page(&self, c: &mut Canvas, content: Rect, bs: &BrowserState) {
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
}
