//! view (split out of `fileman.rs`).

use super::*;

/// What activating (Enter, double click) a row asks the desktop to do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Activation {
    /// Nothing happened (empty list, trash item).
    None,
    /// A folder: the view navigated into it.
    Entered,
    /// Open this file.
    Open(Vec<u8>, FileClass),
    /// Apps place: run this app (installing the bundled package first when it is not
    /// installed yet).
    App { id: Vec<u8>, installed: bool },
}

/// One file-manager window's state.
pub struct FileView {
    /// `/`, a folder path, [`TRASH_PATH`] or [`APPS_PATH`].
    pub cwd: Vec<u8>,
    /// The rows shown: the folder's, filtered by the search and sorted.
    pub rows: Vec<Row>,
    /// While a search filter is active: every row of the place (sorted), of which `rows` is
    /// the part that matches. Empty otherwise, so an unfiltered view holds its rows once.
    all: Vec<Row>,
    filter: Vec<u8>,
    pub sort: Sort,
    pub sel: Selection,
    pub history: History,
    /// More rows existed than [`MAX_ROWS`].
    pub truncated: bool,
    /// Bumped each time the view moves to another place, so the window can reset its scroll.
    pub nav_gen: u32,
}

impl Default for FileView {
    fn default() -> Self {
        Self::new()
    }
}

impl FileView {
    /// A view at the root (call [`refresh`](Self::refresh) to fill it).
    pub fn new() -> Self {
        Self::at(b"/")
    }

    /// A view at `path` (call [`refresh`](Self::refresh) to fill it).
    pub fn at(path: &[u8]) -> Self {
        FileView {
            cwd: path.to_vec(),
            rows: Vec::new(),
            all: Vec::new(),
            filter: Vec::new(),
            sort: Sort::DEFAULT,
            sel: Selection::new(),
            history: History::new(path),
            truncated: false,
            nav_gen: 0,
        }
    }

    pub fn in_trash(&self) -> bool {
        self.cwd == TRASH_PATH
    }

    /// In the Apps place (rows come from [`set_apps`](Self::set_apps), not a folder).
    pub fn in_apps(&self) -> bool {
        self.cwd == APPS_PATH
    }

    /// The search text (empty when not searching).
    pub fn filter(&self) -> &[u8] {
        &self.filter
    }

    /// How many rows the place has before the search filter.
    pub fn total_rows(&self) -> usize {
        if self.filter.is_empty() {
            self.rows.len()
        } else {
            self.all.len()
        }
    }

    /// Show only the rows whose name matches `query` (accent and case insensitive); an empty
    /// query shows all. The selection follows the rows by name.
    pub fn set_filter(&mut self, query: &[u8]) {
        if query == &self.filter[..] {
            return;
        }
        let keep = self.selected_names();
        let cursor = self.rows.get(self.sel.cursor()).map(|r| r.name.clone());
        // Back to the full list first, then filter it afresh.
        if !self.filter.is_empty() {
            self.rows = core::mem::take(&mut self.all);
        }
        self.filter = query.to_vec();
        self.apply_filter();
        self.sel.restore(&self.rows, &keep, cursor.as_deref());
    }

    /// Drop the search filter (the rows come back).
    pub fn clear_filter(&mut self) {
        self.set_filter(b"");
    }

    /// Filter `rows` (which hold the full list) in place, parking the full list in `all`.
    pub(super) fn apply_filter(&mut self) {
        if self.filter.is_empty() {
            return;
        }
        let key = ui::query_key(&self.filter);
        let shown: Vec<Row> = self
            .rows
            .iter()
            .filter(|r| ui::matches_key(&r.name, &key))
            .cloned()
            .collect();
        self.all = core::mem::replace(&mut self.rows, shown);
    }

    pub(super) fn selected_names(&self) -> Vec<Vec<u8>> {
        self.sel
            .selected()
            .into_iter()
            .filter_map(|i| self.rows.get(i).map(|r| r.name.clone()))
            .collect()
    }

    /// Replace the place's rows with `rows` (sorted here), keeping selection by name.
    pub(super) fn load(&mut self, mut rows: Vec<Row>, keep: &[Vec<u8>], cursor: Option<&[u8]>) {
        self.truncated = rows.len() > MAX_ROWS;
        rows.truncate(MAX_ROWS);
        sort_rows(&mut rows, self.sort);
        self.all.clear();
        self.rows = rows;
        self.apply_filter();
        self.sel.restore(&self.rows, keep, cursor);
    }

    /// Reload the rows of the current place from `b`, keeping the selection by name.
    /// A folder that no longer exists moves the view to the closest existing parent.
    pub fn refresh<B: Backend + ?Sized>(&mut self, b: &mut B) -> Result<(), VfsError> {
        if self.in_apps() {
            // The app list is not on the volume: whoever owns the catalog refills it
            // with `set_apps`; the app rows already shown stay until then. Rows left
            // over from the folder we came from (app rows always have an id) go.
            if self.rows.iter().any(|r| r.id.is_empty()) {
                self.rows.clear();
                self.all.clear();
                self.sel.reset(0);
            }
            return Ok(());
        }
        let keep = self.selected_names();
        let cursor = self.rows.get(self.sel.cursor()).map(|r| r.name.clone());
        let rows: Vec<Row> = if self.in_trash() {
            b.trash_list()?
                .into_iter()
                .map(|t| Row {
                    name: t.name,
                    kind: t.kind,
                    size: t.size,
                    mtime: t.deleted_at,
                    id: t.id,
                    installed: false,
                })
                .collect()
        } else {
            loop {
                match b.readdir(&self.cwd) {
                    Ok(list) => break list.into_iter().map(Row::from).collect(),
                    Err(VfsError::NotFound | VfsError::NotDir) if !vfs::is_root(&self.cwd) => {
                        let up = vfs::parent(&self.cwd);
                        self.cwd = up;
                        self.history.replace_current(&self.cwd);
                        self.nav_gen = self.nav_gen.wrapping_add(1);
                    }
                    Err(e) => return Err(e),
                }
            }
        };
        self.load(rows, &keep, cursor.as_deref());
        Ok(())
    }

    /// Go to `path` (a folder or the trash), pushing history; the view is reloaded.
    /// On failure the view stays where it was.
    pub fn navigate<B: Backend + ?Sized>(
        &mut self,
        b: &mut B,
        path: &[u8],
    ) -> Result<(), VfsError> {
        if path != TRASH_PATH && path != APPS_PATH {
            match b.stat(path)? {
                i if i.kind == EntryKind::Dir => {}
                _ => return Err(VfsError::NotDir),
            }
        }
        let old = core::mem::replace(&mut self.cwd, path.to_vec());
        let old_filter = core::mem::take(&mut self.filter);
        let old_all = core::mem::take(&mut self.all);
        self.sel.reset(0);
        if let Err(e) = self.refresh(b) {
            self.cwd = old;
            self.filter = old_filter;
            self.all = old_all;
            let _ = self.refresh(b);
            return Err(e);
        }
        self.nav_gen = self.nav_gen.wrapping_add(1);
        self.history.push(&self.cwd);
        self.select_first();
        Ok(())
    }

    /// Fill the Apps place from the catalog (sorted by the current sort, the rows are
    /// [`apps::rows`]). The cursor stays on the same app (by id); if it is gone, or
    /// nothing was selected, it goes to the first row.
    pub fn set_apps(&mut self, items: &[apps::AppItem]) {
        let cursor_id = self.rows.get(self.sel.cursor()).map(|r| r.id.clone());
        let had_selection = self.sel.count() > 0;
        let was_empty = self.rows.is_empty();
        let rows = apps::rows(items);
        self.truncated = false;
        self.all.clear();
        self.rows = rows;
        sort_rows(&mut self.rows, self.sort);
        self.apply_filter();
        self.sel.reset(self.rows.len());
        if had_selection || was_empty {
            let at = cursor_id
                .and_then(|id| self.rows.iter().position(|r| r.id == id))
                .unwrap_or(0);
            if !self.rows.is_empty() {
                self.sel.only(at);
            }
        }
    }

    /// Put the cursor (and the selection) on the first row, if there is one.
    pub fn select_first(&mut self) {
        if !self.rows.is_empty() {
            self.sel.only(0);
        }
    }

    /// The parent folder (from the trash: the root).
    pub fn go_up<B: Backend + ?Sized>(&mut self, b: &mut B) -> Result<(), VfsError> {
        let up = if self.in_trash() || self.in_apps() {
            b"/".to_vec()
        } else {
            vfs::parent(&self.cwd)
        };
        if up == self.cwd {
            return Ok(());
        }
        self.navigate(b, &up)
    }

    /// History back.
    pub fn go_back<B: Backend + ?Sized>(&mut self, b: &mut B) -> Result<(), VfsError> {
        match self.history.back().map(<[u8]>::to_vec) {
            Some(p) => self.go_to_history(b, &p),
            None => Ok(()),
        }
    }

    /// History forward.
    pub fn go_forward<B: Backend + ?Sized>(&mut self, b: &mut B) -> Result<(), VfsError> {
        match self.history.forward().map(<[u8]>::to_vec) {
            Some(p) => self.go_to_history(b, &p),
            None => Ok(()),
        }
    }

    pub(super) fn go_to_history<B: Backend + ?Sized>(
        &mut self,
        b: &mut B,
        p: &[u8],
    ) -> Result<(), VfsError> {
        self.cwd = p.to_vec();
        self.filter.clear();
        self.all.clear();
        self.nav_gen = self.nav_gen.wrapping_add(1);
        self.sel.reset(0);
        let r = self.refresh(b);
        self.select_first();
        r
    }

    /// Change the sort column (header click) keeping the selection.
    pub fn click_header(&mut self, key: SortKey) {
        self.sort.click(key);
        self.resort();
    }

    /// Sort by `key`: the same key as now flips nothing (menu entries pick, headers toggle).
    pub fn set_sort(&mut self, key: SortKey) {
        if self.sort.key != key {
            self.sort = Sort { key, asc: true };
            self.resort();
        }
    }

    pub(super) fn resort(&mut self) {
        let keep = self.selected_names();
        let cursor = self.rows.get(self.sel.cursor()).map(|r| r.name.clone());
        if self.filter.is_empty() {
            sort_rows(&mut self.rows, self.sort);
        } else {
            sort_rows(&mut self.all, self.sort);
            sort_rows(&mut self.rows, self.sort);
        }
        self.sel.restore(&self.rows, &keep, cursor.as_deref());
    }

    /// Absolute path of row `i` (folder views only; the trash has none).
    pub fn path_of(&self, i: usize) -> Option<Vec<u8>> {
        if self.in_trash() || self.in_apps() {
            return None;
        }
        self.rows.get(i).map(|r| vfs::join(&self.cwd, &r.name))
    }

    /// Paths of the selected rows (empty in the trash).
    pub fn selected_paths(&self) -> Vec<Vec<u8>> {
        self.sel
            .selected()
            .into_iter()
            .filter_map(|i| self.path_of(i))
            .collect()
    }

    /// The selected rows.
    pub fn selected_rows(&self) -> Vec<&Row> {
        self.sel
            .selected()
            .into_iter()
            .filter_map(|i| self.rows.get(i))
            .collect()
    }

    /// Enter / double click on row `i`.
    pub fn activate<B: Backend + ?Sized>(&mut self, b: &mut B, i: usize) -> Activation {
        let Some(row) = self.rows.get(i) else {
            return Activation::None;
        };
        if self.in_trash() {
            return Activation::None;
        }
        if self.in_apps() {
            return Activation::App {
                id: row.id.clone(),
                installed: row.installed,
            };
        }
        let name = row.name.clone();
        let is_dir = row.is_dir();
        let path = vfs::join(&self.cwd, &name);
        if is_dir {
            return match self.navigate(b, &path) {
                Ok(()) => Activation::Entered,
                Err(_) => Activation::None,
            };
        }
        Activation::Open(path, classify(&name))
    }

    /// Select the row called `name`.
    pub fn select_name(&mut self, name: &[u8]) {
        if let Some(i) = self.rows.iter().position(|r| r.name == name) {
            self.sel.only(i);
        }
    }

    /// Select every row whose name is in `names` (after a paste: the new items), the
    /// cursor on the first of them. Does nothing if none match.
    pub fn select_names(&mut self, names: &[Vec<u8>]) {
        let idx: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| names.contains(&r.name))
            .map(|(i, _)| i)
            .collect();
        if idx.is_empty() {
            return;
        }
        self.sel.select_set(&idx);
    }

    /// Status text: item count, or selection count and size.
    pub fn summary(&self) -> String {
        let n = self.rows.len();
        let k = self.sel.count();
        if k == 0 {
            return crate::tp!("files.count", n);
        }
        let bytes: u64 = self
            .selected_rows()
            .iter()
            .filter(|r| !r.is_dir())
            .map(|r| r.size)
            .sum();
        crate::tp!("files.selected", k, size = crate::i18n::bytes(bytes))
    }
}
