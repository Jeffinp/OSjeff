//! Calculadora: a rounded keypad with the accent on the operator column, a large display
//! whose text shrinks to fit, the pending expression and the last results above it, and a
//! copy button. The arithmetic is `osjeff_core::calc` (immediate execution, percent, sign,
//! one memory register, history); the key layout and its hit testing are
//! `osjeff_core::layout::{CALC_KEYS, calc_geom, calc_hit}`.

use super::kit;
use super::ui;
use super::*;
use crate::text::{self, FOOTNOTE, Weight};
use core::cell::Cell;
use core::ops::{Deref, DerefMut};
use osjeff_core::calc::pretty;
use osjeff_core::iconart::Glyph;
use osjeff_core::layout::{CALC_KEYS, CalcHit, calc_geom, calc_hit};

/// Milliseconds a key stays lit after a keyboard press, and the "Copiado" note stays up.
const FLASH_MS: u32 = 140;
const COPIED_MS: u32 = 1400;

// Hover keys: the key index (row * 4 + col + 1), or one of these.
const H_COPY: u32 = 100;
const H_DOWN: u32 = 1 << 31;

/// State of a calculator window: the arithmetic and what the pointer is doing.
pub(crate) struct CalcState {
    pub calc: Calc,
    hover: Cell<u32>,
    /// A key lit by the keyboard, and for how many more milliseconds.
    flash: Cell<(u8, u32)>,
    copied_ms: Cell<u32>,
}

impl CalcState {
    pub(crate) fn new() -> Self {
        Self {
            calc: Calc::new(),
            hover: Cell::new(0),
            flash: Cell::new((0, 0)),
            copied_ms: Cell::new(0),
        }
    }
}

impl Deref for CalcState {
    type Target = Calc;
    fn deref(&self) -> &Calc {
        &self.calc
    }
}

impl DerefMut for CalcState {
    fn deref_mut(&mut self) -> &mut Calc {
        &mut self.calc
    }
}

/// What a key looks like.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Digit,
    Function,
    Operator,
    Equals,
    Memory,
}

fn role(k: u8) -> Role {
    match k {
        b'0'..=b'9' | b'.' | b'n' => Role::Digit,
        b'C' | 0x08 | b'%' => Role::Function,
        b'+' | b'-' | b'*' | b'/' => Role::Operator,
        b'=' => Role::Equals,
        _ => Role::Memory,
    }
}

fn label(k: u8) -> &'static str {
    match k {
        0x01 => "MC",
        0x02 => "MR",
        0x03 => "M−",
        0x04 => "M+",
        b'C' => "C",
        0x08 => "⌫",
        b'%' => "%",
        b'/' => "÷",
        b'*' => "×",
        b'-' => "−",
        b'+' => "+",
        b'=' => "=",
        b'n' => "±",
        b'.' => {
            if osjeff_core::i18n::locale::decimal_sep(osjeff_core::i18n::lang()) == ',' {
                ","
            } else {
                "."
            }
        }
        b'0' => "0",
        b'1' => "1",
        b'2' => "2",
        b'3' => "3",
        b'4' => "4",
        b'5' => "5",
        b'6' => "6",
        b'7' => "7",
        b'8' => "8",
        _ => "9",
    }
}

/// A history line in the typography of the language in effect: `12 × 3 = 36`, with its
/// decimal separator.
fn pretty_line(line: &[u8]) -> String {
    let mut out = String::new();
    for part in line.split(|&b| b == b' ') {
        if !out.is_empty() {
            out.push(' ');
        }
        match part {
            b"*" => out.push('×'),
            b"/" => out.push('÷'),
            b"-" => out.push('−'),
            b"+" | b"=" => out.push(part[0] as char),
            _ => out.push_str(&pretty(part)),
        }
    }
    out
}

impl Desktop {
    /// Feed key `k` to calculator `id` (a click or a typed character) and light its key.
    pub(crate) fn calc_input(&mut self, id: WindowId, k: u8) {
        let k = match k {
            b'x' | b'X' => b'*',
            b':' => b'/',
            b'\r' | b'\n' => b'=',
            other => other,
        };
        if let Some(App::Calculator(c)) = self.app_mut(id) {
            if k == 0x08 {
                c.calc.backspace();
            } else {
                c.calc.input(k);
            }
            c.flash.set((k, FLASH_MS));
        }
    }

    /// Put the display on the clipboard and say so.
    fn calc_copy(&mut self, id: WindowId) {
        let text = match self.app_mut(id) {
            Some(App::Calculator(c)) => {
                c.copied_ms.set(COPIED_MS);
                c.calc.display().to_vec()
            }
            _ => return,
        };
        self.clipboard.set(&text);
    }

    pub(crate) fn calc_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        match calc_hit(rect, px, py) {
            Some(CalcHit::Key(k)) => self.calc_input(id, k),
            Some(CalcHit::Copy) => self.calc_copy(id),
            None => {}
        }
    }

    pub(crate) fn calc_hover(&mut self, cx: i32, cy: i32, down: bool) -> bool {
        let top = if self.overlay_open() {
            None
        } else {
            self.topmost_at(cx, cy)
        };
        let mut changed = false;
        let mut dirty = Vec::new();
        for w in self.wm.windows() {
            let App::Calculator(c) = &w.app.app else {
                continue;
            };
            if !w.shown() {
                continue;
            }
            let mut key = 0;
            if Some(w.id) == top {
                key = match calc_hit(w.rect, cx, cy) {
                    Some(CalcHit::Key(k)) => CALC_KEYS
                        .iter()
                        .flatten()
                        .position(|&x| x == k)
                        .map_or(0, |i| i as u32 + 1),
                    Some(CalcHit::Copy) => H_COPY,
                    None => 0,
                };
                if key != 0 && down {
                    key |= H_DOWN;
                }
            }
            if key != c.hover.get() {
                c.hover.set(key);
                changed = true;
                dirty.push(self.window_box(w));
            }
        }
        for r in dirty {
            self.mark_dirty(r);
        }
        changed
    }

    pub(crate) fn calc_step(&mut self, dt_ms: u32) {
        for w in self.wm.windows() {
            if let App::Calculator(c) = &w.app.app {
                let (k, ms) = c.flash.get();
                c.flash.set((k, ms.saturating_sub(dt_ms)));
                c.copied_ms.set(c.copied_ms.get().saturating_sub(dt_ms));
            }
        }
    }

    pub(crate) fn calc_busy_one(&self, w: &Win) -> bool {
        let App::Calculator(c) = &w.app.app else {
            return false;
        };
        w.shown() && (c.flash.get().1 > 0 || c.copied_ms.get() > 0)
    }

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
                osjeff_core::t!("calc.copied"),
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
