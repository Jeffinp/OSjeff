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
                    theme::accent(),
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
            theme::accent()
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
            let room = ((content.w - 16).max(0) as usize) / font::cell_w(2);
            let t: String = text.chars().take(room).collect();
            font::draw_text(
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
            bar_line(c, Color::rgb(0xE3, 0xE9, 0xF5), theme::TEXT, &t);
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
                theme::WHITE,
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
                let room = ((row.w - 44).max(0) as usize) / font::cell_w(2);
                let label: String = s.label.chars().take(room).collect();
                let label = osjeff_core::web::fold_for_display(&label);
                font::draw_text(
                    c,
                    (row.x + 36) as usize,
                    (row.y + 6) as usize,
                    &label,
                    theme::TEXT,
                    2,
                );
            }
        }
    }

    fn draw_browser_inner(&self, c: &mut Canvas, r: Rect, focused: bool, bs: &BrowserState) {
        let ch = BrowserChrome::of(r);

        // ---- toolbar: nav buttons + address bar + search button ----
        let tool_bg = Color::rgb(0xEC, 0xEF, 0xF5);
        let dim = Color::rgb(0xB8, 0xC0, 0xCE);
        draw_tool_button(c, ch.back, tool_bg, theme::HEADER);
        let on = |yes: bool| if yes { theme::HEADER } else { dim };
        glyph_arrow(c, ch.back, on(bs.browser.can_back()), true);
        draw_tool_button(c, ch.forward, tool_bg, theme::HEADER);
        glyph_arrow(c, ch.forward, on(bs.browser.can_forward()), false);
        draw_tool_button(c, ch.reload, tool_bg, theme::HEADER);
        glyph_reload(c, ch.reload, theme::HEADER, tool_bg);
        draw_tool_button(c, ch.home, tool_bg, theme::HEADER);
        glyph_home(c, ch.home, theme::HEADER);

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
        glyph_star(c, ch.star, bs.browser.is_bookmarked());

        // Connection badge, right-aligned inside the bar. The padlock ("Conexao
        // segura", green) is shown only for a connection whose certificate chain
        // was verified by the TLS client; an https page the user chose to open
        // despite a certificate error says "Certificado invalido" in red, and
        // plain http says it is not encrypted. Text is ASCII (bitmap font).
        let mut badge_reserved = 34;
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
            let px = bar.x + bar.w - pw - 42;
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
            badge_reserved = pw + 46;
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
            if focused && bs.browser.bar_selected() {
                c.fill_rect(
                    tx.saturating_sub(2),
                    ty - 2,
                    shown.len() * font::cell_w(2) + 4,
                    18,
                    Color::rgb(0xA8, 0xCB, 0xFF),
                );
            }
            font::draw_bytes(c, tx, ty, shown, theme::TEXT, 2);
            if focused && bs.browser.bar_focus() {
                let caret = bs.browser.caret().min(shown.len());
                let cx = tx + caret * font::cell_w(2);
                c.fill_rect(cx, ty - 1, 2, 16, theme::accent());
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
        self.paint_web_page(c, page, content, bs.scroll, bs);

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
            c.fill_round_rect(track_x, thumb_y, 4, thumb_h, 2, theme::accent());
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
        bs: &BrowserState,
    ) {
        use osjeff_core::web::Cmd;
        let top = scroll;
        let bottom = top + content.h;
        let ox = content.x;
        let oy = content.y - top;
        // Under the text: find matches, then the mouse selection.
        let mark = |c: &mut Canvas, s: &osjeff_core::web::textops::Span, col: Color| {
            if s.y + s.h < top || s.y > bottom {
                return;
            }
            let py = (oy + s.y).max(content.y);
            let ph = (s.y + s.h + oy).min(content.bottom()) - py;
            let px = ox + s.x;
            let pw = s.w.min(content.right() - px);
            if ph > 0 && pw > 0 {
                c.fill_rect(
                    px.max(0) as usize,
                    py.max(0) as usize,
                    pw as usize,
                    ph as usize,
                    col,
                );
            }
        };
        for s in bs.find.other_spans() {
            mark(c, s, Color::rgb(0xFF, 0xE0, 0x8A));
        }
        for s in bs.find.current_spans() {
            mark(c, s, Color::rgb(0xFF, 0xA9, 0x4D));
        }
        if let Some(range) = bs.sel {
            for s in &page.selection_spans(range) {
                mark(c, s, Color::rgb(0xA8, 0xCB, 0xFF));
            }
        }
        for cmd in &page.cmds {
            match cmd {
                Cmd::Image { x, y, w, h, idx } => {
                    if *y + *h < top || *y > bottom {
                        continue;
                    }
                    let key = bs.img_keys.get(*idx).and_then(|k| k.as_deref());
                    if let Some(img) = key.and_then(|k| bs.images.image(k)) {
                        paint_picture(c, img, ox + *x, oy + *y, *w, *h, content);
                    }
                }
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
        // Form controls: the text they hold, the caret and the focus ring.
        let focus = bs.forms.focus();
        for f in &page.fields {
            if f.y + f.h < top || f.y > bottom {
                continue;
            }
            let (fx, fy) = (ox + f.x, oy + f.y);
            if f.kind.is_text() {
                let cw = font::cell_w(f.scale as usize);
                let cols = ((f.w - 2 * f.pad_x).max(0) as usize / cw).max(1);
                let (shown, caret_col) = bs.forms.visible(&page.forms, f.form, f.field, cols);
                let ty = (fy + f.pad_y).max(content.y);
                if fy + f.pad_y >= content.y
                    && fy + f.pad_y + 9 * f.scale as i32 <= content.bottom()
                {
                    font::draw_text(
                        c,
                        (fx + f.pad_x) as usize,
                        ty as usize,
                        &shown,
                        theme::TEXT,
                        f.scale as usize,
                    );
                    if focus == Some((f.form, f.field)) {
                        c.fill_rect(
                            (fx + f.pad_x) as usize + caret_col * cw,
                            ty as usize,
                            2,
                            9 * f.scale as usize,
                            theme::accent(),
                        );
                    }
                }
            }
            if focus == Some((f.form, f.field)) && fy >= content.y && fy + f.h <= content.bottom() {
                let ring = theme::accent();
                let (x, y, w, h) = (fx as usize, fy as usize, f.w as usize, f.h as usize);
                c.fill_rect(x, y, w, 2, ring);
                c.fill_rect(x, y + h - 2, w, 2, ring);
                c.fill_rect(x, y, 2, h, ring);
                c.fill_rect(x + w - 2, y, 2, h, ring);
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
            theme::accent(),
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
            theme::accent(),
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
    pub(crate) fn draw_wasm(&self, c: &mut Canvas, r: Rect, w: &WasmWin) {
        // Each app renders on the `appd` thread into its own offscreen surface; the
        // compositor just copies the latest finished frame into the window (or shows
        // why the app is not running).
        let cr = wasm_content(r);
        crate::wasm::blit(w.id, c, cr.x, cr.y, cr.w, cr.h);
    }

    pub(crate) fn draw_start(&self, c: &mut Canvas) {
        let rows = self.start_rows();
        let (sx, sy) = start_origin(self.sw, self.sh, rows);
        let h = start_height(rows);
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

        let hovered = start_item_at(
            self.sw,
            self.sh,
            rows,
            self.start_scroll,
            self.cursor_x,
            self.cursor_y,
        );
        let top = sy + START_PAD;
        let max_chars = ((START_W - START_PAD * 2 - 44) as usize / font::cell_w(2)).max(4);
        for i in 0..rows {
            let n = self.start_scroll + i;
            let ry = top + i as i32 * START_ROW_H;
            let (item, label): (StartItem, &str) = match Kind::ALL.get(n) {
                Some(&k) => (StartItem::App(k), k.label()),
                None => match self.apps.get(n - Kind::ALL.len()) {
                    Some(a) => (StartItem::Wasm(n - Kind::ALL.len()), a.name.as_str()),
                    None => continue,
                },
            };
            if hovered == Some(item) {
                start_row_highlight(c, sx, ry, theme::accent());
            }
            let (ix, iy) = (
                (sx + START_PAD) as usize,
                (ry + (START_ROW_H - 24) / 2) as usize,
            );
            match item {
                StartItem::App(k) => icons::draw(c, k.icon(), ix, iy, 24),
                StartItem::Wasm(j) => match self.apps.get(j).and_then(|a| a.icon.as_deref()) {
                    Some(rgba) => c.draw_rgba(rgba, 24, 24, ix, iy),
                    None => icons::draw(c, Icon::WasmApp, ix, iy, 24),
                },
                _ => {}
            }
            let shown = &label[..label.len().min(max_chars)];
            font::draw_text(
                c,
                (sx + START_PAD + 36) as usize,
                (ry + (START_ROW_H - 14) / 2) as usize,
                shown,
                theme::HEADER_TEXT,
                2,
            );
        }
        // Scroll bar when the list does not fit.
        let total = self.start_total();
        if total > rows {
            let track_h = rows as i32 * START_ROW_H;
            let thumb_h = (track_h * rows as i32 / total as i32).max(16);
            let max_scroll = (total - rows) as i32;
            let thumb_y = top + (track_h - thumb_h) * self.start_scroll as i32 / max_scroll.max(1);
            let bx = (sx + START_W - 8) as usize;
            c.fill_rect(bx, top as usize, 3, track_h as usize, theme::DOCK_EDGE);
            c.fill_round_rect(
                bx,
                thumb_y as usize,
                3,
                thumb_h as usize,
                1,
                theme::accent(),
            );
        }

        // Divider, then power actions.
        let pwr_top = top + rows as i32 * START_ROW_H + START_GAP;
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
        let apps = crate::wasm::statuses();
        let apps_h = if apps.is_empty() {
            0
        } else {
            6 + line_h + 2 + apps.len() * line_h
        };
        let reserved = 6 + line_h + 2 + threads * line_h + 30 + apps_h;
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
                    theme::accent(),
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

        // WASM apps: state, CPU over the last second (wall time of their slices) and memory.
        if !apps.is_empty() {
            ty += 6;
            font::draw_text(c, tx, ty, "APPS         ST CPU%  MEM", theme::ACCENT_2, 2);
            ty += line_h + 2;
            for a in &apps {
                if ty + line_h > footer_y {
                    break;
                }
                let mut line = [b' '; 28];
                let name = alloc::format!("{}#{}", a.app_id, a.id);
                let nb = name.as_bytes();
                let n = nb.len().min(12);
                line[..n].copy_from_slice(&nb[..n]);
                line[13..16].copy_from_slice(a.state.label().as_bytes());
                write_uint(&mut line, 17, 3, a.cpu_pct as u32);
                write_uint(&mut line, 22, 5, a.mem_kib);
                line[27] = b'K';
                font::draw_bytes(c, tx, ty, &line, theme::TEXT, 2);
                ty += line_h;
            }
        }

        let footer = "ENTER:open DEL:end R:restart";
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
                    theme::accent(),
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
        let sprite: &[&str] = if self.cursor_is_hand() {
            &HAND
        } else {
            &CURSOR
        };
        for (row, line) in sprite.iter().enumerate() {
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

/// Copy `img` into the box `(x, y, w, h)` (screen coordinates), clipped to `clip`. A picture whose
/// size is not the box's (the layout moved on, an image just arrived) is sampled nearest-neighbour.
fn paint_picture(
    c: &mut Canvas,
    img: &osjeff_core::image::Image,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    clip: Rect,
) {
    let (iw, ih) = (img.width() as i64, img.height() as i64);
    let (w, h) = (i64::from(w.max(1)), i64::from(h.max(1)));
    let px = img.pixels();
    let y0 = y.max(clip.y);
    let y1 = (y + h as i32).min(clip.bottom());
    let x0 = x.max(clip.x);
    let x1 = (x + w as i32).min(clip.right());
    for sy in y0..y1 {
        let iy = ((i64::from(sy - y) * ih / h).min(ih - 1)) as usize;
        let row = &px[iy * iw as usize..(iy + 1) * iw as usize];
        for sx in x0..x1 {
            let ix = ((i64::from(sx - x) * iw / w).min(iw - 1)) as usize;
            let p = row[ix];
            c.put(
                sx as usize,
                sy as usize,
                Color::rgb((p >> 16) as u8, (p >> 8) as u8, p as u8),
            );
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
