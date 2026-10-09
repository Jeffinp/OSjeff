//! App instances: what lives inside each window.
//!
//! The desktop used to keep one field per app (`term`, `editor`, ...) and a
//! fixed window slot per [`Kind`]. Now every window owns an [`Inst`] holding its
//! own [`App`] state, so any number of terminals, editors, file managers and
//! calculators can be open at once, each with its own process-table entry.
//!
//! # Adding an app
//!
//! 1. Add a [`Kind`] variant and fill in its `const fn` metadata below (title,
//!    process name, default size, minimum size, `multi`, `resizable`, icon).
//! 2. Add an [`App`] variant holding the per-window state (box big states so
//!    moving a window record stays cheap) and construct it in [`App::new`].
//! 3. Draw it in `Desktop::draw_window` (`render.rs`) and handle keys / clicks in
//!    `input.rs`. Everything else — z-order, focus, minimize / maximize / resize,
//!    Alt+Tab, the dock indicator, the process entry (`name`, `name 2`, ...) and
//!    teardown on close — is generic and needs no change.
//! 4. Add it to `taskbar::DEFAULT_PINNED` if it should start pinned to the taskbar; every app
//!    appears in the Apps overlay and in Busca on its own, and while it runs the taskbar
//!    shows it.
//!
//! Instance state is plain data; nothing here allocates per frame. The
//! per-instance heap objects (`Box`, `Vec`, `String`) are created when the window
//! opens and freed when it is destroyed.

use crate::desktop::*;
use alloc::boxed::Box;
use kitsune_core::i18n::{Lang, tr, tr_in};
use kitsune_core::tk;

/// Which app a window runs. The order is the Apps overlay / menu order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Terminal,
    Editor,
    TaskMgr,
    Calculator,
    Browser,
    WasmApp,
    Files,
    Settings,
    LogViewer,
    Viewer,
    /// The component gallery (Ctrl+Alt+G): not listed anywhere else.
    Gallery,
}

impl Kind {
    pub(crate) const ALL: [Kind; 10] = [
        Kind::Terminal,
        Kind::Editor,
        Kind::TaskMgr,
        Kind::Calculator,
        Kind::Browser,
        Kind::WasmApp,
        Kind::Files,
        Kind::Settings,
        Kind::LogViewer,
        Kind::Viewer,
    ];

    /// Does the window's content change on its own every second (so the
    /// per-second tick must repaint it)?
    pub(crate) const fn is_live(self) -> bool {
        matches!(self, Kind::TaskMgr | Kind::Settings | Kind::LogViewer)
    }

    /// Process-table name of the first instance (`shell`; later ones get `shell 2`...).
    pub(crate) const fn proc_name(self) -> &'static str {
        match self {
            Kind::Terminal => "shell",
            Kind::Editor => "editor",
            Kind::TaskMgr => "taskmgr",
            Kind::Calculator => "calc",
            Kind::Browser => "browser",
            Kind::WasmApp => "wasmapp",
            Kind::Files => "files",
            Kind::Settings => "settings",
            Kind::LogViewer => "syslog",
            Kind::Viewer => "viewer",
            Kind::Gallery => "gallery",
        }
    }

    /// Catalog key of the app's name.
    pub(crate) const fn name_key(self) -> &'static str {
        match self {
            Kind::Terminal => tk!("app.terminal"),
            Kind::Editor => tk!("app.editor"),
            Kind::TaskMgr => tk!("app.tasks"),
            Kind::Calculator => tk!("app.calculator"),
            Kind::Browser => tk!("app.browser"),
            Kind::WasmApp => tk!("app.wasm"),
            Kind::Files => tk!("app.files"),
            Kind::Settings => tk!("app.settings"),
            Kind::LogViewer => tk!("app.log"),
            Kind::Viewer => tk!("app.viewer"),
            Kind::Gallery => tk!("app.gallery"),
        }
    }

    /// Title-bar text of the first instance, in language `l`.
    pub(crate) fn title_in(self, l: Lang) -> &'static str {
        match self {
            Kind::WasmApp => tr_in(l, tk!("app.wasm_title")),
            _ => tr_in(l, self.name_key()),
        }
    }

    /// Name in menus, the app bar's tooltips, the Apps overlay and Busca.
    pub(crate) fn label(self) -> &'static str {
        tr(self.name_key())
    }

    /// Name in language `l` (Busca also matches the English name in Portuguese).
    pub(crate) fn label_in(self, l: Lang) -> &'static str {
        tr_in(l, self.name_key())
    }

    pub(crate) const fn icon(self) -> Icon {
        match self {
            Kind::Terminal => Icon::Terminal,
            Kind::Editor => Icon::Editor,
            Kind::TaskMgr => Icon::TaskMgr,
            Kind::Calculator => Icon::Calculator,
            Kind::Browser => Icon::Browser,
            Kind::WasmApp => Icon::WasmApp,
            Kind::Files => Icon::Files,
            Kind::Settings => Icon::Settings,
            Kind::LogViewer => Icon::Log,
            Kind::Viewer => Icon::Viewer,
            Kind::Gallery => Icon::WasmApp,
        }
    }

    /// May several windows of this kind be open at once? The task manager and the
    /// browser (one NIC, one fetcher thread) are single-instance for now: launching
    /// them again focuses the existing window. WASM apps are multi-instance: each
    /// window is its own `AppManager` instance.
    pub(crate) const fn multi(self) -> bool {
        matches!(
            self,
            Kind::Terminal
                | Kind::Editor
                | Kind::Calculator
                | Kind::Files
                | Kind::WasmApp
                | Kind::Viewer
        )
    }

    /// Position and size of the first instance (later ones cascade from it).
    pub(crate) const fn default_rect(self) -> Rect {
        match self {
            Kind::Terminal => Rect::new(70, 80, 600, 360),
            Kind::Editor => Rect::new(610, 110, 560, 350),
            Kind::TaskMgr => Rect::new(190, 52, 860, 592),
            Kind::Calculator => Rect::new(470, 100, 320, 520),
            Kind::Browser => Rect::new(150, 60, 916, 560),
            Kind::WasmApp => Rect::new(240, 130, 720, 470),
            Kind::Files => Rect::new(220, 110, 860, 520),
            Kind::Viewer => Rect::new(200, 90, 820, 540),
            Kind::Gallery => Rect::new(160, 70, 900, 600),
            Kind::Settings => Rect::new(220, 56, 820, 596),
            Kind::LogViewer => Rect::new(200, 80, 880, 540),
        }
    }

    /// Smallest size the window can be resized to.
    pub(crate) const fn min_size(self) -> (i32, i32) {
        match self {
            Kind::Terminal => (320, 180),
            Kind::Editor => (320, 200),
            Kind::TaskMgr => (700, 460),
            Kind::Calculator => (280, 440),
            Kind::Browser => (420, 260),
            Kind::WasmApp => (720, 470),
            Kind::Files => (640, 340),
            Kind::Viewer => (520, 340),
            Kind::Gallery => (560, 380),
            Kind::Settings => (720, 480),
            Kind::LogViewer => (720, 360),
        }
    }

    /// Whether the window can be resized / maximized (WASM windows decide per app,
    /// from the manifest: see `Desktop::open_wasm_app`).
    pub(crate) const fn resizable(self) -> bool {
        true
    }
}

/// Title-bar text of instance `index` of `kind` (`KITSUNE SHELL`, `KITSUNE SHELL 2`...).
pub(crate) fn base_title(kind: Kind, index: u8) -> String {
    base_title_in(kitsune_core::i18n::lang(), kind, index)
}

/// [`base_title`] in language `l`.
pub(crate) fn base_title_in(l: Lang, kind: Kind, index: u8) -> String {
    let mut title = String::from(kind.title_in(l));
    if index > 1 {
        // The same " N" suffix as the process name.
        let mut tmp = [0u8; 16];
        let k = numbered_name("", index, &mut tmp);
        title.push_str(core::str::from_utf8(&tmp[..k]).unwrap_or(""));
    }
    title
}

/// Per-window app state.
pub(crate) enum App {
    Terminal(Box<TermState>),
    Editor(Box<EditorState>),
    Tarefas(Box<TarefasState>),
    Calculator(Box<apps::calculadora::CalcState>),
    Browser(Box<BrowserState>),
    Wasm(Box<WasmWin>),
    Files(Box<FilesState>),
    Viewer(Box<ViewerState>),
    Settings(Box<SettingsState>),
    Log(Box<LogState>),
    Gallery(Box<apps::gallery::GalleryState>),
}

impl App {
    /// Fresh state for a new window of `kind`.
    pub(crate) fn new(kind: Kind) -> App {
        match kind {
            Kind::Terminal => App::Terminal(Box::new(TermState::new())),
            Kind::Editor => App::Editor(Box::new(EditorState::new())),
            Kind::TaskMgr => App::Tarefas(Box::new(TarefasState::new(0))),
            Kind::Calculator => App::Calculator(Box::new(apps::calculadora::CalcState::new())),
            Kind::Browser => App::Browser(Box::new(BrowserState::new())),
            Kind::WasmApp => App::Wasm(Box::new(WasmWin {
                id: 0,
                app_id: String::new(),
            })),
            Kind::Files => App::Files(Box::new(FilesState::new())),
            Kind::Viewer => App::Viewer(Box::new(ViewerState::new())),
            Kind::Settings => App::Settings(Box::new(SettingsState::new())),
            Kind::LogViewer => App::Log(Box::new(LogState::new())),
            Kind::Gallery => App::Gallery(Box::new(apps::gallery::GalleryState::new())),
        }
    }

    pub(crate) fn kind(&self) -> Kind {
        match self {
            App::Terminal(_) => Kind::Terminal,
            App::Editor(_) => Kind::Editor,
            App::Tarefas(_) => Kind::TaskMgr,
            App::Calculator(_) => Kind::Calculator,
            App::Browser(_) => Kind::Browser,
            App::Wasm(_) => Kind::WasmApp,
            App::Files(_) => Kind::Files,
            App::Settings(_) => Kind::Settings,
            App::Log(_) => Kind::LogViewer,
            App::Viewer(_) => Kind::Viewer,
            App::Gallery(_) => Kind::Gallery,
        }
    }
}

/// What a window record carries: the app, its process, and its identity.
pub(crate) struct Inst {
    pub app: App,
    /// Process-table id (`0` = none, if the table was full).
    pub pid: u16,
    /// 1-based instance number within its kind (`shell` = 1, `shell 2` = 2...).
    pub index: u8,
    pub title: String,
    /// TSC cycles spent drawing this window since the last once-a-second sample
    /// (Tarefas' per-app figure).
    pub cost: core::cell::Cell<u64>,
    /// Draw cost of the last full second, in tenths of a percent of wall time.
    pub cost_pm: core::cell::Cell<u16>,
}

impl Inst {
    pub(crate) fn kind(&self) -> Kind {
        self.app.kind()
    }
}

/// A window record of this desktop.
pub(crate) type Win = kitsune_core::winman::Window<Inst>;

pub(crate) use kitsune_core::winman::numbered_name;
