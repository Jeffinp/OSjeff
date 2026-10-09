//! The properties and confirmation sheets.

use super::labels::prop;
use crate::desktop::apps::files::labels::ToStringLossy;
use crate::desktop::*;
use kitsune_core::fileman::apps::{self as fapps};
use kitsune_core::fileman::ui::{self};
use kitsune_core::fileman::{self, FileClass};
use kitsune_core::{t, tk, tp};

impl Desktop {
    /// Build the information sheet of the selection (or of the folder).
    pub(super) fn files_properties(
        &mut self,
        id: WindowId,
        in_trash: bool,
        cwd: &[u8],
        paths: &[Vec<u8>],
    ) {
        let mut lines: Vec<String> = Vec::new();
        let free = vfs::statfs();
        let show = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        if in_trash {
            lines.push(prop(tk!("files.prop.location"), t!("files.place.trash")));
            if let Some(f) = self.files_mut(id) {
                lines.push(prop(
                    tk!("files.prop.items"),
                    &f.view.rows.len().to_string_lossy(),
                ));
            }
        } else if paths.len() == 1 {
            let p = &paths[0];
            lines.push(prop(tk!("files.prop.name"), &show(vfs::base_name(p))));
            lines.push(prop(tk!("files.prop.location"), &show(&vfs::parent(p))));
            match vfs::stat(p) {
                Ok(info) => {
                    if info.kind == vfs::EntryKind::Dir {
                        let t = vfs::with_backend(|b| kitsune_core::vfs::tree_size(b, p));
                        lines.push(prop(tk!("files.prop.type"), t!("files.kind.folder")));
                        if let Ok(Ok(t)) = t {
                            lines.push(prop(
                                tk!("files.prop.contents"),
                                &alloc::format!(
                                    "{}, {}",
                                    tp!("files.prop.n_files", t.files),
                                    tp!("files.prop.n_folders", t.dirs.saturating_sub(1))
                                ),
                            ));
                            lines
                                .push(prop(tk!("files.prop.size"), &fileman::format_size(t.bytes)));
                        }
                    } else {
                        lines.push(prop(
                            tk!("files.prop.type"),
                            &ui::kind_label(vfs::base_name(p), false),
                        ));
                        lines.push(prop(
                            tk!("files.prop.size"),
                            &t!(
                                "files.prop.size_bytes",
                                size = &fileman::format_size(info.size),
                                bytes =
                                    kitsune_core::i18n::num(info.size.min(i64::MAX as u64) as i64)
                            ),
                        ));
                    }
                    lines.push(prop(tk!("files.prop.created"), &local_time(info.ctime)));
                    lines.push(prop(tk!("files.prop.modified"), &local_time(info.mtime)));
                    if info.kind == vfs::EntryKind::File
                        && fileman::classify(vfs::base_name(p)) == FileClass::Wasm
                    {
                        lines.extend(self.wasm_property_lines(p));
                    }
                }
                Err(e) => lines.push(prop(tk!("files.prop.error"), e.message())),
            }
        } else if paths.len() > 1 {
            lines.push(prop(
                tk!("files.prop.selection"),
                &tp!("files.count", paths.len()),
            ));
            let mut bytes = 0u64;
            for p in paths {
                if let Ok(Ok(t)) = vfs::with_backend(|b| kitsune_core::vfs::tree_size(b, p)) {
                    bytes += t.bytes;
                }
            }
            lines.push(prop(
                tk!("files.prop.total_size"),
                &fileman::format_size(bytes),
            ));
        } else {
            lines.push(prop(tk!("files.prop.folder"), &show(cwd)));
            if let Some(f) = self.files_mut(id) {
                lines.push(prop(
                    tk!("files.prop.items"),
                    &f.view.rows.len().to_string_lossy(),
                ));
            }
        }
        lines.push(prop(
            tk!("files.prop.free"),
            &t!(
                "files.prop.free_of",
                free = &fileman::format_size(free.free),
                total = &fileman::format_size(free.total)
            ),
        ));
        if vfs::volume() == vfs::Volume::Memory {
            lines.push(prop(
                tk!("files.prop.volume"),
                t!("files.prop.memory_volume"),
            ));
        }
        if let Some(f) = self.files_mut(id) {
            f.props = Some(lines);
            f.open_sheet();
        }
    }
}

/// Which sheet a file manager shows, and its size.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SheetKind {
    Confirm,
    Info,
    Copy,
}

impl SheetKind {
    /// Button labels, left to right (the last is the default).
    pub(crate) fn buttons(self) -> [&'static str; 2] {
        match self {
            SheetKind::Confirm => [t!("common.cancel"), t!("files.sheet.delete")],
            SheetKind::Info => ["", t!("files.sheet.done")],
            SheetKind::Copy => ["", t!("common.cancel")],
        }
    }
}

/// The sheet window state asks for now: kind and size.
pub(crate) fn files_sheet_kind(f: &FilesState) -> (SheetKind, (i32, i32)) {
    if f.confirm.is_some() {
        (SheetKind::Confirm, (400, 156))
    } else if let Some(lines) = &f.props {
        (SheetKind::Info, (460, 112 + lines.len() as i32 * 24))
    } else {
        (SheetKind::Copy, (400, 148))
    }
}

impl Desktop {
    /// Properties lines of a `.wasm` file: whether it is a valid app package and, if
    /// so, its manifest (permissions and limits) and whether it is installed.
    fn wasm_property_lines(&self, path: &[u8]) -> Vec<String> {
        let bytes = match vfs::read_range(path, 0, kitsune_core::appinstall::MAX_PACKAGE_BYTES + 1)
        {
            Ok(b) => b,
            Err(e) => return alloc::vec![prop(tk!("files.prop.error"), e.message())],
        };
        match kitsune_core::appinstall::check(&bytes) {
            Ok(m) => {
                let mut v =
                    alloc::vec![prop(tk!("files.prop.package"), t!("files.prop.pkg_valid"))];
                v.extend(fapps::manifest_lines(&m));
                let state = if self.apps.iter().any(|a| a.id == m.id) {
                    t!("files.prop.installed")
                } else {
                    t!("files.prop.not_installed_hint")
                };
                v.push(prop(tk!("files.prop.state"), state));
                v
            }
            Err(e) => alloc::vec![prop(
                tk!("files.prop.package"),
                &t!("files.prop.pkg_invalid", detail = &alloc::format!("{e}")),
            )],
        }
    }

    /// Properties of the selected app in the Apps place: manifest, state and package.
    pub(super) fn files_app_properties(&mut self, id: WindowId) {
        let Some(row) = self
            .files_mut(id)
            .and_then(|f| f.view.selected_rows().first().map(|r| (*r).clone()))
        else {
            return;
        };
        let app_id = String::from_utf8_lossy(&row.id).into_owned();
        let mut lines = match self.app_manifest(&app_id) {
            Some(m) => fapps::manifest_lines(&m),
            None => alloc::vec![prop(
                tk!("files.prop.manifest"),
                t!("files.prop.manifest_missing"),
            )],
        };
        lines.push(prop(
            tk!("files.prop.state"),
            fapps::status_label(row.installed),
        ));
        lines.push(prop(
            tk!("files.prop.package"),
            &fileman::format_size(row.size),
        ));
        if row.installed {
            lines.push(prop(
                tk!("files.prop.file"),
                &alloc::format!("/apps/{app_id}.wasm"),
            ));
        } else {
            lines.push(prop(
                tk!("files.prop.origin"),
                t!("files.prop.origin_bundled"),
            ));
        }
        if let Some(f) = self.files_mut(id) {
            f.props = Some(lines);
            f.open_sheet();
        }
    }
}
