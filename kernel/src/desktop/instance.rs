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
//! 4. Give it a dock slot: `osjeff_core::layout::DOCK_COUNT` and the icon list in
//!    `widgets::paint_background`.
//!
//! Instance state is plain data; nothing here allocates per frame. The
//! per-instance heap objects (`Box`, `Vec`, `String`) are created when the window
//! opens and freed when it is destroyed.

use super::*;
use alloc::boxed::Box;

/// Which app a window runs. The order is the dock / start-panel / menu order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Terminal,
    Editor,
    TaskMgr,
    Calculator,
    Browser,
    WasmApp,
    Files,
}

impl Kind {
    pub(crate) const ALL: [Kind; 7] = [
        Kind::Terminal,
        Kind::Editor,
        Kind::TaskMgr,
        Kind::Calculator,
        Kind::Browser,
        Kind::WasmApp,
        Kind::Files,
    ];

    /// Position in [`Kind::ALL`] (dock slot is this + 1: slot 0 is the system icon).
    pub(crate) fn index(self) -> usize {
        Kind::ALL.iter().position(|&k| k == self).unwrap_or(0)
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
        }
    }

    /// Title-bar text of the first instance.
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Kind::Terminal => "OSJEFF SHELL",
            Kind::Editor => "OSJEFF EDIT",
            Kind::TaskMgr => "TASK MANAGER",
            Kind::Calculator => "CALCULATOR",
            Kind::Browser => "NAVEGADOR",
            Kind::WasmApp => "WASM APP",
            Kind::Files => "ARQUIVOS",
        }
    }

    /// Label in the context menu and the start panel.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Kind::Terminal => "Terminal",
            Kind::Editor => "Editor",
            Kind::TaskMgr => "Task Manager",
            Kind::Calculator => "Calculator",
            Kind::Browser => "Navegador",
            Kind::WasmApp => "WASM App",
            Kind::Files => "Arquivos",
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
        }
    }

    /// May several windows of this kind be open at once? The task manager and the
    /// browser (one NIC, one fetcher thread) are single-instance for now: launching
    /// them again focuses the existing window. WASM apps are multi-instance: each
    /// window is its own `AppManager` instance.
    pub(crate) const fn multi(self) -> bool {
        matches!(
            self,
            Kind::Terminal | Kind::Editor | Kind::Calculator | Kind::Files | Kind::WasmApp
        )
    }

    /// Position and size of the first instance (later ones cascade from it).
    pub(crate) const fn default_rect(self) -> Rect {
        match self {
            Kind::Terminal => Rect::new(70, 80, 512, 320),
            Kind::Editor => Rect::new(610, 110, 560, 350),
            Kind::TaskMgr => Rect::new(360, 200, 392, 300),
            Kind::Calculator => Rect::new(470, 150, 300, 420),
            Kind::Browser => Rect::new(150, 60, 916, 560),
            Kind::WasmApp => Rect::new(240, 130, 720, 470),
            Kind::Files => Rect::new(250, 120, 780, 520),
        }
    }

    /// Smallest size the window can be resized to.
    pub(crate) const fn min_size(self) -> (i32, i32) {
        match self {
            Kind::Terminal => (320, 180),
            Kind::Editor => (320, 200),
            Kind::TaskMgr => (360, 260),
            Kind::Calculator => (280, 360),
            Kind::Browser => (420, 260),
            Kind::WasmApp => (720, 470),
            Kind::Files => (440, 260),
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
    pub page: Option<osjeff_core::web::Page>,
    pub scroll: i32,
    /// Cleaned response body the page was rendered from (empty when none).
    pub body: Vec<u8>,
    /// Viewport width `page` was laid out for.
    pub layout_w: i32,
}

/// An editor window: the buffer and the file it was opened from / saves to.
pub(crate) struct EditorState {
    pub editor: Editor,
    pub file: FileName,
    /// Directory slot holding `file` (`fs::ROOT` at the top level), so Ctrl+S
    /// rewrites the file that was opened rather than a same-named one in the root.
    pub dir: u8,
}

/// A file-manager window: selected row, view (0 = files, 1 = trash, 2/3 = disk
/// panels, 4 = installed apps) and the current directory slot.
#[derive(Clone, Copy)]
pub(crate) struct FilesState {
    pub sel: usize,
    pub view: u8,
    pub cwd: u8,
}

/// A WASM app window: the `AppManager` instance behind it and which package it is.
pub(crate) struct WasmWin {
    /// Handle in the manager (`0` = none).
    pub id: crate::wasm::AppId,
    /// Manifest id of the package (`snake`, `notes`, ...).
    pub app_id: String,
}

/// Per-window app state.
pub(crate) enum App {
    Terminal(Box<Terminal>),
    Editor(Box<EditorState>),
    TaskMgr,
    Calculator(Box<Calc>),
    Browser(Box<BrowserState>),
    Wasm(Box<WasmWin>),
    Files(FilesState),
}

impl App {
    /// Fresh state for a new window of `kind`.
    pub(crate) fn new(kind: Kind) -> App {
        match kind {
            Kind::Terminal => App::Terminal(Box::new(Terminal::new())),
            Kind::Editor => App::Editor(Box::new(EditorState {
                editor: Editor::new(),
                // `notes.txt` is a valid name (non-empty, <= FNAME_MAX).
                file: FileName::parse(b"notes.txt").unwrap(),
                dir: fs::ROOT,
            })),
            Kind::TaskMgr => App::TaskMgr,
            Kind::Calculator => App::Calculator(Box::new(Calc::new())),
            Kind::Browser => App::Browser(Box::new(BrowserState {
                browser: osjeff_core::Browser::new(),
                page: None,
                scroll: 0,
                body: Vec::new(),
                layout_w: 0,
            })),
            Kind::WasmApp => App::Wasm(Box::new(WasmWin {
                id: 0,
                app_id: String::new(),
            })),
            Kind::Files => App::Files(FilesState {
                sel: 0,
                view: 0,
                cwd: fs::ROOT,
            }),
        }
    }

    pub(crate) fn kind(&self) -> Kind {
        match self {
            App::Terminal(_) => Kind::Terminal,
            App::Editor(_) => Kind::Editor,
            App::TaskMgr => Kind::TaskMgr,
            App::Calculator(_) => Kind::Calculator,
            App::Browser(_) => Kind::Browser,
            App::Wasm(_) => Kind::WasmApp,
            App::Files(_) => Kind::Files,
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
}

impl Inst {
    pub(crate) fn kind(&self) -> Kind {
        self.app.kind()
    }
}

/// A window record of this desktop.
pub(crate) type Win = osjeff_core::winman::Window<Inst>;

pub(crate) use osjeff_core::winman::numbered_name;

// ---------------------------------------------------------------- file manager

impl FilesState {
    /// The effective current directory: `cwd` if it is still a live folder, else
    /// fall back to the root (e.g. after the folder was trashed).
    pub(crate) fn cwd(&self) -> u8 {
        let c = self.cwd;
        if c == fs::ROOT {
            return fs::ROOT;
        }
        let img = disk();
        if fs::is_active(img, c as usize) && fs::is_dir(img, c as usize) {
            c
        } else {
            fs::ROOT
        }
    }

    /// Rows shown in the current view. Views: 0 = files (within the current
    /// directory), 1 = trash, 2/3 = the boot / filesystem disk panels (no rows).
    pub(crate) fn rows(&self) -> usize {
        let img = disk();
        match self.view {
            0 => {
                let cwd = self.cwd();
                (0..fs::MAX_FILES)
                    .filter(|&i| fs::is_active(img, i) && fs::parent_at(img, i) == cwd)
                    .count()
            }
            1 => fs::count_trashed(img),
            _ => 0,
        }
    }

    /// The filesystem slot backing visible row `n` of the current view.
    pub(crate) fn slot(&self, n: usize) -> Option<usize> {
        let img = disk();
        match self.view {
            0 => {
                let cwd = self.cwd();
                (0..fs::MAX_FILES)
                    .filter(|&i| fs::is_active(img, i) && fs::parent_at(img, i) == cwd)
                    .nth(n)
            }
            1 => (0..fs::MAX_FILES)
                .filter(|&i| fs::is_trashed(img, i))
                .nth(n),
            _ => None,
        }
    }

    /// Switch the view from a sidebar selection (0..=3).
    pub(crate) fn set_view(&mut self, v: u8) {
        self.view = v.min(4);
        self.sel = 0;
    }

    pub(crate) fn move_sel(&mut self, delta: i32) {
        let rows = self.rows();
        if rows == 0 {
            self.sel = 0;
            return;
        }
        let cur = self.sel.min(rows - 1) as i32;
        self.sel = (cur + delta).clamp(0, rows as i32 - 1) as usize;
    }

    /// Cycle through the sidebar views (Files -> Trash -> boot -> FS -> ...).
    pub(crate) fn toggle_view(&mut self) {
        self.view = (self.view + 1) % 5;
        self.sel = 0;
    }

    /// Delete: in Files view, move the selection (a folder takes its contents) to
    /// the trash; in Trash view, delete it permanently. Recursive for folders.
    pub(crate) fn delete(&mut self) {
        let Some(slot) = self.slot(self.sel) else {
            return;
        };
        if self.view == 0 {
            fs::trash_slot(disk(), slot);
        } else {
            fs::purge_slot(disk(), slot);
        }
        flush_disk();
        self.move_sel(0);
    }

    /// Go to the parent directory (Files view only).
    pub(crate) fn up(&mut self) {
        if self.view == 0 && self.cwd != fs::ROOT {
            self.cwd = fs::parent_at(disk(), self.cwd as usize);
            self.sel = 0;
        }
    }

    /// Create a new auto-named folder in the current directory (Files view).
    pub(crate) fn mkdir(&mut self) {
        if self.view != 0 {
            return;
        }
        let cwd = self.cwd();
        let base: &[u8] = b"nova pasta";
        let mut name = [0u8; fs::MAX_NAME];
        for n in 1..=9u8 {
            let len = if n == 1 {
                name[..base.len()].copy_from_slice(base);
                base.len()
            } else {
                name[..base.len()].copy_from_slice(base);
                name[base.len()] = b' ';
                name[base.len() + 1] = b'0' + n;
                base.len() + 2
            };
            if fs::find_in(disk(), cwd, &name[..len]).is_none() {
                let _ = fs::mkdir(disk(), cwd, &name[..len]);
                flush_disk();
                break;
            }
        }
        self.move_sel(0);
    }

    /// Select row `row` (for clicks); ignored past the last row.
    pub(crate) fn select_at(&mut self, row: usize) {
        if row < self.rows() {
            self.sel = row;
        }
    }

    /// Enter / primary action on the selection. Trash view: restore; a folder:
    /// open it. For a file returns its slot so the desktop can open it in an
    /// editor window (it owns the window table).
    pub(crate) fn primary(&mut self) -> Option<usize> {
        let slot = self.slot(self.sel)?;
        if self.view == 1 {
            fs::restore_slot(disk(), slot);
            flush_disk();
            self.move_sel(0);
            return None;
        }
        if fs::is_dir(disk(), slot) {
            self.cwd = slot as u8;
            self.sel = 0;
            return None;
        }
        Some(slot)
    }
}
