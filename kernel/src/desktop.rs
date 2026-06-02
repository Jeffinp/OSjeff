//! Desktop compositor: window manager, app rendering, open/close animations,
//! and a process table surfaced through a Task Manager app.
//!
//! All non-trivial logic (terminal, editor, keymap, window geometry, easing,
//! process table) lives in `osjeff_core` and is unit-tested. This module is
//! hardware-facing glue: pixels, z-order, animation stepping, dispatch.

use crate::fb::{Canvas, Color};
use crate::font;
use crate::icons::{self, Icon};
use osjeff_core::window::TITLE_H;
use osjeff_core::{
    Action, Anim, Editor, Key, Keymap, ProcKind, ProcState, ProcessTable, Rect, Terminal, Time,
};

const TASKBAR_H: i32 = 48;
const SLIDE_PX: f32 = 28.0;

// Scratch buffer to snapshot the area behind an animating window (largest
// window + margin). Lets fades composite over real content, not the wallpaper.
const SCRATCH_BYTES: usize = 640 * 440 * 4;
static mut SCRATCH: [u8; SCRATCH_BYTES] = [0; SCRATCH_BYTES];

const TERM: usize = 0;
const EDIT: usize = 1;
const TASK: usize = 2;
const WIN_COUNT: usize = 3;

/// Cursor sprite bounding box (used by the dirty-rect overlay path).
pub const CURSOR_W: i32 = 10;
pub const CURSOR_H: i32 = 16;

// Right-click context menu geometry.
const MENU_W: i32 = 220;
const MENU_ITEM_H: i32 = 32;
const MENU_PAD: i32 = 6;
const MENU_ITEMS: [(&str, usize); 3] =
    [("Terminal", TERM), ("Editor", EDIT), ("Task Manager", TASK)];

/// What changed after a mouse packet, so the caller can pick the cheap
/// cursor-only repaint vs a full scene recompose.
pub struct MouseResult {
    pub scene_dirty: bool,
    pub cursor_moved: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Terminal,
    Editor,
    TaskMgr,
}

struct Win {
    rect: Rect,
    visible: bool,
    kind: Kind,
    title: &'static str,
    proc_name: &'static [u8],
    anim: Option<Anim>,
    pid: u16, // 0 = no live process (app closed)
}

impl Win {
    /// A window accepts focus/clicks only when shown and not animating out.
    fn active(&self) -> bool {
        self.visible && !matches!(self.anim, Some(a) if a.is_closing())
    }
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
    procs: ProcessTable,
    windows: [Win; WIN_COUNT],
    order: [usize; WIN_COUNT], // back -> front
    drag: Option<Drag>,
    menu: Option<(i32, i32)>,
    cursor_x: i32,
    cursor_y: i32,
    prev_left: bool,
    prev_right: bool,
}

impl Desktop {
    pub fn new(sw: i32, sh: i32) -> Self {
        // At boot only the system processes and the open terminal exist.
        // Apps spawn a fresh process when opened and are removed when closed.
        let mut procs = ProcessTable::new();
        procs.spawn(b"kernel", ProcKind::System, ProcState::Running);
        procs.spawn(b"compositor", ProcKind::System, ProcState::Running);
        let shell_pid = procs
            .spawn(b"shell", ProcKind::App, ProcState::Running)
            .unwrap();

        let windows = [
            Win {
                rect: Rect::new(70, 80, 512, 320),
                visible: true,
                kind: Kind::Terminal,
                title: "OSJEFF SHELL",
                proc_name: b"shell",
                anim: Some(Anim::open()),
                pid: shell_pid,
            },
            Win {
                rect: Rect::new(610, 110, 560, 350),
                visible: false,
                kind: Kind::Editor,
                title: "OSJEFF EDIT",
                proc_name: b"editor",
                anim: None,
                pid: 0,
            },
            Win {
                rect: Rect::new(360, 200, 392, 300),
                visible: false,
                kind: Kind::TaskMgr,
                title: "TASK MANAGER",
                proc_name: b"taskmgr",
                anim: None,
                pid: 0,
            },
        ];

        Self {
            sw,
            sh,
            term: Terminal::new(),
            editor: Editor::new(),
            keymap: Keymap::new(),
            procs,
            windows,
            order: [TASK, EDIT, TERM], // terminal focused
            drag: None,
            menu: None,
            cursor_x: sw / 2,
            cursor_y: sh / 2,
            prev_left: false,
            prev_right: false,
        }
    }

    pub fn cursor(&self) -> (i32, i32) {
        (self.cursor_x, self.cursor_y)
    }

    // ---- focus / z-order ----

    fn focused(&self) -> Option<usize> {
        for i in (0..WIN_COUNT).rev() {
            let w = self.order[i];
            if self.windows[w].active() {
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
        if self.windows[win].visible {
            // Already shown (or animating out): cancel any close and just focus.
            if matches!(self.windows[win].anim, Some(a) if a.is_closing()) {
                self.windows[win].anim = Some(Anim::open());
            }
            self.bring_to_front(win);
            return;
        }
        // Launching a closed app spawns a fresh process (new pid, uptime 0).
        self.windows[win].visible = true;
        self.windows[win].anim = Some(Anim::open());
        let pid = self
            .procs
            .spawn(
                self.windows[win].proc_name,
                ProcKind::App,
                ProcState::Running,
            )
            .unwrap_or(0);
        self.windows[win].pid = pid;
        self.bring_to_front(win);
    }

    fn request_close(&mut self, win: usize) {
        // Ignore if already animating out.
        if matches!(self.windows[win].anim, Some(a) if a.is_closing()) {
            return;
        }
        self.windows[win].anim = Some(Anim::close());
    }

    fn window_of_pid(&self, pid: u16) -> Option<usize> {
        if pid == 0 {
            return None;
        }
        (0..WIN_COUNT).find(|&w| self.windows[w].pid == pid)
    }

    fn topmost_at(&self, px: i32, py: i32) -> Option<usize> {
        for i in (0..WIN_COUNT).rev() {
            let w = self.order[i];
            if self.windows[w].active() && self.windows[w].rect.contains(px, py) {
                return Some(w);
            }
        }
        None
    }

    // ---- animation & scheduler ----

    /// Advance all running animations by `dt`. Returns `true` while any window
    /// is still animating (the caller keeps rendering).
    pub fn animate(&mut self, dt: f32) -> bool {
        let mut active = false;
        for w in 0..WIN_COUNT {
            if let Some(a) = self.windows[w].anim.as_mut() {
                a.step(dt);
                if a.finished() {
                    let closing = a.is_closing();
                    self.windows[w].anim = None;
                    if closing {
                        // Closing an app terminates its process (removed from
                        // the table), matching how a desktop app behaves.
                        self.windows[w].visible = false;
                        let pid = self.windows[w].pid;
                        self.procs.kill(pid);
                        self.windows[w].pid = 0;
                    }
                } else {
                    active = true;
                }
            }
        }
        active
    }

    /// One scheduler quantum: advance CPU-time of running processes.
    pub fn tick_processes(&mut self) {
        self.procs.tick();
    }

    // ---- input ----

    pub fn handle_key(&mut self, scan: u8, extended: bool, pressed: bool, time: Time) -> bool {
        let Some(key) = self.keymap.process(scan, extended, pressed) else {
            return false;
        };
        let Some(top) = self.focused() else {
            return false;
        };
        match self.windows[top].kind {
            Kind::Terminal => match self.term.on_key(key, time) {
                Action::OpenEditor => self.open(EDIT),
                Action::OpenTasks => self.open(TASK),
                Action::None => {}
            },
            Kind::Editor => {
                if key == Key::Esc {
                    self.request_close(EDIT);
                } else {
                    self.editor.on_key(key);
                }
            }
            Kind::TaskMgr => self.task_key(key),
        }
        true
    }

    fn task_key(&mut self, key: Key) {
        match key {
            Key::Up => self.procs.select_prev(),
            Key::Down => self.procs.select_next(),
            Key::Enter => {
                if let Some(pid) = self.procs.selected_pid() {
                    if let Some(w) = self.window_of_pid(pid) {
                        self.open(w);
                    }
                }
            }
            Key::Delete => {
                if let Some(pid) = self.procs.selected_pid() {
                    if let Some(p) = self.procs.get(pid) {
                        if p.kind == ProcKind::App {
                            if let Some(w) = self.window_of_pid(pid) {
                                self.request_close(w);
                            }
                        }
                    }
                }
            }
            Key::Esc => self.request_close(TASK),
            _ => {}
        }
    }

    pub fn handle_mouse(&mut self, dx: i32, dy: i32, left: bool, right: bool) -> MouseResult {
        self.cursor_x = (self.cursor_x + dx).clamp(0, self.sw - 1);
        self.cursor_y = (self.cursor_y - dy).clamp(0, self.sh - 1);
        let (cx, cy) = (self.cursor_x, self.cursor_y);
        let cursor_moved = dx != 0 || dy != 0;
        let mut scene = false;

        let left_pressed = left && !self.prev_left;
        let right_pressed = right && !self.prev_right;
        let released = !left && self.prev_left;

        // Right click opens the context menu at the cursor.
        if right_pressed {
            self.menu = Some(self.clamp_menu(cx, cy));
            scene = true;
        }

        if left_pressed {
            if let Some((mx, my)) = self.menu {
                // A click while the menu is open selects an item or dismisses it.
                if let Some(i) = menu_item_at(mx, my, cx, cy) {
                    self.open(MENU_ITEMS[i].1);
                }
                self.menu = None;
                scene = true;
            } else if let Some(w) = self.topmost_at(cx, cy) {
                let rect = self.windows[w].rect;
                if rect.on_close(cx, cy) {
                    self.request_close(w);
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
                scene = true;
            } else if let Some(app) = self.taskbar_hit(cx, cy) {
                self.open(app);
                scene = true;
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
                scene = true;
            } else {
                self.drag = None;
            }
        }

        // While the menu is open, movement must repaint for hover highlight.
        if self.menu.is_some() && cursor_moved {
            scene = true;
        }

        self.prev_left = left;
        self.prev_right = right;
        MouseResult {
            scene_dirty: scene,
            cursor_moved,
        }
    }

    fn clamp_menu(&self, x: i32, y: i32) -> (i32, i32) {
        let h = MENU_PAD * 2 + MENU_ITEMS.len() as i32 * MENU_ITEM_H;
        let mx = x.min(self.sw - MENU_W).max(0);
        let my = y.min(self.sh - h).max(0);
        (mx, my)
    }

    fn taskbar_hit(&self, px: i32, py: i32) -> Option<usize> {
        if py < self.sh - TASKBAR_H {
            return None;
        }
        let icons = taskbar_icon_rects(self.sw, self.sh);
        for (i, r) in icons.iter().enumerate() {
            if r.contains(px, py) {
                return match i {
                    0 | 1 => Some(TERM),
                    2 => Some(EDIT),
                    3 => Some(TASK),
                    _ => None,
                };
            }
        }
        None
    }

    // ---- rendering ----

    pub fn render(&self, back: &mut [u8], info: bootloader_api::info::FrameBufferInfo, time: Time) {
        let bpp = info.bytes_per_pixel;
        let scratch: &mut [u8] = unsafe {
            core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(SCRATCH) as *mut u8, SCRATCH_BYTES)
        };
        let mut c = Canvas::new(back, info);

        for i in 0..WIN_COUNT {
            let w = self.order[i];
            if !self.windows[w].visible {
                continue;
            }
            let focused = self.focused() == Some(w);
            let anim = self.windows[w].anim;
            let yoff = anim.map(|a| a.slide(SLIDE_PX) as i32).unwrap_or(0);
            let mut rect = self.windows[w].rect;
            rect.y += yoff;

            // Shadows only when settled (not dragging, not animating): keeps drag
            // fast and avoids shadow "leftovers" during a fade.
            let shadow = self.drag.is_none() && anim.is_none();

            match anim {
                Some(a) => {
                    let x = rect.x.max(0) as usize;
                    let y = rect.y.max(0) as usize;
                    let (rw, rh) = (rect.w as usize, rect.h as usize);
                    if rw * rh * bpp <= scratch.len() {
                        // Snapshot the backdrop (lower windows + wallpaper), draw
                        // the window, then fade it toward that snapshot — so the
                        // window behind shows through instead of the wallpaper.
                        c.snapshot_region(scratch, x, y, rw, rh);
                        self.draw_window(&mut c, w, rect, focused, shadow);
                        let alpha = (a.alpha() * 256.0) as u16;
                        c.blend_from_local(scratch, x, y, rw, rh, alpha);
                    } else {
                        self.draw_window(&mut c, w, rect, focused, shadow);
                    }
                }
                None => self.draw_window(&mut c, w, rect, focused, shadow),
            }
        }

        draw_clock(&mut c, time);
        if let Some((mx, my)) = self.menu {
            self.draw_menu(&mut c, mx, my);
        }
    }

    fn draw_menu(&self, c: &mut Canvas, mx: i32, my: i32) {
        let count = MENU_ITEMS.len() as i32;
        let h = MENU_PAD * 2 + count * MENU_ITEM_H;

        // Drop shadow + panel.
        c.fill_round_rect_alpha(
            (mx + 4) as usize,
            (my + 6) as usize,
            MENU_W as usize,
            h as usize,
            10,
            Color::rgb(0, 0, 0),
            30,
        );
        c.fill_round_rect(
            mx as usize,
            my as usize,
            MENU_W as usize,
            h as usize,
            8,
            Color::rgb(0xFB, 0xFC, 0xFE),
        );

        let item_icons = [Icon::Terminal, Icon::Editor, Icon::TaskMgr];
        for (i, (label, _)) in MENU_ITEMS.iter().enumerate() {
            let iy = my + MENU_PAD + i as i32 * MENU_ITEM_H;
            if menu_item_at(mx, my, self.cursor_x, self.cursor_y) == Some(i) {
                c.fill_round_rect(
                    (mx + 4) as usize,
                    (iy + 2) as usize,
                    (MENU_W - 8) as usize,
                    (MENU_ITEM_H - 4) as usize,
                    6,
                    Color::rgb(0xDC, 0xE8, 0xFB),
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
                Color::rgb(0x14, 0x1A, 0x28),
                2,
            );
        }
    }

    /// Draws the mouse cursor. Called as an overlay directly onto the
    /// framebuffer so cursor moves don't require a full scene recompose.
    pub fn draw_cursor_overlay(&self, c: &mut Canvas) {
        self.draw_cursor(c);
    }

    fn draw_window(&self, c: &mut Canvas, win: usize, r: Rect, focused: bool, shadow: bool) {
        let x = r.x.max(0) as usize;
        let y = r.y.max(0) as usize;
        let w = r.w as usize;
        let h = r.h as usize;
        let th = TITLE_H as usize;

        // Soft drop shadow (two layers). Skipped while dragging/animating so the
        // alpha fill never costs on the hot path.
        if shadow {
            let black = Color::rgb(0, 0, 0);
            for &(off, exp, a) in &[(5usize, 4usize, 26u16), (12, 10, 14)] {
                let sx = x.saturating_sub(exp);
                let sy = y + off;
                c.fill_round_rect_alpha(sx, sy, w + exp * 2, h + exp, 12 + exp, black, a);
            }
        }

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

        let cb = r.close_rect();
        c.fill_round_rect(
            cb.x.max(0) as usize,
            cb.y.max(0) as usize,
            cb.w as usize,
            cb.h as usize,
            4,
            Color::rgb(0xE8, 0x4C, 0x3D),
        );

        match self.windows[win].kind {
            Kind::Terminal => self.draw_terminal(c, x, y, focused),
            Kind::Editor => self.draw_editor(c, x, y, focused),
            Kind::TaskMgr => self.draw_taskmgr(c, x, y),
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
        let (cxs, cys) = self.editor.cursor();
        if focused {
            let caret_x = tx + cxs * font::cell_w(2);
            let caret_y = top + cys * line_h;
            c.fill_rect(caret_x, caret_y, 2, 14, Color::rgb(0x1F, 0x53, 0xA8));
        }

        let mut status = [b' '; 22];
        status[..3].copy_from_slice(b"Ln ");
        two(&mut status, 3, (cys + 1) as u8);
        status[5..10].copy_from_slice(b"  Col");
        two(&mut status, 11, (cxs + 1) as u8);
        if self.editor.dirty() {
            status[14] = b'*';
        }
        font::draw_bytes(
            c,
            tx,
            y + 350 - 22,
            &status,
            Color::rgb(0x55, 0x5E, 0x70),
            2,
        );
    }

    fn draw_taskmgr(&self, c: &mut Canvas, x: usize, y: usize) {
        let pad = 10usize;
        let line_h = 18usize;
        let tx = x + pad;
        let mut ty = y + TITLE_H as usize + 6;

        font::draw_text(
            c,
            tx,
            ty,
            "PID NAME        ST   UP",
            Color::rgb(0x33, 0x3A, 0x4A),
            2,
        );
        ty += line_h + 2;

        for i in 0..self.procs.len() {
            let p = match self.procs.at(i) {
                Some(p) => p,
                None => break,
            };
            if i == self.procs.selected() {
                c.fill_rect(
                    tx - 4,
                    ty - 2,
                    24 * font::cell_w(2),
                    line_h,
                    Color::rgb(0xD6, 0xE3, 0xFB),
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
            font::draw_bytes(c, tx, ty, &line, Color::rgb(0x16, 0x1C, 0x2C), 2);
            ty += line_h;
        }

        let footer = "UP/DN  ENTER:open  DEL:end";
        font::draw_text(c, tx, y + 300 - 24, footer, Color::rgb(0x55, 0x5E, 0x70), 2);
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

/// Index of the context-menu item under `(px, py)`, if any.
fn menu_item_at(mx: i32, my: i32, px: i32, py: i32) -> Option<usize> {
    if px < mx + MENU_PAD || px >= mx + MENU_W - MENU_PAD {
        return None;
    }
    let rel = py - (my + MENU_PAD);
    if rel < 0 {
        return None;
    }
    let i = (rel / MENU_ITEM_H) as usize;
    (i < MENU_ITEMS.len()).then_some(i)
}

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

pub fn paint_background(c: &mut Canvas) {
    let w = c.width();
    let h = c.height();

    // Base vertical gradient.
    let top = Color::rgb(0x0C, 0x22, 0x47);
    let bottom = Color::rgb(0x2B, 0x6F, 0xD6);
    for yy in 0..h {
        let t = ((yy * 255) / h.max(1)) as u16;
        c.fill_rect(0, yy, w, 1, top.lerp(bottom, t));
    }

    // Soft radial glow upper-center for depth.
    let glow = (w / 2).max(400);
    c.fill_round_rect_alpha(
        w / 2 - glow / 2,
        0usize.saturating_add(h / 6).saturating_sub(glow / 2),
        glow,
        glow,
        glow / 2,
        Color::rgb(0x5A, 0x9B, 0xFF),
        22,
    );
    // Subtle vignette at the bottom corners.
    let vig = w / 3;
    c.fill_round_rect_alpha(0, h - vig / 2, vig, vig, vig / 2, Color::rgb(0, 0, 0), 12);
    c.fill_round_rect_alpha(
        w - vig,
        h - vig / 2,
        vig,
        vig,
        vig / 2,
        Color::rgb(0, 0, 0),
        12,
    );

    // Floating taskbar with a top highlight line + shadow.
    let bar_h = TASKBAR_H as usize;
    let bar_y = h.saturating_sub(bar_h);
    c.fill_round_rect_alpha(0, bar_y - 8, w, 8, 0, Color::rgb(0, 0, 0), 26);
    c.fill_rect(0, bar_y, w, bar_h, Color::rgb(0x18, 0x1D, 0x2A));
    c.fill_rect(0, bar_y, w, 1, Color::rgb(0x3A, 0x46, 0x60));
    font::draw_text(c, 16, bar_y + 16, "OSJEFF", Color::rgb(0xE6, 0xED, 0xFF), 2);

    // Real app icons.
    let r = taskbar_icon_rects(w as i32, h as i32);
    let s = r[0].w as usize;
    icons::draw(c, Icon::Start, r[0].x as usize, r[0].y as usize, s);
    icons::draw(c, Icon::Terminal, r[1].x as usize, r[1].y as usize, s);
    icons::draw(c, Icon::Editor, r[2].x as usize, r[2].y as usize, s);
    icons::draw(c, Icon::TaskMgr, r[3].x as usize, r[3].y as usize, s);
}

fn draw_clock(c: &mut Canvas, t: Time) {
    let w = c.width();
    let bar_y = c.height().saturating_sub(TASKBAR_H as usize);
    let mut buf = [b'0'; 8];
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

/// Right-align `v` as decimal digits in `buf[start..start+width]`.
fn write_uint(buf: &mut [u8], start: usize, width: usize, mut v: u32) {
    let mut i = start + width;
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 || i == start {
            break;
        }
    }
}

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
