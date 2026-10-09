//! Ajustes: the preferences window. A sidebar of sections (Aparência, Papel de parede, Barra
//! de apps, Teclado, Data e hora, Rede, Disco, Energia, Sobre) and a page of grouped controls
//! built from the toolkit.
//!
//! The model (`osjeff_core::settings::Settings`), its text form, the wallpaper presets, the
//! time-zone table and the clock arithmetic are pure and tested in `osjeff_core`. This file is
//! the window. Each page is written once, as a function of a [`Ui`]; the same code draws the
//! page, finds what a click hit and what the pointer is over, so the three can never disagree
//! about where a control is. [`Desktop::settings_apply`] makes a new `Settings` take effect
//! at once (accent, appearance, keyboard, time zone, clock format, wallpaper, bar zoom) and
//! stores it through the `SettingsStore` (`/etc/osjeff.conf`).

use super::kit;
use super::ui::{self, ButtonKind};
use super::*;
use crate::text::{self, BODY, CALLOUT, CAPTION, FOOTNOTE, TITLE1, TITLE2, Weight};
use core::cell::Cell;
use osjeff_core::activity::{self, Glide};
use osjeff_core::hw::rtc::{DateTime, Field, local_to_utc, utc_to_local};
use osjeff_core::iconart::Glyph;
use osjeff_core::keymap::Layout;
use osjeff_core::settings::{
    ACCENT_NAMES, ACCENTS, DOCK_ZOOM_MAX, PATH_CAP, Settings, TIMEZONES, TOAST_SECS_MAX,
    TOAST_SECS_MIN, WallpaperChoice, search_timezones, utc_label,
};
use osjeff_core::sysif::{DiskUsage, SettingsStore};
use osjeff_core::wallpaper::{self, PRESETS, Style};
use osjeff_core::widgets as wg;

const SIDE_W: i32 = 208;
const ROW: i32 = 48;
/// Longest settings page text column.
const COL_MAX: i32 = 600;

/// The sections, in order: name, glyph and the colour of its badge.
const SECTIONS: [(&str, Glyph, u32); 9] = [
    ("Aparência", Glyph::Sun, 0x5B5CF6),
    ("Papel de parede", Glyph::Image, 0xEC4899),
    ("Barra de apps", Glyph::Dock, 0x14B8C4),
    ("Teclado", Glyph::Keyboard, 0x8E8E93),
    ("Data e hora", Glyph::Clock, 0xFB6F4B),
    ("Rede", Glyph::Network, 0x0A84FF),
    ("Disco", Glyph::Disk, 0x8E8E93),
    ("Energia", Glyph::Power, 0xFF9F0A),
    ("Sobre", Glyph::Info, 0x8B90A0),
];
/// Section numbers other code opens.
pub(crate) const ABOUT: u8 = 8;
const S_TIME: u8 = 4;

// Control ids (what a click or the pointer can hit).
const A_SEC: u32 = 0x100;
const A_THEME: u32 = 0x200;
const A_SWATCH: u32 = 0x210;
const A_MOTION: u32 = 0x220;
const A_TOASTS: u32 = 0x221;
const A_TOAST_SECS: u32 = 0x222;
const A_WALL: u32 = 0x230;
const A_PATH: u32 = 0x240;
const A_APPLY: u32 = 0x241;
const A_PICK: u32 = 0x242;
const A_ZOOM: u32 = 0x250;
const A_LAYOUT: u32 = 0x260;
const A_KBD: u32 = 0x264;
const A_CLOCK24: u32 = 0x270;
const A_TZSEARCH: u32 = 0x271;
const A_TZLIST: u32 = 0x272;
const A_UP: u32 = 0x280;
const A_DOWN: u32 = 0x290;
const A_SETTIME: u32 = 0x2A0;
const A_READTIME: u32 = 0x2A1;
const A_REBOOT: u32 = 0x2B0;
const A_SHUTDOWN: u32 = 0x2B1;
const A_TZ: u32 = 0x300;
const DOWN: u32 = 1 << 31;

/// Rows of the time-zone list shown at once.
const TZ_ROWS: i32 = 7;
const TZ_ROW_H: i32 = 28;

/// Which text field takes the keys.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    None,
    Path,
    TzSearch,
    Kbd,
}

/// Per-window state of the settings app.
pub(crate) struct SettingsState {
    section: u8,
    path: [u8; PATH_CAP],
    path_len: usize,
    focus: Focus,
    msg: Option<(String, bool)>,
    /// The date and time being edited (local time) on "Data e hora".
    edit: DateTime,
    kbd_test: Vec<u8>,
    tz_query: String,
    tz_scroll: Glide,
    /// Page scroll in pixels, and the overlay scrollbar.
    scroll: Glide,
    sb: wg::ScrollbarFade,
    content_h: Cell<i32>,
    /// What the pointer is over (a control id, top bit = pressed).
    hover: Cell<u32>,
    /// A slider being dragged (its control id).
    drag: u32,
    /// Switch knobs: reduzir movimento, notificações, 24 horas.
    knobs: [Glide; 3],
}

impl SettingsState {
    /// Show section `i` (the system menu's "Sobre" opens the last one).
    pub(crate) fn select_section(&mut self, i: u8) {
        self.section = i.min(SECTIONS.len() as u8 - 1);
        self.focus = Focus::None;
        self.scroll = Glide::at(0);
        if self.section == S_TIME {
            self.edit = read_local();
            self.tz_scroll = Glide::at(tz_scroll_for(crate::settings::get().tz_city, ""));
        }
    }

    pub(crate) fn new() -> Self {
        let s = crate::settings::get();
        let mut st = Self {
            section: 0,
            path: [0; PATH_CAP],
            path_len: 0,
            focus: Focus::None,
            msg: None,
            edit: read_local(),
            kbd_test: Vec::new(),
            tz_query: String::new(),
            tz_scroll: Glide::at(0),
            scroll: Glide::at(0),
            sb: wg::ScrollbarFade::new(),
            content_h: Cell::new(0),
            hover: Cell::new(0),
            drag: 0,
            knobs: [
                Glide::at(if s.reduce_motion { 256 } else { 0 }),
                Glide::at(if s.toasts { 256 } else { 0 }),
                Glide::at(if s.clock24 { 256 } else { 0 }),
            ],
        };
        let p = s.image_path();
        st.path[..p.len()].copy_from_slice(p);
        st.path_len = p.len();
        st
    }

    fn say(&mut self, m: &str, error: bool) {
        self.msg = Some((String::from(m), error));
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.kbd_test.capacity() + self.tz_query.capacity()
    }
}

/// Scroll position (pixels) that shows `city` in the middle of the zone list filtered by
/// `query`; 0 when it is not in the list.
fn tz_scroll_for(city: u8, query: &str) -> i32 {
    let found = search_timezones(query);
    let n = found.len() as i32;
    let Some(i) = found.iter().position(|&c| c == city) else {
        return 0;
    };
    let max = (n * TZ_ROW_H - TZ_ROWS * TZ_ROW_H).max(0);
    (i as i32 * TZ_ROW_H - (TZ_ROWS / 2) * TZ_ROW_H).clamp(0, max)
}

/// The RTC as local date and time.
fn read_local() -> DateTime {
    utc_to_local(crate::rtc::read_utc(), crate::rtc::tz_minutes())
}

// ------------------------------------------------------------------ immediate-mode page builder

struct Probe {
    px: i32,
    py: i32,
    /// Match this control wherever the pointer is (a slider being dragged).
    only: Option<u32>,
}

/// The page builder: draws when it has a canvas, finds a hit when it has a probe.
struct Ui<'u, 'a> {
    c: Option<&'u mut Canvas<'a>>,
    /// The visible part of the page.
    view: Rect,
    x: i32,
    w: i32,
    y: i32,
    probe: Option<Probe>,
    hit: Option<(u32, Rect)>,
    hv: u32,
    st: &'u SettingsState,
    s: Settings,
}

impl<'u, 'a> Ui<'u, 'a> {
    fn new(
        c: Option<&'u mut Canvas<'a>>,
        pane: Rect,
        st: &'u SettingsState,
        probe: Option<Probe>,
    ) -> Self {
        let w = (pane.w - 56).min(COL_MAX);
        Ui {
            c,
            view: pane,
            x: pane.x + 28,
            w,
            y: pane.y + 24 - st.scroll.value(),
            probe,
            hit: None,
            hv: st.hover.get(),
            st,
            s: crate::settings::get(),
        }
    }

    /// Register `r` as control `id` for a probe; `true` when the probe is on it.
    fn hit(&mut self, r: Rect, id: u32) -> bool {
        let Some(p) = &self.probe else {
            return false;
        };
        let inside = match p.only {
            Some(o) => o == id,
            None => r.contains(p.px, p.py) && self.view.contains(p.px, p.py),
        };
        if inside && self.hit.is_none() {
            self.hit = Some((id, r));
        }
        inside
    }

    fn hovered(&self, id: u32) -> bool {
        self.hv & !DOWN == id
    }

    fn state(&self, id: u32, enabled: bool) -> ui::Control {
        kit::control_state(self.hovered(id), self.hv & DOWN != 0, enabled)
    }

    /// The page title.
    fn title(&mut self, t: &str) {
        let r = Rect::new(self.x, self.y, self.w, 32);
        if let Some(c) = self.c.as_deref_mut() {
            text::draw_left(c, r, t, TITLE1, Weight::Semibold, kit::ink());
        }
        self.y += 48;
    }

    /// A small heading above a card.
    fn header(&mut self, t: &str) {
        let r = Rect::new(self.x + 4, self.y, self.w - 8, 20);
        if let Some(c) = self.c.as_deref_mut()
            && self.view.intersection(&r).is_some()
        {
            text::draw_left(c, r, t, FOOTNOTE, Weight::Medium, kit::ink2());
        }
        self.y += 26;
    }

    /// A card `h` high; the cursor moves past it.
    fn card(&mut self, h: i32) -> Rect {
        let r = Rect::new(self.x, self.y, self.w, h);
        if let Some(c) = self.c.as_deref_mut()
            && self.view.intersection(&r).is_some()
        {
            kit::card(c, r);
        }
        self.y += h + 22;
        r
    }

    /// Row `i` of a card.
    fn row(&mut self, card: Rect, i: i32) -> Rect {
        let r = Rect::new(card.x, card.y + i * ROW, card.w, ROW);
        if i > 0
            && let Some(c) = self.c.as_deref_mut()
            && self.view.intersection(&r).is_some()
        {
            ui::separator(c, Rect::new(r.x + 16, r.y, r.w - 32, 1));
        }
        r
    }

    /// A row's label (and an optional line under it) at the left.
    fn label(&mut self, r: Rect, t: &str, sub: &str, enabled: bool) {
        let Some(c) = self.c.as_deref_mut() else {
            return;
        };
        if self.view.intersection(&r).is_none() {
            return;
        }
        let col = if enabled { kit::ink() } else { kit::ink3() };
        if sub.is_empty() {
            text::draw_left(
                c,
                Rect::new(r.x + 16, r.y, r.w - 32 - 120, r.h),
                t,
                BODY,
                Weight::Regular,
                col,
            );
        } else {
            text::draw_ellipsis(
                c,
                r.x + 16,
                r.y + 8,
                r.w - 160,
                t,
                BODY,
                Weight::Regular,
                col,
            );
            text::draw_ellipsis(
                c,
                r.x + 16,
                r.y + 27,
                r.w - 160,
                sub,
                FOOTNOTE,
                Weight::Regular,
                kit::ink2(),
            );
        }
    }

    /// A row with a switch; `knob` is its animated position.
    fn row_switch(&mut self, card: Rect, i: i32, t: &str, sub: &str, knob: i32, id: u32) -> bool {
        let r = self.row(card, i);
        self.label(r, t, sub, true);
        let sw = wg::switch_rect(
            r.right() - 16 - wg::SWITCH_W,
            r.y + (ROW - wg::SWITCH_H) / 2,
        );
        if let Some(c) = self.c.as_deref_mut()
            && self.view.intersection(&r).is_some()
        {
            ui::switch(c, sw, knob, true);
        }
        self.hit(r, id)
    }

    /// A row with a slider and its value text.
    #[allow(clippy::too_many_arguments)]
    fn row_slider(
        &mut self,
        card: Rect,
        i: i32,
        t: &str,
        v: i32,
        (min, max): (i32, i32),
        value_text: &str,
        enabled: bool,
        id: u32,
    ) -> bool {
        let r = self.row(card, i);
        self.label(r, t, "", enabled);
        let slider = Rect::new(
            r.x + 190,
            r.y + (ROW - 24) / 2,
            (r.w - 190 - 16 - 64).max(60),
            24,
        );
        if let Some(c) = self.c.as_deref_mut()
            && self.view.intersection(&r).is_some()
        {
            ui::slider(c, slider, v, min, max, enabled);
            text::draw_right(
                c,
                Rect::new(r.right() - 16 - 56, r.y, 56, r.h),
                value_text,
                BODY,
                Weight::Regular,
                if enabled { kit::ink2() } else { kit::ink3() },
            );
        }
        if enabled {
            self.hit(slider.inflated(6), id)
        } else {
            false
        }
    }

    fn button(&mut self, r: Rect, t: &str, kind: ButtonKind, id: u32, enabled: bool) -> bool {
        let st = self.state(id, enabled);
        if let Some(c) = self.c.as_deref_mut()
            && self.view.intersection(&r).is_some()
        {
            ui::push_button(c, r, t, kind, st);
        }
        enabled && self.hit(r, id)
    }

    fn segmented(&mut self, r: Rect, labels: &[&str], sel: usize, id: u32) {
        if let Some(c) = self.c.as_deref_mut()
            && self.view.intersection(&r).is_some()
        {
            ui::segmented(c, r, labels, sel);
        }
        for (i, s) in wg::segmented_rects(r, labels.len()).into_iter().enumerate() {
            self.hit(s, id + i as u32);
        }
    }

    fn field(&mut self, r: Rect, t: &str, placeholder: &str, focus: bool, id: u32) {
        if let Some(c) = self.c.as_deref_mut()
            && self.view.intersection(&r).is_some()
        {
            ui::text_field(c, r, t, placeholder, focus, focus);
        }
        self.hit(r, id);
    }
}

// ------------------------------------------------------------------ pages

fn page(ui: &mut Ui<'_, '_>, d: &Desktop) {
    match ui.st.section {
        0 => page_appearance(ui),
        1 => page_wallpaper(ui),
        2 => page_dock(ui),
        3 => page_keyboard(ui),
        4 => page_time(ui),
        5 => page_network(ui, d),
        6 => page_disk(ui, d),
        7 => page_power(ui),
        _ => page_about(ui, d),
    }
    ui.st
        .content_h
        .set(ui.y + ui.st.scroll.value() - ui.view.y + 8);
}

fn page_appearance(ui: &mut Ui<'_, '_>) {
    use osjeff_core::style::AppearanceSetting as A;
    let s = ui.s;
    ui.title("Aparência");
    ui.header("Tema");
    let card = ui.card(ROW);
    let r = ui.row(card, 0);
    ui.label(r, "Tema", "", true);
    let sel = match s.appearance {
        A::Auto => 0,
        A::Light => 1,
        A::Dark => 2,
    };
    let seg = Rect::new(r.right() - 16 - 300, r.y + 10, 300, 28);
    ui.segmented(seg, &["Automático", "Claro", "Escuro"], sel, A_THEME);

    ui.header("Cor de destaque");
    let card = ui.card(96);
    let pitch = ((card.w - 32 - 28) / 7).max(30);
    for i in 0..8usize {
        let sw = Rect::new(card.x + 16 + i as i32 * pitch, card.y + 18, 28, 28);
        let hov = ui.hovered(A_SWATCH + i as u32);
        if let Some(c) = ui.c.as_deref_mut()
            && ui.view.intersection(&card).is_some()
        {
            let rgb = ACCENTS[i];
            let col = Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
            if s.accent as usize == i {
                c.stroke_rrect(sw.inflated(3), 17, Corner::Circle, col, 256);
                c.stroke_rrect(sw.inflated(4), 18, Corner::Circle, col, 256);
            } else if hov {
                c.stroke_rrect(sw.inflated(3), 17, Corner::Circle, col, 120);
            }
            c.fill_rrect(sw, 14, Corner::Circle, col, 256);
            c.stroke_rrect(sw, 14, Corner::Circle, Color::rgb(0, 0, 0), 36);
            let name = ACCENT_NAMES[i];
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

    ui.header("Movimento");
    let card = ui.card(ROW);
    let k = ui.st.knobs[0].value();
    ui.row_switch(
        card,
        0,
        "Reduzir movimento",
        "As transições terminam na hora",
        k,
        A_MOTION,
    );

    ui.header("Notificações");
    let card = ui.card(2 * ROW);
    let k = ui.st.knobs[1].value();
    ui.row_switch(card, 0, "Mostrar notificações", "", k, A_TOASTS);
    let secs = alloc::format!("{} s", s.toast_secs);
    ui.row_slider(
        card,
        1,
        "Duração",
        s.toast_secs as i32,
        (TOAST_SECS_MIN as i32, TOAST_SECS_MAX as i32),
        &secs,
        s.toasts,
        A_TOAST_SECS,
    );
}

fn page_wallpaper(ui: &mut Ui<'_, '_>) {
    let s = ui.s;
    ui.title("Papel de parede");
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
                p.name
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
                "Sua imagem"
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

    ui.header("Imagem do usuário");
    let card = ui.card(110);
    let field = Rect::new(card.x + 16, card.y + 16, card.w - 32 - 88 - 8, 28);
    let focus = ui.st.focus == Focus::Path;
    let path = String::from_utf8_lossy(&ui.st.path[..ui.st.path_len]).into_owned();
    ui.field(field, &path, "ex.: /Imagens/praia.png", focus, A_PATH);
    ui.button(
        Rect::new(field.right() + 8, field.y, 88, 28),
        "Aplicar",
        ButtonKind::Primary,
        A_APPLY,
        ui.st.path_len > 0,
    );
    ui.button(
        Rect::new(card.x + 16, card.y + 62, 148, 28),
        "Escolher imagem…",
        ButtonKind::Secondary,
        A_PICK,
        true,
    );
    if let Some((m, err)) = &ui.st.msg
        && let Some(c) = ui.c.as_deref_mut()
        && ui.view.intersection(&card).is_some()
    {
        let r = Rect::new(
            card.x + 16 + 148 + 12,
            card.y + 54,
            card.w - 16 - 148 - 12 - 16,
            44,
        );
        let lines = text::wrap(m, FOOTNOTE, Weight::Regular, r.w, 2);
        for (k, (a, b)) in lines.into_iter().enumerate() {
            text::draw(
                c,
                r.x,
                r.y + 4 + k as i32 * 16,
                &m[a..b],
                FOOTNOTE,
                Weight::Regular,
                if *err { kit::red() } else { kit::ink2() },
            );
        }
    }
}

fn page_dock(ui: &mut Ui<'_, '_>) {
    ui.title("Barra de apps");
    ui.header("Uso");
    let card = ui.card(3 * ROW);
    for (i, (name, sub)) in [
        ("Reordenar", "Arraste um ícone fixado para outra posição"),
        (
            "Fixar e desafixar",
            "Botão direito no ícone, em Fixar ou Desafixar",
        ),
        ("Nova janela", "Shift + clique no ícone do app"),
    ]
    .into_iter()
    .enumerate()
    {
        let r = ui.row(card, i as i32);
        ui.label(r, name, sub, true);
    }
}

fn page_keyboard(ui: &mut Ui<'_, '_>) {
    let s = ui.s;
    ui.title("Teclado");
    ui.header("Disposição");
    let card = ui.card(2 * ROW);
    for (i, (name, sub, l)) in [
        ("US", "Padrão internacional", Layout::Us),
        ("ABNT2", "Português do Brasil: ç e acentos", Layout::Abnt2),
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
    ui.header("Teste");
    let card = ui.card(ROW + 16);
    let field = Rect::new(card.x + 16, card.y + 14, card.w - 32, 28);
    let txt = text::from_bytes(&ui.st.kbd_test).into_owned();
    let focus = ui.st.focus == Focus::Kbd;
    ui.field(field, &txt, "Digite aqui para experimentar", focus, A_KBD);
}

fn page_time(ui: &mut Ui<'_, '_>) {
    let s = ui.s;
    ui.title("Data e hora");
    // Now.
    let now = read_local();
    ui.header("Agora");
    let card = ui.card(96);
    if let Some(c) = ui.c.as_deref_mut()
        && ui.view.intersection(&card).is_some()
    {
        let t = if s.clock24 {
            alloc::format!("{:02}:{:02}:{:02}", now.time.h, now.time.m, now.time.s)
        } else {
            let h12 = if now.time.h.is_multiple_of(12) {
                12
            } else {
                now.time.h % 12
            };
            alloc::format!(
                "{:02}:{:02}:{:02} {}",
                h12,
                now.time.m,
                now.time.s,
                if now.time.h < 12 { "AM" } else { "PM" }
            )
        };
        text::draw_left(
            c,
            Rect::new(card.x + 20, card.y + 14, card.w / 2, 40),
            &t,
            TITLE1,
            Weight::Semibold,
            kit::ink(),
        );
        const WD: [&str; 7] = [
            "domingo",
            "segunda-feira",
            "terça-feira",
            "quarta-feira",
            "quinta-feira",
            "sexta-feira",
            "sábado",
        ];
        const MON: [&str; 12] = [
            "janeiro",
            "fevereiro",
            "março",
            "abril",
            "maio",
            "junho",
            "julho",
            "agosto",
            "setembro",
            "outubro",
            "novembro",
            "dezembro",
        ];
        let d = alloc::format!(
            "{}, {} de {} de {}",
            WD[now.weekday() as usize % 7],
            now.date.d,
            MON[(now.date.m as usize).clamp(1, 12) - 1],
            now.date.y
        );
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
    ui.header("Formato");
    let card = ui.card(ROW);
    let k = ui.st.knobs[2].value();
    ui.row_switch(card, 0, "Relógio de 24 horas", "", k, A_CLOCK24);

    // The zone picker.
    ui.header("Fuso horário");
    let card = ui.card(48 + TZ_ROWS * TZ_ROW_H + 8);
    let sf = Rect::new(card.x + 12, card.y + 10, card.w - 24, 28);
    let focus = ui.st.focus == Focus::TzSearch;
    if let Some(c) = ui.c.as_deref_mut()
        && ui.view.intersection(&sf).is_some()
    {
        kit::search_field(c, sf, &ui.st.tz_query, "Buscar cidade", focus, focus);
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
        let (name, mins) = TIMEZONES[ci as usize];
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
            "Nenhuma cidade encontrada.",
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
    ui.header("Ajustar data e hora");
    let card = ui.card(150);
    let fields: [(Field, &str, i32); 6] = [
        (Field::Day, "Dia", 56),
        (Field::Month, "Mês", 56),
        (Field::Year, "Ano", 80),
        (Field::Hour, "Hora", 56),
        (Field::Minute, "Min", 56),
        (Field::Second, "Seg", 56),
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
        "Ler do relógio",
        ButtonKind::Secondary,
        A_READTIME,
        true,
    );
    ui.button(
        Rect::new(card.x + 20 + 148, card.y + 104, 100, 28),
        "Ajustar",
        ButtonKind::Primary,
        A_SETTIME,
        true,
    );
    if let Some((m, err)) = &ui.st.msg
        && let Some(c) = ui.c.as_deref_mut()
        && ui.view.intersection(&card).is_some()
    {
        text::draw_left(
            c,
            Rect::new(card.x + 20 + 256, card.y + 104, card.w - 296, 28),
            m,
            FOOTNOTE,
            Weight::Regular,
            if *err { kit::red() } else { kit::ink2() },
        );
    }
}

/// One `name .... value` row of a read-only card.
fn kv_row(ui: &mut Ui<'_, '_>, card: Rect, i: i32, name: &str, value: &str) {
    let r = ui.row(card, i);
    if let Some(c) = ui.c.as_deref_mut()
        && ui.view.intersection(&r).is_some()
    {
        kit::kv(c, Rect::new(r.x + 16, r.y, r.w - 32, r.h), name, value);
    }
}

fn page_network(ui: &mut Ui<'_, '_>, d: &Desktop) {
    use osjeff_core::netstats::NicKind;
    ui.title("Rede");
    let snap = crate::netd::stats();
    let has_nic = snap.nic != NicKind::None;
    let (word, col) = if !has_nic {
        ("Sem placa de rede", kit::ink3())
    } else if !snap.link_up {
        ("Sem sinal", kit::red())
    } else if snap.config.is_none() {
        ("Procurando endereço", kit::amber())
    } else {
        ("Conectado", kit::green())
    };
    ui.header("Conexão");
    let card = ui.card(ROW);
    let r = ui.row(card, 0);
    if let Some(c) = ui.c.as_deref_mut()
        && ui.view.intersection(&r).is_some()
    {
        kit::chip(c, r.x + 16, r.y + (ROW - 22) / 2, 22, word, col);
        if has_nic {
            text::draw_right(
                c,
                Rect::new(r.x, r.y, r.w - 16, r.h),
                snap.nic.name(),
                BODY,
                Weight::Regular,
                kit::ink2(),
            );
        }
    }
    ui.header("Endereços");
    let card = ui.card(5 * ROW);
    let m = crate::nic::mac();
    let mac = alloc::format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        m[0],
        m[1],
        m[2],
        m[3],
        m[4],
        m[5]
    );
    let none = String::from("—");
    let (mut ip, mut mask, mut gw, mut dns, mut lease) = (
        none.clone(),
        none.clone(),
        none.clone(),
        none.clone(),
        none.clone(),
    );
    if let Some(cfg) = snap.config {
        ip = alloc::format!("{}", cfg.ip);
        let mm = if cfg.prefix == 0 {
            0
        } else {
            u32::MAX << (32 - cfg.prefix as u32)
        };
        mask = alloc::format!(
            "{}.{}.{}.{}",
            mm >> 24,
            (mm >> 16) & 255,
            (mm >> 8) & 255,
            mm & 255
        );
        if let Some(g) = cfg.gateway {
            gw = alloc::format!("{g}");
        }
        if !cfg.dns.is_empty() {
            dns.clear();
            for (i, x) in cfg.dns.as_slice().iter().enumerate() {
                if i > 0 {
                    dns.push_str(", ");
                }
                dns.push_str(&alloc::format!("{x}"));
            }
        }
        lease = if cfg == osjeff_core::net::NetConfig::STATIC_FALLBACK {
            String::from("Estático")
        } else if let Some(ms) = snap.lease_remaining_ms {
            alloc::format!("{} restantes", activity::fmt_elapsed(ms / 1000))
        } else {
            String::from("Sem expiração")
        };
    }
    kv_row(ui, card, 0, "Endereço IP", &ip);
    kv_row(ui, card, 1, "Máscara", &mask);
    kv_row(ui, card, 2, "Roteador", &gw);
    kv_row(ui, card, 3, "DNS", &dns);
    kv_row(ui, card, 4, "Concessão", &lease);
    ui.header("Dispositivo e tráfego");
    let card = ui.card(4 * ROW);
    kv_row(
        ui,
        card,
        0,
        "Endereço físico",
        if has_nic { &mac } else { "—" },
    );
    let rx = alloc::format!(
        "{} · {}",
        activity::fmt_size(snap.rx_bytes),
        activity::fmt_speed(d.sysmon.rx_rate)
    );
    let tx = alloc::format!(
        "{} · {}",
        activity::fmt_size(snap.tx_bytes),
        activity::fmt_speed(d.sysmon.tx_rate)
    );
    kv_row(ui, card, 1, "Recebido", &rx);
    kv_row(ui, card, 2, "Enviado", &tx);
    let pk = alloc::format!(
        "{} recebidos · {} enviados",
        activity::fmt_count(snap.rx_packets),
        activity::fmt_count(snap.tx_packets)
    );
    kv_row(ui, card, 3, "Pacotes", &pk);
}

fn page_disk(ui: &mut Ui<'_, '_>, d: &Desktop) {
    ui.title("Disco");
    let usage = VfsUsage;
    let u = vfs::statfs();
    let (title, sub) = match vfs::volume() {
        vfs::Volume::Disk => (
            "Disco principal",
            String::from("Sistema de arquivos OJFS v3"),
        ),
        vfs::Volume::Memory => (
            "Memória (sem disco)",
            String::from("Os arquivos somem ao desligar"),
        ),
    };
    let _ = usage.label();
    ui.header("Volume");
    let card = ui.card(104);
    if let Some(c) = ui.c.as_deref_mut()
        && ui.view.intersection(&card).is_some()
    {
        text::draw_left(
            c,
            Rect::new(card.x + 20, card.y + 14, card.w / 2, 24),
            title,
            CALLOUT,
            Weight::Semibold,
            kit::ink(),
        );
        text::draw_left(
            c,
            Rect::new(card.x + 20, card.y + 38, card.w - 140, 18),
            &sub,
            FOOTNOTE,
            Weight::Regular,
            kit::ink2(),
        );
        let pm = u.used_permille();
        let pct = activity::fmt_pct(pm);
        text::draw_right(
            c,
            Rect::new(card.x, card.y + 14, card.w - 20, 28),
            kit::fb_str(&pct),
            TITLE2,
            Weight::Semibold,
            theme::accent(),
        );
        kit::bar(
            c,
            Rect::new(card.x + 20, card.y + 66, card.w - 40, 12),
            (pm as i64) << 8,
            if pm >= 900 {
                kit::red()
            } else {
                theme::accent()
            },
        );
    }
    let card = ui.card(4 * ROW);
    kv_row(
        ui,
        card,
        0,
        "Usado",
        kit::fb_str(&activity::fmt_size(u.used())),
    );
    kv_row(
        ui,
        card,
        1,
        "Livre",
        kit::fb_str(&activity::fmt_size(u.free)),
    );
    kv_row(
        ui,
        card,
        2,
        "Total",
        kit::fb_str(&activity::fmt_size(u.total)),
    );
    let files = if u.inodes_total > 0 {
        alloc::format!("{}", activity::fmt_count(u.inodes_used() as u64))
    } else {
        String::from("—")
    };
    kv_row(ui, card, 3, "Arquivos e pastas", &files);
    ui.header("Dispositivos");
    let card = ui.card(2 * ROW);
    for (i, label) in ["Disco de inicialização", "Disco de arquivos"]
        .iter()
        .enumerate()
    {
        let v = match d.disks.get(i).copied().flatten() {
            Some(dk) => alloc::format!(
                "{} · {}",
                dk.model_name(),
                activity::fmt_size(dk.mib() << 20)
            ),
            None => String::from("ausente"),
        };
        kv_row(ui, card, i as i32, label, &v);
    }
}

fn page_power(ui: &mut Ui<'_, '_>) {
    ui.title("Energia");
    let card = ui.card(2 * ROW);
    for (i, (name, label, id)) in [
        ("Reiniciar", "Reiniciar…", A_REBOOT),
        ("Desligar", "Desligar…", A_SHUTDOWN),
    ]
    .into_iter()
    .enumerate()
    {
        let r = ui.row(card, i as i32);
        ui.label(r, name, "", true);
        ui.button(
            Rect::new(r.right() - 16 - 120, r.y + 10, 120, 28),
            label,
            if i == 1 {
                ButtonKind::Destructive
            } else {
                ButtonKind::Secondary
            },
            id,
            true,
        );
    }
}

fn page_about(ui: &mut Ui<'_, '_>, d: &Desktop) {
    ui.title("Sobre");
    let head = Rect::new(ui.x, ui.y, ui.w, 88);
    if let Some(c) = ui.c.as_deref_mut()
        && ui.view.intersection(&head).is_some()
    {
        icons::blit(c, Icon::Brand, head.x, head.y, 72, 256);
        text::draw_left(
            c,
            Rect::new(head.x + 92, head.y + 6, head.w - 92, 32),
            "OSjeff",
            TITLE1,
            Weight::Semibold,
            kit::ink(),
        );
        let v = alloc::format!(
            "Versão {} · compilação {}",
            env!("CARGO_PKG_VERSION"),
            if cfg!(debug_assertions) {
                "de depuração"
            } else {
                "otimizada"
            }
        );
        text::draw_left(
            c,
            Rect::new(head.x + 92, head.y + 42, head.w - 92, 22),
            &v,
            BODY,
            Weight::Regular,
            kit::ink2(),
        );
    }
    ui.y += 104;
    let card = ui.card(6 * ROW);
    let si = crate::sysinfo::get();
    let cpu = si
        .map(|s| {
            let b = core::str::from_utf8(s.brand.as_bytes())
                .unwrap_or("")
                .trim();
            if b.is_empty() {
                String::from(core::str::from_utf8(s.vendor.as_bytes()).unwrap_or("—"))
            } else {
                String::from(b)
            }
        })
        .unwrap_or_else(|| String::from("—"));
    kv_row(ui, card, 0, "Processador", &cpu);
    let ram = si.map_or(String::from("—"), |s| {
        String::from(kit::fb_str(&activity::fmt_size(s.total_ram())))
    });
    kv_row(ui, card, 1, "Memória", &ram);
    let up = alloc::format!("{}", activity::fmt_clock(d.sysmon.uptime_s));
    kv_row(ui, card, 2, "Tempo ligado", &up);
    let res = si.map_or(String::from("—"), |s| {
        alloc::format!("{} × {} pixels", s.width, s.height)
    });
    kv_row(ui, card, 3, "Tela", &res);
    let boot = si.map_or(String::from("—"), |s| {
        let vm = if s.hypervisor.as_bytes().is_empty() {
            ""
        } else {
            " · máquina virtual"
        };
        alloc::format!("{}{vm}", s.boot_mode)
    });
    kv_row(ui, card, 4, "Inicialização", &boot);
    let sec = match crate::rng::quality() {
        osjeff_core::entropy::Quality::Strong => "Fonte forte de números aleatórios",
        osjeff_core::entropy::Quality::Mixed => "Números aleatórios de fonte mista",
        osjeff_core::entropy::Quality::Weak => "Números aleatórios de fonte fraca",
    };
    kv_row(ui, card, 5, "Segurança", sec);
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
        let lut = osjeff_core::raster::glow_lut();
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

// ------------------------------------------------------------------ geometry of the window

/// The content pane and the sidebar rows.
fn pane_of(r: Rect) -> Rect {
    let b = r.body();
    Rect::new(b.x + SIDE_W, b.y, b.w - SIDE_W, b.h)
}

fn side_rect(r: Rect, i: usize) -> Rect {
    Rect::new(r.x + 10, r.body().y + 12 + i as i32 * 36, SIDE_W - 20, 32)
}

// --------------------------------------------------------------------- actions

impl Desktop {
    fn settings_mut(&mut self, id: WindowId) -> Option<&mut SettingsState> {
        match self.app_mut(id) {
            Some(App::Settings(s)) => Some(s),
            _ => None,
        }
    }

    /// Make `new` the settings in effect: accent, keyboard layout, time zone, clock format,
    /// bar zoom and toasts apply at once, a changed wallpaper or accent asks the compositor
    /// to repaint the background, and the text form is stored through the `SettingsStore`
    /// (`/etc/osjeff.conf`).
    pub(crate) fn settings_apply(
        &mut self,
        new: Settings,
    ) -> Result<(), osjeff_core::sysif::SinkError> {
        self.settings_apply_live(new);
        VfsStore.save(&new.to_text())
    }

    /// Make `new` take effect without storing it (a slider being dragged: the file is
    /// written once, when the button is released).
    fn settings_apply_live(&mut self, new: Settings) {
        let old = crate::settings::get();
        crate::settings::set(new);
        self.keymap.set_layout(new.layout);
        if new.wallpaper != old.wallpaper
            || new.image_path() != old.image_path()
            || new.accent != old.accent
        {
            self.bg_dirty = true;
        }
        if new.lang != old.lang {
            self.language_changed(old.lang);
        }
        if new.appearance != old.appearance {
            // Re-resolve the look now (Auto follows the clock, the others are fixed).
            self.poll_appearance(crate::rtc::now().h);
        }
        // Everything on screen may change (clock text, accent colours).
        self.force_full = true;
    }

    /// Store the settings in effect (after a drag).
    pub(crate) fn settings_persist(&mut self) {
        let _ = VfsStore.save(&crate::settings::get().to_text());
    }

    /// Load the stored settings at boot (before the first wallpaper paint).
    pub fn load_settings(&mut self) {
        match VfsStore.load() {
            Some(text) => {
                let s = Settings::parse(&text);
                crate::settings::set(s);
                self.keymap.set_layout(s.layout);
                crate::klog!(
                    Info,
                    "settings: loaded {} bytes from osjeff.conf",
                    text.len()
                );
            }
            None => crate::settings::set(Settings::default()),
        }
        // The first wallpaper is painted in the look the clock asks for.
        let hour = crate::rtc::now().h;
        crate::theme::set_appearance(crate::settings::get().appearance.resolve(hour));
        self.shell.last_hour = hour;
    }

    /// Use the image at `path` as the wallpaper (file manager, viewer). `Some(message)`
    /// is the reason it was refused.
    pub(crate) fn set_wallpaper_path(&mut self, path: &[u8]) -> Option<String> {
        let (w, h) = (self.sw as usize, self.sh as usize);
        let mut s = crate::settings::get();
        if !s.set_image_path(path) {
            return Some(String::from(
                "Caminho inválido para papel de parede (use letras, números . _ - /).",
            ));
        }
        match read_path(path) {
            None => return Some(String::from("Arquivo não encontrado.")),
            Some(bytes) => {
                if let Err(e) = wallpaper::load(&bytes, w, h) {
                    return Some(alloc::format!("Imagem recusada: {e}"));
                }
            }
        }
        s.wallpaper = WallpaperChoice::Image;
        match self.settings_apply(s) {
            Ok(()) => None,
            Err(_) => Some(String::from("Papel de parede aplicado, mas não foi salvo.")),
        }
    }

    /// Did the wallpaper or accent change since the compositor last painted it?
    /// Consumed by the main loop, which repaints the cached background.
    pub fn take_bg_repaint(&mut self) -> bool {
        core::mem::take(&mut self.bg_dirty)
    }

    /// Apply the path typed on the wallpaper page.
    fn settings_use_path(&mut self, id: WindowId) {
        let Some(st) = self.settings_mut(id) else {
            return;
        };
        let path = st.path[..st.path_len].to_vec();
        if path.is_empty() {
            return;
        }
        match self.set_wallpaper_path(&path) {
            Some(m) => {
                if let Some(st) = self.settings_mut(id) {
                    st.say(&m, true);
                }
            }
            None => {
                if let Some(st) = self.settings_mut(id) {
                    st.say("Papel de parede aplicado.", false);
                }
            }
        }
    }

    /// Run the page in probe mode at `(px, py)`: the control there and its rectangle.
    fn settings_probe(
        &self,
        r: Rect,
        st: &SettingsState,
        px: i32,
        py: i32,
        only: Option<u32>,
    ) -> Option<(u32, Rect)> {
        // The sidebar.
        if only.is_none() {
            for i in 0..SECTIONS.len() {
                if side_rect(r, i).contains(px, py) {
                    return Some((A_SEC + i as u32, side_rect(r, i)));
                }
            }
        }
        let pane = pane_of(r);
        let mut ui = Ui::new(None, pane, st, Some(Probe { px, py, only }));
        page(&mut ui, self);
        ui.hit
    }

    pub(crate) fn settings_key(&mut self, id: WindowId, key: Key) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let view_h = pane_of(rect).h;
        let Some(st) = self.settings_mut(id) else {
            return;
        };
        match st.focus {
            Focus::Path => {
                match key {
                    Key::Enter => {
                        self.settings_use_path(id);
                        return;
                    }
                    Key::Esc => st.focus = Focus::None,
                    Key::Backspace => st.path_len = st.path_len.saturating_sub(1),
                    Key::Char(b) if (0x21..0x7F).contains(&b) && st.path_len < PATH_CAP => {
                        st.path[st.path_len] = b;
                        st.path_len += 1;
                    }
                    _ => {}
                }
                return;
            }
            Focus::TzSearch => {
                match key {
                    Key::Esc => {
                        st.tz_query.clear();
                        st.focus = Focus::None;
                    }
                    Key::Backspace => {
                        st.tz_query.pop();
                    }
                    Key::Enter => {
                        let first = search_timezones(&st.tz_query).first().copied();
                        if let Some(i) = first {
                            let mut s = crate::settings::get();
                            s.set_city(i);
                            let _ = self.settings_apply(s);
                            if let Some(st) = self.settings_mut(id) {
                                st.edit = read_local();
                            }
                        }
                        return;
                    }
                    Key::Char(b) if (0x20..0x7F).contains(&b) && st.tz_query.len() < 24 => {
                        st.tz_query.push(b as char);
                    }
                    _ => return,
                }
                st.tz_scroll = Glide::at(0);
                return;
            }
            Focus::Kbd => {
                match key {
                    Key::Esc => st.focus = Focus::None,
                    Key::Backspace => {
                        st.kbd_test.pop();
                    }
                    Key::Char(b) if st.kbd_test.len() < 40 => st.kbd_test.push(b),
                    _ => {}
                }
                return;
            }
            Focus::None => {}
        }
        let n = SECTIONS.len() as u8;
        let max = (st.content_h.get() - view_h).max(0);
        match key {
            Key::Esc => {
                self.request_close(id);
                return;
            }
            Key::Tab | Key::Down => st.select_section((st.section + 1) % n),
            Key::Up => st.select_section((st.section + n - 1) % n),
            Key::PageDown => st
                .scroll
                .set((st.scroll.target() + view_h - 40).clamp(0, max)),
            Key::PageUp => st
                .scroll
                .set((st.scroll.target() - view_h + 40).clamp(0, max)),
            Key::Home => st.scroll.set(0),
            Key::End => st.scroll.set(max),
            _ => return,
        }
        st.sb.touch(super::toasts_ui::now_ms());
    }

    pub(crate) fn settings_wheel(&mut self, id: WindowId, notches: i32) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let (cx, cy) = (self.cursor_x, self.cursor_y);
        let view_h = pane_of(rect).h;
        let over_list = {
            let Some(App::Settings(st)) = self.wm.get(id).map(|w| &w.app.app) else {
                return;
            };
            self.settings_probe(rect, st, cx, cy, None)
                .is_some_and(|(h, _)| h == A_TZLIST || h >= A_TZ)
        };
        let Some(st) = self.settings_mut(id) else {
            return;
        };
        if over_list {
            let n = search_timezones(&st.tz_query).len() as i32;
            let max = (n * TZ_ROW_H - TZ_ROWS * TZ_ROW_H).max(0);
            st.tz_scroll
                .set((st.tz_scroll.target() + notches * TZ_ROW_H * 2).clamp(0, max));
        } else {
            let max = (st.content_h.get() - view_h).max(0);
            st.scroll
                .set((st.scroll.target() + notches * 56).clamp(0, max));
        }
        st.sb.touch(super::toasts_ui::now_ms());
    }

    pub(crate) fn settings_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        let hit = {
            let Some(App::Settings(st)) = self.wm.get(id).map(|w| &w.app.app) else {
                return;
            };
            self.settings_probe(rect, st, px, py, None)
        };
        let Some(st) = self.settings_mut(id) else {
            return;
        };
        st.msg = None;
        st.focus = Focus::None;
        let Some((hid, hrect)) = hit else {
            return;
        };
        let mut s = crate::settings::get();
        match hid {
            h if (A_SEC..A_SEC + 16).contains(&h) => {
                st.select_section((h - A_SEC) as u8);
                return;
            }
            h if (A_THEME..A_THEME + 3).contains(&h) => {
                use osjeff_core::style::AppearanceSetting as A;
                s.appearance = [A::Auto, A::Light, A::Dark][(h - A_THEME) as usize];
            }
            h if (A_SWATCH..A_SWATCH + 8).contains(&h) => s.accent = (h - A_SWATCH) as u8,
            A_MOTION => s.reduce_motion = !s.reduce_motion,
            A_TOASTS => s.toasts = !s.toasts,
            A_TOAST_SECS | A_ZOOM => {
                st.drag = hid;
                self.settings_slider(id, hid, hrect, px, &mut s);
                self.settings_apply_live(s);
                self.drag = Some(Drag {
                    win: id,
                    mode: DragMode::Ui,
                });
                return;
            }
            h if (A_WALL..A_WALL + 8).contains(&h) => {
                let i = (h - A_WALL) as usize;
                if i < PRESETS.len() {
                    s.wallpaper = WallpaperChoice::Preset(i as u8);
                } else if st.path_len == 0 {
                    st.say(
                        "Escolha uma imagem: digite o caminho abaixo ou use o Arquivos.",
                        false,
                    );
                    return;
                } else {
                    self.settings_use_path(id);
                    return;
                }
            }
            A_PATH => {
                st.focus = Focus::Path;
                return;
            }
            A_APPLY => {
                self.settings_use_path(id);
                return;
            }
            A_PICK => {
                st.say(
                    "No Arquivos, clique com o botão direito na imagem e escolha “Definir como papel de parede”.",
                    false,
                );
                self.launch(Kind::Files);
                return;
            }
            h if (A_LAYOUT..A_LAYOUT + 2).contains(&h) => {
                s.layout = if h == A_LAYOUT {
                    Layout::Us
                } else {
                    Layout::Abnt2
                };
            }
            A_KBD => {
                st.focus = Focus::Kbd;
                return;
            }
            A_CLOCK24 => s.clock24 = !s.clock24,
            A_TZSEARCH => {
                st.focus = Focus::TzSearch;
                return;
            }
            A_TZLIST => return,
            h if h >= A_TZ && h < A_TZ + TIMEZONES.len() as u32 => {
                s.set_city((h - A_TZ) as u8);
                let _ = self.settings_apply(s);
                if let Some(st) = self.settings_mut(id) {
                    st.edit = read_local();
                }
                return;
            }
            h if (A_UP..A_UP + 6).contains(&h) || (A_DOWN..A_DOWN + 6).contains(&h) => {
                let (i, d) = if h >= A_DOWN {
                    ((h - A_DOWN) as usize, -1)
                } else {
                    ((h - A_UP) as usize, 1)
                };
                let f = [
                    Field::Day,
                    Field::Month,
                    Field::Year,
                    Field::Hour,
                    Field::Minute,
                    Field::Second,
                ][i];
                st.edit = st.edit.step(f, d);
                return;
            }
            A_READTIME => {
                st.edit = read_local();
                st.say("Lido do relógio.", false);
                return;
            }
            A_SETTIME => {
                let tzm = crate::rtc::tz_minutes();
                if st.edit.is_valid() {
                    crate::rtc::set_utc(&local_to_utc(st.edit, tzm));
                    st.say("Relógio ajustado.", false);
                    crate::klog!(Info, "rtc: clock set by the user");
                } else {
                    st.say("Data inválida.", true);
                }
                return;
            }
            A_REBOOT => {
                self.ask_power(false);
                return;
            }
            A_SHUTDOWN => {
                self.ask_power(true);
                return;
            }
            _ => return,
        }
        let _ = self.settings_apply(s);
    }

    /// Set the slider `hid` (whose rectangle is `rect`) from a pointer at `px`.
    fn settings_slider(&mut self, _id: WindowId, hid: u32, rect: Rect, px: i32, s: &mut Settings) {
        let track = rect.inflated(-6);
        match hid {
            A_TOAST_SECS => {
                s.toast_secs =
                    wg::slider_value(track, px, TOAST_SECS_MIN as i32, TOAST_SECS_MAX as i32) as u8;
            }
            A_ZOOM => {
                s.dock_zoom = wg::slider_value(track, px, 0, DOCK_ZOOM_MAX as i32) as u8;
            }
            _ => {}
        }
    }

    /// A slider drag in window `id` reached `(cx, cy)`.
    pub(crate) fn settings_drag(&mut self, id: WindowId, cx: i32, cy: i32) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let (drag, hit) = {
            let Some(App::Settings(st)) = self.wm.get(id).map(|w| &w.app.app) else {
                return;
            };
            if st.drag == 0 {
                return;
            }
            (
                st.drag,
                self.settings_probe(rect, st, cx, cy, Some(st.drag)),
            )
        };
        let Some((_, hrect)) = hit else {
            return;
        };
        let mut s = crate::settings::get();
        self.settings_slider(id, drag, hrect, cx, &mut s);
        if s != crate::settings::get() {
            self.settings_apply_live(s);
        }
    }

    /// Hover keys of every settings window for a pointer at `(cx, cy)`.
    pub(crate) fn settings_hover(&mut self, cx: i32, cy: i32, down: bool) -> bool {
        let top = if self.overlay_open() {
            None
        } else {
            self.topmost_at(cx, cy)
        };
        let mut changed = false;
        let mut dirty = Vec::new();
        for w in self.wm.windows() {
            let App::Settings(st) = &w.app.app else {
                continue;
            };
            if !w.shown() {
                continue;
            }
            let mut key = 0;
            if Some(w.id) == top && w.rect.body().contains(cx, cy) {
                key = self
                    .settings_probe(w.rect, st, cx, cy, None)
                    .map_or(0, |(h, _)| h);
                if key != 0 && down {
                    key |= DOWN;
                }
            }
            if key != st.hover.get() {
                st.hover.set(key);
                changed = true;
                dirty.push(self.window_box(w));
            }
        }
        for r in dirty {
            self.mark_dirty(r);
        }
        changed
    }

    pub(crate) fn settings_step(&mut self, dt_ms: u32) {
        if !self
            .wm
            .windows()
            .iter()
            .any(|w| matches!(w.app.app, App::Settings(_)))
        {
            return;
        }
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Settings(_)))
            .map(|w| w.id)
            .collect();
        let s = crate::settings::get();
        for id in ids {
            if let Some(st) = self.settings_mut(id) {
                st.knobs[0].set(if s.reduce_motion { 256 } else { 0 });
                st.knobs[1].set(if s.toasts { 256 } else { 0 });
                st.knobs[2].set(if s.clock24 { 256 } else { 0 });
                for k in st.knobs.iter_mut() {
                    k.step(dt_ms, 60);
                }
                st.scroll.step(dt_ms, 80);
                st.tz_scroll.step(dt_ms, 80);
            }
        }
    }

    pub(crate) fn settings_busy_one(&self, w: &Win) -> bool {
        let App::Settings(st) = &w.app.app else {
            return false;
        };
        w.shown()
            && (st.knobs.iter().any(|k| k.moving())
                || st.scroll.moving()
                || st.tz_scroll.moving()
                || st.sb.active(super::toasts_ui::now_ms())
                || {
                    let s = crate::settings::get();
                    [s.reduce_motion, s.toasts, s.clock24]
                        .iter()
                        .zip(&st.knobs)
                        .any(|(on, k)| k.target() != if *on { 256 } else { 0 })
                })
    }

    // ------------------------------------------------------------------ drawing

    pub(crate) fn draw_settings(&self, c: &mut Canvas, r: Rect, st: &SettingsState) {
        let p = theme::pal();
        let body = r.body();
        // The sidebar.
        ui::fill(
            c,
            Rect::new(body.x, body.y, SIDE_W, body.h),
            theme::sidebar(),
        );
        ui::separator(c, Rect::new(body.x + SIDE_W - 1, body.y, 1, body.h));
        let hv = st.hover.get() & !DOWN;
        for (i, (name, glyph, rgb)) in SECTIONS.iter().enumerate() {
            let rr = side_rect(r, i);
            let selected = st.section as usize == i;
            let fg = if selected {
                c.fill_rrect(rr, 8, Corner::Circle, theme::accent(), 256);
                theme::ACCENT_TEXT
            } else {
                if hv == A_SEC + i as u32 {
                    ui::fill_token(c, rr, 8, p.hover);
                }
                kit::ink()
            };
            let badge = Rect::new(rr.x + 8, rr.y + 5, 22, 22);
            let col = Color::rgb((*rgb >> 16) as u8, (*rgb >> 8) as u8, *rgb as u8);
            c.fill_rrect(badge, 6, Corner::Circle, col, 256);
            ui::draw_glyph(c, *glyph, badge.x + 3, badge.y + 3, 16, 0xFFFF_FFFF);
            text::draw_left(
                c,
                Rect::new(badge.right() + 10, rr.y, rr.w - 50, rr.h),
                name,
                BODY,
                if selected {
                    Weight::Medium
                } else {
                    Weight::Regular
                },
                fg,
            );
        }
        // The page.
        let pane = pane_of(r);
        let saved = kit::clip_to(c, pane);
        let mut ui = Ui::new(Some(c), pane, st, None);
        page(&mut ui, self);
        let c = ui.c.take().expect("canvas");
        c.restore_clip(saved);
        let total = st.content_h.get().max(1);
        let rows = pane.h.max(1);
        ui::overlay_scrollbar(
            c,
            Rect::new(pane.right() - 10, pane.y + 4, 10, pane.h - 8),
            st.scroll.value().max(0) as usize,
            total as usize,
            rows as usize,
            st.sb.alpha(super::toasts_ui::now_ms()),
        );
    }
}
