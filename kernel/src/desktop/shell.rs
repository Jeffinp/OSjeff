//! The system shell: state of the menu bar menus, popovers, the confirmation
//! sheet, the Apps overlay, Busca and the app bar (dock), the commands they
//! issue, and the per-frame stepping of their animations.
//!
//! Drawing lives in `menubar.rs`, `dock.rs` and `overlays.rs`; this file is the
//! model and the glue to the rest of the desktop.

use super::glass::BackdropSlot;
use super::*;
use core::cell::Cell;
use kitsune_core::anim::{Tween, curves};
use kitsune_core::chrome::{MenuGeom, MenuRow};
use kitsune_core::raster::Surface;
use kitsune_core::snap::SnapZone;

/// Everything a menu entry or a button can ask the desktop to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Cmd {
    /// A separator line (menus only).
    Sep,
    About,
    Settings,
    Reboot,
    Shutdown,
    NewWindow,
    CloseWindow,
    Minimize,
    Zoom,
    Copy,
    Cut,
    Paste,
    SelectAll,
    Undo,
    Redo,
    OpenFile,
    SaveFile,
    ShowApps,
    ShowSearch,
    Gallery,
    Activate(WindowId),
    Launch(Kind),
    BrowserZoomIn,
    BrowserZoomOut,
    BrowserZoomReset,
    /// Tile the focused window to a half or a quarter of the work area.
    Snap(SnapZone),
    /// Open one more window of an app (taskbar menu).
    NewOf(Kind),
    /// Move the focused window to workspace `n` (0-based) and go there.
    MoveToWorkspace(u8),
    /// Pin an app to the taskbar / take it off.
    Pin(Kind),
    Unpin(Kind),
    /// Minimise every window, or bring back the ones that command hid.
    ShowDesktop,
    /// Close every window of an app (app-bar menu).
    QuitOf(Kind),
    /// A command of the file manager (its context and sort menus, the View menu).
    Files(kitsune_core::fileman::Cmd),
}

/// One row of a menu.
#[derive(Clone)]
pub(crate) struct Entry {
    pub label: String,
    pub shortcut: &'static str,
    pub cmd: Cmd,
    pub enabled: bool,
    pub checked: bool,
}

impl Entry {
    pub(crate) fn item(label: &str, shortcut: &'static str, cmd: Cmd) -> Entry {
        Entry {
            label: String::from(label),
            shortcut,
            cmd,
            enabled: true,
            checked: false,
        }
    }

    pub(crate) fn sep() -> Entry {
        Entry::item("", "", Cmd::Sep)
    }
}

/// An item of the top panel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PanelItem {
    /// The Kitsune mark and "Apps": opens the launcher.
    Apps,
    /// Busca.
    Search,
    /// The workspace dots: click one to go there.
    Workspaces,
    /// The date and time: the calendar and notification centre.
    Clock,
    /// The status pill: Quick Settings.
    Tray,
}

/// Where an open menu came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MenuOrigin {
    /// The menu button in a window's title bar.
    Window(WindowId),
    /// A right-click menu (desktop or an app-bar icon).
    Context,
}

/// An open menu (dropdown or context menu), fading in.
pub(crate) struct OpenMenu {
    pub origin: MenuOrigin,
    pub entries: Vec<Entry>,
    pub rows: Vec<MenuRow>,
    pub geom: MenuGeom,
    pub hover: Option<usize>,
    pub t: Tween,
    pub closing: bool,
    pub glass: BackdropSlot,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PopKind {
    /// Quick Settings, under the status pill.
    Quick,
    /// The calendar and notification centre, under the clock.
    Centre,
}

/// A popover under a panel item.
pub(crate) struct Popover {
    pub kind: PopKind,
    pub rect: Rect,
    pub t: Tween,
    pub closing: bool,
    pub glass: BackdropSlot,
    /// Calendar: months shown relative to the current one.
    pub month_off: i32,
}

/// A confirmation sheet (restart / shut down).
pub(crate) struct Dialog {
    pub title: String,
    pub body: String,
    pub ok: String,
    pub cmd: Cmd,
    pub t: Tween,
    pub closing: bool,
    /// 0 = cancel, 1 = confirm (keyboard focus).
    pub focus: usize,
}

/// What a tile of the Apps overlay opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Target {
    Kind(Kind),
    Wasm(usize),
}

pub(crate) struct Tile {
    pub label: String,
    pub target: Target,
    pub icon: Surface,
    /// Its category in the rail and the key *Recentes* remembers it by.
    pub cat: kitsune_core::launcher::Category,
    pub key: String,
}

/// The Apps overlay: every app in a grid with a search field.
pub(crate) struct AppsView {
    pub tiles: Vec<Tile>,
    /// The rail's selection (index into `launcher::CATEGORIES`) and the row under the pointer.
    pub cat: usize,
    pub rail_hover: Option<usize>,
    pub query: String,
    /// Indices into `tiles` matching `query`.
    pub shown: Vec<usize>,
    pub scroll: usize,
    pub hover: Option<usize>,
    pub t: Tween,
    pub closing: bool,
    pub backdrop: BackdropSlot,
}

/// What a Busca result does.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum HitKind {
    App(Target),
    /// An alias that opens Tarefas on a tab ("monitor", "memória", ...).
    Tab(u8),
    File(Vec<u8>),
    Calc,
}

#[derive(Clone)]
pub(crate) struct SearchHit {
    pub title: String,
    pub sub: String,
    pub kind: HitKind,
}

/// The Busca overlay: one field; results are apps, files and calculator answers.
pub(crate) struct SearchView {
    pub query: String,
    pub hits: Vec<SearchHit>,
    pub sel: usize,
    /// Files known to the index: `(path, name)`, built when it opened.
    pub files: Vec<(Vec<u8>, String)>,
    pub t: Tween,
    pub closing: bool,
    pub glass: BackdropSlot,
}

/// The outline previewing where a dragged window snaps: it travels from the window's rectangle
/// to the zone's while it fades in.
pub(crate) struct SnapPreview {
    pub zone: SnapZone,
    pub from: Rect,
    pub to: Rect,
    pub t: Tween,
}

/// All shell state of the desktop.
pub(crate) struct Shell {
    /// The snap preview while a window is dragged to an edge.
    pub snap: Option<SnapPreview>,
    pub menu: Option<OpenMenu>,
    pub pop: Option<Popover>,
    pub dialog: Option<Dialog>,
    pub apps: Option<AppsView>,
    pub search: Option<SearchView>,
    pub task: super::taskbar::TaskbarState,
    /// Panel item under the pointer (highlight).
    pub panel_hover: Option<PanelItem>,
    /// The notification centre's history (oldest first), the log cursor that feeds it and how
    /// many entries arrived since the centre was last opened.
    pub notifs: Vec<super::panel::Notif>,
    pub notif_seen: u32,
    pub notif_unread: usize,
    /// The apps launched most recently (the launcher's *Recentes* row).
    pub recents: kitsune_core::launcher::Recents,
    /// The performance HUD (Ctrl+Alt+H).
    pub hud: bool,
    /// Region of the Apps overlay that changed since it was last painted.
    pub dirty: Cell<Rect>,
    /// Quick Settings and the notification centre: their switches animate here.
    pub knobs: [Tween; 3],
    /// Last time of day (seconds) the appearance was resolved for (Auto).
    pub last_hour: u8,
    /// Blurred backdrop of the Alt+Tab panel, captured when it first draws.
    pub switcher_glass: BackdropSlot,
}

impl Shell {
    pub(crate) fn new() -> Shell {
        Shell {
            snap: None,
            menu: None,
            pop: None,
            dialog: None,
            apps: None,
            search: None,
            task: super::taskbar::TaskbarState::new(),
            panel_hover: None,
            notifs: Vec::new(),
            notif_seen: crate::klog::seq(),
            notif_unread: 0,
            recents: kitsune_core::launcher::Recents::new(),
            hud: false,
            dirty: Cell::new(Rect::new(0, 0, 0, 0)),
            knobs: [Tween::at(0.0); 3],
            last_hour: 255,
            switcher_glass: Default::default(),
        }
    }
}

/// Fade-in seconds of menus and popovers, and of the full-screen overlays.
pub(crate) const MENU_FADE: f32 = 0.14;
pub(crate) const OVERLAY_FADE: f32 = 0.22;

/// A tween starting a fade from nothing.
pub(crate) fn fade_in(secs: f32) -> Tween {
    let mut t = Tween::at(0.0);
    t.retarget(1.0, secs, curves::ENTER);
    t
}

pub(crate) fn fade_out(t: &mut Tween, secs: f32) {
    t.retarget(0.0, secs, curves::EXIT);
}

/// Opacity 0..=256 of a fade tween.
pub(crate) fn level(t: &Tween) -> u32 {
    (t.value().clamp(0.0, 1.0) * 256.0) as u32
}

impl Desktop {
    // ---- stepping ----

    /// Advance the shell's animations by `dt` seconds; `true` while any still runs.
    pub(crate) fn step_shell(&mut self, dt: f32) -> bool {
        let mut busy = false;
        let sh = &mut self.shell;
        if let Some(m) = sh.menu.as_mut() {
            busy |= m.t.step(dt);
        }
        if let Some(p) = sh.pop.as_mut() {
            busy |= p.t.step(dt);
        }
        if let Some(d) = sh.dialog.as_mut() {
            busy |= d.t.step(dt);
        }
        if let Some(a) = sh.apps.as_mut() {
            busy |= a.t.step(dt);
        }
        if let Some(s) = sh.search.as_mut() {
            busy |= s.t.step(dt);
        }
        for k in sh.knobs.iter_mut() {
            busy |= k.step(dt);
        }
        if let Some(sp) = sh.snap.as_mut() {
            busy |= sp.t.step(dt);
        }
        // Closing overlays disappear when their fade-out ends.
        if sh
            .menu
            .as_ref()
            .is_some_and(|m| m.closing && m.t.finished())
        {
            sh.menu = None;
            self.force_full = true;
        }
        if sh.pop.as_ref().is_some_and(|p| p.closing && p.t.finished()) {
            sh.pop = None;
            self.force_full = true;
        }
        if sh
            .dialog
            .as_ref()
            .is_some_and(|d| d.closing && d.t.finished())
        {
            sh.dialog = None;
            self.force_full = true;
        }
        if sh
            .apps
            .as_ref()
            .is_some_and(|a| a.closing && a.t.finished())
        {
            sh.apps = None;
            self.force_full = true;
        }
        if sh
            .search
            .as_ref()
            .is_some_and(|s| s.closing && s.t.finished())
        {
            sh.search = None;
            self.force_full = true;
        }
        busy | self.step_dock(dt)
    }

    /// Any shell animation or overlay transition is running (needs frames).
    pub(crate) fn shell_animating(&self) -> bool {
        let sh = &self.shell;
        sh.menu.as_ref().is_some_and(|m| !m.t.finished())
            || sh.pop.as_ref().is_some_and(|p| !p.t.finished())
            || sh.dialog.as_ref().is_some_and(|d| !d.t.finished())
            || sh.apps.as_ref().is_some_and(|a| !a.t.finished())
            || sh.search.as_ref().is_some_and(|s| !s.t.finished())
            || sh.knobs.iter().any(|k| !k.finished())
            || sh.snap.as_ref().is_some_and(|p| !p.t.finished())
            || self.dock_animating()
    }

    // ---- overlay bookkeeping ----

    /// True while a transient overlay (menu, popover, sheet, Apps, Busca, Alt+Tab) is shown.
    pub fn overlay_open(&self) -> bool {
        let sh = &self.shell;
        sh.menu.is_some()
            || sh.pop.is_some()
            || sh.dialog.is_some()
            || sh.apps.is_some()
            || sh.search.is_some()
            || self.switcher.is_some()
    }

    /// The compositor painted the dirty region.
    pub(crate) fn overlay_painted(&self) {
        self.shell.dirty.set(Rect::new(0, 0, 0, 0));
    }

    /// Mark the whole screen dirty for the Apps overlay.
    pub(crate) fn apps_dirty_all(&self) {
        self.shell.dirty.set(Rect::new(0, 0, self.sw, self.sh));
    }

    /// Close every overlay that a click elsewhere dismisses (fade out).
    pub(crate) fn close_transients(&mut self) {
        let sh = &mut self.shell;
        if let Some(m) = sh.menu.as_mut()
            && !m.closing
        {
            m.closing = true;
            fade_out(&mut m.t, MENU_FADE);
        }
        if let Some(p) = sh.pop.as_mut()
            && !p.closing
        {
            p.closing = true;
            fade_out(&mut p.t, MENU_FADE);
        }
        self.force_full = true;
    }

    /// Is a modal layer (sheet, Apps, Busca) up? Mouse and keys go to it exclusively.
    pub(crate) fn modal_open(&self) -> bool {
        let sh = &self.shell;
        sh.dialog.as_ref().is_some_and(|d| !d.closing)
            || sh.apps.as_ref().is_some_and(|a| !a.closing)
            || sh.search.as_ref().is_some_and(|s| !s.closing)
    }

    // ---- commands ----

    /// The window the menu bar acts on: the focused one.
    fn target_window(&self) -> Option<(WindowId, Kind)> {
        let id = self.focused()?;
        Some((id, self.kind_of(id)?))
    }

    /// Run a menu or button command.
    pub(crate) fn execute(&mut self, cmd: Cmd) {
        use kitsune_core::input::{KeyCode, KeyEvent, Mods};
        let target = self.target_window();
        let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), Mods::CTRL);
        match cmd {
            Cmd::Sep => {}
            Cmd::About => self.open_settings(super::settings_ui::ABOUT),
            Cmd::Settings => self.open_settings(0),
            Cmd::Reboot => self.ask_power(false),
            Cmd::Shutdown => self.ask_power(true),
            Cmd::NewWindow => {
                let kind = target.map_or(Kind::Terminal, |(_, k)| k);
                if let Some((id, Kind::Browser)) = target {
                    // One browser window: "new" opens a tab.
                    self.menu_ctrl_key_browser(id, 't');
                } else {
                    self.new_window(kind);
                }
            }
            Cmd::CloseWindow => {
                if let Some((id, kind)) = target {
                    if kind == Kind::Browser {
                        self.browser_close_active(id);
                    } else {
                        self.request_close(id);
                    }
                }
            }
            Cmd::Minimize => {
                if let Some((id, _)) = target {
                    self.wm.minimize(id);
                }
            }
            Cmd::Zoom => {
                if let Some((id, _)) = target {
                    self.toggle_maximize(id);
                }
            }
            Cmd::Copy | Cmd::Paste | Cmd::Cut | Cmd::SelectAll
                if target.is_some_and(|(_, k)| k == Kind::Files) =>
            {
                use kitsune_core::fileman::Cmd as F;
                self.files_run_focused(match cmd {
                    Cmd::Copy => F::Copy,
                    Cmd::Paste => F::Paste,
                    Cmd::Cut => F::Cut,
                    _ => F::SelectAll,
                });
            }
            Cmd::Copy => self.copy_from_focused(),
            Cmd::Paste => self.paste_into_focused(),
            Cmd::Cut => self.menu_ctrl(ctrl('x')),
            Cmd::SelectAll => self.menu_ctrl(ctrl('a')),
            Cmd::Undo => self.menu_ctrl(ctrl('z')),
            Cmd::Redo => self.menu_ctrl(ctrl('y')),
            Cmd::OpenFile => self.menu_ctrl(ctrl('o')),
            Cmd::SaveFile => self.menu_ctrl(ctrl('s')),
            Cmd::BrowserZoomIn => self.menu_ctrl(ctrl('=')),
            Cmd::BrowserZoomOut => self.menu_ctrl(ctrl('-')),
            Cmd::BrowserZoomReset => self.menu_ctrl(ctrl('0')),
            Cmd::ShowApps => self.open_apps(),
            Cmd::ShowSearch => self.open_search(),
            Cmd::Gallery => {
                self.launch(Kind::Gallery);
            }
            Cmd::MoveToWorkspace(n) => {
                if let Some((id, _)) = target {
                    self.move_window_to_workspace(id, n);
                }
            }
            Cmd::Pin(k) => self.set_pinned(k, true),
            Cmd::Unpin(k) => self.set_pinned(k, false),
            Cmd::ShowDesktop => self.show_desktop(),
            Cmd::Snap(zone) => {
                if let Some((id, _)) = target {
                    self.snap_window(id, zone);
                }
            }
            Cmd::Activate(id) => {
                self.wm.activate(id);
            }
            Cmd::Launch(k) => {
                self.launch(k);
            }
            Cmd::NewOf(k) => {
                self.new_window(k);
            }
            Cmd::Files(c) => self.files_run_focused(c),
            Cmd::QuitOf(k) => {
                let ids: Vec<WindowId> = self
                    .wm
                    .windows()
                    .iter()
                    .filter(|w| w.app.kind() == k)
                    .map(|w| w.id)
                    .collect();
                for id in ids {
                    self.request_close(id);
                }
            }
        }
        self.force_full = true;
    }

    /// A Ctrl chord for browser window `id` from a menu entry.
    fn menu_ctrl_key_browser(&mut self, id: WindowId, c: char) {
        self.browser_ctrl_chord(id, c);
    }

    /// Send a Ctrl chord to the focused app as if typed (menu entries reuse the keys).
    fn menu_ctrl(&mut self, ev: kitsune_core::input::KeyEvent) {
        let Some((id, kind)) = self.target_window() else {
            return;
        };
        match kind {
            Kind::Editor => {
                self.editor_event(id, ev);
            }
            Kind::Terminal => {
                self.term_event(id, ev);
            }
            Kind::Browser => {
                use kitsune_core::input::KeyCode;
                if let KeyCode::Char(c) = ev.code {
                    self.browser_ctrl_chord(id, c);
                }
            }
            _ => {}
        }
    }

    /// Open the Settings window on `section`.
    pub(crate) fn open_settings(&mut self, section: u8) {
        if let Some(id) = self.launch(Kind::Settings)
            && let Some(App::Settings(s)) = self.app_mut(id)
        {
            s.select_section(section);
        }
    }

    /// Ask before restarting / shutting down.
    pub(crate) fn ask_power(&mut self, shutdown: bool) {
        self.close_transients();
        self.shell.dialog = Some(Dialog {
            title: String::from(if shutdown {
                kitsune_core::t!("power.shutdown_title")
            } else {
                kitsune_core::t!("power.restart_title")
            }),
            body: String::from(kitsune_core::t!("power.body")),
            ok: String::from(if shutdown {
                kitsune_core::t!("power.shutdown")
            } else {
                kitsune_core::t!("power.restart")
            }),
            cmd: if shutdown { Cmd::Shutdown } else { Cmd::Reboot },
            t: fade_in(MENU_FADE),
            closing: false,
            focus: 0,
        });
    }

    /// The sheet's confirm button: really do it.
    pub(crate) fn power_now(&mut self, cmd: Cmd) {
        if self.guard_unsaved() {
            return;
        }
        match cmd {
            Cmd::Shutdown => crate::power::shutdown(),
            _ => crate::power::reboot(),
        }
    }

    /// Fade the sheet out (cancel or after confirming).
    pub(crate) fn close_dialog(&mut self) {
        if let Some(d) = self.shell.dialog.as_mut()
            && !d.closing
        {
            d.closing = true;
            fade_out(&mut d.t, MENU_FADE);
        }
    }

    /// The HUD toggle (Ctrl+Alt+H).
    pub fn hud_visible(&self) -> bool {
        self.shell.hud
    }

    /// Resolve the appearance setting for the current local hour (Auto follows the
    /// clock) and tell the compositor when the look changed.
    pub(crate) fn poll_appearance(&mut self, hour: u8) {
        let s = crate::settings::get();
        let want = s.appearance.resolve(hour);
        if crate::theme::set_appearance(want) {
            self.bg_dirty = true;
            self.force_full = true;
            // Backdrops of open overlays were captured in the old look.
            self.close_transients();
        }
        self.shell.last_hour = hour;
    }
}
