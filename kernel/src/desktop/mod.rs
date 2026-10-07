//! Desktop compositor: window manager, app rendering, open/close animations,
//! and a process table surfaced through a Task Manager app.
//!
//! All non-trivial logic (terminal, editor, keymap, window geometry, easing,
//! process table, the dynamic window table) lives in `osjeff_core` and is
//! unit-tested. This module is hardware-facing glue: pixels, animation
//! stepping, dispatch, and one [`instance::App`] state per window.

pub(crate) use crate::fb::{Canvas, Color};
pub(crate) use crate::font;
pub(crate) use crate::icons::{self, Icon};
pub(crate) use crate::logo;
pub(crate) use crate::sched;
pub(crate) use crate::sync::RacyCell;
pub(crate) use crate::theme;
pub(crate) use alloc::string::String;
pub(crate) use alloc::vec::Vec;
pub(crate) use osjeff_core::clipboard::{self, Clipboard};
pub(crate) use osjeff_core::fs;
pub(crate) use osjeff_core::window::{ResizeEdge, TITLE_H, WindowId};
pub(crate) use osjeff_core::winman::{ClickTracker, Switcher, WindowManager, WindowSpec};
pub(crate) use osjeff_core::{
    Action, Calc, Editor, FileName, Key, Keymap, ProcKind, ProcState, ProcessTable, Rect, Terminal,
    Time,
};

// Dock / menu / start-panel / keypad geometry lives in `osjeff_core::layout`.
use osjeff_core::layout::{
    CALC_KEYS, DOCK_MARGIN, MENU_ITEM_H, MENU_PAD, MENU_W, START_GAP, START_PAD, START_ROW_H,
    START_W,
};

const SLIDE_PX: f32 = 28.0;

/// Longest gap between the two presses of a double click, in timer ticks
/// (250 Hz): 500 ms.
const DOUBLE_CLICK_TICKS: u64 = 125;

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
    // SAFETY: DISK is `DISK_BYTES` long and only accessed from the compositor thread (Desktop,
    // files UI, terminal), so the slice is valid.
    // NOTE: not guaranteed by the type: safe fn returning `&'static mut`; `FilesState::rows`/`slot`
    // hold one while `cwd` calls `disk()` again (read-only aliasing, docs/audit/01 #6).
    unsafe { core::slice::from_raw_parts_mut(DISK.get() as *mut u8, DISK_BYTES) }
}

/// Whether the in-memory image may be written back to the disk. Cleared when the
/// boot-time read failed: the image in RAM is then a freshly formatted
/// placeholder, and flushing it would overwrite a perfectly good filesystem
/// after a transient ATA error.
static PERSIST: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

/// Persist the in-memory filesystem image to the ATA disk (best effort: a no-op
/// if there is no disk, or if the disk could not be read at boot).
fn flush_disk() {
    if PERSIST.load(core::sync::atomic::Ordering::Relaxed) && !crate::ata::write_image(disk()) {
        crate::klog!(Error, "disk: write failed, changes are only in RAM");
    }
}

/// Cursor sprite bounding box (used by the dirty-rect overlay path).
pub const CURSOR_W: i32 = 10;
pub const CURSOR_H: i32 = 16;

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
    /// Resizing from `edge`; `start` is the window rect and `(ox, oy)` the
    /// pointer position when the drag began.
    Resize {
        edge: ResizeEdge,
        start: Rect,
        ox: i32,
        oy: i32,
    },
}

pub(crate) struct Drag {
    pub win: WindowId,
    pub mode: DragMode,
}

pub struct Desktop {
    sw: i32,
    sh: i32,
    // One snapshot of the two IDE disks (boot + filesystem) for the file
    // manager's disk panels.
    disks: [Option<crate::ata::DiskInfo>; 2],
    clipboard: Clipboard,
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
    net: Option<(bool, osjeff_core::net::NetConfig)>,
    /// The dynamic window table; every window owns an app instance.
    wm: WindowManager<Inst>,
    drag: Option<Drag>,
    menu: Option<MenuState>,
    start_open: bool,
    /// The Alt+Tab switcher while Alt is held.
    switcher: Option<Switcher>,
    /// Window under the cursor (its title-bar buttons are shown).
    hover: Option<WindowId>,
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
    /// Last message of the Files "Apps" view (an install/remove error).
    files_msg: Option<(String, u8)>,
}

impl Desktop {
    pub fn new(sw: i32, sh: i32) -> Self {
        // At boot only the system processes and the open terminal exist.
        // Apps spawn a fresh process when opened and are removed when closed.
        let mut procs = ProcessTable::new();
        procs.spawn(b"kernel", ProcKind::System, ProcState::Running);
        procs.spawn(b"compositor", ProcKind::System, ProcState::Running);

        // Load the filesystem from disk. If no disk responds or it holds no
        // valid filesystem (blank / first boot), format and persist a fresh one.
        // A missing disk simply leaves us with a RAM-only filesystem.
        let read_ok = crate::ata::read_image(disk());
        if !read_ok {
            PERSIST.store(false, core::sync::atomic::Ordering::Relaxed);
            crate::klog!(
                Warn,
                "OJFS: disk read failed; RAM-only filesystem, disk left untouched"
            );
        }
        if !read_ok || !fs::is_formatted(disk()) {
            fs::format(disk());
            // Seed a couple of welcome files so the file manager has content on a
            // fresh disk (and to document its keys).
            let _ = fs::write(
                disk(),
                b"leiame.txt",
                // Fits the editor grid (44 columns x 18 rows), so it can be opened and
                // saved without loss.
                b"Bem-vindo ao OSjeff.\nGerenciador de arquivos:\n setas   navegam\n Del     manda pra lixeira\n Tab     alterna arquivos/lixeira\n Enter   abre\n",
            );
            let _ = fs::write(disk(), b"notas.txt", b"Arquivo de exemplo do OSjeff.");
            if let Ok(d) = fs::mkdir(disk(), fs::ROOT, b"Documentos") {
                let _ = fs::write_in(
                    disk(),
                    d as u8,
                    b"projeto.txt",
                    b"Arquivo dentro de uma pasta.",
                );
            }
            flush_disk();
        }

        let mut desk = Self {
            sw,
            sh,
            disks: [
                crate::ata::identify(0x1F0, 0x3F6, false),
                crate::ata::identify(0x170, 0x376, false),
            ],
            clipboard: Clipboard::new(),
            keymap: Keymap::new(),
            procs,
            sysmon: SysMon::new(),
            bg_dirty: false,
            toasts: osjeff_core::notify::Toasts::new(),
            toast_seen: crate::klog::seq(),
            toast_dirty: false,
            net: None,
            wm: WindowManager::new(osjeff_core::winman::DEFAULT_MAX_WINDOWS),
            drag: None,
            menu: None,
            start_open: false,
            switcher: None,
            hover: None,
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
            files_msg: None,
        };
        // Install the bundled apps into /apps (first boot) and build the launcher catalog.
        desk.init_apps();
        // The terminal is open (and focused) at boot.
        desk.open_new(Kind::Terminal);
        desk
    }

    pub fn cursor(&self) -> (i32, i32) {
        (self.cursor_x, self.cursor_y)
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
        let mut title = String::from(kind.title());
        if index > 1 {
            // "OSJEFF SHELL 2": the same " N" suffix as the process name.
            let mut tmp = [0u8; 16];
            let k = numbered_name("", index, &mut tmp);
            title.push_str(core::str::from_utf8(&tmp[..k]).unwrap_or(""));
        }
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
            Ok(id) => Some(id),
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

    pub(crate) fn term_mut(&mut self, id: WindowId) -> Option<&mut Terminal> {
        match self.app_mut(id) {
            Some(App::Terminal(t)) => Some(t),
            _ => None,
        }
    }

    pub(crate) fn editor_mut(&mut self, id: WindowId) -> Option<&mut EditorState> {
        match self.app_mut(id) {
            Some(App::Editor(e)) => Some(e),
            _ => None,
        }
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

    /// The (single) browser window, if open.
    fn browser_id(&self) -> Option<WindowId> {
        self.wm
            .windows()
            .iter()
            .find(|w| w.app.kind() == Kind::Browser && !w.is_closing())
            .map(|w| w.id)
    }

    pub(crate) fn request_close(&mut self, id: WindowId) {
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
        let (active, gone) = self.wm.step(dt);
        for w in gone {
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

    /// On-screen rect of window `w`, including its current animation slide.
    pub(crate) fn window_box(&self, w: &Win) -> Rect {
        let mut r = w.rect;
        if let Some(a) = w.anim {
            r.y += a.slide(SLIDE_PX) as i32;
        }
        r
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
            || self.drag.as_ref().is_some_and(|d| d.win == w.id)
            // A visible WASM app renders a fresh frame every tick (it may animate
            // on its own clock), so it is kept out of the cached static layer and
            // repainted through the per-frame damage path like an animation.
            || (w.shown() && w.app.kind() == Kind::WasmApp)
    }

    /// True while any window is opening, closing, being dragged, or is a live
    /// WASM app — i.e. the compositor should run its per-frame damage path so the
    /// app gets continuous frames, rather than the steady (repaint-on-change) one.
    pub fn has_animation(&self) -> bool {
        self.drag.is_some()
            || self
                .wm
                .windows()
                .iter()
                .any(|w| w.shown() && (w.anim.is_some() || w.app.kind() == Kind::WasmApp))
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
        let body = osjeff_core::browser::page_body(resp);
        if let Some(b) = self.browser_state_mut(id) {
            b.page = Some(osjeff_core::web::render(&body, content_w));
            b.body = body;
            b.layout_w = content_w;
            b.scroll = 0;
            b.browser.loaded_with(conn, truncated);
        }
    }

    /// Mark the in-flight browser fetch as failed.
    pub fn browser_fail(&mut self, reason: osjeff_core::browser::FailReason) {
        if let Some(id) = self.browser_id()
            && let Some(b) = self.browser_state_mut(id)
        {
            b.page = None;
            b.body = Vec::new();
            b.browser.fail_with(reason);
        }
    }

    /// Scroll the rendered page of browser window `id` by `dy` pixels, clamped
    /// to its content height.
    pub(crate) fn scroll_page(&mut self, id: WindowId, dy: i32) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let view_h = BrowserChrome::of(rect).content.h;
        if let Some(b) = self.browser_state_mut(id) {
            let max = b
                .page
                .as_ref()
                .map(|p| (p.height - view_h).max(0))
                .unwrap_or(0);
            b.scroll = (b.scroll + dy).clamp(0, max);
        }
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
            let page = osjeff_core::web::render(&b.body, content.w);
            let max = (page.height - content.h).max(0);
            b.scroll = b.scroll.clamp(0, max);
            b.page = Some(page);
            b.layout_w = content.w;
        }
    }
}

mod apps;
mod files;
mod files_ui;
mod input;
mod instance;
mod logview;
mod monitor;
mod render;
mod settings_ui;
mod sysstore;
mod toasts_ui;
mod ui;
mod wasmwin;
mod widgets;
pub(crate) use instance::*;
pub(crate) use logview::LogState;
pub use monitor::SysInputs;
pub(crate) use monitor::{MonitorState, SysMon};
pub(crate) use settings_ui::SettingsState;
pub(crate) use sysstore::*;
pub(crate) use wasmwin::*;
pub(crate) use widgets::*;
