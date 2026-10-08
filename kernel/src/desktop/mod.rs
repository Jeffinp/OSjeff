//! Desktop compositor: window manager, app rendering, open/close animations,
//! and a process table surfaced through a Task Manager app.
//!
//! All non-trivial logic (terminal, editor, keymap, window geometry, easing,
//! process table, the dynamic window table) lives in `osjeff_core` and is
//! unit-tested. This module is hardware-facing glue: pixels, animation
//! stepping, dispatch, and one [`instance::App`] state per window.

pub(crate) use crate::fb::{Canvas, Color, Corner, Shadow};
pub(crate) use crate::font;
pub(crate) use crate::icons::{self, Icon};
pub(crate) use crate::logo;
pub(crate) use crate::sched;
pub(crate) use crate::sync::RacyCell;
pub(crate) use crate::theme;
pub(crate) use alloc::string::String;
pub(crate) use alloc::vec::Vec;
pub(crate) use osjeff_core::clipboard::{self, Clipboard};
pub(crate) use osjeff_core::window::{ResizeEdge, TITLE_H, WindowId};
pub(crate) use osjeff_core::winman::{ClickTracker, Switcher, WindowManager, WindowSpec};
pub(crate) use osjeff_core::{Calc, Key, Keymap, ProcKind, ProcState, ProcessTable, Rect, Time};

// Dock / menu / start-panel / keypad geometry lives in `osjeff_core::layout`.
use osjeff_core::layout::{
    CALC_KEYS, DOCK_MARGIN, MENU_ITEM_H, MENU_PAD, MENU_W, START_GAP, START_PAD, START_ROW_H,
    START_W,
};

/// Longest gap between the two presses of a double click, in timer ticks
/// (250 Hz): 500 ms.
const DOUBLE_CLICK_TICKS: u64 = 125;

// Offscreen texture of a window while it opens / closes / minimises / zooms: the
// window is drawn once at its resting size, then resampled into the animated
// rectangle every frame (see `draw_animating`). Windows larger than this animate
// without the scale effect.
pub(crate) const TEXTURE_BYTES: usize = 1280 * 720 * 4;
#[repr(C, align(64))]
struct AlignedTexture([u8; TEXTURE_BYTES]);
static TEXTURE: RacyCell<AlignedTexture> = RacyCell::new(AlignedTexture([0; TEXTURE_BYTES]));

/// The rectangle a window and its shadow cover (the damage of a moving window):
/// the ambient layer reaches 20 px sideways, 20 above (shifted down 14) and 34 below.
pub(crate) fn shadow_box(r: Rect) -> Rect {
    Rect::new(r.x - 22, r.y - 8, r.w + 44, r.h + 44)
}

/// Cursor sprite bounding box: the area `osjeff_core::cursor::CursorTrack` restores from the back
/// buffer before every frame. Every sprite (`widgets::CURSOR`, `widgets::HAND`) must fit in it;
/// the assertion below makes a larger sprite a build error instead of a trail of ghosts.
pub const CURSOR_W: i32 = 10;
pub const CURSOR_H: i32 = 16;

const fn sprite_fits(rows: &[&str]) -> bool {
    if rows.len() > CURSOR_H as usize {
        return false;
    }
    let mut i = 0;
    while i < rows.len() {
        if rows[i].len() > CURSOR_W as usize {
            return false;
        }
        i += 1;
    }
    true
}
const _: () = assert!(sprite_fits(&widgets::CURSOR) && sprite_fits(&widgets::HAND));

/// Most app rows the start panel shows at once (more scroll).
pub(crate) const START_MAX_ROWS: usize = 11;

/// An entry in the start panel.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartItem {
    App(Kind),
    /// An installed WASM app (index into the catalog).
    Wasm(usize),
    Reboot,
    Shutdown,
}

/// What a dock icon click triggers.
pub(crate) enum DockAction {
    Start,
    Open(Kind),
}

/// What changed after a mouse packet, so the caller can pick the cheap
/// cursor-only repaint vs a full scene recompose.
pub struct MouseResult {
    pub scene_dirty: bool,
    pub cursor_moved: bool,
}

/// Which context menu is open.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuKind {
    /// Right click on the desktop: every app.
    Desktop,
    /// Right click on a dock icon: that app's window actions.
    Dock(Kind),
}

/// What choosing a context-menu entry does.
#[derive(Clone, Copy)]
pub(crate) enum MenuAction {
    /// Focus the app's window, opening one if there is none.
    Launch(Kind),
    /// Open one more window of the app.
    NewWindow(Kind),
}

impl MenuKind {
    pub(crate) fn len(self) -> usize {
        match self {
            MenuKind::Desktop => Kind::ALL.len(),
            MenuKind::Dock(_) => 1,
        }
    }

    /// Label, icon and action of entry `i`.
    pub(crate) fn entry(self, i: usize) -> Option<(&'static str, Kind, MenuAction)> {
        match self {
            MenuKind::Desktop => Kind::ALL
                .get(i)
                .map(|&k| (k.label(), k, MenuAction::Launch(k))),
            MenuKind::Dock(k) if i == 0 => Some(if k.multi() {
                ("Nova janela", k, MenuAction::NewWindow(k))
            } else {
                ("Abrir", k, MenuAction::Launch(k))
            }),
            MenuKind::Dock(_) => None,
        }
    }
}

/// An open context menu: top-left corner and kind.
#[derive(Clone, Copy)]
pub(crate) struct MenuState {
    pub x: i32,
    pub y: i32,
    pub kind: MenuKind,
}

/// A pointer drag in progress.
#[derive(Clone, Copy)]
pub(crate) enum DragMode {
    /// Moving the window: offset of the grab point inside it.
    Move { grab_dx: i32, grab_dy: i32 },
    /// Dragging the image of a viewer window: last pointer position.
    Pan { last_x: i32, last_y: i32 },
    /// Extending a text selection in an editor window.
    Select,
    /// Resizing from `edge`; `start` is the window rect and `(ox, oy)` the
    /// pointer position when the drag began.
    Resize {
        edge: ResizeEdge,
        start: Rect,
        ox: i32,
        oy: i32,
    },
    /// Selecting text on a browser page (the anchor lives in the window's browser state).
    PageSelect,
}

pub(crate) struct Drag {
    pub win: WindowId,
    pub mode: DragMode,
}

pub struct Desktop {
    sw: i32,
    // One-time snapshot of the two IDE disks (boot + filesystem) for the Settings disk panel.
    disks: [Option<crate::ata::DiskInfo>; 2],
    sh: i32,
    clipboard: Clipboard,
    /// Paths set aside by the file managers' Copy / Cut (shared by all windows).
    pathclip: osjeff_core::fileman::PathClip,
    keymap: Keymap,
    procs: ProcessTable,
    /// Sampled system history for the resource monitor.
    sysmon: SysMon,
    /// The wallpaper or accent changed: the compositor must repaint the cached background.
    bg_dirty: bool,
    /// The toast overlay (see `toasts_ui`).
    toasts: osjeff_core::notify::Toasts,
    /// Log sequence number up to which WARN+ records already became toasts.
    toast_seen: u32,
    /// A toast appeared or was dismissed since the compositor last repainted them.
    toast_dirty: bool,
    /// Network identity the boot obtained (`nic present`, config), for the settings page.
    /// The dynamic window table; every window owns an app instance.
    wm: WindowManager<Inst>,
    drag: Option<Drag>,
    menu: Option<MenuState>,
    start_open: bool,
    /// The Alt+Tab switcher while Alt is held.
    switcher: Option<Switcher>,
    /// Window under the cursor (its title-bar buttons are shown).
    hover: Option<WindowId>,
    /// Which window (and size) the offscreen texture currently holds.
    tex_key: core::cell::Cell<Option<(WindowId, i32, i32)>>,
    clicks: ClickTracker,
    /// Set by operations that change pixels outside the focused window (maximize,
    /// restore, ...); makes the next steady frame upload the whole screen.
    force_full: bool,
    /// Region to upload on the next steady frame besides the focused window.
    extra_dirty: Rect,
    cursor_x: i32,
    cursor_y: i32,
    prev_left: bool,
    prev_right: bool,
    /// Installed WASM apps (the start panel lists them after the system apps).
    apps: Vec<AppEntry>,
    /// First visible app row of the (scrollable) start panel.
    start_scroll: usize,
    /// The WASM window that owns the pressed mouse button.
    wasm_grab: Option<WindowId>,
    /// Last seen generation of app-side clipboard writes.
    clip_gen: u64,
    /// `vfs::generation()` the file managers last loaded, and the tick of that check.
    fs_gen: u32,
    fs_gen_tick: u64,
}

impl Desktop {
    pub fn new(sw: i32, sh: i32) -> Self {
        // At boot only the system processes and the open terminal exist.
        // Apps spawn a fresh process when opened and are removed when closed.
        let mut procs = ProcessTable::new();
        procs.spawn(b"kernel", ProcKind::System, ProcState::Running);
        procs.spawn(b"compositor", ProcKind::System, ProcState::Running);

        let mut desk = Self {
            disks: [
                crate::ata::identify(0x1F0, 0x3F6, false),
                crate::ata::identify(0x170, 0x376, false),
            ],
            sw,
            sh,
            clipboard: Clipboard::new(),
            pathclip: osjeff_core::fileman::PathClip::new(),
            keymap: Keymap::new(),
            procs,
            sysmon: SysMon::new(),
            bg_dirty: false,
            toasts: osjeff_core::notify::Toasts::new(),
            toast_seen: crate::klog::seq(),
            toast_dirty: false,
            wm: WindowManager::new(osjeff_core::winman::DEFAULT_MAX_WINDOWS),
            drag: None,
            menu: None,
            start_open: false,
            switcher: None,
            hover: None,
            tex_key: core::cell::Cell::new(None),
            clicks: ClickTracker::new(DOUBLE_CLICK_TICKS),
            force_full: false,
            extra_dirty: Rect::new(0, 0, 0, 0),
            cursor_x: sw / 2,
            cursor_y: sh / 2,
            prev_left: false,
            prev_right: false,
            apps: Vec::new(),
            start_scroll: 0,
            wasm_grab: None,
            clip_gen: crate::wasm::clip_generation(),
            fs_gen: vfs::generation(),
            fs_gen_tick: 0,
        };
        // Install the bundled apps into /apps (first boot) and build the launcher catalog.
        desk.init_apps();
        // The terminal is open (and focused) at boot.
        desk.open_new(Kind::Terminal);
        desk
    }

    /// Everything that decides the cursor's pixels: where it is and which sprite (arrow or
    /// hand). The compositor repaints the sprite whenever this differs from what it painted.
    pub fn pointer(&self) -> osjeff_core::cursor::Pointer {
        osjeff_core::cursor::Pointer {
            x: self.cursor_x,
            y: self.cursor_y,
            shape: u8::from(self.cursor_is_hand()),
        }
    }

    // ---- window lifecycle ----

    /// Lowest unused 1-based instance number of `kind` (so closing `shell 2` and
    /// opening a terminal again names it `shell 2` once more).
    fn free_index(&self, kind: Kind) -> u8 {
        let mut idx = 1u8;
        while self
            .wm
            .windows()
            .iter()
            .any(|w| w.app.kind() == kind && w.app.index == idx)
        {
            idx += 1;
        }
        idx
    }

    /// Opens a new window of `kind` with a new process. `None` when the window
    /// table or the process table is full (nothing is created then).
    pub(crate) fn open_new(&mut self, kind: Kind) -> Option<WindowId> {
        if kind == Kind::WasmApp {
            return self.open_default_wasm();
        }
        if self.wm.is_full() {
            return None;
        }
        let index = self.free_index(kind);
        let mut name = [0u8; 16];
        let n = numbered_name(kind.proc_name(), index, &mut name);
        let pid = self
            .procs
            .spawn(&name[..n], ProcKind::App, ProcState::Running)?;
        let rect = if index == 1 {
            kind.default_rect()
        } else {
            osjeff_core::winman::cascade_rect(
                kind.default_rect(),
                index as usize - 1,
                self.work_area(),
            )
        };
        let (min_w, min_h) = kind.min_size();
        let title = base_title(kind, index);
        let inst = Inst {
            app: App::new(kind),
            pid,
            index,
            title,
            cost: core::cell::Cell::new(0),
            cost_pm: core::cell::Cell::new(0),
        };
        let spec = WindowSpec {
            rect,
            min_w,
            min_h,
            resizable: kind.resizable(),
        };
        match self.wm.open(spec, inst) {
            Ok(id) => {
                if kind == Kind::Files {
                    self.files_refresh(id);
                    if let Some(f) = self.files_mut(id) {
                        f.view.select_first();
                    }
                }
                if kind == Kind::Editor {
                    self.sync_editor(id);
                    self.refresh_editor_title(id);
                }
                Some(id)
            }
            Err(inst) => {
                self.procs.kill(inst.pid);
                None
            }
        }
    }

    /// Dock-click semantics: focus (restoring if minimized) the most recently
    /// used window of `kind`, or open one when there is none.
    pub(crate) fn launch(&mut self, kind: Kind) -> Option<WindowId> {
        if kind == Kind::WasmApp {
            return self.launch_default_wasm();
        }
        if let Some(id) = self.mru_of_kind(kind) {
            self.wm.activate(id);
            return Some(id);
        }
        self.open_new(kind)
    }

    /// Ctrl+N / "Nova janela": one more window of `kind`; single-instance apps
    /// just focus their window.
    pub(crate) fn new_window(&mut self, kind: Kind) -> Option<WindowId> {
        if kind.multi() {
            self.open_new(kind)
        } else {
            self.launch(kind)
        }
    }

    /// The most recently used live window of `kind`.
    pub(crate) fn mru_of_kind(&self, kind: Kind) -> Option<WindowId> {
        self.wm
            .switch_list()
            .into_iter()
            .find(|&id| self.kind_of(id) == Some(kind))
    }

    pub(crate) fn kind_of(&self, id: WindowId) -> Option<Kind> {
        self.wm.get(id).map(|w| w.app.kind())
    }

    /// The rectangle windows maximize into.
    pub(crate) fn work_area(&self) -> Rect {
        osjeff_core::layout::work_area(self.sw, self.sh)
    }

    pub(crate) fn app_mut(&mut self, id: WindowId) -> Option<&mut App> {
        self.wm.get_mut(id).map(|w| &mut w.app.app)
    }

    pub(crate) fn calc_mut(&mut self, id: WindowId) -> Option<&mut Calc> {
        match self.app_mut(id) {
            Some(App::Calculator(c)) => Some(c),
            _ => None,
        }
    }

    pub(crate) fn browser_state_mut(&mut self, id: WindowId) -> Option<&mut BrowserState> {
        match self.app_mut(id) {
            Some(App::Browser(b)) => Some(b),
            _ => None,
        }
    }

    pub(crate) fn files_mut(&mut self, id: WindowId) -> Option<&mut FilesState> {
        match self.app_mut(id) {
            Some(App::Files(f)) => Some(f),
            _ => None,
        }
    }

    pub(crate) fn viewer_mut(&mut self, id: WindowId) -> Option<&mut ViewerState> {
        match self.app_mut(id) {
            Some(App::Viewer(v)) => Some(v),
            _ => None,
        }
    }

    /// The (single) browser window, if open.
    fn browser_id(&self) -> Option<WindowId> {
        self.wm
            .windows()
            .iter()
            .find(|w| w.app.kind() == Kind::Browser && !w.is_closing())
            .map(|w| w.id)
    }

    pub(crate) fn request_close(&mut self, id: WindowId) {
        // An editor with unsaved changes asks before it goes.
        if self.editor_holds_close(id) {
            return;
        }
        // A WASM app gets `on_close` (a chance to save) while the window animates away.
        if let Some(h) = self.wasm_handle(id) {
            crate::wasm::request_close(h);
        }
        self.wm.request_close(id);
    }

    /// The window owning process `pid`.
    pub(crate) fn window_of_pid(&self, pid: u16) -> Option<WindowId> {
        if pid == 0 {
            return None;
        }
        self.wm
            .windows()
            .iter()
            .find(|w| w.app.pid == pid)
            .map(|w| w.id)
    }

    pub(crate) fn focused(&self) -> Option<WindowId> {
        self.wm.focused()
    }

    pub(crate) fn topmost_at(&self, px: i32, py: i32) -> Option<WindowId> {
        self.wm.topmost_at(px, py)
    }

    // ---- animation & scheduler ----

    /// Advance all running animations by `dt`. Returns `true` while any window
    /// is still animating (the caller keeps rendering).
    pub fn animate(&mut self, dt: f32) -> bool {
        self.step_file_jobs();
        self.step_shell_jobs();
        self.sync_text_windows();
        let (active, gone) = self.wm.step(dt);
        if !active {
            // Nothing animates any more: the next animation re-captures its window.
            self.tex_key.set(None);
        }
        for mut w in gone {
            if let App::Files(f) = &mut w.app.app
                && let Some(mut job) = f.job.take()
            {
                vfs::copy_abort(&mut job.copy);
            }
            // Closing an app terminates its process (removed from the table),
            // matching how a desktop app behaves; dropping `w` frees its state.
            self.procs.kill(w.app.pid);
            // The window is gone: the app's runtime (memory, descriptors) goes with it.
            if let App::Wasm(ww) = &w.app.app {
                crate::wasm::close(ww.id);
                if self.wasm_grab == Some(w.id) {
                    self.wasm_grab = None;
                }
            }
            if self.hover == Some(w.id) {
                self.hover = None;
            }
            if self.drag.as_ref().is_some_and(|d| d.win == w.id) {
                self.drag = None;
            }
            if w.minimized {
                // A hidden window vanished: its dock indicator must go.
                self.force_full = true;
            }
        }
        active
    }

    /// One scheduler quantum: advance CPU-time of running processes.
    pub fn tick_processes(&mut self) {
        self.procs.tick();
        self.refresh_logs();
    }

    pub(crate) fn clamp_menu(&self, x: i32, y: i32, items: usize) -> (i32, i32) {
        osjeff_core::layout::clamp_menu(self.sw, self.sh, x, y, items)
    }

    pub(crate) fn dock_hit(&self, px: i32, py: i32) -> Option<DockAction> {
        match osjeff_core::layout::dock_slot_at(self.sw, self.sh, px, py)? {
            0 => Some(DockAction::Start), // system icon -> start panel
            n => Kind::ALL.get(n - 1).map(|&k| DockAction::Open(k)),
        }
    }

    /// True while a transient overlay (menu / start panel / Alt+Tab) is shown.
    pub fn overlay_open(&self) -> bool {
        self.menu.is_some() || self.start_open || self.switcher.is_some()
    }

    /// Screen rect of the bottom-right clock pill, including its drop shadow, so
    /// a per-second tick can repaint just this region instead of the whole
    /// framebuffer. Mirrors the geometry in [`draw_clock`].
    pub fn clock_rect(&self) -> Rect {
        let tw =
            (osjeff_core::hw::rtc::clock_len(crate::settings::clock24()) * font::cell_w(2)) as i32;
        let pad = 14;
        let pw = tw + pad * 2;
        let ph = 34;
        let px = self.sw - pw - DOCK_MARGIN;
        let py = self.sh - ph - DOCK_MARGIN;
        // +6 (and a little slack) covers the shadow draw_clock offsets below.
        Rect::new(px, py, pw, ph + 8)
    }

    /// True when the per-second clock tick can be repainted locally: nothing else
    /// that changes each second is on screen (the Task Manager redraws its CPU
    /// figures), and no visible window — including the drop shadow it casts
    /// (up to 12 px to the sides, 26 px below) — reaches the clock pill, so the
    /// pixels under the pill are exactly the wallpaper. The caller must also
    /// know `back` holds the last fully composed scene (steady frame, no
    /// animation or overlay).
    pub fn clock_repaint_is_local(&self) -> bool {
        /// Generous bound on how far a window's shadow extends past its rect.
        const SHADOW_REACH: i32 = 32;
        if self.task_window_rect().is_some() {
            return false;
        }
        let pill = self.clock_rect();
        !self.wm.windows().iter().any(|w| {
            w.shown()
                && self
                    .window_box(w)
                    .inflated(SHADOW_REACH)
                    .intersection(&pill)
                    .is_some()
        })
    }

    /// Redo only the clock pill in `back`: restore the wallpaper under it, then
    /// draw the pill. Valid when [`clock_repaint_is_local`] holds.
    pub fn repaint_clock(
        &self,
        back: &mut [u8],
        bg: &[u8],
        info: bootloader_api::info::FrameBufferInfo,
        time: Time,
    ) {
        copy_region(back, bg, info, self.clock_rect());
        let mut c = Canvas::new(back, info);
        draw_clock(&mut c, time);
    }

    /// Screen rect covering every visible window whose content changes by
    /// itself each second (the Task Manager's CPU figures, the log viewer's new
    /// lines): the cheap clock-tick path must repaint them too. `None` when no
    /// such window is shown.
    pub fn task_window_rect(&self) -> Option<Rect> {
        self.wm
            .windows()
            .iter()
            .filter(|w| w.app.kind().is_live() && w.shown())
            .map(|w| self.window_box(w))
            .reduce(|a, b| a.union(&b))
    }

    /// Bounding rect of the open overlay(s), inflated for their drop shadows and
    /// clamped to the screen. Empty when nothing is open. Drives the overlay
    /// damage repaint.
    pub fn overlay_bounds(&self) -> Rect {
        let mut bounds: Option<Rect> = None;
        if let Some(m) = self.menu {
            let h = osjeff_core::layout::menu_height(m.kind.len());
            bounds = Some(Rect::new(m.x, m.y, MENU_W, h));
        }
        if self.start_open {
            let rows = self.start_rows();
            let (sx, sy) = start_origin(self.sw, self.sh, rows);
            let sr = Rect::new(sx, sy, START_W, start_height(rows));
            bounds = Some(bounds.map_or(sr, |b| b.union(&sr)));
        }
        if let Some(sw) = &self.switcher {
            let r = self.switcher_rect(sw.list().len());
            bounds = Some(bounds.map_or(r, |b| b.union(&r)));
        }
        match bounds {
            Some(b) => b.inflated(12).clamped_to(self.sw, self.sh),
            None => Rect::new(0, 0, 0, 0),
        }
    }

    /// On-screen rect of window `w` right now: its resting rectangle, the in-flight
    /// rectangle of a zoom, or the scaled/moved one of an open / close / minimise.
    pub(crate) fn window_box(&self, w: &Win) -> Rect {
        if let Some(a) = w.anim {
            return a.frame(w.rect, self.dock_target(w)).rect;
        }
        w.visual_rect()
    }

    /// Screen rectangle of the dock icon a window flies to / from when it is
    /// minimised or restored; `None` for apps without a dock icon.
    pub(crate) fn dock_target(&self, w: &Win) -> Option<Rect> {
        let kind = w.app.kind();
        if !kind.in_dock() {
            return None;
        }
        let (_, icons) = dock_layout(self.sw, self.sh);
        icons.get(kind.index() + 1).copied()
    }

    /// On-screen rect of the focused window, or `None` if none is focused. Lets
    /// the steady-state loop repaint only this window on a content change (a
    /// keystroke, a calc button) instead of blitting the whole framebuffer.
    pub fn focused_box(&self) -> Option<Rect> {
        let id = self.focused()?;
        self.wm.get(id).map(|w| self.window_box(w))
    }

    /// A "dynamic" window is one the compositor must redraw every frame and
    /// keep OUT of the cached static layer: one that is opening/closing, or the
    /// one currently being dragged or resized. Treating a drag like an
    /// animation lets the existing damage-tracking fast-path move it by
    /// repainting only its old+new rectangle each frame, instead of recomposing
    /// the whole desktop + an 8 MiB blit on every mouse step.
    pub(crate) fn is_dynamic(&self, w: &Win) -> bool {
        w.anim.is_some()
            || w.zoom.is_some()
            || self.drag.as_ref().is_some_and(|d| d.win == w.id)
            // A visible WASM app renders a fresh frame every tick (it may animate
            // on its own clock), so it is kept out of the cached static layer and
            // repainted through the per-frame damage path like an animation.
            || (w.shown() && w.app.kind() == Kind::WasmApp)
            || (w.shown() && matches!(&w.app.app, App::Files(f) if f.job.is_some()))
            || (w.shown() && matches!(&w.app.app, App::Terminal(t) if t.term.is_running()))
    }

    /// True while any window is opening, closing, being dragged, or is a live
    /// WASM app — i.e. the compositor should run its per-frame damage path so the
    /// app gets continuous frames, rather than the steady (repaint-on-change) one.
    pub fn has_animation(&self) -> bool {
        self.drag.is_some()
            || self.wm.windows().iter().any(|w| {
                w.shown()
                    && (w.anim.is_some()
                        || w.zoom.is_some()
                        || w.app.kind() == Kind::WasmApp
                        || matches!(&w.app.app, App::Files(f) if f.job.is_some())
                        || matches!(&w.app.app, App::Terminal(t) if t.term.is_running()))
            })
    }

    /// Consume the "repaint everything" request (maximize, restore, ...).
    pub fn take_full_repaint(&mut self) -> bool {
        core::mem::take(&mut self.force_full)
    }

    /// Consume the extra region the next steady frame must upload.
    pub fn take_extra_dirty(&mut self) -> Option<Rect> {
        let r = core::mem::replace(&mut self.extra_dirty, Rect::new(0, 0, 0, 0));
        (!r.is_empty()).then_some(r)
    }

    /// Add `r` to the extra upload region.
    pub(crate) fn mark_dirty(&mut self, r: Rect) {
        self.extra_dirty = self.extra_dirty.union(&r);
    }

    // ---- browser networking hand-off (driven by the kernel main loop) ----

    /// If the browser app has a pending navigation, copy the target URL into
    /// `out` and return its length (clearing the pending flag). The kernel then
    /// performs the blocking fetch and reports back with [`browser_load`] /
    /// [`browser_fail`].
    pub fn browser_take_request(&mut self, out: &mut [u8]) -> Option<usize> {
        let id = self.browser_id()?;
        self.browser_state_mut(id)?
            .browser
            .take_request()
            .map(|url| {
                let n = url.len().min(out.len());
                out[..n].copy_from_slice(&url[..n]);
                n
            })
    }

    /// The host the user allowed past a certificate error for the navigation just
    /// taken by [`browser_take_request`] (empty when none): copied into `out`.
    pub fn browser_insecure_host(&mut self, out: &mut [u8]) -> usize {
        let Some(id) = self.browser_id() else {
            return 0;
        };
        let Some(b) = self.browser_state_mut(id) else {
            return 0;
        };
        match b.browser.insecure_host() {
            Some(h) => {
                let n = h.len().min(out.len());
                out[..n].copy_from_slice(&h[..n]);
                n
            }
            None => 0,
        }
    }

    /// Render a fetched raw HTTP response with the `web` engine and keep the
    /// resulting display list for painting/scrolling. `conn` says how the final
    /// connection was authenticated and `truncated` that the response hit the
    /// size cap; both feed the address-bar badge and the truncation notice.
    /// Dropped when the browser window was closed meanwhile.
    pub fn browser_load(&mut self, resp: &[u8], conn: osjeff_core::browser::Conn, truncated: bool) {
        let Some(id) = self.browser_id() else {
            return;
        };
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let content_w = BrowserChrome::of(rect).content.w;
        // A cut or damaged compressed body still renders what was decoded; the note says why
        // the page is partial (shown as a banner).
        let page = osjeff_core::browser::page_body_partial(resp, truncated);
        let body = page.body;
        if let Some(b) = self.browser_state_mut(id) {
            b.doc = Some(osjeff_core::web::Doc::parse(&body));
            b.images.begin_page();
            b.img_inflight = None;
            b.scroll = 0;
            b.browser.loaded_with_note(conn, page.note);
            b.layout_w = content_w;
            layout_browser(b, content_w, true);
        }
        self.browser_page_changed(id);
    }

    /// Render the HTML of a page the browser generated itself (`osjeff://...`), if one was just
    /// opened. Called every frame by the main loop, right next to the network hand-off.
    pub fn browser_poll_internal(&mut self) -> bool {
        let Some(id) = self.browser_id() else {
            return false;
        };
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return false;
        };
        let content_w = BrowserChrome::of(rect).content.w;
        let Some(b) = self.browser_state_mut(id) else {
            return false;
        };
        let Some(html) = b.browser.take_internal() else {
            return false;
        };
        b.doc = Some(osjeff_core::web::Doc::parse(&html));
        b.images.begin_page();
        b.img_inflight = None;
        b.scroll = 0;
        b.layout_w = content_w;
        layout_browser(b, content_w, true);
        self.browser_page_changed(id);
        true
    }

    /// A new page is on screen: window title from its `<title>`, selection and find reset.
    fn browser_page_changed(&mut self, id: WindowId) {
        let mut title = String::from("NAVEGADOR");
        if let Some(b) = self.browser_state_mut(id) {
            let t = b
                .doc
                .as_ref()
                .map(|d| String::from(d.title()))
                .unwrap_or_default();
            b.browser.set_page_title(&t);
            b.sel = None;
            b.sel_anchor = None;
            b.notice = None;
            b.find.refresh(b.page.as_ref());
            if !t.is_empty() {
                title.push_str(" - ");
                title.extend(t.chars().take(48));
            }
        }
        if let Some(w) = self.wm.get_mut(id) {
            w.app.title = title;
        }
    }

    /// Mark the in-flight browser fetch as failed.
    pub fn browser_fail(&mut self, reason: osjeff_core::browser::FailReason) {
        if let Some(id) = self.browser_id()
            && let Some(b) = self.browser_state_mut(id)
        {
            b.page = None;
            b.doc = None;
            b.sel = None;
            b.browser.fail_with(reason);
        }
        if let Some(id) = self.browser_id()
            && let Some(w) = self.wm.get_mut(id)
        {
            w.app.title = String::from("NAVEGADOR");
        }
    }

    /// Scroll the rendered page of browser window `id` by `dy` pixels, clamped
    /// to its content height.
    pub(crate) fn scroll_page(&mut self, id: WindowId, dy: i32) -> bool {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return false;
        };
        let view_h = BrowserChrome::of(rect).content.h;
        let Some(b) = self.browser_state_mut(id) else {
            return false;
        };
        let max = b
            .page
            .as_ref()
            .map(|p| (p.height - view_h).max(0))
            .unwrap_or(0);
        let before = b.scroll;
        b.scroll = (b.scroll + dy).clamp(0, max);
        b.scroll != before
    }

    /// Wheel over browser window `id`: three text lines per notch.
    pub(crate) fn browser_wheel(&mut self, id: WindowId, notches: i32) -> bool {
        self.scroll_page(id, notches * 54)
    }

    /// Lay the browser page out again for the window's current width (after a
    /// resize or maximize). Cheap no-op when the width did not change.
    pub(crate) fn relayout_browser(&mut self, id: WindowId) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let content = BrowserChrome::of(rect).content;
        if let Some(b) = self.browser_state_mut(id)
            && b.page.is_some()
            && b.layout_w != content.w
        {
            b.layout_w = content.w;
            layout_browser(b, content.w, false);
            let max = b.page.as_ref().map_or(0, |p| (p.height - content.h).max(0));
            b.scroll = b.scroll.clamp(0, max);
        }
    }

    /// The next picture of the page that has to be downloaded, if the fetcher is free: copies its
    /// URL into `out` and returns the length and the column width to scale it to. Inline `data:`
    /// pictures are decoded right here (they are small) and never reach the fetcher.
    pub fn browser_next_image(&mut self, out: &mut [u8]) -> Option<(usize, usize)> {
        let id = self.browser_id()?;
        let rect = self.wm.get(id).map(|w| w.rect)?;
        let content = BrowserChrome::of(rect).content;
        let b = self.browser_state_mut(id)?;
        if b.img_inflight.is_some() {
            return None;
        }
        let fit_w = (content.w - 24).max(16) as usize;
        let mut inline_done = false;
        let mut result = None;
        while let Some((key, data)) = b.images.next_pending() {
            if let Some(uri) = data {
                let r = osjeff_core::web::imgcache::decode_data_uri(&uri, fit_w);
                b.images.finish(&key, r);
                inline_done = true;
                continue;
            }
            let n = key.len().min(out.len());
            out[..n].copy_from_slice(&key.as_bytes()[..n]);
            b.img_inflight = Some(key);
            result = Some((n, fit_w));
            break;
        }
        if inline_done {
            layout_browser(b, content.w, false);
        }
        result
    }

    /// A picture request finished: store it and lay the page out again (images change sizes).
    pub fn browser_image_done(
        &mut self,
        res: Result<osjeff_core::web::imgcache::Loaded, osjeff_core::web::imgcache::ImgFail>,
    ) {
        let Some(id) = self.browser_id() else {
            return;
        };
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let content = BrowserChrome::of(rect).content;
        if let Some(b) = self.browser_state_mut(id)
            && let Some(key) = b.img_inflight.take()
        {
            b.images.finish(&key, res);
            layout_browser(b, content.w, false);
            let max = b.page.as_ref().map_or(0, |p| (p.height - content.h).max(0));
            b.scroll = b.scroll.clamp(0, max);
        }
    }
}

/// Lay the document of `b` out for `width` pixels with the current zoom and whatever the image
/// cache knows. `register` (a new page) first lays out with the images unknown to learn which
/// pictures the page has, asks the cache for them, then lays out again.
fn layout_browser(b: &mut BrowserState, width: i32, register: bool) {
    use osjeff_core::web::imgcache::{PageImages, image_key};
    use osjeff_core::web::{Cmd, Layout};
    let Some(doc) = &b.doc else {
        return;
    };
    let base = b.browser.nav_url().to_vec();
    let lay = |images: &osjeff_core::web::imgcache::ImageCache| {
        doc.layout(&Layout {
            width,
            zoom: b.zoom,
            images: &PageImages {
                cache: images,
                base: &base,
            },
        })
    };
    let mut page = lay(&b.images);
    if register {
        b.forms = osjeff_core::web::form::FormState::new(&page.forms);
        b.img_keys.clear();
        for r in &page.images {
            let key = image_key(&base, &r.src);
            if let Some(k) = &key {
                let data = (k.starts_with("data:#")).then_some(r.src.as_str());
                b.images.want(k, data);
            }
            b.img_keys.push(key);
        }
        if page.images.len() > osjeff_core::web::imgcache::MAX_PAGE_IMAGES {
            page = lay(&b.images);
        }
    }
    // Make each stored picture exactly the size of its box so painting is a plain copy.
    let mut changed = false;
    for c in &page.cmds {
        if let Cmd::Image { w, h, idx, .. } = c
            && let Some(Some(k)) = b.img_keys.get(*idx)
        {
            changed |= b.images.fit_to(k, *w as usize, *h as usize);
        }
    }
    let _ = changed;
    b.page = Some(page);
    // The words moved: the selection (word indices) and the find matches follow the new layout.
    b.find.refresh(b.page.as_ref());
    if let Some(p) = &b.page
        && b.sel.is_some_and(|(_, e)| e >= p.word_count())
    {
        b.sel = None;
    }
}

mod apps;
mod edit;
mod files;
mod files_ui;
mod input;
mod instance;
mod logview;
mod monitor;
mod render;
mod settings_ui;
mod shellhost;
mod sysstore;
mod term;
mod toasts_ui;
mod ui;
pub(crate) mod vfs;
mod viewer;
mod wasmwin;
mod widgets;
pub(crate) use edit::EditorState;
pub(crate) use input::Special;
pub(crate) use instance::*;
pub(crate) use logview::LogState;
pub use monitor::SysInputs;
pub(crate) use monitor::{MonitorState, SysMon};
pub(crate) use settings_ui::SettingsState;
pub use shellhost::{worker as shell_worker, worker2 as shell_worker2};
pub(crate) use sysstore::*;
pub(crate) use term::TermState;
pub(crate) use wasmwin::*;
pub(crate) use widgets::*;
