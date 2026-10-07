//! Installing, removing and listing apps in `/apps/<id>.wasm`.
//!
//! `install` validates **before** it writes: the module's section table, the
//! manifest (grammar, permissions, quotas under the system ceilings), the icon,
//! and that the id is not already installed. The file is written under a
//! temporary name and renamed into place, so an interrupted install never leaves
//! a half-written `<id>.wasm` for the launcher to trip over.

use crate::appfs::{AppFs, FsError, Kind};
use crate::appmanifest::{self, Manifest, PackageError, valid_id};
use crate::image::{Filter, Image};
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// Where installed apps live.
pub const APPS_DIR: &str = "/apps";
const TMP: &str = "/apps/.install.tmp";
/// Largest package the installer accepts.
pub const MAX_PACKAGE_BYTES: usize = 4 << 20;
/// Most apps in the catalog (a bound for the launcher UI and the heap).
pub const MAX_APPS: usize = 64;
/// Launcher icon edge, in pixels.
pub const ICON_SIZE: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallError {
    TooLarge,
    Package(PackageError),
    /// An app with this id is already installed.
    Duplicate,
    NotInstalled,
    BadId,
    /// Too many apps installed.
    Full,
    Fs(FsError),
}

impl From<FsError> for InstallError {
    fn from(e: FsError) -> Self {
        InstallError::Fs(e)
    }
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::TooLarge => f.write_str("package is larger than 4 MiB"),
            InstallError::Package(e) => e.fmt(f),
            InstallError::Duplicate => f.write_str("an app with this id is already installed"),
            InstallError::NotInstalled => f.write_str("app is not installed"),
            InstallError::BadId => f.write_str("invalid app id"),
            InstallError::Full => f.write_str("too many installed apps"),
            InstallError::Fs(e) => write!(f, "file system: {e}"),
        }
    }
}

fn package_path(id: &str) -> String {
    let mut p = String::from(APPS_DIR);
    p.push('/');
    p.push_str(id);
    p.push_str(".wasm");
    p
}

/// Is `<id>` installed?
pub fn is_installed(fs: &mut dyn AppFs, id: &str) -> bool {
    valid_id(id)
        && matches!(
            fs.stat(&package_path(id)),
            Ok(s) if s.kind == Kind::File
        )
}

/// Validates `wasm` without touching the filesystem.
pub fn check(wasm: &[u8]) -> Result<Manifest, InstallError> {
    if wasm.len() > MAX_PACKAGE_BYTES {
        return Err(InstallError::TooLarge);
    }
    appmanifest::parse_package(wasm)
        .map(|p| p.manifest)
        .map_err(InstallError::Package)
}

/// Installs `wasm` as `/apps/<id>.wasm`. Refuses an invalid package, a duplicate
/// id, and a full catalog; nothing is written in those cases.
pub fn install(fs: &mut dyn AppFs, wasm: &[u8]) -> Result<Manifest, InstallError> {
    let manifest = check(wasm)?;
    fs.mkdir_all(APPS_DIR)?;
    if is_installed(fs, &manifest.id) {
        return Err(InstallError::Duplicate);
    }
    if installed_ids(fs)?.len() >= MAX_APPS {
        return Err(InstallError::Full);
    }
    let _ = fs.remove(TMP); // leftover of an interrupted install
    fs.create(TMP)?;
    let result =
        write_all(fs, TMP, wasm).and_then(|()| fs.rename(TMP, &package_path(&manifest.id)));
    if let Err(e) = result {
        let _ = fs.remove(TMP);
        return Err(e.into());
    }
    Ok(manifest)
}

fn write_all(fs: &mut dyn AppFs, path: &str, data: &[u8]) -> Result<(), FsError> {
    let mut off = 0usize;
    while off < data.len() {
        let n = (data.len() - off).min(32 * 1024);
        let w = fs.write_at(path, off as u64, &data[off..off + n])?;
        if w == 0 {
            return Err(FsError::Io);
        }
        off += w;
    }
    Ok(())
}

/// Removes an installed app's package. Its data in `/data/<id>` is kept.
pub fn remove(fs: &mut dyn AppFs, id: &str) -> Result<(), InstallError> {
    if !valid_id(id) {
        return Err(InstallError::BadId);
    }
    match fs.remove(&package_path(id)) {
        Ok(()) => Ok(()),
        Err(FsError::NotFound) => Err(InstallError::NotInstalled),
        Err(e) => Err(e.into()),
    }
}

/// Ids of the installed packages (file names `<id>.wasm`, in name order).
pub fn installed_ids(fs: &mut dyn AppFs) -> Result<Vec<String>, FsError> {
    let mut ids = Vec::new();
    for i in 0..MAX_APPS + 8 {
        match fs.read_dir(APPS_DIR, i) {
            Ok(Some(e)) => {
                if e.kind == Kind::File
                    && let Some(id) = e.name.strip_suffix(".wasm")
                    && valid_id(id)
                {
                    ids.push(String::from(id));
                }
            }
            Ok(None) => break,
            Err(FsError::NotFound) => break,
            Err(e) => return Err(e),
        }
    }
    Ok(ids)
}

/// Reads an installed package back.
pub fn read_package(fs: &mut dyn AppFs, id: &str) -> Result<Vec<u8>, InstallError> {
    if !valid_id(id) {
        return Err(InstallError::BadId);
    }
    let path = package_path(id);
    let size = match fs.stat(&path) {
        Ok(s) if s.kind == Kind::File => s.size as usize,
        Ok(_) => return Err(InstallError::NotInstalled),
        Err(FsError::NotFound) => return Err(InstallError::NotInstalled),
        Err(e) => return Err(e.into()),
    };
    if size > MAX_PACKAGE_BYTES {
        return Err(InstallError::TooLarge);
    }
    let mut buf = Vec::new();
    buf.try_reserve_exact(size)
        .map_err(|_| InstallError::Fs(FsError::NoSpace))?;
    buf.resize(size, 0);
    let mut off = 0;
    while off < size {
        let n = fs.read_at(&path, off as u64, &mut buf[off..])?;
        if n == 0 {
            return Err(InstallError::Fs(FsError::Io));
        }
        off += n;
    }
    Ok(buf)
}

/// A catalog entry for the launcher: the manifest and a 24x24 icon.
#[derive(Debug)]
pub struct CatalogEntry {
    pub manifest: Manifest,
    /// `ICON_SIZE * ICON_SIZE` pixels, `0xAARRGGBB`; `None` = use the default icon.
    pub icon: Option<Vec<u32>>,
}

/// Scales a package icon to the launcher size.
pub fn launcher_icon(img: &Image) -> Option<Vec<u32>> {
    let scaled = img.resize(ICON_SIZE, ICON_SIZE, Filter::Auto).ok()?;
    let mut px = Vec::with_capacity(ICON_SIZE * ICON_SIZE);
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            px.push(scaled.get(x, y)?);
        }
    }
    Some(px)
}

/// Reads every installed package and builds the launcher catalog. Packages that
/// no longer validate, or whose manifest id differs from the file name, are
/// skipped (never trusted just because they sit in `/apps`).
pub fn load_catalog(fs: &mut dyn AppFs) -> Vec<CatalogEntry> {
    let mut out = Vec::new();
    let Ok(ids) = installed_ids(fs) else {
        return out;
    };
    for id in ids {
        let Ok(bytes) = read_package(fs, &id) else {
            continue;
        };
        let Ok(pkg) = appmanifest::parse_package(&bytes) else {
            continue;
        };
        if pkg.manifest.id != id {
            continue;
        }
        let icon = pkg.icon.as_ref().and_then(launcher_icon);
        out.push(CatalogEntry {
            manifest: pkg.manifest,
            icon,
        });
    }
    out.sort_by(|a, b| {
        a.manifest
            .name
            .to_ascii_lowercase()
            .cmp(&b.manifest.name.to_ascii_lowercase())
    });
    out
}

/// First-boot seeding: installs each bundled package that is not installed yet
/// and never overwrites one that is (the user's copy wins). Returns how many
/// were installed.
pub fn seed(fs: &mut dyn AppFs, bundled: &[&[u8]]) -> usize {
    let mut n = 0;
    for pkg in bundled {
        if install(fs, pkg).is_ok() {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests;
