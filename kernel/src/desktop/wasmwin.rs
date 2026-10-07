//! `Desktop` methods: WASM app windows, the installed-app catalog and the
//! install / remove actions.
//!
//! Every WASM window is an instance of the `AppManager` (`crate::wasm`); the
//! window record only keeps the instance handle ([`WasmWin`]). The catalog (what
//! the Start panel lists) is rebuilt from `/apps` whenever something is installed
//! or removed. See `docs/design/apps.md`.

use super::*;
use crate::serial_println;
use crate::wasm::{self, appfs_backend};
use alloc::boxed::Box;
use osjeff_core::appinstall;
use osjeff_core::appmanifest::{Abi, Manifest};

/// Pixels the window frame adds around a WASM app's content area.
pub(crate) const FRAME_W: i32 = 28;
pub(crate) const FRAME_H: i32 = TITLE_H + 26;

/// Content area of a WASM window of rect `r` (origin, size).
pub(crate) fn wasm_content(r: Rect) -> Rect {
    Rect::new(
        r.x + 14,
        r.y + TITLE_H + 12,
        (r.w - FRAME_W).max(1),
        (r.h - FRAME_H).max(1),
    )
}

/// An installed app as the launcher shows it.
pub(crate) struct AppEntry {
    pub id: String,
    pub name: String,
    /// 24x24 RGBA bytes, or `None` for the default icon.
    pub icon: Option<Vec<u8>>,
    pub manifest: Manifest,
}

/// What the Files "Apps" view lists: installed packages and bundled ones that are
/// not installed.
pub(crate) struct AppRow {
    pub id: String,
    pub name: String,
    pub installed: bool,
    /// Size of the package file in bytes.
    pub size: u64,
}

fn argb_to_rgba(px: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(px.len() * 4);
    for &p in px {
        let [a, r, g, b] = p.to_be_bytes();
        out.extend_from_slice(&[r, g, b, a]);
    }
    out
}

impl Desktop {
    // ---- catalog ----

    /// First-boot seeding of the bundled packages, then the catalog.
    pub(crate) fn init_apps(&mut self) {
        // Once per volume: a bundled app the user removed does not come back at the next boot.
        let n = appfs_backend::with(|fs| appinstall::seed_once(fs, wasm::BUNDLED)).unwrap_or(0);
        serial_println!("apps: {} bundled packages installed into /apps", n);
        self.refresh_catalog();
    }

    /// Rebuild the launcher catalog from `/apps`.
    pub(crate) fn refresh_catalog(&mut self) {
        let cat = appfs_backend::with(|fs| appinstall::load_catalog(fs)).unwrap_or_default();
        self.apps = cat
            .into_iter()
            .map(|c| AppEntry {
                id: c.manifest.id.clone(),
                name: c.manifest.name.clone(),
                icon: c.icon.as_deref().map(argb_to_rgba),
                manifest: c.manifest,
            })
            .collect();
        self.start_scroll = self.start_scroll.min(self.start_max_scroll());
        serial_println!("apps: catalog has {} apps", self.apps.len());
        // File managers showing the Apps place follow the catalog.
        self.refresh_apps_views();
    }

    /// Rows of the Files "Apps" view: installed apps first (name order), then the
    /// bundled packages that are not installed.
    pub(crate) fn app_rows(&self) -> Vec<AppRow> {
        // One lock round for all the package sizes.
        let sizes: Vec<u64> = appfs_backend::with(|fs| {
            self.apps
                .iter()
                .map(|a| {
                    fs.stat(&alloc::format!("{}/{}.wasm", appinstall::APPS_DIR, a.id))
                        .map_or(0, |s| s.size)
                })
                .collect()
        })
        .unwrap_or_default();
        let mut rows: Vec<AppRow> = self
            .apps
            .iter()
            .enumerate()
            .map(|(i, a)| AppRow {
                id: a.id.clone(),
                name: a.name.clone(),
                installed: true,
                size: sizes.get(i).copied().unwrap_or(0),
            })
            .collect();
        for pkg in wasm::BUNDLED {
            if let Ok(m) = appinstall::check(pkg)
                && !rows.iter().any(|r| r.id == m.id)
            {
                rows.push(AppRow {
                    id: m.id,
                    name: m.name,
                    installed: false,
                    size: pkg.len() as u64,
                });
            }
        }
        rows
    }

    /// The manifest of app `id` for Properties: the installed one, else the bundled
    /// package's.
    pub(crate) fn app_manifest(&self, id: &str) -> Option<Manifest> {
        if let Some(a) = self.apps.iter().find(|a| a.id == id) {
            return Some(a.manifest.clone());
        }
        wasm::BUNDLED
            .iter()
            .filter_map(|p| appinstall::check(p).ok())
            .find(|m| m.id == id)
    }

    /// Open a `.wasm` file of the file system (Files, Enter): validate it as a package,
    /// install it when it is new (the installer refuses a bad manifest or quota, see the
    /// serial log) and run it.
    pub(crate) fn open_wasm_path(&mut self, path: &[u8]) {
        let Ok(bytes) = vfs::read_file(path) else {
            return;
        };
        let manifest = match appinstall::check(&bytes) {
            Ok(m) => m,
            Err(e) => {
                serial_println!("apps: .wasm file refused: {}", e);
                return;
            }
        };
        let installed =
            appfs_backend::with(|fs| appinstall::is_installed(fs, &manifest.id)).unwrap_or(false);
        if !installed {
            match appfs_backend::try_with(|fs| appinstall::install(fs, &bytes)) {
                Ok(m) => serial_println!("apps: installed `{}` {} from a file", m.id, m.version),
                Err(e) => {
                    serial_println!("apps: install refused: {}", e);
                    return;
                }
            }
            self.refresh_catalog();
        }
        self.launch_wasm_app(&manifest.id);
    }

    /// Install the bundled package `id` (Files, `I`). `Err` carries the reason.
    pub(crate) fn install_bundled(&mut self, id: &str) -> Result<(), String> {
        let pkg = wasm::BUNDLED
            .iter()
            .find(|p| appinstall::check(p).is_ok_and(|m| m.id == id))
            .ok_or_else(|| String::from("pacote nao encontrado"))?;
        let r = appfs_backend::try_with(|fs| appinstall::install(fs, pkg));
        match r {
            Ok(m) => serial_println!("apps: installed `{}` {}", m.id, m.version),
            Err(e) => {
                serial_println!("apps: install of `{}` refused: {}", id, e);
                return Err(alloc::format!("{e}"));
            }
        }
        self.refresh_catalog();
        Ok(())
    }

    /// Remove an installed app (Files, `Del`). Its open windows keep running until closed.
    pub(crate) fn remove_app(&mut self, id: &str) -> Result<(), String> {
        let r = appfs_backend::try_with(|fs| appinstall::remove(fs, id));
        match r {
            Ok(()) => serial_println!("apps: removed `{}`", id),
            Err(e) => return Err(alloc::format!("{e}")),
        }
        self.refresh_catalog();
        Ok(())
    }

    // ---- start panel geometry ----

    /// Entries in the start panel's app list: the system apps then the installed ones.
    pub(crate) fn start_total(&self) -> usize {
        Kind::ALL.len() + self.apps.len()
    }

    /// Rows the panel shows at once.
    pub(crate) fn start_rows(&self) -> usize {
        self.start_total().min(START_MAX_ROWS)
    }

    pub(crate) fn start_max_scroll(&self) -> usize {
        self.start_total().saturating_sub(self.start_rows())
    }

    pub(crate) fn scroll_start(&mut self, delta: i32) {
        let max = self.start_max_scroll() as i32;
        self.start_scroll = (self.start_scroll as i32 + delta).clamp(0, max) as usize;
    }

    // ---- windows ----

    fn free_index_wasm(&self, app_id: &str) -> u8 {
        let mut idx = 1u8;
        while self
            .wm
            .windows()
            .iter()
            .any(|w| matches!(&w.app.app, App::Wasm(x) if x.app_id == app_id) && w.app.index == idx)
        {
            idx += 1;
        }
        idx
    }

    /// The most recently used live window of installed app `app_id`.
    fn mru_of_app(&self, app_id: &str) -> Option<WindowId> {
        self.wm.switch_list().into_iter().find(|&id| {
            self.wm
                .get(id)
                .is_some_and(|w| matches!(&w.app.app, App::Wasm(x) if x.app_id == app_id))
        })
    }

    /// Focus the app's window, opening one when there is none.
    pub(crate) fn launch_wasm_app(&mut self, app_id: &str) -> Option<WindowId> {
        if let Some(id) = self.mru_of_app(app_id) {
            self.wm.activate(id);
            return Some(id);
        }
        self.open_wasm_app(app_id)
    }

    /// Open installed app `app_id` in a new window.
    pub(crate) fn open_wasm_app(&mut self, app_id: &str) -> Option<WindowId> {
        let manifest = self.apps.iter().find(|e| e.id == app_id)?.manifest.clone();
        let bytes = match appfs_backend::try_with(|fs| appinstall::read_package(fs, app_id)) {
            Ok(b) => b,
            Err(e) => {
                serial_println!("apps: cannot read `{}`: {}", app_id, e);
                return None;
            }
        };
        self.open_wasm(manifest, bytes)
    }

    /// The dock's WASM icon: the legacy embedded app (DOOM / C demo) when the image
    /// has one, else the default packaged app.
    pub(crate) fn launch_default_wasm(&mut self) -> Option<WindowId> {
        if wasm::LEGACY_APP.is_empty() {
            return self.launch_wasm_app(wasm::DEFAULT_APP);
        }
        if let Some(id) = self.mru_of_app("app") {
            self.wm.activate(id);
            return Some(id);
        }
        let manifest = Manifest::legacy("app", "WASM App");
        self.open_wasm(manifest, wasm::LEGACY_APP.to_vec())
    }

    /// A new window for the default app (generic `open_new(Kind::WasmApp)`).
    pub(crate) fn open_default_wasm(&mut self) -> Option<WindowId> {
        if wasm::LEGACY_APP.is_empty() {
            return self.open_wasm_app(wasm::DEFAULT_APP);
        }
        let manifest = Manifest::legacy("app", "WASM App");
        self.open_wasm(manifest, wasm::LEGACY_APP.to_vec())
    }

    fn open_wasm(&mut self, manifest: Manifest, bytes: Vec<u8>) -> Option<WindowId> {
        if self.wm.is_full() {
            return None;
        }
        let app_id = manifest.id.clone();
        let (cw, ch) = (manifest.win_w as i32, manifest.win_h as i32);
        let v1 = manifest.abi == Abi::V1;
        let (min_w, min_h) = (
            manifest.win_min_w as i32 + FRAME_W,
            manifest.win_min_h as i32 + FRAME_H,
        );
        let resizable = manifest.resizable;
        let title = manifest.name.to_ascii_uppercase();
        let handle = match wasm::launch(bytes, manifest, v1, cw, ch) {
            Ok(h) => h,
            Err(e) => {
                serial_println!("apps: cannot start `{}`: {:?}", app_id, e);
                return None;
            }
        };
        let index = self.free_index_wasm(&app_id);
        let mut name = [0u8; 16];
        let n = numbered_name(&app_id, index, &mut name);
        let Some(pid) = self
            .procs
            .spawn(&name[..n], ProcKind::App, ProcState::Running)
        else {
            wasm::close(handle);
            return None;
        };
        let work = self.work_area();
        let (ow, oh) = ((cw + FRAME_W).min(work.w), (ch + FRAME_H).min(work.h));
        let base = Rect::new(240, 130, ow, oh);
        // New WASM windows cascade from the first position, whatever the app.
        let open_wasm = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Wasm(_)) && !w.is_closing())
            .count();
        let mut rect = if open_wasm == 0 {
            base
        } else {
            osjeff_core::winman::cascade_rect(base, open_wasm, work)
        };
        rect.x = rect.x.clamp(work.x, (work.right() - rect.w).max(work.x));
        rect.y = rect.y.clamp(work.y, (work.bottom() - rect.h).max(work.y));
        let mut full_title = title;
        if index > 1 {
            let mut tmp = [0u8; 16];
            let k = numbered_name("", index, &mut tmp);
            full_title.push_str(core::str::from_utf8(&tmp[..k]).unwrap_or(""));
        }
        let inst = Inst {
            app: App::Wasm(Box::new(WasmWin { id: handle, app_id })),
            pid,
            index,
            title: full_title,
            cost: core::cell::Cell::new(0),
            cost_pm: core::cell::Cell::new(0),
        };
        let spec = WindowSpec {
            rect,
            min_w: min_w.min(rect.w),
            min_h: min_h.min(rect.h),
            resizable,
        };
        match self.wm.open(spec, inst) {
            Ok(id) => Some(id),
            Err(inst) => {
                self.procs.kill(inst.pid);
                wasm::close(handle);
                None
            }
        }
    }

    /// The manager handle of a WASM window.
    pub(crate) fn wasm_handle(&self, id: WindowId) -> Option<wasm::AppId> {
        match &self.wm.get(id)?.app.app {
            App::Wasm(w) if w.id != 0 => Some(w.id),
            _ => None,
        }
    }

    /// Once per frame: tell the manager each window's content size and visibility,
    /// adopt guest-set titles, free finished instances and mirror the clipboard.
    pub fn wasm_sync(&mut self) {
        wasm::reap();
        self.poll_fs_changes();
        let mut titles: Vec<(WindowId, String)> = Vec::new();
        for w in self.wm.windows() {
            if let App::Wasm(ww) = &w.app.app {
                let c = wasm_content(w.rect);
                if let Some(t) = wasm::sync_window(ww.id, c.w, c.h, w.shown()) {
                    let mut t = t.to_ascii_uppercase();
                    if w.app.index > 1 {
                        let mut tmp = [0u8; 16];
                        let k = numbered_name("", w.app.index, &mut tmp);
                        t.push_str(core::str::from_utf8(&tmp[..k]).unwrap_or(""));
                    }
                    if t != w.app.title {
                        titles.push((w.id, t));
                    }
                }
            }
        }
        for (id, t) in titles {
            if let Some(w) = self.wm.get_mut(id) {
                w.app.title = t;
            }
        }
        let g = wasm::clip_generation();
        if g != self.clip_gen {
            self.clip_gen = g;
            let mut b = [0u8; clipboard::CAP];
            let n = wasm::clip_get(&mut b);
            self.clipboard.set(&b[..n]);
        }
    }

    /// Any WASM window on screen? (keeps the per-frame damage path running)
    pub(crate) fn wasm_pointer_target(&self, px: i32, py: i32) -> Option<(WindowId, i32, i32)> {
        let id = self.topmost_at(px, py)?;
        let w = self.wm.get(id)?;
        if !matches!(w.app.app, App::Wasm(_)) {
            return None;
        }
        let c = wasm_content(w.rect);
        (px >= c.x && py >= c.y && px < c.x + c.w && py < c.y + c.h).then_some((
            id,
            px - c.x,
            py - c.y,
        ))
    }

    /// Deliver the mouse state to the WASM window under the cursor (or the one that
    /// grabbed the button), as content-local coordinates.
    pub(crate) fn wasm_pointer(&mut self, left: bool, right: bool, moved: bool) {
        let buttons = left as i32 | ((right as i32) << 1);
        let changed = left != self.prev_left || right != self.prev_right;
        if !moved && !changed {
            return;
        }
        if buttons == 0
            && let Some(g) = self.wasm_grab.take()
        {
            // release: send it to the window that owns the press, even outside it
            if let (Some(h), Some(r)) = (self.wasm_handle(g), self.wm.get(g).map(|w| w.rect)) {
                let c = wasm_content(r);
                wasm::pointer(h, self.cursor_x - c.x, self.cursor_y - c.y, 0);
            }
            return;
        }
        let (cx, cy) = (self.cursor_x, self.cursor_y);
        if let Some(g) = self.wasm_grab {
            if let (Some(h), Some(r)) = (self.wasm_handle(g), self.wm.get(g).map(|w| w.rect)) {
                let c = wasm_content(r);
                wasm::pointer(h, cx - c.x, cy - c.y, buttons);
            }
            return;
        }
        if self.overlay_open() {
            return;
        }
        if let Some((id, lx, ly)) = self.wasm_pointer_target(cx, cy)
            && let Some(h) = self.wasm_handle(id)
        {
            if buttons != 0 && changed {
                self.wasm_grab = Some(id);
            }
            wasm::pointer(h, lx, ly, buttons);
        }
    }
}

/// ABI key code and modifier bits of a logical key: ASCII, 10 Enter, 27 Esc,
/// 8 Backspace, 9 Tab, 127 Delete, 0x100.. arrows / Home / End / PageUp / PageDown.
pub(crate) fn wasm_key_code(key: Key) -> i32 {
    match key {
        Key::Char(b) => b as i32,
        Key::Enter => 10,
        Key::Esc => 27,
        Key::Backspace => 8,
        Key::Tab => 9,
        Key::Delete => 127,
        Key::Left => 0x100,
        Key::Right => 0x101,
        Key::Up => 0x102,
        Key::Down => 0x103,
        Key::Home => 0x104,
        Key::End => 0x105,
        Key::PageUp => 0x106,
        Key::PageDown => 0x107,
    }
}
