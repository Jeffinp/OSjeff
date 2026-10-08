//! Calculadora: the keypad window. The arithmetic is `osjeff_core::calc`; the key layout is
//! `osjeff_core::layout::CALC_KEYS`.

use super::*;

impl Desktop {
    pub(crate) fn draw_calculator(&self, c: &mut Canvas, r: Rect, calc: &Calc) {
        use crate::text::{self, TITLE1, TITLE3, Weight};
        let p = theme::pal();
        let body = r.body();
        c.fill_rect(
            body.x.max(0) as usize,
            body.y.max(0) as usize,
            body.w.max(0) as usize,
            body.h.max(0) as usize,
            theme::window_body(),
        );
        let pad = 14;
        // Display: the result right-aligned in a large size (smaller when it is long).
        let disp = Rect::new(r.x + pad, r.y + TITLE_H + 12, r.w - pad * 2, 52);
        let shown = text::from_bytes(calc.display()).into_owned();
        let px = if text::measure(&shown, TITLE1, Weight::Regular) > disp.w - 8 {
            TITLE3
        } else {
            TITLE1
        };
        let col = if calc.is_error() {
            theme::danger()
        } else {
            theme::solid(p.text)
        };
        let w = text::measure(&shown, px, Weight::Regular);
        let ty = text::center_y(disp.y, disp.h, px, Weight::Regular);
        text::draw(
            c,
            (disp.right() - 8 - w).max(disp.x),
            ty,
            &shown,
            px,
            Weight::Regular,
            col,
        );

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
                let b = Rect::new(
                    gx + col as i32 * (cw + gap),
                    gy + row as i32 * (ch + gap),
                    bw,
                    bh,
                );
                let (bg, fg) = key_style(k, pending);
                let pressed = self.prev_left && b.contains(self.cursor_x, self.cursor_y);
                let bg = if pressed {
                    bg.lerp(Color::rgb(0x80, 0x80, 0x88), 50)
                } else {
                    bg
                };
                c.fill_rrect(b, 10, Corner::Circle, bg, 256);
                if matches!(k, b'0'..=b'9' | b'.') {
                    ui::stroke_token(c, b, 10, p.separator);
                }
                let label = match k {
                    0x08 => "⌫",
                    b'*' => "×",
                    b'/' => "÷",
                    b'-' => "−",
                    _ => core::str::from_utf8(core::slice::from_ref(&k)).unwrap_or("?"),
                };
                text::draw_centered(c, b, label, TITLE3, Weight::Medium, fg);
            }
        }
    }
}
