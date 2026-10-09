//! The component gallery (Ctrl+Alt+G, also in the system menu): every widget of the
//! toolkit, the type scale, the colour tokens and the icon set on one page, live
//! in the current appearance. It is how the design is checked and how wave-2 app
//! authors see what they can reuse.

use crate::desktop::kit::ui::{ButtonKind, Control};
use crate::desktop::*;
use crate::text::{self, BODY, CALLOUT, CAPTION, FOOTNOTE, TITLE1, TITLE2, TITLE3, Weight};
use kitsune_core::iconart::Glyph;
use kitsune_core::style::{LIGHT, Palette};
use kitsune_core::{t, tk};

const TABS: [&str; 5] = [
    tk!("kit.tab.controls"),
    tk!("kit.tab.type"),
    tk!("kit.tab.colors"),
    tk!("kit.tab.icons"),
    tk!("kit.tab.shell"),
];

/// State of the gallery window: the widgets it shows are real and interactive.
pub(crate) struct GalleryState {
    pub tab: usize,
    pub segment: usize,
    pub switch_on: bool,
    pub slider: i32,
    pub checks: [bool; 2],
    pub radio: usize,
    pub field: String,
    pub field_focus: bool,
    pub list_sel: usize,
}

impl GalleryState {
    pub(crate) fn new() -> GalleryState {
        GalleryState {
            tab: 0,
            segment: 1,
            switch_on: true,
            slider: 40,
            checks: [true, false],
            radio: 0,
            field: String::new(),
            field_focus: false,
            list_sel: 1,
        }
    }
}

/// Rectangles of the interactive widgets of the "Controles" page.
struct Layout {
    tabs: Rect,
    segmented: Rect,
    switch: Rect,
    slider: Rect,
    checks: [Rect; 2],
    radios: [Rect; 3],
    field: Rect,
    list: Rect,
}

fn layout(body: Rect) -> Layout {
    let x = body.x + 24;
    let col2 = body.x + 556;
    let mut y = body.y + 60;
    let tabs = Rect::new(body.x + 24, body.y + 16, 480, 28);
    let _ = y;
    // Row 1 is the buttons (drawn, not interactive here).
    y += 40;
    let segmented = Rect::new(x, y, 300, 28);
    y += 44;
    let switch = crate::desktop::wlogic::switch_rect(x, y);
    let slider = Rect::new(x + 60, y - 2, 240, 26);
    y += 40;
    let checks = [Rect::new(x, y, 150, 20), Rect::new(x + 160, y, 150, 20)];
    y += 32;
    let radios = [
        Rect::new(x, y, 96, 20),
        Rect::new(x + 100, y, 96, 20),
        Rect::new(x + 200, y, 96, 20),
    ];
    y += 36;
    let field = Rect::new(x, y, 300, 30);
    let list = Rect::new(col2, body.y + 60, (body.w - 556 - 24).max(160), 132);
    Layout {
        tabs,
        segmented,
        switch,
        slider,
        checks,
        radios,
        field,
        list,
    }
}

impl Desktop {
    pub(crate) fn draw_gallery(&self, c: &mut Canvas, r: Rect, g: &GalleryState) {
        let p = theme::pal();
        let body = r.body();
        c.fill_rect(
            body.x.max(0) as usize,
            body.y.max(0) as usize,
            body.w.max(0) as usize,
            body.h.max(0) as usize,
            theme::solid(p.window_bg),
        );
        let l = layout(body);
        let tabs = TABS.map(kitsune_core::i18n::tr);
        ui::segmented(c, l.tabs, &tabs, g.tab);
        let pad = 24;
        match g.tab {
            0 => self.gallery_controls(c, body, g, &l, p),
            1 => gallery_type(c, body, pad, p),
            2 => gallery_colors(c, body, pad, p),
            3 => gallery_icons(c, body, pad, p),
            _ => self.gallery_shell(c, body, pad, p),
        }
    }

    fn gallery_controls(
        &self,
        c: &mut Canvas,
        body: Rect,
        g: &GalleryState,
        l: &Layout,
        p: &Palette,
    ) {
        let x = body.x + 24;
        let mut by = body.y + 60;
        // Buttons in their states.
        for (i, (label, kind, st)) in [
            (
                t!("kit.btn.default"),
                ButtonKind::Secondary,
                Control::Normal,
            ),
            (t!("kit.btn.primary"), ButtonKind::Primary, Control::Normal),
            (
                t!("kit.btn.destructive"),
                ButtonKind::Destructive,
                Control::Normal,
            ),
            (
                t!("kit.btn.pressed"),
                ButtonKind::Secondary,
                Control::Pressed,
            ),
            (
                t!("kit.state.disabled"),
                ButtonKind::Primary,
                Control::Disabled,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let bw = if i == 3 || i == 4 { 108 } else { 84 };
            let off = [0, 92, 184, 276, 392][i];
            ui::push_button(c, Rect::new(x + off, by, bw, 28), label, kind, st);
        }
        by += 40;
        let _ = by;
        ui::segmented(
            c,
            l.segmented,
            &[t!("kit.seg.day"), t!("kit.seg.week"), t!("kit.seg.month")],
            g.segment,
        );
        ui::switch(c, l.switch, if g.switch_on { 256 } else { 0 }, true);
        ui::slider(c, l.slider, g.slider, 0, 100, true);
        let v = alloc::format!("{}", g.slider);
        text::draw_left(
            c,
            Rect::new(l.slider.right() + 12, l.slider.y, 40, l.slider.h),
            &v,
            BODY,
            Weight::Regular,
            theme::solid(p.text_secondary),
        );
        for (i, (cr, label)) in l
            .checks
            .iter()
            .zip([t!("kit.check.remember"), t!("kit.check.notify")])
            .enumerate()
        {
            ui::checkbox(c, cr.x, cr.y + 2, g.checks[i]);
            text::draw_left(
                c,
                Rect::new(cr.x + 24, cr.y, cr.w - 24, cr.h),
                label,
                BODY,
                Weight::Regular,
                theme::solid(p.text),
            );
        }
        for (i, (rr, label)) in l
            .radios
            .iter()
            .zip([
                t!("kit.radio.first"),
                t!("kit.radio.second"),
                t!("kit.radio.third"),
            ])
            .enumerate()
        {
            ui::radio(c, rr.x, rr.y + 2, g.radio == i);
            text::draw_left(
                c,
                Rect::new(rr.x + 24, rr.y, rr.w - 24, rr.h),
                label,
                BODY,
                Weight::Regular,
                theme::solid(p.text),
            );
        }
        ui::text_field(
            c,
            l.field,
            &g.field,
            t!("kit.field"),
            g.field_focus,
            g.field_focus,
        );
        // Progress, below the field.
        let py = l.field.bottom() + 24;
        ui::progress(c, Rect::new(x, py, 300, 6), 640);
        // Right column: a grouped list, a menu preview and a tooltip.
        ui::group_box(c, l.list);
        for (i, label) in [
            t!("launcher.recents"),
            t!("kit.list.documents"),
            t!("kit.list.images"),
            t!("kit.list.downloads"),
        ]
        .iter()
        .enumerate()
        {
            let row = Rect::new(l.list.x + 4, l.list.y + 6 + i as i32 * 30, l.list.w - 8, 30);
            ui::list_row(c, row, g.list_sel == i, false, label);
        }
        let mx = l.list.x;
        let my = l.list.bottom() + 20;
        let menu = Rect::new(mx, my, 220, 24 * 3 + 12);
        c.draw_shadow(
            menu,
            Shadow {
                blur: 12,
                dy: 8,
                alpha: 70,
            },
            Rect::new(menu.x, menu.y + 8, menu.w, menu.h - 16),
        );
        ui::fill_token(c, menu, kitsune_core::style::R_MENU + 2, p.menu_tint);
        ui::stroke_token(c, menu, kitsune_core::style::R_MENU + 2, p.separator);
        ui::menu_item(
            c,
            Rect::new(menu.x + 6, menu.y + 6, menu.w - 12, 24),
            t!("menu.file.new_window"),
            "Ctrl+N",
            true,
            true,
            false,
        );
        ui::menu_item(
            c,
            Rect::new(menu.x + 6, menu.y + 30, menu.w - 12, 24),
            t!("kit.menu.show_bar"),
            "",
            false,
            true,
            true,
        );
        ui::menu_item(
            c,
            Rect::new(menu.x + 6, menu.y + 54, menu.w - 12, 24),
            t!("kit.state.disabled"),
            "",
            false,
            false,
            false,
        );
        ui::tooltip(c, mx + 60, my + menu.h + 44, t!("kit.tooltip"));
        let _ = (CAPTION, FOOTNOTE);
    }

    /// A click in gallery window `id` at screen `(x, y)`.
    pub(crate) fn gallery_click(&mut self, id: WindowId, rect: Rect, x: i32, y: i32) {
        let l = layout(rect.body());
        let Some(App::Gallery(g)) = self.app_mut(id) else {
            return;
        };
        g.field_focus = false;
        if let Some(i) = crate::desktop::wlogic::segmented_hit(l.tabs, TABS.len(), x, y) {
            g.tab = i;
            return;
        }
        if g.tab != 0 {
            return;
        }
        if let Some(i) = crate::desktop::wlogic::segmented_hit(l.segmented, 3, x, y) {
            g.segment = i;
        } else if l.switch.inflated(4).contains(x, y) {
            g.switch_on = !g.switch_on;
        } else if l.slider.contains(x, y) {
            g.slider = crate::desktop::wlogic::slider_value(l.slider, x, 0, 100);
        } else if let Some(i) = l.checks.iter().position(|r| r.contains(x, y)) {
            g.checks[i] = !g.checks[i];
        } else if let Some(i) = l.radios.iter().position(|r| r.contains(x, y)) {
            g.radio = i;
        } else if l.field.contains(x, y) {
            g.field_focus = true;
        } else if l.list.contains(x, y) {
            g.list_sel = ((y - l.list.y - 6) / 30).clamp(0, 3) as usize;
        }
    }

    /// Typing into the gallery's text field.
    pub(crate) fn gallery_key(&mut self, id: WindowId, key: Key) {
        let Some(App::Gallery(g)) = self.app_mut(id) else {
            return;
        };
        match key {
            Key::Char(b) if g.field_focus && (0x20..0x7F).contains(&b) && g.field.len() < 40 => {
                g.field.push(char::from(b));
            }
            Key::Backspace if g.field_focus => {
                g.field.pop();
            }
            Key::Esc => g.field_focus = false,
            Key::Left | Key::Right => {
                let n = TABS.len();
                g.tab = if key == Key::Right {
                    (g.tab + 1) % n
                } else {
                    (g.tab + n - 1) % n
                };
            }
            _ => {}
        }
    }
}

impl Desktop {
    /// The shell's own parts: window buttons (rest, hover, close hover, restore), Quick Settings
    /// tiles, the taskbar indicators, the snap preview and the pointers.
    fn gallery_shell(&self, c: &mut Canvas, body: Rect, pad: i32, p: &Palette) {
        use kitsune_core::window::TitleBtn;
        let label = |c: &mut Canvas, x: i32, y: i32, t: &str| {
            text::draw(
                c,
                x,
                y,
                t,
                FOOTNOTE,
                Weight::Medium,
                theme::solid(p.text_secondary),
            );
        };
        let x = body.x + pad;
        let mut y = body.y + 60;
        label(c, x, y, t!("kit.shell.window_buttons"));
        y += 22;
        for (i, (hover, restore)) in [
            (None, false),
            (Some(TitleBtn::Maximize), false),
            (Some(TitleBtn::Close), false),
            (None, true),
        ]
        .into_iter()
        .enumerate()
        {
            let bar = Rect::new(x + i as i32 * 210, y, 200, 32);
            ui::fill_token(c, bar, 6, p.content_bg);
            ui::stroke_token(c, bar, 6, p.separator);
            let lay = bar.title_layout(true, true);
            windows::chrome::draw_title_buttons(c, &lay, hover, restore, 255, p);
        }
        y += 54;
        label(c, x, y, t!("kit.shell.quick"));
        y += 22;
        self.draw_tile(
            c,
            Rect::new(x, y, 152, 56),
            Glyph::Bell,
            t!("quick.dnd"),
            t!("quick.dnd.off"),
            false,
            false,
        );
        self.draw_tile(
            c,
            Rect::new(x + 160, y, 152, 56),
            Glyph::Wave,
            t!("quick.motion"),
            t!("quick.motion.reduced"),
            true,
            false,
        );
        // The taskbar's indicators: the pill (focused) and the dot (running).
        let ix = x + 360;
        label(c, ix, y - 22, t!("kit.shell.indicators"));
        for (k, kind) in [Icon::Files, Icon::Terminal, Icon::Editor]
            .into_iter()
            .enumerate()
        {
            let r = Rect::new(ix + k as i32 * 56, y, 40, 40);
            icons::blit(c, kind, r.x, r.y, r.w, 256);
            match k {
                0 => c.fill_rrect(
                    Rect::new(r.x + 12, r.bottom() + 6, 16, 3),
                    1,
                    Corner::Circle,
                    theme::accent(),
                    256,
                ),
                1 => c.fill_rrect(
                    Rect::new(r.x + 18, r.bottom() + 5, 4, 4),
                    2,
                    Corner::Circle,
                    theme::solid(p.text_secondary),
                    256,
                ),
                _ => {}
            }
        }
        y += 84;
        label(c, x, y, t!("kit.shell.snap"));
        let prev = Rect::new(x, y + 22, 150, 90);
        let acc = theme::accent();
        c.fill_rrect(prev, kitsune_core::style::R_WINDOW, Corner::Circle, acc, 46);
        c.stroke_rrect(
            prev,
            kitsune_core::style::R_WINDOW,
            Corner::Circle,
            acc,
            230,
        );
        let px = x + 220;
        label(c, px, y, t!("kit.shell.pointers"));
        for (i, sh) in [
            kitsune_core::pointer::Shape::Arrow,
            kitsune_core::pointer::Shape::Hand,
            kitsune_core::pointer::Shape::IBeam,
        ]
        .into_iter()
        .enumerate()
        {
            let s = kitsune_core::pointer::render(sh);
            c.blit_surface(&s, px + i as i32 * 56, y + 26, 256);
        }
    }
}

fn gallery_type(c: &mut Canvas, body: Rect, pad: i32, p: &Palette) {
    let mut y = body.y + 64;
    let fg = theme::solid(p.text);
    let sample = t!("kit.type.sample");
    for (name, px, w) in [
        (t!("kit.type.title1"), TITLE1, Weight::Semibold),
        (t!("kit.type.title2"), TITLE2, Weight::Semibold),
        (t!("kit.type.title3"), TITLE3, Weight::Semibold),
        (t!("kit.type.callout"), CALLOUT, Weight::Regular),
        (t!("kit.type.body"), BODY, Weight::Regular),
        (t!("kit.type.body_medium"), BODY, Weight::Medium),
        (t!("kit.type.footnote"), FOOTNOTE, Weight::Regular),
        (t!("kit.type.caption"), CAPTION, Weight::Medium),
    ] {
        text::draw(c, body.x + pad, y, sample, px, w, fg);
        let lh = text::line_height(px);
        text::draw(
            c,
            body.x + body.w - pad - 130,
            y + (lh - 12) / 2,
            name,
            FOOTNOTE,
            Weight::Regular,
            theme::solid(p.text_tertiary),
        );
        y += lh + 8;
    }
    y += 6;
    text::draw_mono(c, body.x + pad, y, t!("kit.type.mono"), text::MONO_PX, fg);
}

fn gallery_colors(c: &mut Canvas, body: Rect, pad: i32, p: &Palette) {
    let items: [(&str, u32); 12] = [
        (t!("kit.color.window"), p.window_bg),
        (t!("kit.color.content"), p.content_bg),
        (t!("kit.color.sidebar"), p.sidebar_bg),
        (t!("kit.color.text"), p.text),
        (t!("kit.color.text2"), p.text_secondary),
        (t!("kit.color.text3"), p.text_tertiary),
        (t!("kit.color.field"), p.field_bg),
        (t!("kit.color.control"), p.control_bg),
        (t!("kit.color.hover"), p.hover),
        (t!("kit.color.danger"), p.danger),
        (t!("kit.color.menu"), p.menu_tint),
        (t!("kit.color.tooltip"), p.tooltip_bg),
    ];
    for (i, (name, argb)) in items.iter().enumerate() {
        let (col, row) = ((i % 4) as i32, (i / 4) as i32);
        let r = Rect::new(body.x + pad + col * 150, body.y + 64 + row * 84, 134, 56);
        // Checker so translucency is visible.
        ui::fill_token(
            c,
            r,
            8,
            if theme::dark() {
                0xFF50_5058
            } else {
                0xFFE0_E0E6
            },
        );
        ui::fill_token(
            c,
            Rect::new(r.x + r.w / 2, r.y, r.w / 2, r.h),
            8,
            if theme::dark() {
                0xFF18_181A
            } else {
                0xFFFF_FFFF
            },
        );
        ui::fill_token(c, r, 8, *argb);
        ui::stroke_token(c, r, 8, p.separator);
        text::draw(
            c,
            r.x,
            r.bottom() + 6,
            name,
            FOOTNOTE,
            Weight::Regular,
            theme::solid(p.text_secondary),
        );
    }
    let ay = body.y + 64 + 3 * 84 + 8;
    text::draw(
        c,
        body.x + pad,
        ay,
        t!("kit.accent_colors"),
        FOOTNOTE,
        Weight::Medium,
        theme::solid(p.text_secondary),
    );
    for (i, rgb) in kitsune_core::settings::ACCENTS.iter().enumerate() {
        let col = Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, *rgb as u8);
        let r = Rect::new(body.x + pad + i as i32 * 40, ay + 22, 28, 28);
        c.fill_rrect(r, 14, Corner::Circle, col, 256);
    }
    let _ = LIGHT;
}

fn gallery_icons(c: &mut Canvas, body: Rect, pad: i32, p: &Palette) {
    let all = [
        Icon::Brand,
        Icon::Halo,
        Icon::Files,
        Icon::Browser,
        Icon::Terminal,
        Icon::Editor,
        Icon::Calculator,
        Icon::Viewer,
        Icon::TaskMgr,
        Icon::Monitor,
        Icon::Settings,
        Icon::Log,
        Icon::WasmApp,
    ];
    for (i, ic) in all.iter().enumerate() {
        let (col, row) = ((i % 7) as i32, (i / 7) as i32);
        icons::blit(
            c,
            *ic,
            body.x + pad + col * 84,
            body.y + 64 + row * 84,
            64,
            256,
        );
    }
    let gy = body.y + 64 + 2 * 84 + 12;
    text::draw(
        c,
        body.x + pad,
        gy,
        t!("kit.glyphs"),
        FOOTNOTE,
        Weight::Medium,
        theme::solid(p.text_secondary),
    );
    let glyphs = [
        Glyph::Search,
        Glyph::Network,
        Glyph::NetworkOff,
        Glyph::Control,
        Glyph::Check,
        Glyph::ChevronRight,
        Glyph::ChevronDown,
        Glyph::Close,
        Glyph::Plus,
        Glyph::Minus,
        Glyph::Brand,
        Glyph::Sun,
        Glyph::Moon,
        Glyph::Info,
        Glyph::Power,
        Glyph::Bell,
        Glyph::Wave,
        Glyph::Clock,
    ];
    let col = 0xFF00_0000
        | (((p.text >> 16) & 0xFF) << 16)
        | (((p.text >> 8) & 0xFF) << 8)
        | (p.text & 0xFF);
    for (i, g) in glyphs.iter().enumerate() {
        ui::draw_glyph(c, *g, body.x + pad + i as i32 * 36, gy + 24, 24, col);
    }
}
