//! Desktop compositor: window manager, app rendering, open/close animations,
//! and a process table surfaced through a Task Manager app.
//!
//! All non-trivial logic (terminal, editor, keymap, window geometry, easing,
//! process table) lives in `osjeff_core` and is unit-tested. This module is
//! hardware-facing glue: pixels, z-order, animation stepping, dispatch.

pub(crate) use crate::fb::{Canvas, Color};
pub(crate) use crate::font;
pub(crate) use crate::icons::{self, Icon};
pub(crate) use crate::logo;
pub(crate) use crate::sched;
pub(crate) use crate::sync::RacyCell;
pub(crate) use crate::theme;
pub(crate) use osjeff_core::clipboard::{self, Clipboard};
pub(crate) use osjeff_core::fs;
pub(crate) use osjeff_core::window::TITLE_H;
pub(crate) use osjeff_core::{
    Action, Anim, Calc, Editor, FileName, Key, Keymap, ProcKind, ProcState, ProcessTable, Rect,
    Terminal, Time,
};

const SLIDE_PX: f32 = 28.0;

// Floating dock geometry.
const DOCK_ICON: i32 = 40;
const DOCK_GAP: i32 = 14;
const DOCK_PAD: i32 = 12;
const DOCK_COUNT: i32 = 6; // brand + terminal + editor + taskmgr + calculator + browser
const DOCK_MARGIN: i32 = 16; // gap from screen bottom

// Calculator keypad: the input byte for each cell (0x08 = backspace). Duplicate
// cells (`0` spanning two columns, `=` spanning two rows) map to the same byte;
// the draw code merges them visually.
const CALC_KEYS: [[u8; 4]; 5] = [*b"C\x08/*", *b"789-", *b"456+", *b"123=", *b"00.="];

// Scratch buffer to snapshot the area behind an animating window (largest
// window + margin). Lets fades composite over real content, not the wallpaper.
const SCRATCH_BYTES: usize = 640 * 440 * 4;
#[repr(C, align(64))]
struct AlignedScratch([u8; SCRATCH_BYTES]);
static SCRATCH: RacyCell<AlignedScratch> = RacyCell::new(AlignedScratch([0; SCRATCH_BYTES]));

// In-memory copy of the filesystem image, loaded from / flushed to the ATA disk
// (sector-aligned so whole 512-byte sectors transfer cleanly). The fs logic only
// touches the first `fs::IMAGE_SIZE` bytes; the tail is sector padding.
const DISK_BYTES: usize = fs::IMAGE_SIZE.div_ceil(512) * 512;
static DISK: RacyCell<[u8; DISK_BYTES]> = RacyCell::new([0; DISK_BYTES]);

fn disk() -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(DISK.get() as *mut u8, DISK_BYTES) }
}

/// Persist the in-memory filesystem image to the ATA disk (best effort: a no-op
/// if there is no disk).
fn flush_disk() {
    let _ = crate::ata::write_image(disk());
}

const TERM: usize = 0;
const EDIT: usize = 1;
const TASK: usize = 2;
const CALC: usize = 3;
const BROWSER: usize = 4;
const WIN_COUNT: usize = 5;

/// Cursor sprite bounding box (used by the dirty-rect overlay path).
pub const CURSOR_W: i32 = 10;
pub const CURSOR_H: i32 = 16;

// Right-click context menu geometry.
const MENU_W: i32 = 220;
const MENU_ITEM_H: i32 = 32;
const MENU_PAD: i32 = 6;
const MENU_ITEMS: [(&str, usize); 5] = [
    ("Terminal", TERM),
    ("Editor", EDIT),
    ("Task Manager", TASK),
    ("Calculator", CALC),
    ("Navegador", BROWSER),
];

// Start panel (system icon → all apps + power).
const START_W: i32 = 240;
const START_ROW_H: i32 = 38;
const START_PAD: i32 = 10;
const START_GAP: i32 = 12; // divider gap before the power rows
const START_APPS: [(&str, usize); 5] = [
    ("Terminal", TERM),
    ("Editor", EDIT),
    ("Task Manager", TASK),
    ("Calculator", CALC),
    ("Navegador", BROWSER),
];

/// An entry in the start panel.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartItem {
    App(usize),
    Reboot,
    Shutdown,
}

/// What a dock icon click triggers.
pub(crate) enum DockAction {
    Start,
    Open(usize),
}

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
    Calculator,
    Browser,
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
    pub(crate) fn active(&self) -> bool {
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
    calc: Calc,
    browser: osjeff_core::Browser,
    // Rendered web page (display list from the `web` engine) + its pixel scroll.
    web_page: Option<osjeff_core::web::Page>,
    page_scroll: i32,
    clipboard: Clipboard,
    keymap: Keymap,
    procs: ProcessTable,
    windows: [Win; WIN_COUNT],
    order: [usize; WIN_COUNT], // back -> front
    drag: Option<Drag>,
    menu: Option<(i32, i32)>,
    start_open: bool,
    editor_file: FileName,
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

        // Load the filesystem from disk. If no disk responds or it holds no
        // valid filesystem (blank / first boot), format and persist a fresh one.
        // A missing disk simply leaves us with a RAM-only filesystem.
        if !crate::ata::read_image(disk()) || !fs::is_formatted(disk()) {
            fs::format(disk());
            let _ = crate::ata::write_image(disk());
        }

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
            Win {
                rect: Rect::new(470, 150, 300, 420),
                visible: false,
                kind: Kind::Calculator,
                title: "CALCULATOR",
                proc_name: b"calc",
                anim: None,
                pid: 0,
            },
            Win {
                rect: Rect::new(150, 60, 916, 560),
                visible: false,
                kind: Kind::Browser,
                title: "NAVEGADOR",
                proc_name: b"browser",
                anim: None,
                pid: 0,
            },
        ];

        Self {
            sw,
            sh,
            term: Terminal::new(),
            editor: Editor::new(),
            calc: Calc::new(),
            browser: osjeff_core::Browser::new(),
            web_page: None,
            page_scroll: 0,
            clipboard: Clipboard::new(),
            keymap: Keymap::new(),
            procs,
            windows,
            order: [BROWSER, TASK, CALC, EDIT, TERM], // terminal focused
            drag: None,
            menu: None,
            start_open: false,
            editor_file: FileName::parse(b"notes.txt").unwrap(),
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

    pub(crate) fn focused(&self) -> Option<usize> {
        for i in (0..WIN_COUNT).rev() {
            let w = self.order[i];
            if self.windows[w].active() {
                return Some(w);
            }
        }
        None
    }

    pub(crate) fn bring_to_front(&mut self, win: usize) {
        let pos = self.order.iter().position(|&w| w == win).unwrap_or(0);
        for i in pos..WIN_COUNT - 1 {
            self.order[i] = self.order[i + 1];
        }
        self.order[WIN_COUNT - 1] = win;
    }

    pub(crate) fn open(&mut self, win: usize) {
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

    pub(crate) fn request_close(&mut self, win: usize) {
        // Ignore if already animating out.
        if matches!(self.windows[win].anim, Some(a) if a.is_closing()) {
            return;
        }
        self.windows[win].anim = Some(Anim::close());
    }

    pub(crate) fn window_of_pid(&self, pid: u16) -> Option<usize> {
        if pid == 0 {
            return None;
        }
        (0..WIN_COUNT).find(|&w| self.windows[w].pid == pid)
    }

    pub(crate) fn topmost_at(&self, px: i32, py: i32) -> Option<usize> {
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

    pub(crate) fn clamp_menu(&self, x: i32, y: i32) -> (i32, i32) {
        let h = MENU_PAD * 2 + MENU_ITEMS.len() as i32 * MENU_ITEM_H;
        let mx = x.min(self.sw - MENU_W).max(0);
        let my = y.min(self.sh - h).max(0);
        (mx, my)
    }

    pub(crate) fn dock_hit(&self, px: i32, py: i32) -> Option<DockAction> {
        let (_, icons) = dock_layout(self.sw, self.sh);
        for (i, r) in icons.iter().enumerate() {
            if r.contains(px, py) {
                return match i {
                    0 => Some(DockAction::Start), // system icon → start panel
                    1 => Some(DockAction::Open(TERM)),
                    2 => Some(DockAction::Open(EDIT)),
                    3 => Some(DockAction::Open(TASK)),
                    4 => Some(DockAction::Open(CALC)),
                    5 => Some(DockAction::Open(BROWSER)),
                    _ => None,
                };
            }
        }
        None
    }

    /// True while a transient overlay (menu / start panel) is shown.
    pub fn overlay_open(&self) -> bool {
        self.menu.is_some() || self.start_open
    }

    /// Screen rect of the bottom-right clock pill, including its drop shadow, so
    /// a per-second tick can repaint just this region instead of the whole
    /// framebuffer. Mirrors the geometry in [`draw_clock`].
    pub fn clock_rect(&self) -> Rect {
        let tw = font::text_width("00:00:00", 2) as i32;
        let pad = 14;
        let pw = tw + pad * 2;
        let ph = 34;
        let px = self.sw - pw - DOCK_MARGIN;
        let py = self.sh - ph - DOCK_MARGIN;
        // +6 (and a little slack) covers the shadow draw_clock offsets below.
        Rect::new(px, py, pw, ph + 8)
    }

    /// Screen rect of the Task Manager window when it is visible — its CPU
    /// figures refresh every second, so the cheap clock-tick path must repaint
    /// it too. `None` when the window is hidden.
    pub fn task_window_rect(&self) -> Option<Rect> {
        if self.windows[TASK].visible {
            Some(self.window_box(TASK))
        } else {
            None
        }
    }

    /// Bounding rect of the open overlay(s), inflated for their drop shadows and
    /// clamped to the screen. Empty when nothing is open. Drives the overlay
    /// damage repaint.
    pub fn overlay_bounds(&self) -> Rect {
        let mut bounds: Option<Rect> = None;
        if let Some((mx, my)) = self.menu {
            let h = MENU_PAD * 2 + MENU_ITEMS.len() as i32 * MENU_ITEM_H;
            bounds = Some(Rect::new(mx, my, MENU_W, h));
        }
        if self.start_open {
            let (sx, sy) = start_origin(self.sw, self.sh);
            let sr = Rect::new(sx, sy, START_W, start_height());
            bounds = Some(bounds.map_or(sr, |b| b.union(&sr)));
        }
        match bounds {
            Some(b) => b.inflated(12).clamped_to(self.sw, self.sh),
            None => Rect::new(0, 0, 0, 0),
        }
    }

    /// On-screen rect of window `w`, including its current animation slide.
    pub(crate) fn window_box(&self, w: usize) -> Rect {
        let mut r = self.windows[w].rect;
        if let Some(a) = self.windows[w].anim {
            r.y += a.slide(SLIDE_PX) as i32;
        }
        r
    }

    /// On-screen rect of the focused window, or `None` if none is focused. Lets
    /// the steady-state loop repaint only this window on a content change (a
    /// keystroke, a calc button) instead of blitting the whole framebuffer.
    pub fn focused_box(&self) -> Option<Rect> {
        self.focused().map(|w| self.window_box(w))
    }

    /// A "dynamic" window is one the compositor must redraw every frame and
    /// keep OUT of the cached static layer: one that is opening/closing, or the
    /// one currently being dragged. Treating a drag like an animation lets the
    /// existing damage-tracking fast-path move it by repainting only its old+new
    /// rectangle each frame, instead of recomposing the whole desktop + an 8 MiB
    /// blit on every mouse step.
    pub(crate) fn is_dynamic(&self, w: usize) -> bool {
        self.windows[w].anim.is_some() || self.drag.as_ref().is_some_and(|d| d.win == w)
    }

    /// True while any window is opening, closing, or being dragged — i.e. the
    /// compositor should run its per-frame damage path rather than the steady one.
    pub fn has_animation(&self) -> bool {
        self.drag.is_some()
            || (0..WIN_COUNT).any(|w| self.windows[w].visible && self.windows[w].anim.is_some())
    }

    // ---- browser networking hand-off (driven by the kernel main loop) ----

    /// If the browser app has a pending navigation, copy the target URL into
    /// `out` and return its length (clearing the pending flag). The kernel then
    /// performs the blocking fetch and reports back with [`browser_load`] /
    /// [`browser_fail`].
    pub fn browser_take_request(&mut self, out: &mut [u8]) -> Option<usize> {
        self.browser.take_request().map(|url| {
            let n = url.len().min(out.len());
            out[..n].copy_from_slice(&url[..n]);
            n
        })
    }

    /// Render a fetched raw HTTP response with the `web` engine and keep the
    /// resulting display list for painting/scrolling.
    pub fn browser_load(&mut self, resp: &[u8]) {
        let content_w = BrowserChrome::of(self.windows[BROWSER].rect).content.w;
        let body = osjeff_core::browser::page_body(resp);
        self.web_page = Some(osjeff_core::web::render(&body, content_w));
        self.page_scroll = 0;
        self.browser.loaded();
    }

    /// Mark the in-flight browser fetch as failed.
    pub fn browser_fail(&mut self) {
        self.web_page = None;
        self.browser.fail();
    }

    /// Scroll the rendered page by `dy` pixels, clamped to its content height.
    pub(crate) fn scroll_page(&mut self, dy: i32) {
        let view_h = BrowserChrome::of(self.windows[BROWSER].rect).content.h;
        let max = self
            .web_page
            .as_ref()
            .map(|p| (p.height - view_h).max(0))
            .unwrap_or(0);
        self.page_scroll = (self.page_scroll + dy).clamp(0, max);
    }
}

mod apps;
mod files;
mod input;
mod render;
mod widgets;
pub(crate) use widgets::*;

