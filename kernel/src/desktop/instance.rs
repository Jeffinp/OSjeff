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
//! 4. Give it an app-bar slot (`shell::DOCK_ITEMS`) if it should live there; every app
//!    appears in the Apps overlay and in Busca on its own.
//!
//! Instance state is plain data; nothing here allocates per frame. The
//! per-instance heap objects (`Box`, `Vec`, `String`) are created when the window
//! opens and freed when it is destroyed.

use super::*;
use alloc::boxed::Box;

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

    /// Title-bar text of the first instance.
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Kind::Terminal => "Terminal",
            Kind::Editor => "Editor",
            Kind::TaskMgr => "Tarefas",
            Kind::Calculator => "Calculadora",
            Kind::Browser => "Navegador",
            Kind::WasmApp => "Aplicativo",
            Kind::Files => "Arquivos",
            Kind::Settings => "Configurações",
            Kind::LogViewer => "Registro",
            Kind::Viewer => "Imagens",
            Kind::Gallery => "Componentes",
        }
    }

    /// Name in menus, the app bar's tooltips, the Apps overlay and Busca.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Kind::Terminal => "Terminal",
            Kind::Editor => "Editor",
            Kind::TaskMgr => "Tarefas",
            Kind::Calculator => "Calculadora",
            Kind::Browser => "Navegador",
            Kind::WasmApp => "Aplicativos",
            Kind::Files => "Arquivos",
            Kind::Settings => "Configurações",
            Kind::LogViewer => "Registro",
            Kind::Viewer => "Imagens",
            Kind::Gallery => "Componentes",
        }
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
            Kind::Gallery => Icon::Settings,
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
            Kind::Calculator => Rect::new(470, 150, 300, 420),
            Kind::Browser => Rect::new(150, 60, 916, 560),
            Kind::WasmApp => Rect::new(240, 130, 720, 470),
            Kind::Files => Rect::new(220, 110, 860, 520),
            Kind::Viewer => Rect::new(200, 90, 820, 540),
            Kind::Gallery => Rect::new(160, 70, 900, 600),
            Kind::Settings => Rect::new(220, 84, 820, 560),
            Kind::LogViewer => Rect::new(200, 80, 880, 540),
        }
    }

    /// Smallest size the window can be resized to.
    pub(crate) const fn min_size(self) -> (i32, i32) {
        match self {
            Kind::Terminal => (320, 180),
            Kind::Editor => (320, 200),
            Kind::TaskMgr => (700, 460),
            Kind::Calculator => (280, 360),
            Kind::Browser => (420, 260),
            Kind::WasmApp => (720, 470),
            Kind::Files => (580, 320),
            Kind::Viewer => (360, 260),
            Kind::Gallery => (560, 380),
            Kind::Settings => (700, 460),
            Kind::LogViewer => (720, 360),
        }
    }

    /// Whether the window can be resized / maximized (WASM windows decide per app,
    /// from the manifest: see `Desktop::open_wasm_app`).
    pub(crate) const fn resizable(self) -> bool {
        true
    }
}

/// Browser window state: the engine, the rendered page and its scroll, plus the
/// HTML body so the page can be laid out again when the window is resized.
pub(crate) struct BrowserState {
    pub browser: osjeff_core::Browser,
    /// The parsed document (kept so the page can be laid out again without parsing).
    pub doc: Option<osjeff_core::web::Doc>,
    pub page: Option<osjeff_core::web::Page>,
    pub scroll: i32,
    /// Viewport width `page` was laid out for.
    pub layout_w: i32,
    /// Pictures of the page being shown (and a few recent ones).
    pub images: osjeff_core::web::imgcache::ImageCache,
    /// Cache key of each `page.images` entry (`None`: not fetchable).
    pub img_keys: Vec<Option<String>>,
    /// The picture the fetcher is working on.
    pub img_inflight: Option<String>,
    /// Page zoom in percent.
    pub zoom: u16,
    /// What the user typed into the page's form controls.
    pub forms: osjeff_core::web::form::FormState,
    /// Ctrl+F bar and matches.
    pub find: osjeff_core::web::find::FindBar,
    /// Start of a mouse selection (page coordinates) and the selected word range.
    pub sel_anchor: Option<(i32, i32)>,
    pub sel: Option<(usize, usize)>,
    /// A one-line message over the bottom of the page (cleared by the next key or click).
    pub notice: Option<String>,
}

/// The single place that decides where the browser's favourites live: the file
/// `/home/.bookmarks` on the desktop volume (written after every change; a missing or
/// damaged file starts an empty list).
fn new_bookmark_store() -> Box<dyn osjeff_core::browser::BookmarkStore> {
    const PATH: &[u8] = b"/home/.bookmarks";
    let text = super::vfs::read_file(PATH).unwrap_or_default();
    Box::new(osjeff_core::browser::SavedBookmarks::load(&text, |t| {
        if !super::vfs::exists(b"/home") {
            let _ = super::vfs::mkdir(b"/home");
        }
        if super::vfs::write_file(PATH, t).is_err() {
            crate::klog!(Warn, "bookmarks: could not be saved");
        }
    }))
}

/// What the inline name field of a file manager is for.
pub(crate) enum EditPurpose {
    NewFile,
    NewFolder,
    /// Renaming the item at this path.
    Rename(Vec<u8>),
}

/// The inline name editor (new file / new folder / rename).
pub(crate) struct NameEdit {
    pub input: osjeff_core::fileman::TextInput,
    pub purpose: EditPurpose,
}

/// A question the file manager waits on (Enter confirms, Esc cancels).
pub(crate) enum Confirm {
    /// Delete these paths for good.
    Purge(Vec<Vec<u8>>),
    /// Delete these trash items for good (by trash id).
    PurgeTrash(Vec<Vec<u8>>),
    EmptyTrash,
}

/// An open context menu of a file manager (screen coordinates).
pub(crate) struct CtxMenu {
    pub x: i32,
    pub y: i32,
    pub items: Vec<(osjeff_core::fileman::Cmd, &'static str)>,
}

/// A copy running in steps (see `Desktop::step_file_jobs`).
pub(crate) struct Job {
    pub copy: vfs::CopyJob,
    pub label: &'static str,
}

/// A file-manager window.
pub(crate) struct FilesState {
    pub view: osjeff_core::fileman::FileView,
    pub input: Option<NameEdit>,
    pub confirm: Option<Confirm>,
    /// Lines of the properties panel while it is open.
    pub props: Option<Vec<String>>,
    pub menu: Option<CtxMenu>,
    pub job: Option<Job>,
    /// Last status message and whether it is an error.
    pub msg: Option<(String, bool)>,
    pub usage: vfs::Usage,
}

impl FilesState {
    pub(crate) fn new() -> Self {
        FilesState {
            view: osjeff_core::fileman::FileView::new(),
            input: None,
            confirm: None,
            props: None,
            menu: None,
            job: None,
            msg: None,
            usage: vfs::Usage::default(),
        }
    }

    pub(crate) fn say(&mut self, text: &str, error: bool) {
        self.msg = Some((String::from(text), error));
    }

    /// True while a modal element (name field, question, properties) owns the keys.
    pub(crate) fn modal(&self) -> bool {
        self.input.is_some() || self.confirm.is_some() || self.props.is_some()
    }
}

/// An image-viewer window.
pub(crate) struct ViewerState {
    pub path: Vec<u8>,
    pub image: Option<osjeff_core::image::Image>,
    /// Box-filtered copy for zooms below 100 %: `(zoom, image)`.
    pub scaled: Option<(u32, osjeff_core::image::Image)>,
    pub opaque: bool,
    pub view: osjeff_core::viewer::View,
    pub list: osjeff_core::viewer::ImageList,
    pub format: Option<osjeff_core::image::Format>,
    pub file_bytes: u64,
    /// Why the file could not be shown.
    pub error: Option<[String; 2]>,
    pub show_info: bool,
    /// The "save as" prompt.
    pub save: Option<osjeff_core::fileman::TextInput>,
    pub msg: Option<(String, bool)>,
}

impl ViewerState {
    pub(crate) fn new() -> Self {
        ViewerState {
            path: Vec::new(),
            image: None,
            scaled: None,
            opaque: true,
            view: osjeff_core::viewer::View::default(),
            list: osjeff_core::viewer::ImageList::default(),
            format: None,
            file_bytes: 0,
            error: None,
            show_info: false,
            save: None,
            msg: None,
        }
    }
}

/// A WASM app window: the `AppManager` instance behind it and which package it is.
pub(crate) struct WasmWin {
    /// Handle in the manager (`0` = none).
    pub id: crate::wasm::AppId,
    /// Manifest id of the package (`snake`, `notes`, ...).
    pub app_id: String,
}

/// Title-bar text of instance `index` of `kind` (`OSJEFF SHELL`, `OSJEFF SHELL 2`...).
pub(crate) fn base_title(kind: Kind, index: u8) -> String {
    let mut title = String::from(kind.title());
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
    Calculator(Box<Calc>),
    Browser(Box<BrowserState>),
    Wasm(Box<WasmWin>),
    Files(Box<FilesState>),
    Viewer(Box<ViewerState>),
    Settings(Box<SettingsState>),
    Log(Box<LogState>),
    Gallery(Box<gallery::GalleryState>),
}

impl App {
    /// Fresh state for a new window of `kind`.
    pub(crate) fn new(kind: Kind) -> App {
        match kind {
            Kind::Terminal => App::Terminal(Box::new(TermState::new())),
            Kind::Editor => App::Editor(Box::new(EditorState::new())),
            Kind::TaskMgr => App::Tarefas(Box::new(TarefasState::new(0))),
            Kind::Calculator => App::Calculator(Box::new(Calc::new())),
            Kind::Browser => App::Browser(Box::new(BrowserState {
                browser: osjeff_core::Browser::with_store(new_bookmark_store()),
                doc: None,
                page: None,
                scroll: 0,
                layout_w: 0,
                images: osjeff_core::web::imgcache::ImageCache::new(),
                img_keys: Vec::new(),
                img_inflight: None,
                zoom: 100,
                forms: osjeff_core::web::form::FormState::default(),
                find: osjeff_core::web::find::FindBar::new(),
                sel_anchor: None,
                sel: None,
                notice: None,
            })),
            Kind::WasmApp => App::Wasm(Box::new(WasmWin {
                id: 0,
                app_id: String::new(),
            })),
            Kind::Files => App::Files(Box::new(FilesState::new())),
            Kind::Viewer => App::Viewer(Box::new(ViewerState::new())),
            Kind::Settings => App::Settings(Box::new(SettingsState::new())),
            Kind::LogViewer => App::Log(Box::new(LogState::new())),
            Kind::Gallery => App::Gallery(Box::new(gallery::GalleryState::new())),
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
pub(crate) type Win = osjeff_core::winman::Window<Inst>;

pub(crate) use osjeff_core::winman::numbered_name;
