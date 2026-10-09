//! The file manager's **Apps** place: installed and bundled app packages, with
//! install, remove and run, and the text of an app's manifest for Properties.
//!
//! Only decisions live here (they run on the host, tested): which rows to show
//! ([`rows`]), what a key does to a row ([`app_action`]) and how a manifest reads
//! to the user ([`manifest_lines`]). The kernel owns the catalog and the
//! installer (`Desktop::{app_rows, install_bundled, remove_app}`) and carries out
//! the [`AppAction`].

use super::Row;
use crate::appmanifest::{Abi, ClipPerm, FsPerm, Manifest, NetPerm};
use crate::vfs::EntryKind;
use alloc::string::String;
use alloc::vec::Vec;

/// One app of the list: installed, or a bundled package waiting to be installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppItem {
    pub id: String,
    pub name: String,
    pub installed: bool,
    /// Size of the package file in bytes.
    pub size: u64,
}

/// The rows of the Apps place. `Row::id` is the app id, `Row::installed` the state,
/// `Row::mtime` is 1 for an installed app and 0 otherwise (so the "Estado" column
/// sorts installed apps together).
pub fn rows(items: &[AppItem]) -> Vec<Row> {
    items
        .iter()
        .map(|a| Row {
            name: a.name.as_bytes().to_vec(),
            kind: EntryKind::File,
            size: a.size,
            mtime: u64::from(a.installed),
            id: a.id.as_bytes().to_vec(),
            installed: a.installed,
        })
        .collect()
}

/// The text of the "Estado" column, in the language in effect.
pub fn status_label(installed: bool) -> &'static str {
    if installed {
        crate::t!("files.app.installed")
    } else {
        crate::t!("files.app.not_installed")
    }
}

/// The keys of the Apps place that act on the selected app.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppKey {
    /// Enter / double click: run it (installing the bundled package first).
    Enter,
    /// `I`: install.
    Install,
    /// `Del`: remove.
    Remove,
}

/// What the desktop must do for a key on an app.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AppAction {
    /// Focus or open the installed app.
    Launch(String),
    /// Install the bundled package, then open it.
    InstallAndLaunch(String),
    Install(String),
    Remove(String),
}

/// What `key` does on `row`, or the message to show (`Err`) when it does not apply
/// (installing what is installed, removing what is not).
pub fn app_action(row: &Row, key: AppKey) -> Result<AppAction, &'static str> {
    let id = String::from_utf8_lossy(&row.id).into_owned();
    match (key, row.installed) {
        (AppKey::Enter, true) => Ok(AppAction::Launch(id)),
        (AppKey::Enter, false) => Ok(AppAction::InstallAndLaunch(id)),
        (AppKey::Install, false) => Ok(AppAction::Install(id)),
        (AppKey::Install, true) => Err(crate::t!("files.app.already_installed")),
        (AppKey::Remove, true) => Ok(AppAction::Remove(id)),
        (AppKey::Remove, false) => Err(crate::t!("files.app.not_installed_cap")),
    }
}

/// The permissions and limits of `m` as lines for the Properties panel (`label: value`), in the
/// language in effect.
pub fn manifest_lines(m: &Manifest) -> Vec<String> {
    let abi = match m.abi {
        Abi::V1 => crate::t!("files.app.abi1"),
        Abi::V2 => crate::t!("files.app.abi2"),
    };
    let fs = match m.fs {
        FsPerm::None => String::from(crate::t!("files.app.fs_none")),
        FsPerm::Own => crate::t!("files.app.fs_own", id = m.id.as_str()),
        FsPerm::Home => String::from(crate::t!("files.app.fs_home")),
    };
    let net = match m.net {
        NetPerm::None => crate::t!("files.app.net_none"),
        NetPerm::Http => crate::t!("files.app.net_http"),
        NetPerm::Tcp => crate::t!("files.app.net_tcp"),
    };
    let clip = match m.clipboard {
        ClipPerm::None => crate::t!("files.app.clip_none"),
        ClipPerm::Rw => crate::t!("files.app.clip_rw"),
    };
    alloc::vec![
        crate::t!(
            "files.app.line.app",
            name = m.name.as_str(),
            id = m.id.as_str()
        ),
        crate::t!(
            "files.app.line.version",
            version = crate::i18n::Arg::Display(&m.version),
            abi = abi
        ),
        crate::t!("files.app.line.files", value = &fs),
        crate::t!("files.app.line.net", value = net),
        crate::t!("files.app.line.clipboard", value = clip),
        crate::t!("files.app.line.limits", mem = m.mem_mib, disk = m.disk_kib),
        crate::t!(
            "files.app.line.window",
            fds = m.max_fds,
            w = m.win_w,
            h = m.win_h,
            resizable = if m.resizable { "+" } else { "" }
        ),
    ]
}
