//! Drawing the calculator.

use super::state::CalcState;
use super::state::H_COPY;
use super::state::H_DOWN;
use super::state::Role;
use super::state::label;
use super::state::pretty_line;
use super::state::role;
use crate::desktop::kit;
use crate::desktop::kit::ui;
use crate::desktop::*;
use crate::text::{self, FOOTNOTE, Weight};
use kitsune_core::calc::pretty;
use kitsune_core::iconart::Glyph;
use kitsune_core::layout::{CALC_KEYS, calc_geom};

impl Desktop {
    pub(crate) fn draw_calculator(&self, c: &mut Canvas, r: Rect, st: &CalcState) {
        let p = theme::pal();
        let g = calc_geom(r);
        let hv = st.hover.get();
        let hover_key = hv & !H_DOWN;
        let down = hv & H_DOWN != 0;
        let calc = &st.calc;

        // History strip: the last results, then the pending expression.
        let mut lines: Vec<String> = calc.history().map(pretty_line).collect();
        let n = lines.len();
        let keep = if calc.operator().is_some() { 1 } else { 2 };
        lines.drain(..n.saturating_sub(keep));
        if let Some((t, len)) = calc.pending_text() {
            let mut s = String::new();
            let mut parts = t[..len].split(|&b| b == b' ');
            if let Some(n) = parts.next() {
                s.push_str(&pretty(n));
            }
            if let Some(op) = parts.next() {
                s.push(' ');
                s.push_str(match op {
                    b"*" => "×",
                    b"/" => "÷",
                    b"-" => "−",
                    _ => "+",
                });
            }
            lines.push(s);
        }
        let ly = g.history.bottom() - 2 - lines.len() as i32 * 16;
        for (i, l) in lines.iter().enumerate() {
            let y = ly + i as i32 * 16;
            let edge = if y < g.copy.bottom() {
                g.copy.x - 6
            } else {
                g.history.right()
            };
            let last = i + 1 == lines.len();
            text::draw_right(
                c,
                Rect::new(g.history.x, y, edge - g.history.x, 16),
                l,
                FOOTNOTE,
                Weight::Regular,
                if last && calc.operator().is_some() {
                    kit::ink2()
                } else {
                    kit::ink3()
                },
            );
        }
        // Copy button, with the confirmation after a click.
        if st.copied_ms.get() > 0 {
            text::draw_right(
                c,
                Rect::new(g.history.x, g.copy.y, g.copy.x - g.history.x - 6, g.copy.h),
                kitsune_core::t!("calc.copied"),
                FOOTNOTE,
                Weight::Medium,
                theme::accent(),
            );
        }
        if hover_key == H_COPY {
            ui::fill_token(c, g.copy, 6, p.hover);
        }
        ui::draw_glyph(
            c,
            Glyph::Copy,
            g.copy.x + 6,
            g.copy.y + 4,
            16,
            kit::argb(if hover_key == H_COPY {
                kit::ink()
            } else {
                kit::ink2()
            }),
        );

        // The display: the largest size that fits.
        let shown = pretty(calc.display());
        let room = g.display.w - 8;
        let px = [48u16, 42, 36, 32, 28, 24, 20]
            .into_iter()
            .find(|&px| text::measure(&shown, px, Weight::Regular) <= room)
            .unwrap_or(20);
        let col = if calc.is_error() {
            theme::danger()
        } else {
            kit::ink()
        };
        let tw = text::measure(&shown, px, Weight::Regular).min(room);
        let ty = text::center_y(g.display.y, g.display.h, px, Weight::Regular);
        text::draw_ellipsis(
            c,
            g.display.right() - 4 - tw,
            ty,
            room,
            &shown,
            px,
            Weight::Regular,
            col,
        );
        if calc.has_memory() {
            kit::chip(c, g.display.x, g.display.y + 8, 18, "M", theme::accent());
        }

        // The keys.
        let (fk, fms) = st.flash.get();
        for (row, cells) in g.keys.iter().enumerate() {
            for (col, b) in cells.iter().enumerate() {
                if b.w <= 0 || b.h <= 0 {
                    continue;
                }
                let k = CALC_KEYS[row][col];
                let idx = (row * 4 + col) as u32 + 1;
                let hovered = hover_key == idx;
                let pressed = (hovered && down) || (fms > 0 && fk == k);
                self.calc_key(c, *b, k, hovered, pressed, calc.operator());
            }
        }
    }

    fn calc_key(
        &self,
        c: &mut Canvas,
        b: Rect,
        k: u8,
        hovered: bool,
        pressed: bool,
        pending: Option<u8>,
    ) {
        let p = theme::pal();
        let acc = theme::accent();
        let white = Color::rgb(0xFF, 0xFF, 0xFF);
        let rad = if b.h < 40 { b.h / 2 } else { 12 };
        let role = role(k);
        let (bg, fg): (Color, Color) = match role {
            Role::Operator => {
                if pending == Some(k) {
                    (white, acc)
                } else {
                    (acc, white)
                }
            }
            Role::Equals => (acc.lerp(Color::rgb(0, 0, 0), 28), white),
            Role::Function => (theme::tool_bg(), kit::ink()),
            Role::Digit => (theme::button_bg(), kit::ink()),
            Role::Memory => (theme::tool_bg(), kit::ink2()),
        };
        let bg = if pressed {
            bg.lerp(Color::rgb(0x40, 0x40, 0x48), 60)
        } else if hovered {
            bg.lerp(Color::rgb(0xA0, 0xA0, 0xA8), 34)
        } else {
            bg
        };
        if role == Role::Memory {
            // Flat pills: only a wash behind the label.
            c.fill_rrect(
                b,
                rad,
                Corner::Circle,
                bg,
                if hovered || pressed { 256 } else { 150 },
            );
        } else {
            c.fill_rrect(b, rad, Corner::Circle, bg, 256);
        }
        if role == Role::Digit {
            ui::stroke_token(c, b, rad, p.separator);
        }
        let (px, w) = match role {
            Role::Memory => (13, Weight::Medium),
            Role::Digit => (22, Weight::Medium),
            _ => (24, Weight::Medium),
        };
        text::draw_centered(c, b, label(k), px, w, fg);
    }
}
