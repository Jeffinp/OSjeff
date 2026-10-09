//! Appearance, wallpaper and dock pages.

use crate::desktop::apps::ajustes::builder::Ui;
use crate::desktop::apps::ajustes::state::A_APPLY;
use crate::desktop::apps::ajustes::state::A_MOTION;
use crate::desktop::apps::ajustes::state::A_PATH;
use crate::desktop::apps::ajustes::state::A_PICK;
use crate::desktop::apps::ajustes::state::A_SWATCH;
use crate::desktop::apps::ajustes::state::A_THEME;
use crate::desktop::apps::ajustes::state::A_TOAST_SECS;
use crate::desktop::apps::ajustes::state::A_TOASTS;
use crate::desktop::apps::ajustes::state::A_WALL;
use crate::desktop::apps::ajustes::state::Focus;
use crate::desktop::apps::ajustes::state::ROW;
use crate::desktop::kit;
use crate::desktop::kit::ui::ButtonKind;
use crate::desktop::*;
use crate::text::{self, CAPTION, FOOTNOTE, Weight};
use kitsune_core::settings::{
    ACCENTS, TOAST_SECS_MAX, TOAST_SECS_MIN, WallpaperChoice, accent_name,
};
use kitsune_core::t;
use kitsune_core::wallpaper::{self, PRESETS, Style};

pub(super) fn page_appearance(ui: &mut Ui<'_, '_>) {
    use kitsune_core::style::AppearanceSetting as A;
    let s = ui.s;
    ui.title(t!("settings.sec.appearance"));
    ui.header(t!("settings.theme"));
    let card = ui.card(ROW);
    let r = ui.row(card, 0);
    ui.label(r, t!("settings.theme"), "", true);
    let sel = match s.appearance {
        A::Auto => 0,
        A::Light => 1,
        A::Dark => 2,
    };
    let seg = Rect::new(r.right() - 16 - 300, r.y + 10, 300, 28);
    ui.segmented(
        seg,
        &[
            t!("quick.appearance.auto"),
            t!("quick.appearance.light"),
            t!("quick.appearance.dark"),
        ],
        sel,
        A_THEME,
    );

    ui.header(t!("quick.accent"));
    let card = ui.card(96);
    let pitch = ((card.w - 32 - 28) / 7).max(30);
    for (i, &rgb) in ACCENTS.iter().enumerate() {
        let sw = Rect::new(card.x + 16 + i as i32 * pitch, card.y + 18, 28, 28);
        let hov = ui.hovered(A_SWATCH + i as u32);
        if let Some(c) = ui.c.as_deref_mut()
            && ui.view.intersection(&card).is_some()
        {
            let col = Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
            if s.accent as usize == i {
                c.stroke_rrect(sw.inflated(3), 17, Corner::Circle, col, 256);
                c.stroke_rrect(sw.inflated(4), 18, Corner::Circle, col, 256);
            } else if hov {
                c.stroke_rrect(sw.inflated(3), 17, Corner::Circle, col, 120);
            }
            c.fill_rrect(sw, 14, Corner::Circle, col, 256);
            c.stroke_rrect(sw, 14, Corner::Circle, Color::rgb(0, 0, 0), 36);
            let name = accent_name(i);
            let mid = sw.x + sw.w / 2;
            let nw = text::measure(name, CAPTION, Weight::Regular) + 8;
            text::draw_centered(
                c,
                Rect::new(mid - nw / 2, card.y + 58, nw, 18),
                name,
                CAPTION,
                if s.accent as usize == i {
                    Weight::Medium
                } else {
                    Weight::Regular
                },
                if s.accent as usize == i {
                    kit::ink()
                } else {
                    kit::ink3()
                },
            );
        }
        ui.hit(sw.inflated(6), A_SWATCH + i as u32);
    }

    ui.header(t!("quick.motion"));
    let card = ui.card(ROW);
    let k = ui.st.knobs[0].value();
    ui.row_switch(
        card,
        0,
        t!("settings.motion.reduce"),
        t!("settings.motion.reduce_sub"),
        k,
        A_MOTION,
    );

    ui.header(t!("centre.notifications"));
    let card = ui.card(2 * ROW);
    let k = ui.st.knobs[1].value();
    ui.row_switch(card, 0, t!("settings.notif.show"), "", k, A_TOASTS);
    let secs = t!("settings.unit_secs", n = s.toast_secs as i64);
    ui.row_slider(
        card,
        1,
        t!("settings.notif.duration"),
        s.toast_secs as i32,
        (TOAST_SECS_MIN as i32, TOAST_SECS_MAX as i32),
        &secs,
        s.toasts,
        A_TOAST_SECS,
    );
}

pub(super) fn page_wallpaper(ui: &mut Ui<'_, '_>) {
    let s = ui.s;
    ui.title(t!("settings.sec.wallpaper"));
    // The grid: five presets and the user's image, three to a row.
    let gap = 16;
    let tw = (ui.w - 2 * gap) / 3;
    let th = tw * 9 / 16;
    let tiles = PRESETS.len() + 1;
    let top = ui.y;
    for i in 0..tiles {
        let (col, row) = ((i % 3) as i32, (i / 3) as i32);
        let t = Rect::new(ui.x + col * (tw + gap), top + row * (th + 36), tw, th);
        let selected = match s.wallpaper {
            WallpaperChoice::Preset(n) => i == n as usize,
            WallpaperChoice::Image => i == PRESETS.len(),
        };
        let hov = ui.hovered(A_WALL + i as u32);
        if let Some(c) = ui.c.as_deref_mut()
            && ui.view.intersection(&t).is_some()
        {
            if selected {
                c.stroke_rrect(t.inflated(3), 13, Corner::Circle, theme::accent(), 256);
                c.stroke_rrect(t.inflated(4), 14, Corner::Circle, theme::accent(), 256);
            } else if hov {
                c.stroke_rrect(t.inflated(3), 13, Corner::Circle, kit::ink3(), 200);
            }
            let name = if let Some(p) = PRESETS.get(i) {
                preview(c, t, p);
                p.name()
            } else {
                // The user's picture: a framed landscape on a dark tile.
                c.fill_rrect(t, 10, Corner::Circle, Color::rgb(0x2B, 0x2F, 0x45), 256);
                c.fill_rrect(
                    Rect::new(t.x + t.w / 2 + 14, t.y + 16, 14, 14),
                    7,
                    Corner::Circle,
                    Color::rgb(0xFF, 0xC2, 0x4A),
                    256,
                );
                c.fill_rrect(
                    Rect::new(t.x + 14, t.y + t.h - 34, t.w - 28, 26),
                    8,
                    Corner::Circle,
                    Color::rgb(0x3F, 0xB9, 0x8A),
                    256,
                );
                t!("settings.wp.yours")
            };
            text::draw_centered(
                c,
                Rect::new(t.x, t.bottom() + 6, t.w, 18),
                name,
                FOOTNOTE,
                if selected {
                    Weight::Medium
                } else {
                    Weight::Regular
                },
                if selected { kit::ink() } else { kit::ink2() },
            );
        }
        ui.hit(t.inflated(4), A_WALL + i as u32);
    }
    let rows = tiles.div_ceil(3) as i32;
    ui.y = top + rows * (th + 36) + 4;

    ui.header(t!("settings.wp.custom"));
    let card = ui.card(110);
    let field = Rect::new(card.x + 16, card.y + 16, card.w - 32 - 88 - 8, 28);
    let focus = ui.st.focus == Focus::Path;
    let path = String::from_utf8_lossy(&ui.st.path[..ui.st.path_len]).into_owned();
    ui.field(field, &path, t!("settings.wp.path_hint"), focus, A_PATH);
    ui.button(
        Rect::new(field.right() + 8, field.y, 88, 28),
        t!("settings.wp.apply"),
        ButtonKind::Primary,
        A_APPLY,
        ui.st.path_len > 0,
    );
    ui.button(
        Rect::new(card.x + 16, card.y + 62, 148, 28),
        t!("settings.wp.choose"),
        ButtonKind::Secondary,
        A_PICK,
        true,
    );
    if let Some((m, err)) = ui.st.message()
        && let Some(c) = ui.c.as_deref_mut()
        && ui.view.intersection(&card).is_some()
    {
        let r = Rect::new(
            card.x + 16 + 148 + 12,
            card.y + 54,
            card.w - 16 - 148 - 12 - 16,
            44,
        );
        let lines = text::wrap(&m, FOOTNOTE, Weight::Regular, r.w, 2);
        for (k, (a, b)) in lines.into_iter().enumerate() {
            text::draw(
                c,
                r.x,
                r.y + 4 + k as i32 * 16,
                &m[a..b],
                FOOTNOTE,
                Weight::Regular,
                if err { kit::red() } else { kit::ink2() },
            );
        }
    }
}

pub(super) fn page_dock(ui: &mut Ui<'_, '_>) {
    ui.title(t!("settings.sec.dock"));
    ui.header(t!("settings.dock.usage"));
    let card = ui.card(3 * ROW);
    for (i, (name, sub)) in [
        (t!("settings.dock.reorder"), t!("settings.dock.reorder_sub")),
        (t!("settings.dock.pin"), t!("settings.dock.pin_sub")),
        (
            t!("settings.dock.new_window"),
            t!("settings.dock.new_window_sub"),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let r = ui.row(card, i as i32);
        ui.label(r, name, sub, true);
    }
}

/// A wallpaper thumbnail: the gradient / solid colour and glows the preset paints.
fn preview(c: &mut Canvas, r: Rect, pr: &wallpaper::Preset) {
    let rgb = |v: u32| Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
    let sc = pr.scheme(theme::dark());
    let rad = 10;
    if pr.style == Style::Solid {
        c.fill_rrect(r, rad, Corner::Circle, rgb(sc.top), 256);
    } else {
        c.fill_rrect_vgrad(r, rad, Corner::Circle, rgb(sc.top), rgb(sc.bottom), 256);
    }
    if pr.style == Style::Glow {
        let saved = kit::clip_to(c, r.inflated(-1));
        let lut = kitsune_core::raster::glow_lut();
        for b in sc.blobs.iter().filter(|b| b.alpha > 0 && b.r > 0) {
            let cx = r.x + (r.w as i64 * b.x as i64 / 1000) as i32;
            let cy = r.y + (r.h as i64 * b.y as i64 / 1000) as i32;
            let rr = (r.w as i64 * b.r as i64 / 1000) as i32;
            c.glow(cx, cy, rr, rgb(b.color), b.alpha as u32, &lut);
        }
        c.restore_clip(saved);
    }
    c.stroke_rrect(r, rad, Corner::Circle, Color::rgb(0, 0, 0), 40);
}
