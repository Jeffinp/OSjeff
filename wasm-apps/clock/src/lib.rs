//! `clock` — an analog and digital clock (ABI v2).
//!
//! Event driven: the manifest asks for one `on_tick` per second and the app redraws
//! only then (an idle clock costs no CPU between ticks). It scales with the window.
//! Time comes from `now_ms()` (wall clock since local midnight); sines and cosines
//! come from a 60-entry table, so there is no floating point.

#![no_std]

use core::fmt::Write;
use osjeff_sdk::*;

manifest!(
    "id=clock\nname=Clock\nname.pt=Relógio\nname.en=Clock\nversion=1.0.0\nabi=2\ntick_ms=1000\nwin_w=300\nwin_h=320\nwin_min_w=200\nwin_min_h=240\nmem_mib=2\n"
);
icon!(include_bytes!("../icon.png"));

/// sin(i * 6 degrees) * 1000, i = 0..59.
const SIN: [i32; 60] = [
    0, 105, 208, 309, 407, 500, 588, 669, 743, 809, 866, 914, 951, 978, 994, 1000, 994, 978, 951,
    914, 866, 809, 743, 669, 588, 500, 407, 309, 208, 105, 0, -105, -208, -309, -407, -500, -588,
    -669, -743, -809, -866, -914, -951, -978, -994, -1000, -994, -978, -951, -914, -866, -809,
    -743, -669, -588, -500, -407, -309, -208, -105,
];

fn sin60(i: i32) -> i32 {
    SIN[i.rem_euclid(60) as usize]
}
fn cos60(i: i32) -> i32 {
    SIN[(i + 15).rem_euclid(60) as usize]
}

struct Clock {
    secs: i32,
}

impl Clock {
    fn hms(&self) -> (i32, i32, i32) {
        (self.secs / 3600 % 24, self.secs / 60 % 60, self.secs % 60)
    }

    /// A thick line from the center to `len` along minute-position `pos` (0..60).
    fn hand(c: &mut Canvas, cx: i32, cy: i32, pos: i32, len: i32, thick: i32, rgb: u32) {
        let steps = len;
        for s in 0..=steps {
            let x = cx + sin60(pos) * s / 1000;
            let y = cy - cos60(pos) * s / 1000;
            c.fill_rect(x - thick / 2, y - thick / 2, thick, thick, rgb);
        }
    }
}

impl App for Clock {
    fn new() -> Self {
        Clock {
            secs: (now_ms() / 1000) as i32,
        }
    }

    fn on_tick(&mut self, _dt: i32) {
        self.secs = (now_ms() / 1000) as i32;
    }

    fn render(&mut self, c: &mut Canvas) {
        self.secs = (now_ms() / 1000) as i32;
        let (w, h) = c.size();
        c.clear(0x10141F);
        let (hh, mm, ss) = self.hms();
        // digital time on top
        let mut t = StrBuf::<16>::new();
        let _ = write!(t, "{:02}:{:02}:{:02}", hh, mm, ss);
        let scale = if w >= 300 { 4 } else { 2 };
        let tw = Canvas::text_width(t.as_str(), scale);
        c.text((w - tw) / 2, 14, t.as_str(), 0xFFFFFF, scale);
        // analog face below
        let top = 14 + 8 * scale + 14;
        let r = ((w.min(h - top) / 2) - 10).max(20);
        let (cx, cy) = (w / 2, top + (h - top) / 2);
        for i in 0..60 {
            let (big, col) = if i % 5 == 0 {
                (3, 0xFFFFFF)
            } else {
                (1, 0x6A7488)
            };
            let x = cx + sin60(i) * r / 1000;
            let y = cy - cos60(i) * r / 1000;
            c.fill_rect(x - big / 2, y - big / 2, big + 1, big + 1, col);
        }
        // hour, minute (positions in 6-degree steps), second
        let hour_pos = (hh % 12) * 5 + mm / 12;
        Clock::hand(c, cx, cy, hour_pos, r * 5 / 10, 5, 0xFFFFFF);
        Clock::hand(c, cx, cy, mm, r * 8 / 10, 3, 0x9AA6BD);
        Clock::hand(c, cx, cy, ss, r * 9 / 10, 2, 0xE54B4B);
        c.fill_rect(cx - 3, cy - 3, 7, 7, 0xE54B4B);
    }
}

export_app!(Clock);
