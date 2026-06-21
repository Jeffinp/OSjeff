//! `Desktop` methods: apps. Split out of the former monolithic desktop.rs.

use super::*;

impl Desktop {
    pub(crate) fn draw_menu(&self, c: &mut Canvas, mx: i32, my: i32) {
        let count = MENU_ITEMS.len() as i32;
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

        let item_icons = [
            Icon::Terminal,
            Icon::Editor,
            Icon::TaskMgr,
            Icon::Calculator,
        ];
        for (i, (label, _)) in MENU_ITEMS.iter().enumerate() {
            let iy = my + MENU_PAD + i as i32 * MENU_ITEM_H;
            if menu_item_at(mx, my, self.cursor_x, self.cursor_y) == Some(i) {
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
            icons::draw(c, item_icons[i], (mx + 10) as usize, iyc, isz);
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

    pub(crate) fn draw_calculator(&self, c: &mut Canvas, r: Rect, _focused: bool) {
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
        let disp = self.calc.display();
        let dscale = 3usize;
        let tw = disp.len() * font::cell_w(dscale);
        let tx = if tw + 16 < dw {
            dx + dw - tw - 16
        } else {
            dx + 10
        };
        let color = if self.calc.is_error() {
            theme::CLOSE
        } else {
            theme::ACCENT
        };
        font::draw_bytes(c, tx, dy + (dh - 7 * dscale) / 2, disp, color, dscale);

        // Keypad.
        let (gx, gy, cw, ch, gap) = calc_layout(r);
        let pending = self.calc.operator();
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
        let app_icons = [
            Icon::Terminal,
            Icon::Editor,
            Icon::TaskMgr,
            Icon::Calculator,
        ];

        for (i, (label, win)) in START_APPS.iter().enumerate() {
            let ry = top + i as i32 * START_ROW_H;
            if hovered == Some(StartItem::App(*win)) {
                start_row_highlight(c, sx, ry, theme::ACCENT);
            }
            icons::draw(
                c,
                app_icons[i],
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

        // Divider, then power actions.
        let pwr_top = top + START_APPS.len() as i32 * START_ROW_H + START_GAP;
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

    pub(crate) fn draw_terminal(&self, c: &mut Canvas, x: usize, y: usize, focused: bool) {
        let pad = 12usize;
        let line_h = 18usize;
        let tx = x + pad;
        let mut ty = y + TITLE_H as usize + 8;
        let fg = theme::TEXT;

        for i in 0..self.term.row_count() {
            font::draw_bytes(c, tx, ty, self.term.row(i), fg, 2);
            ty += line_h;
        }
        font::draw_bytes(
            c,
            tx,
            ty,
            osjeff_core::terminal::PROMPT,
            theme::TERM_PROMPT,
            2,
        );
        let ix = tx + osjeff_core::terminal::PROMPT.len() * font::cell_w(2);
        font::draw_bytes(c, ix, ty, self.term.input(), fg, 2);
        if focused {
            let caret_x = ix + self.term.caret() * font::cell_w(2);
            c.fill_rect(caret_x, ty, 2, 16, theme::TERM_PROMPT);
        }
    }

    pub(crate) fn draw_editor(&self, c: &mut Canvas, x: usize, y: usize, focused: bool) {
        let pad = 10usize;
        let line_h = 16usize;
        let tx = x + pad;
        let top = y + TITLE_H as usize + 6;
        let fg = theme::TEXT;

        for i in 0..self.editor.rows() {
            font::draw_bytes(c, tx, top + i * line_h, self.editor.line(i), fg, 2);
        }
        let (cxs, cys) = self.editor.cursor();
        if focused {
            let caret_x = tx + cxs * font::cell_w(2);
            let caret_y = top + cys * line_h;
            c.fill_rect(caret_x, caret_y, 2, 14, theme::ACCENT);
        }

        let mut status = [b' '; 22];
        status[..3].copy_from_slice(b"Ln ");
        two(&mut status, 3, (cys + 1) as u8);
        status[5..10].copy_from_slice(b"  Col");
        two(&mut status, 11, (cxs + 1) as u8);
        if self.editor.dirty() {
            status[14] = b'*';
        }
        font::draw_bytes(c, tx, y + 350 - 22, &status, theme::TEXT_MUTED, 2);
    }

    pub(crate) fn draw_taskmgr(&self, c: &mut Canvas, x: usize, y: usize) {
        let pad = 10usize;
        let line_h = 18usize;
        let tx = x + pad;
        let mut ty = y + TITLE_H as usize + 6;

        font::draw_text(c, tx, ty, "PID NAME        ST   UP", theme::TEXT_MUTED, 2);
        ty += line_h + 2;

        for i in 0..self.procs.len() {
            let p = match self.procs.at(i) {
                Some(p) => p,
                None => break,
            };
            if i == self.procs.selected() {
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
        ty += 6;
        font::draw_text(c, tx, ty, "KERNEL THREADS   CPU", theme::ACCENT_2, 2);
        ty += line_h + 2;
        for i in 0..sched::thread_count() {
            let name = sched::thread_name(i).as_bytes();
            let mut line = [b' '; 24];
            let n = name.len().min(14);
            line[..n].copy_from_slice(&name[..n]);
            write_uint(&mut line, 17, 6, sched::thread_ticks(i) as u32);
            font::draw_bytes(c, tx, ty, &line, theme::TEXT, 2);
            ty += line_h;
        }

        let footer = "UP/DN  ENTER:open  DEL:end";
        font::draw_text(c, tx, y + 300 - 24, footer, theme::TEXT_MUTED, 2);
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
