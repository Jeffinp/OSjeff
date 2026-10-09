//! Window lifecycle: opening a window of a kind with its process, launching (focus or open),
//! finding windows, closing and the typed accessors of the per-window app state.

use crate::desktop::*;

impl Desktop {
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
            kitsune_core::winman::cascade_rect(
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
        kitsune_core::layout::work_area(self.sw, self.sh)
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
}
