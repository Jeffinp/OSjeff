//! The gallery's other sections: the shell chrome, the type scale, colours and icons.

use crate::desktop::*;
use crate::text::{self, BODY, CALLOUT, CAPTION, FOOTNOTE, TITLE1, TITLE2, TITLE3, Weight};
use kitsune_core::iconart::Glyph;
use kitsune_core::style::{LIGHT, Palette};
use kitsune_core::t;

impl Desktop {
    /// The shell's own parts: window buttons (rest, hover, close hover, restore), Quick Settings
    /// tiles, the taskbar indicators, the snap preview and the pointers.
    pub(super) fn gallery_shell(&self, c: &mut Canvas, body: Rect, pad: i32, p: &Palette) {
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

pub(super) fn gallery_type(c: &mut Canvas, body: Rect, pad: i32, p: &Palette) {
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

pub(super) fn gallery_colors(c: &mut Canvas, body: Rect, pad: i32, p: &Palette) {
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

pub(super) fn gallery_icons(c: &mut Canvas, body: Rect, pad: i32, p: &Palette) {
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
