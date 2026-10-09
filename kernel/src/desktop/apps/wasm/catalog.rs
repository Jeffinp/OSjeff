//! The installed-app catalog (what the launcher lists), first-boot seeding of the bundled packages and the install / remove actions.

use super::window::argb_to_rgba;
use crate::desktop::*;
use crate::serial_println;
use crate::wasm::{self, appfs_backend};
use kitsune_core::appinstall;
use kitsune_core::appmanifest::Manifest;
use kitsune_core::t;

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

impl Desktop {
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
                name: String::from(c.manifest.display_name()),
                icon: c.icon.as_deref().map(argb_to_rgba),
                manifest: c.manifest,
            })
            .collect();
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
                let name = String::from(m.display_name());
                rows.push(AppRow {
                    id: m.id,
                    name,
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
                crate::notify::notify_why(
                    crate::klog::Level::Warn,
                    kitsune_core::tk!("notify.app_refused"),
                    |l| e.message_in(l),
                );
                return;
            }
        };
        let installed =
            appfs_backend::with(|fs| appinstall::is_installed(fs, &manifest.id)).unwrap_or(false);
        if !installed {
            match appfs_backend::try_with(|fs| appinstall::install(fs, &bytes)) {
                Ok(m) => serial_println!("apps: installed `{}` {} from a file", m.id, m.version),
                Err(e) => {
                    crate::notify::notify_why(
                        crate::klog::Level::Warn,
                        kitsune_core::tk!("notify.app_refused"),
                        |l| e.message_in(l),
                    );
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
            .ok_or_else(|| String::from(t!("apps.err.not_bundled")))?;
        let r = appfs_backend::try_with(|fs| appinstall::install(fs, pkg));
        match r {
            Ok(m) => serial_println!("apps: installed `{}` {}", m.id, m.version),
            Err(e) => {
                serial_println!("apps: could not install `{}`: {}", id, e);
                return Err(e.message());
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
            Err(e) => return Err(e.message()),
        }
        self.refresh_catalog();
        Ok(())
    }
}
