//! Desktop compositor: window manager + app rendering.
//!
//! Owns the two apps (terminal, editor) and their windows, routes PS/2 input
//! to the focused window, and paints everything. All non-trivial *logic* lives
//! in `osjeff_core` (and is unit-tested there); this module is hardware-facing
//! glue: pixels, hit-testing dispatch, and z-order bookkeeping.

use crate::fb::{Canvas, Color};
use crate::font;
use osjeff_core::window::TITLE_H;
use osjeff_core::{Action, Editor, Key, Keymap, Rect, Terminal, Time};

const TASKBAR_H: i32 = 48;

const TERM: usize = 0;
const EDIT: usize = 1;
const WIN_COUNT: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Terminal,
    Editor,
}

struct Win {
    rect: Rect,
    visible: bool,
    kind: Kind,
    title: &'static str,
}

struct Drag {
    win: usize,
    grab_dx: i32,
    grab_dy: i32,
}

pub struct Desktop {
    sw: i32,
    sh: i32,
    term: Terminal,
    editor: Editor,
    keymap: Keymap,
    windows: [Win; WIN_COUNT],
    order: [usize; WIN_COUNT], // back -> front
    drag: Option<Drag>,
    cursor_x: i32,
    cursor_y: i32,
    prev_left: bool,
}

impl Desktop {
    pub fn new(sw: i32, sh: i32) -> Self {
        let windows = [
            Win {
                rect: Rect::new(70, 80, 512, 320),
                visible: true,
                kind: Kind::Terminal,
                title: "OSJEFF SHELL",
            },
            Win {
                rect: Rect::new(610, 110, 560, 350),
                visible: false,
                kind: Kind::Editor,
                title: "OSJEFF EDIT",
            },
        ];
        Self {
            sw,
            sh,
            term: Terminal::new(),
            editor: Editor::new(),
            keymap: Keymap::new(),
            windows,
            order: [EDIT, TERM], // terminal starts focused (front)
            drag: None,
            cursor_x: sw / 2,
            cursor_y: sh / 2,
            prev_left: false,
        }
    }

    // ---- focus / z-order ----

    fn focused(&self) -> Option<usize> {
        for i in (0..WIN_COUNT).rev() {
            let w = self.order[i];
            if self.windows[w].visible {
                return Some(w);
            }
        }
        None
    }

    fn bring_to_front(&mut self, win: usize) {
        let pos = self.order.iter().position(|&w| w == win).unwrap_or(0);
        for i in pos..WIN_COUNT - 1 {
            self.order[i] = self.order[i + 1];
        }
        self.order[WIN_COUNT - 1] = win;
    }

    fn open(&mut self, win: usize) {
        self.windows[win].visible = true;
        self.bring_to_front(win);
    }

    fn topmost_at(&self, px: i32, py: i32) -> Option<usize> {
        for i in (0..WIN_COUNT).rev() {
            let w = self.order[i];
            if self.windows[w].visible && self.windows[w].rect.contains(px, py) {
                return Some(w);
            }
        }
        None
    }

    // ---- input ----

    /// Returns `true` when the frame must be repainted.
    pub fn handle_key(&mut self, scan: u8, extended: bool, pressed: bool, time: Time) -> bool {
        let Some(key) = self.keymap.process(scan, extended, pressed) else {
            return false;
        };
        let Some(top) = self.focused() else {
            return false;
        };
        match self.windows[top].kind {
            Kind::Terminal => {
                if self.term.on_key(key, time) == Action::OpenEditor {
                    self.open(EDIT);
                }
            }
            Kind::Editor => {
                if key == Key::Esc {
                    // Esc closes the editor and returns focus to the shell.
                    self.windows[EDIT].visible = false;
                } else {
                    self.editor.on_key(key);
                }
            }
        }
        true
    }

    /// Processes one mouse packet. Returns `true` (mouse always dirties).
    pub fn handle_mouse(&mut self, dx: i32, dy: i32, left: bool) -> bool {
        self.cursor_x = (self.cursor_x + dx).clamp(0, self.sw - 1);
        self.cursor_y = (self.cursor_y - dy).clamp(0, self.sh - 1); // Y inverted
        let (cx, cy) = (self.cursor_x, self.cursor_y);

        let pressed = left && !self.prev_left;
        let released = !left && self.prev_left;

        if pressed {
            if let Some(w) = self.topmost_at(cx, cy) {
                let rect = self.windows[w].rect;
                if rect.on_close(cx, cy) {
                    self.windows[w].visible = false;
                    self.drag = None;
                } else {
                    self.bring_to_front(w);
                    if rect.on_title(cx, cy) {
                        self.drag = Some(Drag {
                            win: w,
                            grab_dx: cx - rect.x,
                            grab_dy: cy - rect.y,
                        });
                    }
                }
            } else if let Some(app) = self.taskbar_hit(cx, cy) {
                self.open(app);
            }
        }

        if released {
            self.drag = None;
        }

        if let Some(d) = &self.drag {
            if left {
                let w = d.win;
                let mut r = self.windows[w].rect;
                r.x = cx - d.grab_dx;
                r.y = cy - d.grab_dy;
                let (x, y) = r.clamped_pos(self.sw, self.sh);
                self.windows[w].rect.x = x;
                self.windows[w].rect.y = y;
            } else {
                self.drag = None;
            }
        }

        self.prev_left = left;
        true
    }

    fn taskbar_hit(&self, px: i32, py: i32) -> Option<usize> {
        if py < self.sh - TASKBAR_H {
            return None;
        }
        let icons = taskbar_icon_rects(self.sw, self.sh);
        for (i, r) in icons.iter().enumerate() {
            if r.contains(px, py) {
                return match i {
                    0 | 1 => Some(TERM), // start logo + blue icon -> shell
                    2 => Some(EDIT),     // green icon -> editor
                    _ => None,
                };
            }
        }
        None
    }

    // ---- rendering ----

    pub fn render(&self, c: &mut Canvas, time: Time) {
        // Windows back-to-front.
        for i in 0..WIN_COUNT {
            let w = self.order[i];
            if !self.windows[w].visible {
                continue;
            }
            let focused = self.focused() == Some(w);
            self.draw_window(c, w, focused);
        }
        draw_clock(c, time);
        self.draw_cursor(c);
    }

    fn draw_window(&self, c: &mut Canvas, win: usize, focused: bool) {
        let r = self.windows[win].rect;
        let x = r.x as usize;
        let y = r.y as usize;
        let w = r.w as usize;
        let h = r.h as usize;
        let th = TITLE_H as usize;

        // Body + title bar (title color depends on focus).
        c.fill_round_rect(x, y, w, h, 10, Color::rgb(0xF4, 0xF6, 0xFA));
        let bar = if focused {
            Color::rgb(0x1F, 0x53, 0xA8)
        } else {
            Color::rgb(0x5A, 0x64, 0x70)
        };
        c.fill_rect(x, y + 6, w, th - 6, bar);
        c.fill_round_rect(x, y, w, th, 10, bar);
        font::draw_text(
            c,
            x + 12,
            y + 8,
            self.windows[win].title,
            Color::rgb(0xFF, 0xFF, 0xFF),
            2,
        );

        // Close button.
        let cb = r.close_rect();
        c.fill_round_rect(
            cb.x as usize,
            cb.y as usize,
            cb.w as usize,
            cb.h as usize,
            4,
            Color::rgb(0xE8, 0x4C, 0x3D),
        );

        match self.windows[win].kind {
            Kind::Terminal => self.draw_terminal(c, x, y, focused),
            Kind::Editor => self.draw_editor(c, x, y, focused),
        }
    }

    fn draw_terminal(&self, c: &mut Canvas, x: usize, y: usize, focused: bool) {
        let pad = 12usize;
        let line_h = 18usize;
        let tx = x + pad;
        let mut ty = y + TITLE_H as usize + 8;
        let fg = Color::rgb(0x14, 0x1A, 0x2A);

        for i in 0..self.term.row_count() {
            font::draw_bytes(c, tx, ty, self.term.row(i), fg, 2);
            ty += line_h;
        }

        // Prompt + input line.
        font::draw_bytes(
            c,
            tx,
            ty,
            osjeff_core::terminal::PROMPT,
            Color::rgb(0x0B, 0x55, 0x2B),
            2,
        );
        let ix = tx + osjeff_core::terminal::PROMPT.len() * font::cell_w(2);
        font::draw_bytes(c, ix, ty, self.term.input(), fg, 2);

        // Caret after the caret-th input character.
        if focused {
            let caret_x = ix + self.term.caret() * font::cell_w(2);
            c.fill_rect(caret_x, ty, 2, 16, Color::rgb(0x0B, 0x55, 0x2B));
        }
    }

    fn draw_editor(&self, c: &mut Canvas, x: usize, y: usize, focused: bool) {
        let pad = 10usize;
        let line_h = 16usize;
        let tx = x + pad;
        let top = y + TITLE_H as usize + 6;
        let fg = Color::rgb(0x12, 0x16, 0x20);

        for i in 0..self.editor.rows() {
            font::draw_bytes(c, tx, top + i * line_h, self.editor.line(i), fg, 2);
        }

        // Block caret.
        if focused {
            let (cxs, cys) = self.editor.cursor();
            let caret_x = tx + cxs * font::cell_w(2);
            let caret_y = top + cys * line_h;
            c.fill_rect(caret_x, caret_y, 2, 14, Color::rgb(0x1F, 0x53, 0xA8));
        }

        // Status line: "Ln L  Col C  *"
        let (cxs, cys) = self.editor.cursor();
        let mut status = [b' '; 22];
        status[..3].copy_from_slice(b"Ln ");
        two(&mut status, 3, (cys + 1) as u8);
        status[5..10].copy_from_slice(b"  Col");
        two(&mut status, 11, (cxs + 1) as u8);
        if self.editor.dirty() {
            status[14] = b'*';
        }
        let body_bottom = y + 350 - 22;
        font::draw_bytes(c, tx, body_bottom, &status, Color::rgb(0x55, 0x5E, 0x70), 2);
    }

    fn draw_cursor(&self, c: &mut Canvas) {
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

/// Layout of the 5 taskbar items (start logo + 4 app icons). Mirrors
/// [`paint_background`] so clicks line up with what is drawn.
fn taskbar_icon_rects(sw: i32, sh: i32) -> [Rect; 5] {
    let icon = 32i32;
    let gap = 12i32;
    let count = 5i32;
    let group_w = count * icon + (count - 1) * gap;
    let bar_y = sh - TASKBAR_H;
    let mut x = sw / 2 - group_w / 2;
    let y = bar_y + (TASKBAR_H - icon) / 2;
    let mut out = [Rect::new(0, 0, icon, icon); 5];
    for slot in out.iter_mut() {
        *slot = Rect::new(x, y, icon, icon);
        x += icon + gap;
    }
    out
}

/// Paints the static desktop layer (wallpaper + taskbar + icons). Called once.
pub fn paint_background(c: &mut Canvas) {
    let w = c.width();
    let h = c.height();

    let top = Color::rgb(0x10, 0x2A, 0x52);
    let bottom = Color::rgb(0x2B, 0x6F, 0xD6);
    for yy in 0..h {
        let t = ((yy * 255) / h.max(1)) as u16;
        c.fill_rect(0, yy, w, 1, top.lerp(bottom, t));
    }

    let bar_h = TASKBAR_H as usize;
    let bar_y = h.saturating_sub(bar_h);
    c.fill_rect(0, bar_y, w, bar_h, Color::rgb(0x20, 0x20, 0x28));
    font::draw_text(c, 14, bar_y + 16, "OSJEFF", Color::rgb(0xE6, 0xED, 0xFF), 2);

    let icons = taskbar_icon_rects(w as i32, h as i32);
    draw_start_logo(c, icons[0]);
    let colors = [
        Color::rgb(0x3B, 0x82, 0xF6), // shell
        Color::rgb(0x22, 0xC5, 0x5E), // editor
        Color::rgb(0xF5, 0x9E, 0x0B),
        Color::rgb(0xEF, 0x44, 0x44),
    ];
    for (i, color) in colors.iter().enumerate() {
        let r = icons[i + 1];
        c.fill_round_rect(
            r.x as usize,
            r.y as usize,
            r.w as usize,
            r.h as usize,
            8,
            *color,
        );
    }
}

fn draw_start_logo(c: &mut Canvas, r: Rect) {
    let blue = Color::rgb(0x2D, 0x7D, 0xF6);
    let gap = 3usize;
    let s = (r.w as usize - gap) / 2;
    let (x, y) = (r.x as usize, r.y as usize);
    for (dx, dy) in [(0, 0), (s + gap, 0), (0, s + gap), (s + gap, s + gap)] {
        c.fill_round_rect(x + dx, y + dy, s, s, 2, blue);
    }
}

fn draw_clock(c: &mut Canvas, t: Time) {
    let w = c.width();
    let bar_y = c.height().saturating_sub(TASKBAR_H as usize);
    let mut buf = [b'0'; 8]; // "HH:MM:SS"
    two(&mut buf, 0, t.h);
    buf[2] = b':';
    two(&mut buf, 3, t.m);
    buf[5] = b':';
    two(&mut buf, 6, t.s);
    let clock = unsafe { core::str::from_utf8_unchecked(&buf) };
    let cw = font::text_width(clock, 2);
    font::draw_text(
        c,
        w - cw - 16,
        bar_y + 16,
        clock,
        Color::rgb(0xE6, 0xED, 0xFF),
        2,
    );
}

fn two(buf: &mut [u8], idx: usize, val: u8) {
    buf[idx] = b'0' + (val / 10) % 10;
    buf[idx + 1] = b'0' + val % 10;
}

// Classic arrow cursor. '#' = outline, '.' = fill, ' ' = transparent.
const CURSOR: [&str; 16] = [
    "#         ",
    "##        ",
    "#.#       ",
    "#..#      ",
    "#...#     ",
    "#....#    ",
    "#.....#   ",
    "#......#  ",
    "#.......# ",
    "#....#####",
    "#..#.#    ",
    "#.# #.#   ",
    "##  #.#   ",
    "#    #.#  ",
    "     #.#  ",
    "      #   ",
];
