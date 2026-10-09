//! `Desktop` methods: apps. Split out of the former monolithic desktop.rs.

use super::*;

impl Desktop {
    pub(crate) fn draw_browser(&self, c: &mut Canvas, r: Rect, focused: bool, bs: &BrowserState) {
        self.draw_browser_inner(c, r, focused, bs);
        let ch = BrowserChrome::of(r);
        self.draw_browser_overlays(c, &ch, focused, bs);
    }

    /// What floats over the page: the find bar, the notice line and the suggestion list.
    fn draw_browser_overlays(
        &self,
        c: &mut Canvas,
        ch: &BrowserChrome,
        focused: bool,
        bs: &BrowserState,
    ) {
        let content = ch.content;
        let mut y = content.bottom();
        let mut bar_line = |c: &mut Canvas, bg: Color, fg: Color, text: &str| {
            let h = 26;
            y -= h;
            c.fill_rect(
                content.x as usize,
                y.max(0) as usize,
                content.w as usize,
                h as usize,
                bg,
            );
            let room = ((content.w - 16).max(0) as usize) / crate::text::legacy::cell_w(2);
            let t: String = text.chars().take(room).collect();
            crate::text::legacy::draw_text(
                c,
                (content.x + 8) as usize,
                (y + 6).max(0) as usize,
                &t,
                fg,
                2,
            );
        };
        if let Some(msg) = &bs.notice {
            bar_line(
                c,
                Color::rgb(0xFF, 0xE6, 0x9C),
                Color::rgb(0x6B, 0x3F, 0x00),
                msg,
            );
        }
        if bs.find.is_open() {
            let mut t = String::from("Buscar: ");
            t.push_str(bs.find.query());
            if focused {
                t.push('|');
            }
            let count = alloc::format!("   {}/{}", bs.find.position(), bs.find.count());
            t.push_str(&count);
            bar_line(c, theme::toolbar(), theme::text(), &t);
        }
        // Suggestions under the address bar.
        let sugg = bs.browser.suggestions();
        if !sugg.is_empty() && focused {
            let sel = bs.browser.suggestion_selected();
            let first = osjeff_core::layout::browser_suggestion_row(ch.bar, 0);
            let total = sugg.len() as i32 * first.h;
            c.fill_round_rect_alpha(
                (first.x + 3) as usize,
                (first.y + 5) as usize,
                first.w as usize,
                total as usize,
                8,
                theme::SHADOW,
                40,
            );
            c.fill_round_rect(
                first.x as usize,
                first.y as usize,
                first.w as usize,
                total as usize,
                8,
                theme::button_bg(),
            );
            for (i, s) in sugg.iter().enumerate() {
                let row = osjeff_core::layout::browser_suggestion_row(ch.bar, i);
                if sel == Some(i) {
                    c.fill_round_rect_alpha(
                        (row.x + 3) as usize,
                        (row.y + 1) as usize,
                        (row.w - 6) as usize,
                        (row.h - 2) as usize,
                        6,
                        theme::accent(),
                        60,
                    );
                }
                if s.bookmark {
                    glyph_star(c, Rect::new(row.x + 8, row.y + 3, 20, 20), true);
                }
                let room = ((row.w - 44).max(0) as usize) / crate::text::legacy::cell_w(2);
                let label: String = s.label.chars().take(room).collect();
                crate::text::legacy::draw_text(
                    c,
                    (row.x + 36) as usize,
                    (row.y + 6) as usize,
                    &label,
                    theme::text(),
                    2,
                );
            }
        }
    }

    fn draw_browser_inner(&self, c: &mut Canvas, r: Rect, focused: bool, bs: &BrowserState) {
        let ch = BrowserChrome::of(r);

        // ---- toolbar: nav buttons + address bar + search button ----
        use crate::text::{self, BODY, FOOTNOTE, Weight};
        let p = theme::pal();
        let tool_bg = theme::tool_bg();
        let (ink, dim) = (theme::ink(), theme::ink_dim());
        c.fill_rect(
            r.x.max(0) as usize,
            (r.y + TITLE_H).max(0) as usize,
            r.w.max(0) as usize,
            (ch.content.y - r.y - TITLE_H).max(0) as usize,
            theme::toolbar(),
        );
        draw_tool_button(c, ch.back, tool_bg, ink);
        let on = |yes: bool| if yes { ink } else { dim };
        glyph_arrow(c, ch.back, on(bs.browser.can_back()), true);
        draw_tool_button(c, ch.forward, tool_bg, ink);
        glyph_arrow(c, ch.forward, on(bs.browser.can_forward()), false);
        draw_tool_button(c, ch.reload, tool_bg, ink);
        glyph_reload(c, ch.reload, ink, tool_bg);
        draw_tool_button(c, ch.home, tool_bg, ink);
        glyph_home(c, ch.home, ink);

        // Address bar: a pill with a hairline (an accent ring while it has the focus), a
        // leading globe, the URL (or a placeholder), a caret and the connection badge.
        let bar = ch.bar;
        ui::text_field_frame(c, bar, bar.h / 2, focused && bs.browser.bar_focus());
        glyph_globe(c, Rect::new(bar.x + 10, bar.y + (bar.h - 18) / 2, 18, 18));
        glyph_star(c, ch.star, bs.browser.is_bookmarked());

        // Connection badge, right-aligned inside the bar: the padlock ("Conexão
        // segura", green) only for a chain the TLS client verified; an https page the
        // user opened despite a certificate error says so in red, plain http says it is
        // not encrypted.
        let mut badge_reserved = 34;
        if let Some(label) = bs.browser.security().label() {
            use osjeff_core::browser::Security;
            let (bg, fg) = match bs.browser.security() {
                Security::HttpsVerified => {
                    if theme::dark() {
                        (Color::rgb(0x1E, 0x3B, 0x2A), Color::rgb(0x6E, 0xE7, 0x9A))
                    } else {
                        (Color::rgb(0xDC, 0xF2, 0xDD), Color::rgb(0x1B, 0x5E, 0x20))
                    }
                }
                Security::HttpsInvalid => (Color::rgb(0xC6, 0x28, 0x28), theme::WHITE),
                _ => {
                    if theme::dark() {
                        (Color::rgb(0x4A, 0x24, 0x24), Color::rgb(0xFF, 0xA3, 0xA3))
                    } else {
                        (Color::rgb(0xFF, 0xD9, 0xD9), Color::rgb(0xA3, 0x1D, 0x1D))
                    }
                }
            };
            let label = crate::text::from_bytes(label.as_bytes()).into_owned();
            let tw = text::measure(&label, FOOTNOTE, Weight::Medium);
            let (pw, ph) = (tw + 20, 20);
            let px = bar.x + bar.w - pw - 42;
            let py = bar.y + (bar.h - ph) / 2;
            c.fill_rrect(Rect::new(px, py, pw, ph), ph / 2, Corner::Circle, bg, 256);
            text::draw_centered(
                c,
                Rect::new(px, py, pw, ph),
                &label,
                FOOTNOTE,
                Weight::Medium,
                fg,
            );
            badge_reserved = pw + 46;
        }

        let tx = bar.x + 36;
        let room = (bar.w - 48 - badge_reserved).max(8);
        let ty = text::center_y(bar.y, bar.h, BODY, Weight::Regular);
        let url = bs.browser.url();
        if url.is_empty() {
            text::draw(
                c,
                tx,
                ty,
                "Pesquisar ou digitar um endereço",
                BODY,
                Weight::Regular,
                theme::solid(p.text_tertiary),
            );
        } else {
            let full = crate::text::from_bytes(url).into_owned();
            // Show the tail of an address wider than the field.
            let mut start = 0usize;
            while start < full.len() && text::measure(&full[start..], BODY, Weight::Regular) > room
            {
                start += full[start..].chars().next().map_or(1, char::len_utf8);
            }
            let shown = &full[start..];
            if focused && bs.browser.bar_selected() {
                let w = text::measure(shown, BODY, Weight::Regular);
                c.fill_rrect(
                    Rect::new(tx - 2, bar.y + 5, w + 4, bar.h - 10),
                    4,
                    Corner::Circle,
                    theme::accent(),
                    90,
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
            if focused && bs.browser.bar_focus() {
                let caret = bs
                    .browser
                    .caret()
                    .min(full.len())
                    .saturating_sub(start)
                    .min(shown.len());
                let cx = tx + text::measure(&shown[..caret], BODY, Weight::Regular);
                c.fill_rect(
                    cx.max(0) as usize,
                    (bar.y + 6).max(0) as usize,
                    1,
                    (bar.h - 12).max(0) as usize,
                    theme::accent(),
                );
            }
        }

        // Search/go button (accent) with a magnifier glyph.
        draw_tool_button(c, ch.go, theme::accent(), theme::WHITE);
        glyph_search(c, ch.go, theme::WHITE, theme::accent());

        // ---- body: native start page, loading state, or page text ----
        use osjeff_core::browser::Status;
        if bs.browser.is_home() {
            self.draw_browser_home(c, ch.content);
            return;
        }
        if bs.browser.status() == Status::Loading {
            let msg = "Carregando...";
            let w = crate::text::legacy::text_width(msg, 2);
            crate::text::legacy::draw_text(
                c,
                (ch.content.x + (ch.content.w - w as i32) / 2) as usize,
                (ch.content.y + 40) as usize,
                msg,
                theme::text_muted(),
                2,
            );
            return;
        }

        let content = ch.content;
        let Some(page) = &bs.page else {
            self.draw_browser_error(c, content, bs);
            return;
        };
        self.paint_web_page(c, page, content, bs.scroll, bs);

        // The page is only part of the document (cut at the size cap, connection dropped,
        // damaged compressed data): say so instead of showing it as if it were whole.
        if let Some(note) = bs.browser.note()
            && content.h > 24
        {
            let label = note.label();
            let h = 28;
            let y = content.bottom() - h;
            c.fill_rect(
                content.x as usize,
                y as usize,
                content.w as usize,
                h as usize,
                Color::rgb(0xFF, 0xE6, 0x9C),
            );
            let ty = text::center_y(y, h, FOOTNOTE, Weight::Medium);
            text::draw_ellipsis(
                c,
                content.x + 10,
                ty,
                content.w - 20,
                label,
                FOOTNOTE,
                Weight::Medium,
                Color::rgb(0x6B, 0x3F, 0x00),
            );
        }

        // Scrollbar track + thumb when the rendered page overflows.
        if page.height > content.h && content.h > 0 {
            let track_x = (content.right() - 4) as usize;
            let track_h = content.h as usize;
            c.fill_rect(track_x, content.y as usize, 4, track_h, theme::DOCK_EDGE);
            let thumb_h = ((track_h * track_h) / page.height as usize).max(16);
            let max_scroll = (page.height - content.h) as usize;
            let thumb_y = content.y as usize
                + ((track_h - thumb_h) * bs.scroll as usize)
                    .checked_div(max_scroll)
                    .unwrap_or(0);
            c.fill_round_rect(track_x, thumb_y, 4, thumb_h, 2, theme::accent());
        }
    }

    /// The native start page: brand mark, tagline, and clickable shortcut tiles.
    /// The error page: the reason in red, and for a refused certificate the
    /// explanation, the clock hint (when the time was not confirmed over SNTP) and
    /// the explicit "continue anyway (insecure)" button.
    fn draw_browser_error(&self, c: &mut Canvas, content: Rect, bs: &BrowserState) {
        use crate::text::{self, BODY, FOOTNOTE, TITLE3, Weight};
        use osjeff_core::browser::FailReason;
        let p = theme::pal();
        let reason = bs.browser.fail_reason();
        let x = content.x + 24;
        text::draw(
            c,
            x,
            content.y + 22,
            reason.message(),
            TITLE3,
            Weight::Semibold,
            theme::danger(),
        );
        if let FailReason::Cert(e) = reason {
            text::draw(
                c,
                x,
                content.y + 54,
                "A identidade do servidor não foi comprovada: a conexão pode ser interceptada.",
                BODY,
                Weight::Regular,
                theme::solid(p.text),
            );
            if e.clock_may_be_to_blame() && !crate::clock::confirmed() {
                text::draw(
                    c,
                    x,
                    content.y + 76,
                    "Hora do sistema não confirmada: confira o relógio (sem resposta de servidor de hora).",
                    BODY,
                    Weight::Regular,
                    if theme::dark() {
                        Color::rgb(0xFB, 0xBF, 0x24)
                    } else {
                        Color::rgb(0x8A, 0x4B, 0x00)
                    },
                );
            }
            if bs.browser.can_continue_insecure() {
                let b = osjeff_core::layout::browser_continue_button(content);
                ui::push_button(
                    c,
                    b,
                    "Continuar mesmo assim (inseguro)",
                    ui::ButtonKind::Destructive,
                    ui::Control::Normal,
                );
                text::draw(
                    c,
                    x,
                    b.bottom() + 10,
                    "Vale só para este site, nesta sessão.",
                    FOOTNOTE,
                    Weight::Regular,
                    theme::solid(p.text_secondary),
                );
            }
        }
    }

    pub(crate) fn draw_browser_home(&self, c: &mut Canvas, content: Rect) {
        use crate::text::{self, BODY, TITLE2, TITLE3, Weight};
        let p = theme::pal();
        let (logo, tiles) = browser_home_layout(content);
        c.fill_rect(
            content.x.max(0) as usize,
            content.y.max(0) as usize,
            content.w.max(0) as usize,
            content.h.max(0) as usize,
            theme::window_body(),
        );

        // Brand globe, name and hint, centred.
        icons::blit(c, Icon::Browser, logo.x, logo.y, logo.w, 256);
        let cx = content.x + content.w / 2;
        let line = |y: i32, h: i32| Rect::new(cx - content.w / 2, y, content.w, h);
        text::draw_centered(
            c,
            line(logo.bottom() + 14, 30),
            "Navegador",
            TITLE2,
            Weight::Semibold,
            theme::solid(p.text),
        );
        text::draw_centered(
            c,
            line(logo.bottom() + 46, 22),
            "Pesquise ou digite um endereço na barra acima",
            BODY,
            Weight::Regular,
            theme::solid(p.text_secondary),
        );

        // Shortcut cards: a colour monogram over the name.
        let accents = [
            theme::accent(),
            theme::ACCENT_2,
            Color::rgb(0xF5, 0x9E, 0x0B),
            Color::rgb(0x14, 0xB8, 0xC4),
        ];
        for (i, t) in tiles.iter().enumerate() {
            let (label, _url) = osjeff_core::browser::QUICK_LINKS[i];
            let hole = Rect::new(t.x, t.y + 12, t.w, (t.h - 24).max(0));
            c.draw_shadow(
                *t,
                Shadow {
                    blur: 8,
                    dy: 3,
                    alpha: if theme::dark() { 90 } else { 34 },
                },
                hole,
            );
            ui::fill_token(c, *t, 12, p.content_bg);
            ui::stroke_token(c, *t, 12, p.separator);
            let chip = Rect::new(t.x + (t.w - 36) / 2, t.y + 16, 36, 36);
            c.fill_rrect(chip, 11, Corner::Squircle, accents[i], 256);
            let initial = alloc::format!(
                "{}",
                label.chars().next().unwrap_or('?').to_ascii_uppercase()
            );
            text::draw_centered(c, chip, &initial, TITLE3, Weight::Semibold, theme::WHITE);
            text::draw_centered(
                c,
                Rect::new(t.x + 4, t.bottom() - 34, t.w - 8, 24),
                label,
                BODY,
                Weight::Medium,
                theme::solid(p.text),
            );
        }
    }

    /// Render the resident WASM application into window `r`'s content area. The
    /// guest paints through the host drawing ABI; the engine translates and
    /// clips it to this box (see [`crate::wasm::draw_app`]).
    pub(crate) fn draw_wasm(&self, c: &mut Canvas, r: Rect, w: &WasmWin) {
        // Each app renders on the `appd` thread into its own offscreen surface; the
        // compositor just copies the latest finished frame into the window (or shows
        // why the app is not running).
        let cr = wasm_content(r);
        crate::wasm::blit(w.id, c, cr.x, cr.y, cr.w, cr.h);
    }

    /// Alt+Tab panel geometry for a list of `n` windows: centered, `SWITCH_ROWS`
    /// rows at most.
    pub(crate) fn switcher_rect(&self, n: usize) -> Rect {
        let rows = n.clamp(1, SWITCH_ROWS) as i32;
        let h = SWITCH_PAD * 2 + rows * SWITCH_ROW_H;
        Rect::new(
            (self.sw - SWITCH_W) / 2,
            (self.sh - h) / 2 - 40,
            SWITCH_W,
            h,
        )
    }

    /// The Alt+Tab overlay: every window in most-recently-used order on a glass
    /// panel, the selection highlighted; minimized windows are tagged.
    pub(crate) fn draw_switcher(&self, c: &mut Canvas, sw: &Switcher) {
        use crate::text::{self, BODY, FOOTNOTE, Weight};
        let p = theme::pal();
        let list = sw.list();
        let r = self.switcher_rect(list.len());
        let hole = Rect::new(r.x, r.y + 16, r.w, r.h - 32);
        c.draw_shadow(
            r,
            Shadow {
                blur: 24,
                dy: 14,
                alpha: 90,
            },
            hole,
        );
        self.shell.switcher_glass.draw(c, r, 16, 14, 256);
        ui::fill_token(c, r, 16, p.menu_tint);
        ui::stroke_token(c, r, 16, p.separator);
        let sel = sw.selected_index();
        let first = (sel + 1).saturating_sub(SWITCH_ROWS);
        for (row, idx) in (first..list.len().min(first + SWITCH_ROWS)).enumerate() {
            let ry = r.y + SWITCH_PAD + row as i32 * SWITCH_ROW_H;
            let rr = Rect::new(r.x + 8, ry, r.w - 16, SWITCH_ROW_H);
            let Some(w) = self.wm.get(list[idx]) else {
                continue;
            };
            let fg = if idx == sel {
                c.fill_rrect(rr, 8, Corner::Circle, theme::accent(), 256);
                theme::ACCENT_TEXT
            } else {
                theme::solid(p.text)
            };
            icons::blit(
                c,
                w.app.kind().icon(),
                rr.x + 8,
                ry + (SWITCH_ROW_H - 28) / 2,
                28,
                256,
            );
            let tag_w = if w.minimized { 56 } else { 0 };
            text::draw_left(
                c,
                Rect::new(rr.x + 48, ry, rr.w - 48 - 12 - tag_w, SWITCH_ROW_H),
                &w.app.title,
                BODY,
                Weight::Medium,
                fg,
            );
            if w.minimized {
                let tag = if idx == sel {
                    Color::rgb(0xE6, 0xE6, 0xFF)
                } else {
                    theme::solid(p.text_secondary)
                };
                text::draw_right(
                    c,
                    Rect::new(rr.x, ry, rr.w - 12, SWITCH_ROW_H),
                    "oculta",
                    FOOTNOTE,
                    Weight::Regular,
                    tag,
                );
            }
        }
    }
}

fn draw_tool_button(c: &mut Canvas, r: Rect, bg: Color, _fg: Color) {
    c.fill_round_rect(
        r.x as usize,
        r.y as usize,
        r.w as usize,
        r.h as usize,
        9,
        bg,
    );
}

/// A `thick`-pixel ring (donut) of `color`, punched hollow with `bg`.
fn donut(c: &mut Canvas, cx: i32, cy: i32, rad: i32, thick: i32, color: Color, bg: Color) {
    c.fill_round_rect(
        (cx - rad) as usize,
        (cy - rad) as usize,
        (2 * rad) as usize,
        (2 * rad) as usize,
        rad as usize,
        color,
    );
    let ir = (rad - thick).max(1);
    c.fill_round_rect(
        (cx - ir) as usize,
        (cy - ir) as usize,
        (2 * ir) as usize,
        (2 * ir) as usize,
        ir as usize,
        bg,
    );
}

// Small vector glyphs centered in their button rects (the bitmap font has no
// icon glyphs).
/// A bitmap drawn at integer scale, centered in `r` (`#` = set).
fn draw_bitmap(c: &mut Canvas, r: Rect, rows: &[&str], scale: i32, color: Color) {
    let w = rows.iter().map(|l| l.len()).max().unwrap_or(0) as i32 * scale;
    let h = rows.len() as i32 * scale;
    let (x0, y0) = (r.x + (r.w - w) / 2, r.y + (r.h - h) / 2);
    for (j, line) in rows.iter().enumerate() {
        for (i, b) in line.bytes().enumerate() {
            if b == b'#' {
                c.fill_rect(
                    (x0 + i as i32 * scale) as usize,
                    (y0 + j as i32 * scale) as usize,
                    scale as usize,
                    scale as usize,
                    color,
                );
            }
        }
    }
}

const ARROW_LEFT: [&str; 9] = [
    "....#........",
    "...##........",
    "..###........",
    ".############",
    "#############",
    ".############",
    "..###........",
    "...##........",
    "....#........",
];
const ARROW_RIGHT: [&str; 9] = [
    "........#....",
    "........##...",
    "........###..",
    "############.",
    "#############",
    "############.",
    "........###..",
    "........##...",
    "........#....",
];
const STAR: [&str; 11] = [
    ".....#.....",
    ".....#.....",
    "....###....",
    "###########",
    ".#########.",
    "..#######..",
    "...#####...",
    "..#######..",
    "..###.###..",
    ".###...###.",
    ".#.......#.",
];

fn glyph_arrow(c: &mut Canvas, r: Rect, color: Color, left: bool) {
    draw_bitmap(
        c,
        r,
        if left { &ARROW_LEFT } else { &ARROW_RIGHT },
        2,
        color,
    );
}

fn glyph_star(c: &mut Canvas, r: Rect, on: bool) {
    let color = if on {
        Color::rgb(0xF5, 0x9E, 0x0B)
    } else {
        Color::rgb(0xB8, 0xC0, 0xCE)
    };
    draw_bitmap(c, r, &STAR, 2, color);
}

fn glyph_home(c: &mut Canvas, r: Rect, color: Color) {
    let cx = r.x + r.w / 2;
    let top = r.y + r.h / 2 - 8;
    // Roof: a triangle drawn as widening rows.
    for i in 0..8 {
        c.fill_rect(
            (cx - i) as usize,
            (top + i) as usize,
            (2 * i + 1) as usize,
            1,
            color,
        );
    }
    // Body.
    let bw = 12;
    c.fill_round_rect(
        (cx - bw / 2) as usize,
        (top + 8) as usize,
        bw as usize,
        9,
        1,
        color,
    );
}

fn glyph_reload(c: &mut Canvas, r: Rect, color: Color, bg: Color) {
    let cx = r.x + r.w / 2;
    let cy = r.y + r.h / 2;
    let rad = 9;
    donut(c, cx, cy, rad, 3, color, bg);
    // Break the ring at the top-right and add an arrowhead, hinting "refresh".
    c.fill_rect((cx) as usize, (cy - rad - 1) as usize, 7, 7, bg);
    for i in 0..5 {
        c.fill_rect(
            (cx + 1) as usize,
            (cy - rad + i - 1) as usize,
            (5 - i) as usize,
            1,
            color,
        );
    }
}

fn glyph_search(c: &mut Canvas, r: Rect, color: Color, bg: Color) {
    let cx = r.x + r.w / 2 - 2;
    let cy = r.y + r.h / 2 - 2;
    let rad = 7;
    donut(c, cx, cy, rad, 2, color, bg);
    // Handle: a short thick diagonal off the lower-right of the lens.
    for i in 0..5 {
        c.fill_rect(
            (cx + rad - 2 + i) as usize,
            (cy + rad - 2 + i) as usize,
            3,
            3,
            color,
        );
    }
}

fn glyph_globe(c: &mut Canvas, r: Rect) {
    let cx = r.x + r.w / 2;
    let cy = r.y + r.h / 2;
    let rad = r.w / 2;
    c.fill_round_rect(
        r.x as usize,
        r.y as usize,
        r.w as usize,
        r.w as usize,
        (r.w / 2) as usize,
        theme::accent(),
    );
    c.fill_rect(
        (cx - rad) as usize,
        cy as usize,
        (2 * rad) as usize,
        2,
        theme::WHITE,
    );
    c.fill_rect(
        cx as usize,
        (cy - rad) as usize,
        2,
        (2 * rad) as usize,
        theme::WHITE,
    );
}
