//! Language, keyboard and date and time pages.

use crate::desktop::apps::ajustes::builder::Ui;
use crate::desktop::apps::ajustes::state::A_CLOCK24;
use crate::desktop::apps::ajustes::state::A_CLOCKFMT;
use crate::desktop::apps::ajustes::state::A_DOWN;
use crate::desktop::apps::ajustes::state::A_KBD;
use crate::desktop::apps::ajustes::state::A_KBDUSE;
use crate::desktop::apps::ajustes::state::A_LANG;
use crate::desktop::apps::ajustes::state::A_LAYOUT;
use crate::desktop::apps::ajustes::state::A_READTIME;
use crate::desktop::apps::ajustes::state::A_SETTIME;
use crate::desktop::apps::ajustes::state::A_TZ;
use crate::desktop::apps::ajustes::state::A_TZLIST;
use crate::desktop::apps::ajustes::state::A_TZSEARCH;
use crate::desktop::apps::ajustes::state::A_UP;
use crate::desktop::apps::ajustes::state::DOWN;
use crate::desktop::apps::ajustes::state::Focus;
use crate::desktop::apps::ajustes::state::ROW;
use crate::desktop::apps::ajustes::state::TZ_ROW_H;
use crate::desktop::apps::ajustes::state::TZ_ROWS;
use crate::desktop::apps::ajustes::state::read_local;
use crate::desktop::kit;
use crate::desktop::kit::ui::{self, ButtonKind};
use crate::desktop::*;
use crate::text::{self, BODY, CAPTION, FOOTNOTE, TITLE1, Weight};
use kitsune_core::hw::rtc::Field;
use kitsune_core::i18n::{self, Civil, DateStyle, Lang};
use kitsune_core::iconart::Glyph;
use kitsune_core::keymap::Layout;
use kitsune_core::settings::{TIMEZONES, city_name, search_timezones, utc_label};
use kitsune_core::t;

/// Ajustes > Idioma e região: the language (each option in its own language), the format of
/// the time, an example, and a hint (never a forced change) about the ABNT2 keyboard.
pub(super) fn page_language(ui: &mut Ui<'_, '_>) {
    let s = ui.s;
    ui.title(t!("settings.lang.title"));
    ui.header(t!("settings.lang.h_language"));
    let card = ui.card(Lang::ALL.len() as i32 * ROW);
    for (i, l) in Lang::ALL.into_iter().enumerate() {
        let r = ui.row(card, i as i32);
        ui.label(r, l.native_name(), l.code(), true);
        if let Some(c) = ui.c.as_deref_mut()
            && ui.view.intersection(&r).is_some()
        {
            ui::radio(c, r.right() - 16 - 16, r.y + (ROW - 16) / 2, s.lang == l);
        }
        ui.hit(r, A_LANG + i as u32);
    }
    ui.header(t!("settings.lang.note"));
    if s.lang == Lang::Pt && s.layout == Layout::Us {
        let card = ui.card(ROW);
        let r = ui.row(card, 0);
        ui.label(
            r,
            t!("settings.lang.kbd_hint"),
            t!("settings.lang.kbd_note"),
            true,
        );
        let b = Rect::new(r.right() - 16 - 112, r.y + 8, 112, 32);
        ui.button(
            b,
            t!("settings.lang.kbd_use"),
            ButtonKind::Secondary,
            A_KBDUSE,
            true,
        );
    }
    ui.header(t!("settings.lang.h_formats"));
    let card = ui.card(2 * ROW);
    let r = ui.row(card, 0);
    ui.label(r, t!("settings.lang.clock"), "", true);
    let sel = if s.clock_auto {
        0
    } else if s.clock24 {
        1
    } else {
        2
    };
    let seg = Rect::new(r.right() - 16 - 270, r.y + 10, 270, 28);
    ui.segmented(
        seg,
        &[
            t!("settings.lang.clock_auto"),
            t!("settings.lang.clock_24"),
            t!("settings.lang.clock_12"),
        ],
        sel,
        A_CLOCKFMT,
    );
    let r = ui.row(card, 1);
    let civil = Civil::from_rtc(&read_local());
    let example = alloc::format!(
        "{} \u{b7} {} \u{b7} {}",
        i18n::format_date(civil, DateStyle::Full, s.clock24),
        i18n::format_num(1_234_567),
        i18n::format_size(1536)
    );
    ui.label(r, t!("settings.lang.example"), &example, true);
}

pub(super) fn page_keyboard(ui: &mut Ui<'_, '_>) {
    let s = ui.s;
    ui.title(t!("settings.sec.keyboard"));
    ui.header(t!("settings.kbd.layout"));
    let card = ui.card(2 * ROW);
    for (i, (name, sub, l)) in [
        ("US", t!("settings.kbd.us_sub"), Layout::Us),
        ("ABNT2", t!("settings.kbd.abnt2_sub"), Layout::Abnt2),
    ]
    .into_iter()
    .enumerate()
    {
        let r = ui.row(card, i as i32);
        ui.label(r, name, sub, true);
        if let Some(c) = ui.c.as_deref_mut()
            && ui.view.intersection(&r).is_some()
        {
            ui::radio(c, r.right() - 16 - 16, r.y + (ROW - 16) / 2, s.layout == l);
        }
        ui.hit(r, A_LAYOUT + i as u32);
    }
    ui.header(t!("settings.kbd.test"));
    let card = ui.card(ROW + 16);
    let field = Rect::new(card.x + 16, card.y + 14, card.w - 32, 28);
    let txt = text::from_bytes(&ui.st.kbd_test).into_owned();
    let focus = ui.st.focus == Focus::Kbd;
    ui.field(field, &txt, t!("settings.kbd.test_hint"), focus, A_KBD);
}

pub(super) fn page_time(ui: &mut Ui<'_, '_>) {
    let s = ui.s;
    ui.title(t!("settings.sec.time"));
    // Now.
    let now = read_local();
    ui.header(t!("settings.time.now"));
    let card = ui.card(96);
    if let Some(c) = ui.c.as_deref_mut()
        && ui.view.intersection(&card).is_some()
    {
        let civil = Civil::from_rtc(&now);
        let t = i18n::format_time(civil, s.clock24, true);
        text::draw_left(
            c,
            Rect::new(card.x + 20, card.y + 14, card.w / 2, 40),
            &t,
            TITLE1,
            Weight::Semibold,
            kit::ink(),
        );
        let d = i18n::format_date(civil, DateStyle::Long, s.clock24);
        text::draw_left(
            c,
            Rect::new(card.x + 20, card.y + 56, card.w - 40, 22),
            &d,
            BODY,
            Weight::Regular,
            kit::ink2(),
        );
        let utc = now.shifted(-(s.tz_minutes as i32));
        let u = alloc::format!("UTC {:02}:{:02}:{:02}", utc.time.h, utc.time.m, utc.time.s);
        text::draw_right(
            c,
            Rect::new(card.x, card.y + 22, card.w - 20, 22),
            &u,
            FOOTNOTE,
            Weight::Regular,
            kit::ink3(),
        );
    }
    ui.header(t!("settings.time.format"));
    let card = ui.card(ROW);
    let k = ui.st.knobs[2].value();
    ui.row_switch(card, 0, t!("settings.time.clock24"), "", k, A_CLOCK24);

    // The zone picker.
    ui.header(t!("settings.time.zone"));
    let card = ui.card(48 + TZ_ROWS * TZ_ROW_H + 8);
    let sf = Rect::new(card.x + 12, card.y + 10, card.w - 24, 28);
    let focus = ui.st.focus == Focus::TzSearch;
    if let Some(c) = ui.c.as_deref_mut()
        && ui.view.intersection(&sf).is_some()
    {
        kit::search_field(
            c,
            sf,
            &ui.st.tz_query,
            t!("settings.time.search"),
            focus,
            focus,
        );
    }
    ui.hit(sf, A_TZSEARCH);
    let list = Rect::new(card.x + 8, card.y + 46, card.w - 16, TZ_ROWS * TZ_ROW_H);
    let found = search_timezones(&ui.st.tz_query);
    let scroll = ui.st.tz_scroll.value();
    let saved = ui.c.as_deref_mut().map(|c| kit::clip_to(c, list));
    for (k, &ci) in found.iter().enumerate() {
        let y = list.y + k as i32 * TZ_ROW_H - scroll;
        let r = Rect::new(list.x, y, list.w, TZ_ROW_H);
        if y + TZ_ROW_H <= list.y || y >= list.bottom() {
            continue;
        }
        let selected = s.tz_city == ci;
        let id = A_TZ + ci as u32;
        let (_, mins) = TIMEZONES[ci as usize];
        let name = city_name(ci);
        if let Some(c) = ui.c.as_deref_mut() {
            let fg = if selected {
                c.fill_rrect(r.inflated(-2), 7, Corner::Circle, theme::accent(), 256);
                theme::ACCENT_TEXT
            } else {
                if ui.hv & !DOWN == id {
                    ui::fill_token(c, r.inflated(-2), 7, theme::pal().hover);
                }
                kit::ink()
            };
            text::draw_left(
                c,
                Rect::new(r.x + 12, y, r.w - 120, TZ_ROW_H),
                name,
                BODY,
                Weight::Regular,
                fg,
            );
            text::draw_right(
                c,
                Rect::new(r.x, y, r.w - 12, TZ_ROW_H),
                kit::fb_str(&utc_label(mins as i32)),
                BODY,
                Weight::Regular,
                if selected {
                    Color::rgb(0xE6, 0xE6, 0xFF)
                } else {
                    kit::ink2()
                },
            );
        }
        let vis = r.intersection(&list).unwrap_or(Rect::new(0, 0, 0, 0));
        ui.hit(vis, id);
    }
    if found.is_empty()
        && let Some(c) = ui.c.as_deref_mut()
    {
        text::draw_left(
            c,
            Rect::new(list.x + 12, list.y, list.w - 24, TZ_ROW_H),
            t!("settings.time.no_city"),
            BODY,
            Weight::Regular,
            kit::ink2(),
        );
    }
    if let (Some(c), Some(sv)) = (ui.c.as_deref_mut(), saved) {
        c.restore_clip(sv);
    }
    ui.hit(list, A_TZLIST);

    // The clock editor.
    ui.header(t!("settings.time.adjust"));
    let card = ui.card(150);
    let fields: [(Field, &str, i32); 6] = [
        (Field::Day, t!("settings.time.day"), 56),
        (Field::Month, t!("settings.time.month"), 56),
        (Field::Year, t!("settings.time.year"), 80),
        (Field::Hour, t!("settings.time.hour"), 56),
        (Field::Minute, t!("settings.time.min"), 56),
        (Field::Second, t!("settings.time.sec"), 56),
    ];
    let e = ui.st.edit;
    let vals = [
        e.date.d as u32,
        e.date.m as u32,
        e.date.y as u32,
        e.time.h as u32,
        e.time.m as u32,
        e.time.s as u32,
    ];
    let mut x = card.x + 20;
    for (i, (_, name, w)) in fields.iter().enumerate() {
        if i == 3 {
            x += 16;
        }
        let up = Rect::new(x, card.y + 10, *w, 24);
        let val = Rect::new(x, card.y + 36, *w, 28);
        let dn = Rect::new(x, card.y + 66, *w, 24);
        if let Some(c) = ui.c.as_deref_mut()
            && ui.view.intersection(&card).is_some()
        {
            for (r, g, id) in [
                (up, Glyph::ChevronUp, A_UP + i as u32),
                (dn, Glyph::ChevronDown, A_DOWN + i as u32),
            ] {
                if ui.hv & !DOWN == id {
                    ui::fill_token(c, r, 6, theme::pal().hover);
                }
                ui::draw_glyph(
                    c,
                    g,
                    r.x + (r.w - 12) / 2,
                    r.y + 6,
                    12,
                    kit::argb(kit::ink2()),
                );
            }
            ui::fill_token(c, val, 7, theme::pal().field_bg);
            ui::stroke_token(c, val, 7, theme::pal().control_border);
            let t = if *w > 70 {
                alloc::format!("{:04}", vals[i])
            } else {
                alloc::format!("{:02}", vals[i])
            };
            text::draw_centered(c, val, &t, BODY, Weight::Medium, kit::ink());
            text::draw_centered(
                c,
                Rect::new(x, val.y - 14, *w, 12),
                name,
                CAPTION,
                Weight::Regular,
                kit::ink3(),
            );
        }
        ui.hit(up, A_UP + i as u32);
        ui.hit(dn, A_DOWN + i as u32);
        x += w + 8;
    }
    ui.button(
        Rect::new(card.x + 20, card.y + 104, 140, 28),
        t!("settings.time.read"),
        ButtonKind::Secondary,
        A_READTIME,
        true,
    );
    ui.button(
        Rect::new(card.x + 20 + 148, card.y + 104, 100, 28),
        t!("settings.time.set"),
        ButtonKind::Primary,
        A_SETTIME,
        true,
    );
    if let Some((m, err)) = ui.st.message()
        && let Some(c) = ui.c.as_deref_mut()
        && ui.view.intersection(&card).is_some()
    {
        text::draw_left(
            c,
            Rect::new(card.x + 20 + 256, card.y + 104, card.w - 296, 28),
            &m,
            FOOTNOTE,
            Weight::Regular,
            if err { kit::red() } else { kit::ink2() },
        );
    }
}
