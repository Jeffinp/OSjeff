//! The calendar and notification centre, and the Quick Settings tiles.

use super::bar::age_text;
use super::bar::level_color;
use super::bar::level_title;
use super::helpers::draw_fit;
use super::helpers::mirror_x;
use super::helpers::pack;
use crate::desktop::*;
use crate::text::{self, BODY, FOOTNOTE, TITLE2, TITLE3, Weight};
use kitsune_core::chrome::{self, centre_geom};
use kitsune_core::i18n::{self, Civil, DateStyle};
use kitsune_core::iconart::Glyph;
use kitsune_core::t;

impl Desktop {
    /// One Quick Settings tile: a disc with a glyph, a label and a status line; accent when on.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_tile(
        &self,
        c: &mut Canvas,
        r: Rect,
        glyph: Glyph,
        label: &str,
        sub: &str,
        on: bool,
        hover: bool,
    ) {
        let p = theme::pal();
        let rad = 10;
        if on {
            c.fill_rrect(r, rad, Corner::Circle, theme::accent(), 256);
            if hover {
                c.fill_rrect(r, rad, Corner::Circle, theme::WHITE, 28);
            }
        } else {
            ui::fill_token(c, r, rad, p.control_bg);
            if hover {
                ui::fill_token(c, r, rad, p.hover);
            }
            ui::stroke_token(c, r, rad, p.control_border);
        }
        let disc = Rect::new(r.x + 10, r.y + (r.h - 32) / 2, 32, 32);
        let (dc, da) = if on {
            (theme::WHITE, 56)
        } else {
            theme::tint(p.hover)
        };
        c.fill_rrect(disc, 8, Corner::Circle, dc, da);
        let ink = if on {
            theme::WHITE
        } else {
            theme::solid(p.text)
        };
        ui::draw_glyph(
            c,
            glyph,
            disc.x + 7,
            disc.y + 7,
            18,
            0xFF00_0000 | pack(ink),
        );
        let tx = disc.right() + 10;
        let tw = r.right() - tx - 8;
        let (c1, c2) = if on {
            (theme::WHITE, theme::WHITE)
        } else {
            (theme::solid(p.text), theme::solid(p.text_secondary))
        };
        draw_fit(
            c,
            Rect::new(tx, r.y + 11, tw, 18),
            label,
            BODY,
            Weight::Medium,
            c1,
        );
        draw_fit(
            c,
            Rect::new(tx, r.y + 29, tw, 16),
            sub,
            FOOTNOTE,
            Weight::Regular,
            if on { Color::rgb(0xE8, 0xE8, 0xFF) } else { c2 },
        );
    }

    /// The calendar and notification centre: the date and the notification list at the left, the
    /// month at the right.
    pub(super) fn draw_centre(&self, c: &mut Canvas, r: Rect, month_off: i32) {
        let p = theme::pal();
        let g = centre_geom(r);
        let (year, month, day) = self.today.get();
        let civil = Civil {
            year,
            month,
            day,
            weekday: self.weekday.get(),
            hour: 0,
            minute: 0,
            second: 0,
        };
        let clock24 = crate::settings::clock24();
        text::draw_left(
            c,
            g.day,
            &i18n::format_date(civil, DateStyle::Weekday, clock24),
            TITLE2,
            Weight::Semibold,
            theme::solid(p.text),
        );
        let date = i18n::format_date(civil, DateStyle::LongNoWeekday, clock24);
        text::draw_left(
            c,
            g.date,
            &date,
            BODY,
            Weight::Regular,
            theme::solid(p.text_secondary),
        );
        text::draw_left(
            c,
            g.notif_title,
            t!("centre.notifications"),
            BODY,
            Weight::Semibold,
            theme::solid(p.text),
        );
        let notifs = &self.shell.notifs;
        if !notifs.is_empty() {
            let hov = g.clear.contains(self.cursor_x, self.cursor_y);
            if hov {
                ui::fill_token(c, g.clear, 6, p.hover);
            }
            text::draw_centered(
                c,
                g.clear,
                t!("centre.clear"),
                FOOTNOTE,
                Weight::Medium,
                theme::accent(),
            );
        }
        if notifs.is_empty() {
            text::draw_centered(
                c,
                g.empty,
                t!("centre.empty"),
                BODY,
                Weight::Regular,
                theme::solid(p.text_tertiary),
            );
        } else {
            let now = crate::klog::ticks_to_ms_now();
            // Newest first.
            for (row, n) in g.rows.iter().zip(notifs.iter().rev()) {
                ui::fill_token(c, *row, 8, p.control_bg);
                ui::stroke_token(c, *row, 8, p.control_border);
                let disc = Rect::new(row.x + 10, row.y + (row.h - 24) / 2, 24, 24);
                c.fill_rrect(disc, 12, Corner::Circle, level_color(n.level), 256);
                text::draw_centered(c, disc, "!", BODY, Weight::Semibold, theme::WHITE);
                let tx = disc.right() + 10;
                let age = age_text(now, n.ms);
                let aw = text::measure(&age, FOOTNOTE, Weight::Regular) + 8;
                draw_fit(
                    c,
                    Rect::new(tx, row.y + 5, row.right() - tx - aw - 8, 18),
                    level_title(n.level),
                    BODY,
                    Weight::Medium,
                    theme::solid(p.text),
                );
                text::draw_right(
                    c,
                    Rect::new(row.x, row.y + 5, row.w - 10, 18),
                    &age,
                    FOOTNOTE,
                    Weight::Regular,
                    theme::solid(p.text_tertiary),
                );
                draw_fit(
                    c,
                    Rect::new(tx, row.y + 23, row.right() - tx - 10, 16),
                    &n.text,
                    FOOTNOTE,
                    Weight::Regular,
                    theme::solid(p.text_secondary),
                );
            }
        }
        text::draw_left(
            c,
            g.dnd_label,
            t!("quick.dnd"),
            BODY,
            Weight::Regular,
            theme::solid(p.text),
        );
        ui::switch(
            c,
            g.dnd_switch,
            (self.shell.knobs[2].value() * 256.0) as i32,
            true,
        );
        // A hairline between the two columns.
        let (sc, sa) = theme::tint(p.separator);
        c.blend_rect(Rect::new(g.calendar.x - 12, r.y + 14, 1, r.h - 28), sc, sa);
        self.draw_calendar(c, g.calendar, month_off);
    }

    fn draw_calendar(&self, c: &mut Canvas, r: Rect, month_off: i32) {
        let p = theme::pal();
        let g = chrome::calendar_geom(r);
        let (ty, tm, td) = self.today.get();
        let idx = (ty * 12 + tm as i32 - 1) + month_off;
        let (year, month) = (idx.div_euclid(12), (idx.rem_euclid(12) + 1) as u8);
        let title = i18n::format_date(
            Civil {
                year,
                month,
                day: 1,
                weekday: 0,
                hour: 0,
                minute: 0,
                second: 0,
            },
            DateStyle::MonthYear,
            true,
        );
        text::draw_left(
            c,
            g.title,
            &title,
            TITLE3,
            Weight::Semibold,
            theme::solid(p.text),
        );
        ui::draw_glyph(
            c,
            Glyph::ChevronRight,
            g.next.x + 4,
            g.next.y + 6,
            16,
            0xFF00_0000 | pack(theme::solid(p.text_secondary)),
        );
        // The "previous" chevron is the next one mirrored.
        let chev = crate::glyphs::get(
            Glyph::ChevronRight,
            16,
            0xFF00_0000 | pack(theme::solid(p.text_secondary)),
        );
        let mirrored = mirror_x(chev);
        c.blit_surface(&mirrored, g.prev.x + 4, g.prev.y + 6, 256);
        for (i, wd_rect) in g.weekdays.iter().enumerate() {
            text::draw_centered(
                c,
                *wd_rect,
                i18n::locale::weekday_initial(i18n::lang(), i as u8),
                FOOTNOTE,
                Weight::Medium,
                theme::solid(p.text_tertiary),
            );
        }
        let grid = chrome::month_grid(year, month);
        for (row, line) in grid.iter().enumerate() {
            for (col, &d) in line.iter().enumerate() {
                if d == 0 {
                    continue;
                }
                let cell = g.cells[row][col];
                let today = month_off == 0 && d == td;
                let label = alloc::format!("{d}");
                if today {
                    let disc = Rect::new(cell.x + (cell.w - 28) / 2, cell.y + 3, 28, 28);
                    c.fill_rrect(disc, 14, Corner::Circle, theme::accent(), 256);
                    text::draw_centered(
                        c,
                        disc,
                        &label,
                        BODY,
                        Weight::Semibold,
                        theme::ACCENT_TEXT,
                    );
                } else {
                    let weekend = col == 0 || col == 6;
                    let col_c = if weekend { p.text_secondary } else { p.text };
                    text::draw_centered(
                        c,
                        cell,
                        &label,
                        BODY,
                        Weight::Regular,
                        theme::solid(col_c),
                    );
                }
            }
        }
    }
}
