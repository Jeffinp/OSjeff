//! Desktop compositor: window manager, app rendering, open/close animations,
//! and a process table surfaced through a Task Manager app.
//!
//! All non-trivial logic (terminal, editor, keymap, window geometry, easing,
//! process table, the dynamic window table) lives in `osjeff_core` and is
//! unit-tested. This module is hardware-facing glue: pixels, animation
//! stepping, dispatch, and one [`instance::App`] state per window.

pub(crate) use crate::fb::{Canvas, Color, Corner, Shadow};
pub(crate) use crate::icons::{self, Icon};
pub(crate) use crate::sched;
pub(crate) use crate::sync::RacyCell;
pub(crate) use crate::theme;
pub(crate) use alloc::string::String;
pub(crate) use alloc::vec::Vec;
pub(crate) use osjeff_core::clipboard::{self, Clipboard};
pub(crate) use osjeff_core::window::{MENUBAR_H, ResizeEdge, TITLE_H, WindowId};
pub(crate) use osjeff_core::winman::{ClickTracker, Switcher, WindowManager, WindowSpec};
pub(crate) use osjeff_core::{Calc, Key, Keymap, ProcKind, ProcState, ProcessTable, Rect, Time};
pub(crate) use osjeff_core::{iconart, widgets as wlogic};

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
/// buffer before every frame. Every pointer sprite is rendered into exactly this box
/// (`osjeff_core::pointer::{W, H}`), so no sprite can outgrow what gets erased.
pub const CURSOR_W: i32 = osjeff_core::pointer::W as i32;
pub const CURSOR_H: i32 = osjeff_core::pointer::H as i32;

/// What changed after a mouse packet, so the caller can pick the cheap
/// cursor-only repaint vs a full scene recompose.
pub struct MouseResult {
    pub scene_dirty: bool,
    pub cursor_moved: bool,
}

/// A pointer drag in progress.
#[derive(Clone, Copy)]
pub(crate) enum DragMode {
    /// Moving the window: offset of the grab point inside it.
    Move { grab_dx: i32, grab_dy: i32 },
    /// A maximised or tiled window's title was pressed at `(ox, oy)`: once the pointer moves it
    /// is restored under the pointer and becomes a [`DragMode::Move`].
    Unsnap { ox: i32, oy: i32 },
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
    /// A control of a system app being dragged (a slider of Ajustes).
    Ui,
    /// A press in a file manager: a click, a drag of items or a rubber band (the gesture
    /// lives in the window's state).
    Files,
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
    /// Menus, popovers, sheets, the Apps and Busca overlays and the app bar.
    shell: shell::Shell,
    /// Per-window focus transitions (title bar and shadow cross-fade).
    focus_mix: core::cell::RefCell<Vec<chrome::FocusMix>>,
    /// Local date `(year, month, day)` and weekday (0 = Sunday), refreshed each second.
    today: core::cell::Cell<(i32, u8, u8)>,
    weekday: core::cell::Cell<u8>,
    /// The Alt+Tab switcher while Alt is held.
    switcher: Option<Switcher>,
    /// Window under the cursor.
    hover: Option<WindowId>,
    /// The title-bar button under the pointer (hover fill).
    title_hover: Option<(WindowId, osjeff_core::window::TitleBtn)>,
    /// Which window (and size) the offscreen texture currently holds.
    tex_key: core::cell::Cell<Option<(WindowId, i32, i32)>>,
    clicks: ClickTracker,
    /// Set by operations that change pixels outside the focused window (maximize,
    /// restore, ...); makes the next steady frame upload the whole screen.
    force_full: bool,
    /// A browser window whose client area is the only thing that changed since the last frame
    /// (a scroll, a hover, a keystroke in its bar): the compositor repaints just that area.
    client_dirty: Option<WindowId>,
    /// What the last full animation frame repainted (a client-only frame leaves it alone).
    anim_full: core::cell::Cell<Rect>,
    /// The box of the browser window the last full animation frame drew when it was the only
    /// moving thing on screen: a frame that only repaints its client area needs it unchanged.
    anim_solo_box: core::cell::Cell<Rect>,
    /// Region to upload on the next steady frame besides the focused window.
    extra_dirty: Rect,
    cursor_x: i32,
    cursor_y: i32,
    prev_left: bool,
    prev_right: bool,
    /// Installed WASM apps (the Apps overlay lists them after the system apps).
    apps: Vec<AppEntry>,
    /// The WASM window that owns the pressed mouse button.
    wasm_grab: Option<WindowId>,
    /// Last seen generation of app-side clipboard writes.
    clip_gen: u64,
    /// `vfs::generation()` the file managers last loaded, and the tick of that check.
    fs_gen: u32,
    fs_gen_tick: u64,
    /// Timer tick of the last frame of a window animating its own content (see `render_anim_frame`).
    live_tick: core::cell::Cell<u64>,
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
            shell: shell::Shell::new(),
            focus_mix: core::cell::RefCell::new(Vec::new()),
            today: core::cell::Cell::new((2026, 1, 1)),
            weekday: core::cell::Cell::new(4),
            switcher: None,
            hover: None,
            title_hover: None,
            tex_key: core::cell::Cell::new(None),
            clicks: ClickTracker::new(DOUBLE_CLICK_TICKS),
            force_full: false,
            client_dirty: None,
            anim_full: core::cell::Cell::new(Rect::new(0, 0, 0, 0)),
            anim_solo_box: core::cell::Cell::new(Rect::new(0, 0, 0, 0)),
            extra_dirty: Rect::new(0, 0, 0, 0),
            cursor_x: sw / 2,
            cursor_y: sh / 2,
            prev_left: false,
            prev_right: false,
            apps: Vec::new(),
            wasm_grab: None,
            clip_gen: crate::wasm::clip_generation(),
            fs_gen: vfs::generation(),
            fs_gen_tick: 0,
            live_tick: core::cell::Cell::new(0),
        };
        // Install the bundled apps into /apps (first boot) and build the launcher catalog.
        desk.init_apps();
        // The terminal is open (and focused) at boot.
        desk.open_new(Kind::Terminal);
        desk
    }

    /// Everything that decides the cursor's pixels: the top-left of the sprite box and which
    /// sprite (arrow, hand, I-beam). The compositor repaints the sprite whenever this differs
    /// from what it painted.
    pub fn pointer(&self) -> osjeff_core::cursor::Pointer {
        let (x, y) = self.cursor();
        osjeff_core::cursor::Pointer {
            x,
            y,
            shape: self.cursor_shape() as u8,
        }
    }

    /// Top-left of the pointer sprite box (the pointer position minus the hotspot).
    pub fn cursor(&self) -> (i32, i32) {
        let (hx, hy) = osjeff_core::pointer::hotspot(self.cursor_shape());
        (self.cursor_x - hx, self.cursor_y - hy)
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
                self.dock_bounce(kind);
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
        let files_busy = self.step_files(dt) | self.step_viewers(dt) | self.step_editors(dt);
        self.step_shell_jobs();
        self.sync_text_windows();
        self.live_step(dt);
        let (active, gone) = self.wm.step(dt);
        let focus_busy = self.step_focus(dt);
        let shell_busy = self.step_shell(dt);
        let browser_busy = self.step_browser(dt);
        let active = active || focus_busy || shell_busy || files_busy || browser_busy;
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
            if self.title_hover.is_some_and(|(id, _)| id == w.id) {
                self.title_hover = None;
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
        self.refresh_date();
        self.refresh_notifs();
    }

    /// Read the local date once a second (the panel clock and the calendar).
    fn refresh_date(&mut self) {
        let hour = crate::rtc::now().h;
        if hour != self.shell.last_hour {
            self.poll_appearance(hour);
        }
        let local =
            osjeff_core::hw::rtc::utc_to_local(crate::rtc::read_utc(), crate::rtc::tz_minutes());
        if local.is_valid() {
            self.today
                .set((local.date.y as i32, local.date.m, local.date.d));
            self.weekday.set(local.weekday());
        }
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

    /// On-screen rect of window `w` right now: its resting rectangle, the in-flight
    /// rectangle of a zoom, or the scaled/moved one of an open / close / minimise.
    pub(crate) fn window_box(&self, w: &Win) -> Rect {
        if let Some(a) = w.anim {
            return a.frame(w.rect, self.dock_target(w)).rect;
        }
        w.visual_rect()
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
            || (w.shown() && matches!(&w.app.app, App::Files(f) if f.animating()))
            || (w.shown() && matches!(&w.app.app, App::Viewer(v) if v.animating()))
            || (w.shown()
                && matches!(&w.app.app, App::Editor(e) if e.animating(self.focused() == Some(w.id))))
            || (w.shown()
                && matches!(&w.app.app, App::Terminal(t) if t.term.is_running() || t.animating(self.focused() == Some(w.id))))
            || self.live_dynamic(w)
            || (w.shown() && self.browser_busy(w))
            || self.focus_busy(w.id)
    }

    /// True while any window is opening, closing, being dragged, or is a live
    /// WASM app — i.e. the compositor should run its per-frame damage path so the
    /// app gets continuous frames, rather than the steady (repaint-on-change) one.
    pub fn has_animation(&self) -> bool {
        self.drag.is_some()
            || self.shell_animating()
            || self.toasts_sliding()
            || self.live_busy()
            || self.wm.windows().iter().any(|w| {
                w.shown()
                    && (w.anim.is_some()
                        || w.zoom.is_some()
                        || self.focus_busy(w.id)
                        || w.app.kind() == Kind::WasmApp
                        || matches!(&w.app.app, App::Files(f) if f.animating())
                        || matches!(&w.app.app, App::Viewer(v) if v.animating())
                        || matches!(&w.app.app, App::Editor(e) if e.animating(self.focused() == Some(w.id)))
                        || matches!(&w.app.app, App::Terminal(t) if t.term.is_running() || t.animating(self.focused() == Some(w.id)))
                        || self.browser_busy(w))
            })
    }

    /// Consume the "only this window's client area changed" note.
    pub fn take_client_dirty(&mut self) -> Option<WindowId> {
        self.client_dirty.take()
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
}

mod appart;
mod apps;
mod appui;
mod browser;
mod browser_input;
mod browser_paint;
mod browser_ui;
pub(crate) mod calc_ui;
mod chrome;
mod cursor;
mod edit;
mod edit_ui;
mod files;
mod files_ui;
mod gallery;
mod glass;
mod input;
mod instance;
mod kit;
mod lang;
mod live;
mod logview;
mod overlays;
mod panel;
mod render;
mod settings_ui;
mod shell;
mod shellhost;
mod sysstore;
mod tarefas;
mod taskbar;
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
pub(crate) use settings_ui::SettingsState;
pub use shellhost::{worker as shell_worker, worker2 as shell_worker2};
pub(crate) use sysstore::*;
pub use tarefas::SysInputs;
pub(crate) use tarefas::{SysMon, TarefasState};
pub(crate) use term::TermState;
pub(crate) use wasmwin::*;
pub(crate) use widgets::*;
