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
use alloc::format;
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

/// The text of the "Estado" column.
pub fn status_label(installed: bool) -> &'static str {
    if installed {
        "instalado"
    } else {
        "não instalado"
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
        (AppKey::Install, true) => Err("Já instalado"),
        (AppKey::Remove, true) => Ok(AppAction::Remove(id)),
        (AppKey::Remove, false) => Err("Não instalado"),
    }
}

/// The permissions and limits of `m` as lines for the Properties panel (Portuguese).
pub fn manifest_lines(m: &Manifest) -> Vec<String> {
    let abi = match m.abi {
        Abi::V1 => "1 (desenho contínuo)",
        Abi::V2 => "2 (por eventos)",
    };
    let fs = match m.fs {
        FsPerm::None => String::from("nenhum"),
        FsPerm::Own => format!("só /data/{}", m.id),
        FsPerm::Home => String::from("pasta do usuário (/home)"),
    };
    let net = match m.net {
        NetPerm::None => "nenhuma",
        NetPerm::Http => "HTTP e HTTPS (endereços públicos)",
        NetPerm::Tcp => "HTTP e HTTPS (TCP reservado)",
    };
    let clip = match m.clipboard {
        ClipPerm::None => "não",
        ClipPerm::Rw => "ler e escrever",
    };
    alloc::vec![
        format!("App: {} ({})", m.name, m.id),
        format!("Versão: {}   ABI {}", m.version, abi),
        format!("Arquivos: {fs}"),
        format!("Rede: {net}"),
        format!("Área de transferência: {clip}"),
        format!("Memória: {} MiB   Disco: {} KiB", m.mem_mib, m.disk_kib),
        format!(
            "Arquivos abertos: {}   Janela {}x{}{}",
            m.max_fds,
            m.win_w,
            m.win_h,
            if m.resizable { "+" } else { "" }
        ),
    ]
}
