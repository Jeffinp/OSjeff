//! Opening, loading and navigating pictures: the file, the filmstrip, rotation, saving.

use crate::desktop::kit::appui::{self};
use crate::desktop::*;
use kitsune_core::anim::{Tween, curves};
use kitsune_core::fileman::TextInput;
use kitsune_core::image::{self, Filter, Format, Image};
use kitsune_core::raster::Surface;
use kitsune_core::t;
use kitsune_core::viewer as vw;
use kitsune_core::viewer::ui::{self as vui, FitMode, Layout};

/// Largest file the viewer reads (the decoder bounds the pixels on its own; the kernel
/// heap is 64 MiB, so the file and the decoded pixels must both fit).
const MAX_FILE: u64 = 24 * 1024 * 1024;
/// Largest file decoded for a filmstrip thumbnail.
const MAX_THUMB_FILE: u64 = 3 * 1024 * 1024;
/// Thumbnails made around the current image.
pub(super) const THUMB_LIMIT: usize = 48;
/// Ticks between two thumbnails (a decode blocks the compositor for a moment).
pub(super) const THUMB_EVERY: u64 = 20;

impl Desktop {
    /// Show the image at `path` in a viewer window (the one already showing it, else a
    /// new one). Returns a message when it cannot even open the window.
    pub(crate) fn open_viewer(&mut self, path: &[u8]) -> Option<String> {
        let open = self
            .wm
            .windows()
            .iter()
            .filter(|w| !w.is_closing())
            .find_map(|w| match &w.app.app {
                App::Viewer(v) if v.path == path => Some(w.id),
                _ => None,
            });
        if let Some(id) = open {
            self.wm.activate(id);
            return None;
        }
        let Some(id) = self.open_new(Kind::Viewer) else {
            return Some(String::from(t!("viewer.too_many")));
        };
        self.viewer_load(id, path, true);
        None
    }

    /// The language changed: every viewer's title, caption message and load error were written
    /// in the old one.
    pub(crate) fn viewer_language_changed_all(&mut self) {
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Viewer(_)))
            .map(|w| w.id)
            .collect();
        for id in ids {
            self.viewer_language_changed(id);
        }
    }

    /// An error is rebuilt by reading the file again.
    fn viewer_language_changed(&mut self, id: WindowId) {
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        v.msg = None;
        let path = v.path.clone();
        if path.is_empty() {
            return;
        }
        if v.error.is_some() {
            self.viewer_load(id, &path, false);
            return;
        }
        let title = t!(
            "viewer.title",
            app = &windows::instance::base_title(Kind::Viewer, 0),
            name = &String::from_utf8_lossy(vfs::base_name(&path)).into_owned()
        );
        if let Some(w) = self.wm.get_mut(id) {
            w.app.title = title;
        }
    }

    /// Geometry of viewer window `id` as drawn.
    pub(crate) fn viewer_layout(&self, id: WindowId) -> Option<Layout> {
        let w = self.wm.get(id)?;
        let App::Viewer(v) = &w.app.app else {
            return None;
        };
        Some(Layout::of(
            w.rect,
            v.list.len() > 1,
            self.viewer_info_rows(v),
        ))
    }

    /// The inspector's rows when it is open and there is a picture.
    pub(super) fn viewer_info_rows(&self, v: &ViewerState) -> Option<usize> {
        (v.show_info && v.image.is_some()).then(|| 7 + usize::from(v.list.len() > 1))
    }

    /// Viewport (the area the picture lives in) of window `id`.
    pub(super) fn viewer_vp(&self, id: WindowId) -> (i32, i32) {
        self.viewer_layout(id)
            .map_or((640, 400), |l| (l.canvas.w, l.canvas.h))
    }

    /// Load `path` into window `id`. `rebuild` re-reads the folder for the
    /// previous/next list; a failure shows in the window (never panics).
    pub(crate) fn viewer_load(&mut self, id: WindowId, path: &[u8], rebuild: bool) {
        let loaded: Result<(Image, Option<Format>, u64), [String; 2]> = (|| {
            let friendly = |m: &str| [String::from(t!("viewer.err.cannot_open")), String::from(m)];
            let info = vfs::stat(path).map_err(|e| friendly(e.message()))?;
            if info.size > MAX_FILE {
                return Err([
                    String::from(t!("viewer.err.too_big_title")),
                    String::from(t!("viewer.err.too_big_body")),
                ]);
            }
            let bytes = vfs::read_file(path).map_err(|e| friendly(e.message()))?;
            let format = image::detect(&bytes);
            let img = image::decode(&bytes).map_err(|e| vw::decode_error_message(&e))?;
            Ok((img, format, bytes.len() as u64))
        })();
        let dir = vfs::parent(path);
        let name = vfs::base_name(path).to_vec();
        let list = if rebuild {
            vfs::list(&dir)
                .ok()
                .map(|es| vw::ImageList::from_entries(&dir, &es, &name))
        } else {
            None
        };
        let title = t!(
            "viewer.title",
            app = &windows::instance::base_title(Kind::Viewer, 0),
            name = &String::from_utf8_lossy(&name).into_owned()
        );
        if let Some(w) = self.wm.get_mut(id) {
            w.app.title = title;
        }
        let (vpw, vph) = self.viewer_vp(id);
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        v.path = path.to_vec();
        v.scaled = None;
        v.save = None;
        v.msg = None;
        if let Some(l) = list {
            v.thumbs = (0..l.len()).map(|_| Thumb::Pending).collect();
            v.list = l;
        }
        v.rot = Tween::at(0.0);
        v.enter_t = Tween::at(0.0);
        v.enter_t.retarget(1.0, 0.28, curves::ENTER);
        match loaded {
            Ok((img, format, bytes)) => {
                v.opaque = img.is_opaque();
                v.view.fit_to(img.width(), img.height(), vpw, vph);
                v.image = Some(img);
                v.format = format;
                v.file_bytes = bytes;
                v.error = None;
            }
            Err(e) => {
                v.image = None;
                v.format = None;
                v.file_bytes = 0;
                v.error = Some(e);
            }
        }
        v.vp_seen = (vpw, vph);
        // A new picture takes its place at once; only its fade is animated.
        v.zoom_s.jump(v.view.zoom as f32);
        v.pan_x_s.jump(v.view.pan_x as f32);
        v.pan_y_s.jump(v.view.pan_y as f32);
        v.inertia.stop();
        self.viewer_sync(id);
        self.viewer_aim_strip(id, false);
    }

    /// After a zoom, rotate or resize: refit or re-clamp, rebuild the downscaled copy when the
    /// zoom is below 100 %, and aim the springs at the result. A change of viewport (a resize)
    /// is followed at once; a change the user asked for is animated.
    pub(crate) fn viewer_sync(&mut self, id: WindowId) {
        let (vpw, vph) = self.viewer_vp(id);
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        let resized = v.vp_seen != (vpw, vph);
        v.vp_seen = (vpw, vph);
        if let Some(img) = &v.image {
            let (iw, ih) = (img.width(), img.height());
            v.view.relayout(iw, ih, vpw, vph);
            let z = v.view.zoom;
            if z >= 1000 {
                v.scaled = None;
            } else if v.scaled.as_ref().is_none_or(|(sz, _)| *sz != z) {
                v.scaled = img
                    .resize(vw::scaled_dim(iw, z), vw::scaled_dim(ih, z), Filter::Box)
                    .ok()
                    .map(|s| (z, s));
            }
        }
        v.zoom_s.set_target(v.view.zoom as f32);
        v.pan_x_s.set_target(v.view.pan_x as f32);
        v.pan_y_s.set_target(v.view.pan_y as f32);
        if resized {
            v.zoom_s.jump(v.view.zoom as f32);
            v.pan_x_s.jump(v.view.pan_x as f32);
            v.pan_y_s.jump(v.view.pan_y as f32);
        }
    }

    /// Aim the filmstrip's scroll at the current image (`animate`: glide there).
    pub(super) fn viewer_aim_strip(&mut self, id: WindowId, animate: bool) {
        let Some(lay) = self.viewer_layout(id) else {
            return;
        };
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        let Some(strip) = lay.strip else {
            return;
        };
        let to = vui::strip_scroll_for(strip.w, v.list.len(), v.list.index()) as f32;
        v.strip_scroll.set_target(to);
        if !animate {
            v.strip_scroll.jump(to);
        }
    }

    pub(super) fn viewer_goto(&mut self, id: WindowId, i: usize) {
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        if let Some(p) = v.list.go_to(i)
            && v.list.len() > 1
        {
            self.viewer_load(id, &p, false);
            self.viewer_aim_strip(id, true);
        }
    }

    pub(super) fn viewer_step(&mut self, id: WindowId, forward: bool) {
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        let next = if forward {
            v.list.go_next()
        } else {
            v.list.go_prev()
        };
        let now = appui::ticks();
        v.slideshow.restart(now);
        if let Some(p) = next
            && v.list.len() > 1
        {
            self.viewer_load(id, &p, false);
            self.viewer_aim_strip(id, true);
        }
    }

    pub(crate) fn viewer_save_prompt(&mut self, id: WindowId) {
        if let Some(v) = self.viewer_mut(id)
            && v.image.is_some()
        {
            let sugg = vw::suggest_save_name(vfs::base_name(&v.path));
            let mut input = TextInput::new(&sugg, vfs::MAX_NAME);
            input.select_stem();
            v.save = Some(input);
            v.sheet_t = Tween::at(0.0);
            v.sheet_t.retarget(1.0, 0.24, curves::ENTER);
        }
    }

    /// Enter on the save-as sheet: encode the current (possibly rotated) image.
    pub(super) fn viewer_save_commit(&mut self, id: WindowId) {
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        let Some(input) = v.save.take() else {
            return;
        };
        let Some(img) = &v.image else {
            return;
        };
        let mut name = vfs::trim_name(input.text()).to_vec();
        let fmt = match vw::save_format(&name) {
            Some(f) => f,
            None => {
                name.extend_from_slice(b".png");
                Format::Png
            }
        };
        let dir = vfs::parent(&v.path);
        let result = (|| -> Result<Vec<u8>, String> {
            vfs::validate_name(&name).map_err(|e| String::from(e.message()))?;
            let path = vfs::join(&dir, &name);
            if vfs::exists(&path) {
                return Err(String::from(t!("viewer.exists")));
            }
            let bytes =
                image::encode(img, fmt).map_err(|e| String::from(vw::image_error_message(e)))?;
            vfs::write_file(&path, &bytes).map_err(|e| String::from(e.message()))?;
            Ok(path)
        })();
        match result {
            Ok(p) => {
                v.msg = Some((
                    t!(
                        "viewer.saved_as",
                        name = &String::from_utf8_lossy(vfs::base_name(&p)).into_owned()
                    ),
                    false,
                ));
                self.fs_changed();
                // The new file joins the filmstrip.
                let cur = self.viewer_mut(id).map(|v| v.path.clone());
                if let Some(cur) = cur {
                    let dir = vfs::parent(&cur);
                    let name = vfs::base_name(&cur).to_vec();
                    if let Ok(es) = vfs::list(&dir)
                        && let Some(v) = self.viewer_mut(id)
                    {
                        v.list = vw::ImageList::from_entries(&dir, &es, &name);
                        v.thumbs = (0..v.list.len()).map(|_| Thumb::Pending).collect();
                    }
                }
            }
            Err(m) => {
                // Keep the sheet so the name can be fixed.
                v.save = Some(input);
                v.msg = Some((m, true));
            }
        }
    }

    /// Rotate the picture a quarter turn (`cw`: clockwise), animated.
    pub(super) fn viewer_rotate(&mut self, id: WindowId, cw: bool) {
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        let Some(img) = &v.image else {
            return;
        };
        match if cw { img.rotate90() } else { img.rotate270() } {
            Ok(n) => {
                v.image = Some(n);
                v.scaled = None;
                // The pan turns with the picture.
                let (px, py) = (v.view.pan_x, v.view.pan_y);
                (v.view.pan_x, v.view.pan_y) = if cw { (-py, px) } else { (py, -px) };
                v.pan_x_s.jump(v.view.pan_x as f32);
                v.pan_y_s.jump(v.view.pan_y as f32);
                // Start from the old orientation and turn into the new one.
                v.rot = Tween::at(if cw { -90.0 } else { 90.0 });
                v.rot.retarget(0.0, 0.3, curves::ENTER);
            }
            Err(e) => v.msg = Some((String::from(vw::image_error_message(e)), true)),
        }
        self.viewer_sync(id);
    }

    pub(super) fn viewer_set_mode(&mut self, id: WindowId, mode: FitMode) {
        let (vpw, vph) = self.viewer_vp(id);
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        let Some((iw, ih)) = v.image.as_ref().map(|i| (i.width(), i.height())) else {
            return;
        };
        match mode {
            FitMode::Fit => v.view.fit_to(iw, ih, vpw, vph),
            FitMode::Fill => v.view.fill_to(iw, ih, vpw, vph),
            FitMode::Actual => v.view.actual(),
        }
        v.inertia.stop();
        self.viewer_sync(id);
    }

    pub(super) fn viewer_toggle_info(&mut self, id: WindowId) {
        if let Some(v) = self.viewer_mut(id) {
            v.show_info = !v.show_info;
            v.info_t = Tween::at(if v.show_info { 0.0 } else { 1.0 });
            v.info_t
                .retarget(if v.show_info { 1.0 } else { 0.0 }, 0.2, curves::ENTER);
        }
    }

    pub(super) fn viewer_toggle_slideshow(&mut self, id: WindowId) {
        let now = appui::ticks();
        if let Some(v) = self.viewer_mut(id)
            && v.list.len() > 1
        {
            v.slideshow.toggle(now);
        }
    }
}

/// A filmstrip thumbnail of the picture at `path`: the picture scaled to cover a square and
/// cropped, with rounded corners. `Missing` when it is too big or cannot be read.
pub(super) fn make_thumb(path: &[u8]) -> Thumb {
    let side = vui::THUMB as usize;
    let Ok(info) = vfs::stat(path) else {
        return Thumb::Missing;
    };
    if info.size > MAX_THUMB_FILE {
        return Thumb::Missing;
    }
    let Ok(bytes) = vfs::read_file(path) else {
        return Thumb::Missing;
    };
    let Ok(img) = image::decode(&bytes) else {
        return Thumb::Missing;
    };
    let (cw, ch) = vui::cover_dims(img.width(), img.height(), side);
    let Ok(big) = img.resize(cw, ch, Filter::Box) else {
        return Thumb::Missing;
    };
    let (cx, cy) = ((cw - side) / 2, (ch - side) / 2);
    let Ok(sq) = big.crop(cx, cy, side, side) else {
        return Thumb::Missing;
    };
    let mut s = Surface::new(side, side);
    for (d, &p) in s.px.iter_mut().zip(sq.pixels()) {
        *d = kitsune_core::raster::premul(p);
    }
    s.mask_rrect(8, Corner::Circle, crate::fb::masks());
    Thumb::Ready(s)
}
