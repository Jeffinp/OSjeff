//! The preview pane of the file manager.

use crate::desktop::apps::files::labels::ToStringLossy;
use crate::desktop::*;
use alloc::boxed::Box;
use kitsune_core::fileman::apps::{self as fapps};
use kitsune_core::fileman::ui::{self};
use kitsune_core::fileman::{self};
use kitsune_core::{t, tp};

impl Desktop {
    /// Rebuild the preview pane's content when the selection changed since it was made.
    pub(crate) fn files_sync_preview(&mut self, id: WindowId) {
        let Some(f) = self.files_mut(id) else {
            return;
        };
        if !f.preview_open {
            return;
        }
        // What the pane should show: nothing, one file, or "N itens".
        let key: Vec<u8> = match f.view.sel.count() {
            0 => Vec::new(),
            1 => f
                .view
                .selected_rows()
                .first()
                .map(|r| {
                    let mut k = f.view.cwd.clone();
                    k.push(0);
                    k.extend_from_slice(&r.name);
                    k.push(0);
                    k.extend_from_slice(&r.id);
                    k.extend_from_slice(&r.size.to_le_bytes());
                    k
                })
                .unwrap_or_default(),
            n => alloc::format!("#{n}").into_bytes(),
        };
        if f.preview.as_ref().map(|p| &p.path[..]) == Some(&key[..]) {
            return;
        }
        let data = self.build_preview(id, key);
        if let Some(f) = self.files_mut(id) {
            f.preview = Some(Box::new(data));
        }
    }

    fn build_preview(&mut self, id: WindowId, key: Vec<u8>) -> PreviewData {
        use kitsune_core::appart::FileKind;
        let empty = |key: Vec<u8>| PreviewData {
            path: key,
            name: String::new(),
            kind: FileKind::Generic,
            kind_label: String::new(),
            info: Vec::new(),
            image: None,
            lines: Vec::new(),
            note: None,
        };
        let Some(f) = self.files_mut(id) else {
            return empty(key);
        };
        let rows: Vec<fileman::Row> = f.view.selected_rows().into_iter().cloned().collect();
        let in_trash = f.view.in_trash();
        let in_apps = f.view.in_apps();
        let cwd = f.view.cwd.clone();
        if rows.is_empty() {
            return empty(key);
        }
        if rows.len() > 1 {
            let bytes: u64 = rows.iter().filter(|r| !r.is_dir()).map(|r| r.size).sum();
            let mut d = empty(key);
            d.name = tp!("files.count", rows.len());
            d.kind = FileKind::Folder;
            d.kind_label = String::from(t!("files.preview.selection"));
            d.info.push((
                String::from(t!("files.preview.size")),
                fileman::format_size(bytes),
            ));
            return d;
        }
        let row = &rows[0];
        let name = String::from_utf8_lossy(&row.name).into_owned();
        let pk = ui::preview_kind(&row.name, row.is_dir());
        let mut d = empty(key);
        d.name = name;
        d.kind = ui::icon_kind(&row.name, row.is_dir());
        d.kind_label = ui::kind_label(&row.name, row.is_dir());
        if in_apps {
            d.kind = FileKind::App;
            d.kind_label = String::from(t!("files.kind.app"));
            d.info.push((
                String::from(t!("files.preview.state")),
                String::from(fapps::status_label(row.installed)),
            ));
        }
        if !row.is_dir() {
            d.info.push((
                String::from(if in_apps {
                    t!("files.preview.package")
                } else {
                    t!("files.preview.size")
                }),
                fileman::format_size(row.size),
            ));
        }
        if !in_apps {
            d.info.push((
                String::from(if in_trash {
                    t!("files.preview.deleted")
                } else {
                    t!("files.preview.modified")
                }),
                modified_label(row.mtime),
            ));
        }
        if in_trash || in_apps {
            return d;
        }
        let path = vfs::join(&cwd, &row.name);
        match pk {
            ui::PreviewKind::Image => {
                if row.size > ui::PREVIEW_MAX_IMAGE {
                    d.note = Some(String::from(t!("files.preview.too_big")));
                } else {
                    match vfs::read_file(&path)
                        .ok()
                        .and_then(|b| kitsune_core::image::decode(&b).ok())
                    {
                        Some(img) => {
                            d.info.insert(
                                1,
                                (
                                    String::from(t!("files.preview.dimensions")),
                                    t!("files.dims", w = img.width(), h = img.height()),
                                ),
                            );
                            d.image = thumbnail(&img, 216, 156);
                        }
                        None => d.note = Some(String::from(t!("files.preview.cannot_open"))),
                    }
                }
            }
            ui::PreviewKind::Text | ui::PreviewKind::Other => {
                match vfs::read_range(&path, 0, ui::PREVIEW_TEXT_BYTES) {
                    Ok(head) => {
                        d.lines = ui::text_preview(&head, 14, 34);
                        if d.lines.is_empty() && pk == ui::PreviewKind::Other {
                            d.note = Some(String::from(t!("files.preview.none")));
                        }
                    }
                    Err(e) => d.note = Some(String::from(e.message())),
                }
            }
            ui::PreviewKind::Folder => {
                if let Ok(list) = vfs::list(&path) {
                    d.info.push((
                        String::from(t!("files.preview.items")),
                        list.len().to_string_lossy(),
                    ));
                }
            }
            ui::PreviewKind::App => {}
        }
        d
    }
}

/// A picture scaled to fit `bw x bh`, as a premultiplied surface for the preview pane.
fn thumbnail(
    img: &kitsune_core::image::Image,
    bw: usize,
    bh: usize,
) -> Option<kitsune_core::raster::Surface> {
    use kitsune_core::image::Filter;
    let small = img.fit(bw, bh, false, Filter::Box).ok()?;
    let mut s = kitsune_core::raster::Surface::new(small.width(), small.height());
    for (d, &p) in s.px.iter_mut().zip(small.pixels()) {
        *d = kitsune_core::raster::premul(p);
    }
    Some(s)
}
