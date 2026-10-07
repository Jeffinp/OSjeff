//! `Desktop` methods: apps. Split out of the former monolithic desktop.rs.

use super::*;

impl Desktop {
    pub(crate) fn draw_menu(&self, c: &mut Canvas, m: MenuState) {
        let (mx, my) = (m.x, m.y);
        let count = m.kind.len() as i32;
        let h = MENU_PAD * 2 + count * MENU_ITEM_H;

        // Drop shadow + panel.
        c.fill_round_rect_alpha(
            (mx + 4) as usize,
            (my + 6) as usize,
            MENU_W as usize,
            h as usize,
            12,
            theme::SHADOW,
            34,
        );
        c.fill_round_rect(
            mx as usize,
            my as usize,
            MENU_W as usize,
            h as usize,
            10,
            theme::WINDOW_BODY,
        );

        for i in 0..m.kind.len() {
            let Some((label, kind, _)) = m.kind.entry(i) else {
                continue;
            };
            let iy = my + MENU_PAD + i as i32 * MENU_ITEM_H;
            if menu_item_at(mx, my, self.cursor_x, self.cursor_y, m.kind.len()) == Some(i) {
                c.fill_round_rect_alpha(
                    (mx + 4) as usize,
                    (iy + 2) as usize,
                    (MENU_W - 8) as usize,
                    (MENU_ITEM_H - 4) as usize,
                    6,
                    theme::ACCENT,
                    36,
                );
            }
            let isz = 20usize;
            let iyc = (iy + (MENU_ITEM_H - isz as i32) / 2) as usize;
            icons::draw(c, kind.icon(), (mx + 10) as usize, iyc, isz);
            font::draw_text(
                c,
                (mx + 40) as usize,
                (iy + 8) as usize,
                label,
                theme::TEXT,
                2,
            );
        }
    }

    pub(crate) fn draw_calculator(&self, c: &mut Canvas, r: Rect, calc: &Calc) {
        let x = r.x.max(0) as usize;
        let y = r.y.max(0) as usize;
        let w = r.w as usize;
        let pad = 14usize;

        // Display panel: dark box, result right-aligned in accent.
        let dx = x + pad;
        let dy = y + TITLE_H as usize + 12;
        let dw = w - pad * 2;
        let dh = 48usize;
        c.fill_round_rect(dx, dy, dw, dh, 8, Color::rgb(0x0E, 0x16, 0x28));
        let disp = calc.display();
        let dscale = 3usize;
        let tw = disp.len() * font::cell_w(dscale);
        let tx = if tw + 16 < dw {
            dx + dw - tw - 16
        } else {
            dx + 10
        };
        let color = if calc.is_error() {
            theme::CLOSE
        } else {
            theme::ACCENT
        };
        font::draw_bytes(c, tx, dy + (dh - 7 * dscale) / 2, disp, color, dscale);

        // Keypad.
        let (gx, gy, cw, ch, gap) = calc_layout(r);
        let pending = calc.operator();
        for (row, keys) in CALC_KEYS.iter().enumerate() {
            for (col, &k) in keys.iter().enumerate() {
                // Skip the cells absorbed by a spanning button.
                if (row == 4 && col == 1) || (row == 4 && col == 3) {
                    continue;
                }
                let mut bw = cw;
                let mut bh = ch;
                if row == 4 && col == 0 {
                    bw = cw * 2 + gap; // wide "0"
                }
                if row == 3 && col == 3 {
                    bh = ch * 2 + gap; // tall "="
                }
                let bx = gx + col as i32 * (cw + gap);
                let by = gy + row as i32 * (ch + gap);
                let (bg, fg) = key_style(k, pending);
                c.fill_round_rect(bx as usize, by as usize, bw as usize, bh as usize, 8, bg);
                let label = key_label(&k);
                let lscale = 3usize;
                let lw = label.len() * font::cell_w(lscale);
                let lx = bx as usize + (bw as usize).saturating_sub(lw) / 2;
                let ly = by as usize + (bh as usize).saturating_sub(7 * lscale) / 2;
                font::draw_bytes(c, lx, ly, label, fg, lscale);
            }
        }
    }

    pub(crate) fn draw_browser(&self, c: &mut Canvas, r: Rect, focused: bool, bs: &BrowserState) {
        let ch = BrowserChrome::of(r);

        // ---- toolbar: nav buttons + address bar + search button ----
        let tool_bg = Color::rgb(0xEC, 0xEF, 0xF5);
        draw_tool_button(c, ch.home, tool_bg, theme::HEADER);
        glyph_home(c, ch.home, theme::HEADER);
        draw_tool_button(c, ch.reload, tool_bg, theme::HEADER);
        glyph_reload(c, ch.reload, theme::HEADER, tool_bg);

        // Address bar: white pill with a 1px border, a leading globe, the URL
        // (or a muted placeholder) and a caret when focused.
        let bar = ch.bar;
        c.fill_round_rect(
            bar.x as usize,
            bar.y as usize,
            bar.w as usize,
            bar.h as usize,
            bar.h as usize / 2,
            Color::rgb(0xCE, 0xD6, 0xE6),
        );
        c.fill_round_rect(
            (bar.x + 1) as usize,
            (bar.y + 1) as usize,
            (bar.w - 2) as usize,
            (bar.h - 2) as usize,
            (bar.h as usize - 2) / 2,
            theme::WHITE,
        );
        glyph_globe(c, Rect::new(bar.x + 10, bar.y + (bar.h - 18) / 2, 18, 18));

        // Connection badge, right-aligned inside the bar. The padlock ("Conexao
        // segura", green) is shown only for a connection whose certificate chain
        // was verified by the TLS client; an https page the user chose to open
        // despite a certificate error says "Certificado invalido" in red, and
        // plain http says it is not encrypted. Text is ASCII (bitmap font).
        let mut badge_reserved = 0;
        if let Some(label) = bs.browser.security().label() {
            use osjeff_core::browser::Security;
            let (bg, fg) = match bs.browser.security() {
                Security::HttpsVerified => {
                    (Color::rgb(0xDC, 0xF2, 0xDD), Color::rgb(0x1B, 0x5E, 0x20))
                }
                Security::HttpsInvalid => (Color::rgb(0xC6, 0x28, 0x28), theme::WHITE),
                _ => (Color::rgb(0xFF, 0xD9, 0xD9), Color::rgb(0xA3, 0x1D, 0x1D)),
            };
            let scale = if bar.w >= 520 { 2 } else { 1 };
            let tw = font::text_width(label, scale) as i32;
            let ph = 7 * scale as i32 + 8;
            let pw = tw + 16;
            let px = bar.x + bar.w - pw - 8;
            let py = bar.y + (bar.h - ph) / 2;
            c.fill_round_rect(
                px as usize,
                py as usize,
                pw as usize,
                ph as usize,
                ph as usize / 2,
                bg,
            );
            font::draw_text(c, (px + 8) as usize, (py + 4) as usize, label, fg, scale);
            badge_reserved = pw + 12;
        }

        let tx = (bar.x + 36) as usize;
        let ty = (bar.y + (bar.h - 14) / 2) as usize;
        let url = bs.browser.url();
        let bar_cols = (((bar.w - 48 - badge_reserved).max(0)) as usize / font::cell_w(2)).max(1);
        if url.is_empty() {
            font::draw_text(
                c,
                tx,
                ty,
                "Pesquisar ou digitar um endereco",
                theme::TEXT_MUTED,
                2,
            );
        } else {
            let shown = &url[url.len().saturating_sub(bar_cols)..];
            font::draw_bytes(c, tx, ty, shown, theme::TEXT, 2);
            if focused {
                let caret = bs.browser.caret().min(shown.len());
                let cx = tx + caret * font::cell_w(2);
                c.fill_rect(cx, ty - 1, 2, 16, theme::ACCENT);
            }
        }

        // Search/go button (accent) with a magnifier glyph.
        draw_tool_button(c, ch.go, theme::ACCENT, theme::WHITE);
        glyph_search(c, ch.go, theme::WHITE, theme::ACCENT);

        // ---- body: native start page, loading state, or page text ----
        use osjeff_core::browser::Status;
        if bs.browser.is_home() {
            self.draw_browser_home(c, ch.content);
            return;
        }
        if bs.browser.status() == Status::Loading {
            let msg = "Carregando...";
            let w = font::text_width(msg, 2);
            font::draw_text(
                c,
                (ch.content.x + (ch.content.w - w as i32) / 2) as usize,
                (ch.content.y + 40) as usize,
                msg,
                theme::TEXT_MUTED,
                2,
            );
            return;
        }

        let content = ch.content;
        let Some(page) = &bs.page else {
            self.draw_browser_error(c, content, bs);
            return;
        };
        self.paint_web_page(c, page, content, bs.scroll);

        // The response hit the size cap: say so on the page instead of showing
        // a silently cut document.
        if bs.browser.truncated() && content.h > 24 {
            let h = 24;
            let y = (content.bottom() - h) as usize;
            c.fill_rect(
                content.x as usize,
                y,
                content.w as usize,
                h as usize,
                Color::rgb(0xFF, 0xE6, 0x9C),
            );
            font::draw_text(
                c,
                (content.x + 8) as usize,
                y + 5,
                "Pagina truncada (resposta muito grande)",
                Color::rgb(0x6B, 0x3F, 0x00),
                2,
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
            c.fill_round_rect(track_x, thumb_y, 4, thumb_h, 2, theme::ACCENT);
        }
    }

    /// Rasterize a `web` engine display list into the content box, offset by the
    /// current scroll and clipped vertically to the visible area.
    pub(crate) fn paint_web_page(
        &self,
        c: &mut Canvas,
        page: &osjeff_core::web::Page,
        content: Rect,
        scroll: i32,
    ) {
        use osjeff_core::web::Cmd;
        let top = scroll;
        let bottom = top + content.h;
        let ox = content.x;
        let oy = content.y - top;
        for cmd in &page.cmds {
            match cmd {
                Cmd::Rect { x, y, w, h, color } => {
                    if *y + *h < top || *y > bottom {
                        continue;
                    }
                    let py = (oy + *y).max(content.y);
                    let ph = (*y + *h + oy).min(content.bottom()) - py;
                    if ph > 0 {
                        let cw = (*w).min(content.w - *x);
                        c.fill_rect(
                            (ox + *x).max(0) as usize,
                            py.max(0) as usize,
                            cw.max(0) as usize,
                            ph as usize,
                            rgb(*color),
                        );
                    }
                }
                Cmd::Text {
                    x,
                    y,
                    text,
                    color,
                    scale,
                    bold,
                } => {
                    let lh = 9 * *scale as i32;
                    if *y + lh < top || *y > bottom {
                        continue;
                    }
                    let px = (ox + *x) as usize;
                    let py = (oy + *y) as usize;
                    // Clip to the content box on the right (a page laid out for a
                    // wider window while it is being resized).
                    let room = ((content.right() - (ox + *x)).max(0) as usize)
                        / font::cell_w(*scale as usize);
                    let bytes = text.as_bytes();
                    let bytes = &bytes[..bytes.len().min(room)];
                    font::draw_bytes(c, px, py, bytes, rgb(*color), *scale as usize);
                    if *bold {
                        font::draw_bytes(c, px + 1, py, bytes, rgb(*color), *scale as usize);
                    }
                }
            }
        }
    }

    /// The native start page: brand mark, tagline, and clickable shortcut tiles.
    /// The error page: the reason in red, and for a refused certificate the
    /// explanation, the clock hint (when the time was not confirmed over SNTP) and
    /// the explicit "continue anyway (insecure)" button.
    fn draw_browser_error(&self, c: &mut Canvas, content: Rect, bs: &BrowserState) {
        use osjeff_core::browser::FailReason;
        let reason = bs.browser.fail_reason();
        font::draw_text(
            c,
            (content.x + 8) as usize,
            (content.y + 10) as usize,
            reason.message(),
            theme::CLOSE,
            2,
        );
        if let FailReason::Cert(e) = reason {
            font::draw_text(
                c,
                (content.x + 8) as usize,
                (content.y + 38) as usize,
                "A identidade do servidor nao foi comprovada: a conexao pode ser interceptada.",
                theme::TEXT,
                1,
            );
            if e.clock_may_be_to_blame() && !crate::clock::confirmed() {
                font::draw_text(
                    c,
                    (content.x + 8) as usize,
                    (content.y + 56) as usize,
                    "Hora do sistema nao confirmada: confira o relogio (sem resposta de servidor de hora).",
                    Color::rgb(0x6B, 0x3F, 0x00),
                    1,
                );
            }
            if bs.browser.can_continue_insecure() {
                let b = osjeff_core::layout::browser_continue_button(content);
                c.fill_round_rect(
                    b.x as usize,
                    b.y as usize,
                    b.w as usize,
                    b.h as usize,
                    8,
                    Color::rgb(0xC6, 0x28, 0x28),
                );
                let label = "Continuar mesmo assim (inseguro)";
                let tw = font::text_width(label, 2) as i32;
                font::draw_text(
                    c,
                    (b.x + (b.w - tw).max(0) / 2) as usize,
                    (b.y + (b.h - 14) / 2) as usize,
                    label,
                    theme::WHITE,
                    2,
                );
                font::draw_text(
                    c,
                    (content.x + 8) as usize,
                    (b.bottom() + 8) as usize,
                    "Vale so para este site, nesta sessao.",
                    theme::TEXT_MUTED,
                    1,
                );
            }
        }
    }

    pub(crate) fn draw_browser_home(&self, c: &mut Canvas, content: Rect) {
        let (logo, tiles) = browser_home_layout(content);

        // Brand globe + wordmark + tagline, centered.
        icons::draw(
            c,
            Icon::Browser,
            logo.x as usize,
            logo.y as usize,
            logo.w as usize,
        );
        let cx = content.x + content.w / 2;
        let wm = "OSjeff";
        let wmw = font::text_width(wm, 5) as i32;
        font::draw_text(
            c,
            (cx - wmw / 2) as usize,
            (logo.bottom() + 16) as usize,
            wm,
            theme::HEADER,
            5,
        );
        let sub = "Navegador";
        let sw = font::text_width(sub, 2) as i32;
        font::draw_text(
            c,
            (cx - sw / 2) as usize,
            (logo.bottom() + 60) as usize,
            sub,
            theme::ACCENT,
            2,
        );
        let hint = "Pesquise ou digite um endereco na barra acima";
        let hw = font::text_width(hint, 2) as i32;
        font::draw_text(
            c,
            (cx - hw / 2) as usize,
            (logo.bottom() + 84) as usize,
            hint,
            theme::TEXT_MUTED,
            2,
        );

        // Shortcut tiles: a colored monogram chip over a centered label.
        let accents = [
            theme::ACCENT,
            theme::ACCENT_2,
            Color::rgb(0xF5, 0x9E, 0x0B),
            Color::rgb(0x4C, 0xC2, 0xFF),
        ];
        for (i, t) in tiles.iter().enumerate() {
            let (label, _url) = osjeff_core::browser::QUICK_LINKS[i];
            let accent = accents[i];
            // Card with a soft shadow.
            c.fill_round_rect_alpha(
                (t.x + 3) as usize,
                (t.y + 5) as usize,
                t.w as usize,
                t.h as usize,
                12,
                theme::SHADOW,
                26,
            );
            c.fill_round_rect(
                t.x as usize,
                t.y as usize,
                t.w as usize,
                t.h as usize,
                12,
                theme::WHITE,
            );
            // Monogram chip (first letter of the label).
            let chip = 34;
            let chx = t.x + (t.w - chip) / 2;
            let chy = t.y + 16;
            c.fill_round_rect(
                chx as usize,
                chy as usize,
                chip as usize,
                chip as usize,
                10,
                accent,
            );
            let initial = [label.as_bytes()[0].to_ascii_uppercase()];
            let iw = font::cell_w(3);
            font::draw_bytes(
                c,
                (chx + (chip - iw as i32) / 2) as usize,
                (chy + (chip - 7 * 3) / 2) as usize,
                &initial,
                theme::WHITE,
                3,
            );
            // Label.
            let lw = font::text_width(label, 2) as i32;
            font::draw_text(
                c,
                (t.x + (t.w - lw) / 2) as usize,
                (t.y + t.h - 24) as usize,
                label,
                theme::TEXT,
                2,
            );
        }
    }

    /// Render the resident WASM application into window `r`'s content area. The
    /// guest paints through the host drawing ABI; the engine translates and
    /// clips it to this box (see [`crate::wasm::draw_app`]).
    pub(crate) fn draw_wasm(&self, c: &mut Canvas, r: Rect) {
        // The app renders on its own worker thread into an offscreen surface; the
        // compositor just copies the latest finished frame into the window.
        let pad = 14;
        let cx = r.x + pad;
        let cy = r.y + TITLE_H + 12;
        crate::wasm::blit_surface(c, cx, cy);
    }

    pub(crate) fn draw_start(&self, c: &mut Canvas) {
        let (sx, sy) = start_origin(self.sw, self.sh);
        let h = start_height();
        // Shadow + panel.
        c.fill_round_rect_alpha(
            (sx + 5) as usize,
            (sy + 10) as usize,
            START_W as usize,
            h as usize,
            16,
            theme::SHADOW,
            40,
        );
        c.fill_round_rect(
            sx as usize,
            sy as usize,
            START_W as usize,
            h as usize,
            14,
            theme::DOCK,
        );

        let hovered = start_item_at(self.sw, self.sh, self.cursor_x, self.cursor_y);
        let top = sy + START_PAD;
        for (i, kind) in Kind::ALL.iter().enumerate() {
            let ry = top + i as i32 * START_ROW_H;
            if hovered == Some(StartItem::App(*kind)) {
                start_row_highlight(c, sx, ry, theme::ACCENT);
            }
            icons::draw(
                c,
                kind.icon(),
                (sx + START_PAD) as usize,
                (ry + (START_ROW_H - 24) / 2) as usize,
                24,
            );
            font::draw_text(
                c,
                (sx + START_PAD + 36) as usize,
                (ry + (START_ROW_H - 14) / 2) as usize,
                kind.label(),
                theme::HEADER_TEXT,
                2,
            );
        }

        // Divider, then power actions.
        let pwr_top = top + Kind::ALL.len() as i32 * START_ROW_H + START_GAP;
        c.fill_rect(
            (sx + START_PAD) as usize,
            (pwr_top - START_GAP / 2) as usize,
            (START_W - START_PAD * 2) as usize,
            1,
            theme::DOCK_EDGE,
        );
        let power = [
            (StartItem::Reboot, "Reiniciar"),
            (StartItem::Shutdown, "Desligar"),
        ];
        for (i, (item, label)) in power.iter().enumerate() {
            let ry = pwr_top + i as i32 * START_ROW_H;
            if hovered == Some(*item) {
                start_row_highlight(c, sx, ry, theme::CLOSE);
            }
            icons::draw(
                c,
                Icon::Power,
                (sx + START_PAD) as usize,
                (ry + (START_ROW_H - 24) / 2) as usize,
                24,
            );
            font::draw_text(
                c,
                (sx + START_PAD + 36) as usize,
                (ry + (START_ROW_H - 14) / 2) as usize,
                label,
                theme::HEADER_TEXT,
                2,
            );
        }
    }

    /// The terminal grid is 40 columns x (14 scrollback rows + the prompt line)
    /// of fixed logical size. It is drawn at the biggest integer text scale
    /// (2..=4, never distorted) that fits the window and anchored top-left; when
    /// even scale 2 does not fit (a small window) the newest rows are kept and
    /// long lines are clipped, so nothing is ever drawn outside the window.
    pub(crate) fn draw_terminal(&self, c: &mut Canvas, r: Rect, t: &Terminal, focused: bool) {
        use osjeff_core::terminal::{COLS, PROMPT, ROWS};
        let pad = 12i32;
        let top = TITLE_H + 8;
        let (aw, ah) = (r.w - 2 * pad, r.h - top - 4);
        let s = fit_scale(aw, ah, COLS as i32 * 6, (ROWS as i32 + 1) * 9, 2, 4) as usize;
        let cell = font::cell_w(s);
        let line_h = 9 * s;
        let max_cols = (aw.max(0) as usize / cell).max(1);
        let max_rows = (ah.max(0) as usize / line_h).max(1);
        let tx = r.x.max(0) as usize + pad as usize;
        let mut ty = r.y.max(0) as usize + top as usize;
        let fg = theme::TEXT;

        // Keep the newest rows (and the prompt line) when the window is short.
        let skip = (t.row_count() + 1).saturating_sub(max_rows);
        for i in skip..t.row_count() {
            let row = t.row(i);
            font::draw_bytes(c, tx, ty, &row[..row.len().min(max_cols)], fg, s);
            ty += line_h;
        }
        font::draw_bytes(
            c,
            tx,
            ty,
            &PROMPT[..PROMPT.len().min(max_cols)],
            theme::TERM_PROMPT,
            s,
        );
        let ix = tx + PROMPT.len() * cell;
        // Narrow window: show the tail of the input line, the part being typed.
        let room = max_cols.saturating_sub(PROMPT.len());
        let input = t.input();
        let start = input.len().saturating_sub(room);
        font::draw_bytes(c, ix, ty, &input[start..], fg, s);
        if focused && t.caret() >= start && t.caret() - start <= room {
            let caret_x = ix + (t.caret() - start) * cell;
            c.fill_rect(caret_x, ty, s, 8 * s, theme::TERM_PROMPT);
        }
    }

    /// The editor grid is 44 columns x 18 rows of fixed logical size: drawn at
    /// the biggest integer scale (2..=4) that fits, anchored top-left, scrolled to
    /// keep the caret row visible and clipped to the window.
    pub(crate) fn draw_editor(&self, c: &mut Canvas, r: Rect, e: &EditorState, focused: bool) {
        use osjeff_core::editor::{COLS, ROWS};
        let ed = &e.editor;
        let pad = 10i32;
        let status_h = 22i32;
        let (aw, ah) = (r.w - 2 * pad, r.h - TITLE_H - 6 - status_h);
        let s = fit_scale(aw, ah, COLS as i32 * 6, ROWS as i32 * 8, 2, 4) as usize;
        let cell = font::cell_w(s);
        let line_h = 8 * s;
        let max_cols = (aw.max(0) as usize / cell).max(1);
        let vis = (ah.max(0) as usize / line_h).max(1);
        let tx = r.x.max(0) as usize + pad as usize;
        let top = r.y.max(0) as usize + TITLE_H as usize + 6;
        let fg = theme::TEXT;
        let (cxs, cys) = ed.cursor();

        let first = (cys + 1).saturating_sub(vis);
        for i in first..ed.rows().min(first + vis) {
            let line = ed.line(i);
            font::draw_bytes(
                c,
                tx,
                top + (i - first) * line_h,
                &line[..line.len().min(max_cols)],
                fg,
                s,
            );
        }
        if focused && cxs < max_cols && cys >= first && cys < first + vis {
            let caret_x = tx + cxs * cell;
            let caret_y = top + (cys - first) * line_h;
            c.fill_rect(caret_x, caret_y, s, 7 * s, theme::ACCENT);
        }

        let mut status = [b' '; 22];
        status[..3].copy_from_slice(b"Ln ");
        two(&mut status, 3, (cys + 1) as u8);
        status[5..10].copy_from_slice(b"  Col");
        two(&mut status, 11, (cxs + 1) as u8);
        if ed.dirty() {
            status[14] = b'*';
        }
        // The loaded file did not fit the grid: Ctrl+S is refused (see fs_save_in).
        if ed.is_lossy() {
            status[16..21].copy_from_slice(b"TRUNC");
        }
        let status_y = (r.y + r.h - status_h).max(0) as usize;
        font::draw_bytes(c, tx, status_y, &status, theme::TEXT_MUTED, 2);
    }

    /// Process list (scrolls to keep the selection visible), the real kernel
    /// threads and a footer pinned to the window's bottom edge.
    pub(crate) fn draw_taskmgr(&self, c: &mut Canvas, r: Rect) {
        let (x, y) = (r.x.max(0) as usize, r.y.max(0) as usize);
        let pad = 10usize;
        let line_h = 18usize;
        let tx = x + pad;
        let mut ty = y + TITLE_H as usize + 6;

        font::draw_text(c, tx, ty, "PID NAME        ST   UP", theme::TEXT_MUTED, 2);
        ty += line_h + 2;

        // Rows that fit between the header and the thread block + footer.
        let threads = sched::thread_count();
        let reserved = 6 + line_h + 2 + threads * line_h + 30;
        let list_h = (r.h.max(0) as usize).saturating_sub(ty - y + reserved);
        let visible = (list_h / line_h).max(1);
        let n = self.procs.len();
        let sel = self.procs.selected();
        let first = (sel + 1)
            .saturating_sub(visible)
            .min(n.saturating_sub(visible));

        for i in first..(first + visible).min(n) {
            let p = match self.procs.at(i) {
                Some(p) => p,
                None => break,
            };
            if i == sel {
                c.fill_round_rect_alpha(
                    tx - 4,
                    ty - 2,
                    24 * font::cell_w(2),
                    line_h,
                    4,
                    theme::ACCENT,
                    40,
                );
            }
            let mut line = [b' '; 27];
            write_uint(&mut line, 0, 3, p.pid as u32);
            let name = p.name();
            let n = name.len().min(12);
            line[4..4 + n].copy_from_slice(&name[..n]);
            let st: &[u8; 3] = match p.state {
                ProcState::Running => b"RUN",
                ProcState::Suspended => b"SUS",
                ProcState::Terminated => b"END",
            };
            line[17..20].copy_from_slice(st);
            write_uint(&mut line, 21, 6, p.ticks);
            font::draw_bytes(c, tx, ty, &line, theme::TEXT, 2);
            ty += line_h;
        }

        // Real kernel threads from the scheduler, with live CPU time.
        let footer_y = (r.bottom() - 24).max(0) as usize;
        ty += 6;
        font::draw_text(c, tx, ty, "KERNEL THREADS   CPU", theme::ACCENT_2, 2);
        ty += line_h + 2;
        for i in 0..threads {
            if ty + line_h > footer_y {
                break;
            }
            let name = sched::thread_name(i).as_bytes();
            let mut line = [b' '; 24];
            let n = name.len().min(14);
            line[..n].copy_from_slice(&name[..n]);
            if sched::thread_dead(i) {
                // A dead thread is never scheduled again: show that instead of a stale tick count.
                line[17..21].copy_from_slice(b"DEAD");
            } else {
                write_uint(&mut line, 17, 6, sched::thread_ticks(i) as u32);
            }
            font::draw_bytes(c, tx, ty, &line, theme::TEXT, 2);
            ty += line_h;
        }

        let footer = "UP/DN  ENTER:open  DEL:end";
        font::draw_text(c, tx, footer_y, footer, theme::TEXT_MUTED, 2);
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

    /// The Alt+Tab overlay: every window in most-recently-used order, the
    /// selection highlighted; minimized windows are tagged.
    pub(crate) fn draw_switcher(&self, c: &mut Canvas, sw: &Switcher) {
        let list = sw.list();
        let r = self.switcher_rect(list.len());
        c.fill_round_rect_alpha(
            (r.x + 5) as usize,
            (r.y + 10) as usize,
            r.w as usize,
            r.h as usize,
            16,
            theme::SHADOW,
            40,
        );
        c.fill_round_rect(
            r.x as usize,
            r.y as usize,
            r.w as usize,
            r.h as usize,
            14,
            theme::DOCK,
        );
        let sel = sw.selected_index();
        let first = (sel + 1).saturating_sub(SWITCH_ROWS);
        for (row, idx) in (first..list.len().min(first + SWITCH_ROWS)).enumerate() {
            let ry = r.y + SWITCH_PAD + row as i32 * SWITCH_ROW_H;
            if idx == sel {
                c.fill_round_rect_alpha(
                    (r.x + 6) as usize,
                    (ry + 2) as usize,
                    (r.w - 12) as usize,
                    (SWITCH_ROW_H - 4) as usize,
                    8,
                    theme::ACCENT,
                    48,
                );
            }
            let Some(w) = self.wm.get(list[idx]) else {
                continue;
            };
            icons::draw(
                c,
                w.app.kind().icon(),
                (r.x + 14) as usize,
                (ry + (SWITCH_ROW_H - 24) / 2) as usize,
                24,
            );
            let max_chars = ((r.w - 56 - 70) as usize) / font::cell_w(2);
            let title = w.app.title.as_bytes();
            let title = &title[..title.len().min(max_chars)];
            let ty = (ry + (SWITCH_ROW_H - 14) / 2) as usize;
            font::draw_bytes(c, (r.x + 50) as usize, ty, title, theme::HEADER_TEXT, 2);
            if w.minimized {
                let tag = "min";
                let tw = font::text_width(tag, 2) as i32;
                font::draw_text(
                    c,
                    (r.right() - 16 - tw) as usize,
                    ty,
                    tag,
                    theme::TEXT_MUTED,
                    2,
                );
            }
        }
    }

    pub(crate) fn draw_cursor(&self, c: &mut Canvas) {
        let px = self.cursor_x as usize;
        let py = self.cursor_y as usize;
        let outline = Color::rgb(0x10, 0x10, 0x10);
        let fill = Color::rgb(0xFF, 0xFF, 0xFF);
        for (row, line) in CURSOR.iter().enumerate() {
            for (col, ch) in line.bytes().enumerate() {
                let color = match ch {
                    b'#' => outline,
                    b'.' => fill,
                    _ => continue,
                };
                c.put(px + col, py + row, color);
            }
        }
    }
}

/// Convert a `web` engine color to a framebuffer color.
fn rgb(c: osjeff_core::web::Rgb) -> Color {
    Color::rgb(c.0, c.1, c.2)
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
        theme::ACCENT,
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
