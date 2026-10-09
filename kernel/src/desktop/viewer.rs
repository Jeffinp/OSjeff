//! `Desktop` methods: the image viewer (Kind `Viewer`, "Imagens"). Decoding, zoom and pan decisions
//! are `kitsune_core::image` / `kitsune_core::viewer` (and `viewer::ui` for the window geometry, the
//! filmstrip maths, pan inertia, the slideshow clock and the rotation sampler); this module loads the
//! file through the VFS, keeps the window's state, animates it and paints the pixels.
//!
//! Drawing: when the zoom is at rest below 100 % the image is the box-filtered copy built once per
//! zoom change (`ViewerState::scaled`); in flight and at 100 % and above it is sampled
//! nearest-neighbour straight from the source, so a spring zoom costs no resampling. A rotating
//! picture is drawn through `RotMap`. Rows are converted and written with `Canvas::put_row`;
//! transparency is composited over a checkerboard per pixel.

use super::appui::{self, level};
use super::ui::ButtonKind;
use super::*;
use crate::text::{self, CALLOUT, FOOTNOTE, Weight};
use kitsune_core::anim::{Tween, curves};
use kitsune_core::appart::Tool;
use kitsune_core::fileman::TextInput;
use kitsune_core::image::{self, Filter, Format, Image};
use kitsune_core::raster::Surface;
use kitsune_core::t;
use kitsune_core::viewer as vw;
use kitsune_core::viewer::ui::{self as vui, FitMode, Hit, Layout};

/// Largest file the viewer reads (the decoder bounds the pixels on its own; the kernel
/// heap is 64 MiB, so the file and the decoded pixels must both fit).
const MAX_FILE: u64 = 24 * 1024 * 1024;
/// Largest file decoded for a filmstrip thumbnail.
const MAX_THUMB_FILE: u64 = 3 * 1024 * 1024;
/// Thumbnails made around the current image.
const THUMB_LIMIT: usize = 48;
/// Ticks between two thumbnails (a decode blocks the compositor for a moment).
const THUMB_EVERY: u64 = 20;

fn canvas_bg() -> Color {
    if theme::dark() {
        Color::rgb(0x16, 0x16, 0x18)
    } else {
        Color::rgb(0xE8, 0xE8, 0xED)
    }
}

fn checker(dark_square: bool) -> u32 {
    match (theme::dark(), dark_square) {
        (false, false) => 0xFF_F6_F6_F8,
        (false, true) => 0xFF_DA_DA_E0,
        (true, false) => 0xFF_46_46_4B,
        (true, true) => 0xFF_34_34_39,
    }
}

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
            app = &instance::base_title(Kind::Viewer, 0),
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
    fn viewer_info_rows(&self, v: &ViewerState) -> Option<usize> {
        (v.show_info && v.image.is_some()).then(|| 7 + usize::from(v.list.len() > 1))
    }

    /// Viewport (the area the picture lives in) of window `id`.
    fn viewer_vp(&self, id: WindowId) -> (i32, i32) {
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
            app = &instance::base_title(Kind::Viewer, 0),
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
    fn viewer_aim_strip(&mut self, id: WindowId, animate: bool) {
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

    fn viewer_goto(&mut self, id: WindowId, i: usize) {
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

    fn viewer_step(&mut self, id: WindowId, forward: bool) {
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
    fn viewer_save_commit(&mut self, id: WindowId) {
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
    fn viewer_rotate(&mut self, id: WindowId, cw: bool) {
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

    fn viewer_set_mode(&mut self, id: WindowId, mode: FitMode) {
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

    fn viewer_toggle_info(&mut self, id: WindowId) {
        if let Some(v) = self.viewer_mut(id) {
            v.show_info = !v.show_info;
            v.info_t = Tween::at(if v.show_info { 0.0 } else { 1.0 });
            v.info_t
                .retarget(if v.show_info { 1.0 } else { 0.0 }, 0.2, curves::ENTER);
        }
    }

    fn viewer_toggle_slideshow(&mut self, id: WindowId) {
        let now = appui::ticks();
        if let Some(v) = self.viewer_mut(id)
            && v.list.len() > 1
        {
            v.slideshow.toggle(now);
        }
    }

    /// Keys of the viewer (after the global shortcuts).
    pub(crate) fn viewer_key(&mut self, id: WindowId, key: Key) {
        let shift = self.keymap.shift();
        let (vpw, vph) = self.viewer_vp(id);
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        // The save-as sheet owns the keys while it is open.
        if let Some(input) = v.save.as_mut() {
            match key {
                Key::Esc => v.save = None,
                Key::Enter => self.viewer_save_commit(id),
                Key::Backspace => input.backspace(),
                Key::Delete => input.delete(),
                Key::Left => input.left(),
                Key::Right => input.right(),
                Key::Home => input.home(),
                Key::End => input.end(),
                Key::Char(b) => input.insert(b),
                _ => {}
            }
            return;
        }
        v.msg = None;
        let dims = v.image.as_ref().map(|i| (i.width(), i.height()));
        // Any key but the slideshow's own stops it; Esc then only stops it.
        let was_running = v.slideshow.running();
        if was_running && !matches!(key, Key::Char(b' ')) {
            v.slideshow.stop();
        }
        v.inertia.stop();
        match key {
            Key::Esc if was_running => {}
            Key::Esc => self.request_close(id),
            Key::Char(b' ') => self.viewer_toggle_slideshow(id),
            Key::Char(b'+') | Key::Char(b'=') => {
                if let Some((iw, ih)) = dims {
                    v.view.zoom_in(iw, ih, vpw, vph);
                }
            }
            Key::Char(b'-') | Key::Char(b'_') => {
                if let Some((iw, ih)) = dims {
                    v.view.zoom_out(iw, ih, vpw, vph);
                }
            }
            Key::Char(b'1') => self.viewer_set_mode(id, FitMode::Actual),
            Key::Char(b'0') => self.viewer_set_mode(id, FitMode::Fit),
            Key::Char(b'9') => self.viewer_set_mode(id, FitMode::Fill),
            Key::Char(b'r') | Key::Char(b'R') => self.viewer_rotate(id, !shift),
            Key::Char(b'h') | Key::Char(b'H') => {
                if let Some(img) = v.image.as_mut() {
                    img.flip_horizontal();
                    v.scaled = None;
                    v.enter_t = Tween::at(0.5);
                    v.enter_t.retarget(1.0, 0.2, curves::ENTER);
                }
            }
            Key::Char(b'v') | Key::Char(b'V') => {
                if let Some(img) = v.image.as_mut() {
                    img.flip_vertical();
                    v.scaled = None;
                    v.enter_t = Tween::at(0.5);
                    v.enter_t.retarget(1.0, 0.2, curves::ENTER);
                }
            }
            Key::Char(b'i') | Key::Char(b'I') => self.viewer_toggle_info(id),
            Key::Char(b's') | Key::Char(b'S') => self.viewer_save_prompt(id),
            Key::Char(b'w') | Key::Char(b'W') => {
                let path = v.path.clone();
                let r = self.set_wallpaper_path(&path);
                if let Some(v) = self.viewer_mut(id) {
                    v.msg = Some(match r {
                        Some(m) => (m, true),
                        None => (String::from(t!("viewer.wallpaper")), false),
                    });
                }
            }
            Key::Left if shift => {
                if let Some((iw, ih)) = dims {
                    v.view.pan_by(vw::PAN_STEP, 0, iw, ih, vpw, vph);
                }
            }
            Key::Right if shift => {
                if let Some((iw, ih)) = dims {
                    v.view.pan_by(-vw::PAN_STEP, 0, iw, ih, vpw, vph);
                }
            }
            Key::Up => {
                if let Some((iw, ih)) = dims {
                    v.view.pan_by(0, vw::PAN_STEP, iw, ih, vpw, vph);
                }
            }
            Key::Down => {
                if let Some((iw, ih)) = dims {
                    v.view.pan_by(0, -vw::PAN_STEP, iw, ih, vpw, vph);
                }
            }
            Key::Home => self.viewer_goto(id, 0),
            Key::End => {
                let last = self
                    .viewer_mut(id)
                    .map_or(0, |v| v.list.len().saturating_sub(1));
                self.viewer_goto(id, last);
            }
            Key::Left => self.viewer_step(id, false),
            Key::Right => self.viewer_step(id, true),
            _ => {}
        }
        self.viewer_sync(id);
    }

    /// PageUp / PageDown / numpad +/-: previous / next image, zoom.
    pub(crate) fn viewer_special(&mut self, id: WindowId, sp: Special) -> bool {
        match sp {
            Special::PageUp => self.viewer_step(id, false),
            Special::PageDown => self.viewer_step(id, true),
            Special::KpPlus => self.viewer_key(id, Key::Char(b'+')),
            Special::KpMinus => self.viewer_key(id, Key::Char(b'-')),
            _ => return false,
        }
        true
    }

    /// Mouse wheel over the viewer: zoom around the cursor.
    pub(crate) fn viewer_wheel(&mut self, id: WindowId, notches: i32) {
        let Some(lay) = self.viewer_layout(id) else {
            return;
        };
        let (vpw, vph) = (lay.canvas.w, lay.canvas.h);
        // Over the filmstrip the wheel scrolls it.
        if lay
            .strip
            .is_some_and(|s| s.contains(self.cursor_x, self.cursor_y))
        {
            if let Some(v) = self.viewer_mut(id) {
                let max = (vui::strip_content_w(v.list.len()) - lay.strip.map_or(0, |s| s.w)).max(0)
                    as f32;
                let t = (v.strip_scroll.target() + notches as f32 * 64.0).clamp(0.0, max);
                v.strip_scroll.set_target(t);
            }
            return;
        }
        let ax = self.cursor_x - (lay.canvas.x + vpw / 2);
        let ay = self.cursor_y - (lay.canvas.y + vph / 2);
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        let Some((iw, ih)) = v.image.as_ref().map(|i| (i.width(), i.height())) else {
            return;
        };
        v.inertia.stop();
        v.slideshow.stop();
        for _ in 0..notches.unsigned_abs().min(8) {
            let z = if notches > 0 {
                vw::next_zoom(v.view.zoom)
            } else {
                vw::prev_zoom(v.view.zoom)
            };
            v.view.set_zoom_at(z, (ax, ay), iw, ih, vpw, vph);
        }
        self.viewer_sync(id);
    }

    /// Pan by a mouse drag step (the picture follows the pointer exactly; the speed feeds the
    /// glide that starts when the button is released).
    pub(crate) fn viewer_pan(&mut self, id: WindowId, dx: i32, dy: i32) {
        let (vpw, vph) = self.viewer_vp(id);
        let now = appui::ticks();
        if let Some(v) = self.viewer_mut(id)
            && let Some((iw, ih)) = v.image.as_ref().map(|i| (i.width(), i.height()))
        {
            let dt = now.saturating_sub(v.drag_tick).max(1) as f32 / 250.0;
            v.drag_tick = now;
            let before = (v.view.pan_x, v.view.pan_y);
            v.view.pan_by(dx, dy, iw, ih, vpw, vph);
            // Only the movement that took effect counts towards the speed.
            v.inertia
                .push(v.view.pan_x - before.0, v.view.pan_y - before.1, dt);
            v.pan_x_s.jump(v.view.pan_x as f32);
            v.pan_y_s.jump(v.view.pan_y as f32);
        }
    }

    /// The left button was released after a drag on the picture: let it glide.
    pub(crate) fn viewer_release(&mut self, id: WindowId) {
        let now = appui::ticks();
        if let Some(v) = self.viewer_mut(id) {
            let idle = now.saturating_sub(v.drag_tick) as f32 / 250.0;
            v.inertia.release(idle);
        }
    }

    /// A left press inside the viewer window (content area).
    pub(crate) fn viewer_click(&mut self, id: WindowId, _rect: Rect, px: i32, py: i32) {
        let Some(lay) = self.viewer_layout(id) else {
            return;
        };
        let (count, scroll, saving) = match self.viewer_mut(id) {
            Some(v) => (
                v.list.len(),
                v.strip_scroll.value() as i32,
                v.save.is_some(),
            ),
            None => return,
        };
        if saving {
            self.viewer_sheet_click(id, &lay, px, py);
            return;
        }
        let Some(hit) = lay.hit(px, py, scroll, count) else {
            return;
        };
        let has_image = self.viewer_mut(id).is_some_and(|v| v.image.is_some());
        match hit {
            Hit::RotateLeft if has_image => self.viewer_rotate(id, false),
            Hit::RotateRight if has_image => self.viewer_rotate(id, true),
            Hit::Flip if has_image => self.viewer_key(id, Key::Char(b'h')),
            Hit::Mode(m) if has_image => self.viewer_set_mode(id, m),
            Hit::Slideshow => self.viewer_toggle_slideshow(id),
            Hit::Info if has_image => self.viewer_toggle_info(id),
            Hit::Save if has_image => self.viewer_save_prompt(id),
            Hit::Thumb(i) => {
                if let Some(v) = self.viewer_mut(id) {
                    v.slideshow.stop();
                }
                self.viewer_goto(id, i);
            }
            Hit::Canvas => {
                let double = self.clicks.press(crate::interrupts::ticks(), px, py, id);
                if double {
                    let fit = self.viewer_mut(id).is_some_and(|v| v.view.fit);
                    self.viewer_set_mode(id, if fit { FitMode::Actual } else { FitMode::Fit });
                    return;
                }
                if let Some(v) = self.viewer_mut(id) {
                    v.inertia.grab();
                    v.drag_tick = appui::ticks();
                    v.slideshow.stop();
                }
                self.drag = Some(Drag {
                    win: id,
                    mode: DragMode::Pan {
                        last_x: px,
                        last_y: py,
                    },
                });
            }
            _ => {}
        }
        self.viewer_sync(id);
    }

    /// A press while the save sheet is up: its buttons.
    fn viewer_sheet_click(&mut self, id: WindowId, lay: &Layout, px: i32, py: i32) {
        let panel = appui::sheet_rect(lay.window, (420, 176));
        let btns = appui::button_row(
            panel.right() - appui::SHEET_PAD,
            panel.bottom() - appui::SHEET_PAD - appui::BUTTON_H,
            &[t!("common.cancel"), t!("viewer.save")],
        );
        match btns.iter().position(|b| b.contains(px, py)) {
            Some(0) => {
                if let Some(v) = self.viewer_mut(id) {
                    v.save = None;
                }
            }
            Some(_) => self.viewer_save_commit(id),
            None => {}
        }
    }

    /// The pointer moved over viewer `id`.
    pub(crate) fn viewer_hover(&mut self, id: WindowId, px: i32, py: i32) -> bool {
        let Some(lay) = self.viewer_layout(id) else {
            return false;
        };
        let Some(v) = self.viewer_mut(id) else {
            return false;
        };
        let hit = lay
            .hit(px, py, v.strip_scroll.value() as i32, v.list.len())
            .filter(|h| !matches!(h, Hit::Canvas | Hit::Dead | Hit::InfoPanel));
        if v.hover == hit {
            return false;
        }
        v.hover = hit;
        v.hover_t = Tween::at(0.0);
        v.hover_t.retarget(1.0, 0.12, curves::STANDARD);
        true
    }

    pub(crate) fn viewer_unhover(&mut self, id: WindowId) {
        if let Some(v) = self.viewer_mut(id) {
            v.hover = None;
        }
    }

    /// Refit viewers after a resize or maximize.
    pub(crate) fn relayout_viewer(&mut self, id: WindowId) {
        if self.kind_of(id) == Some(Kind::Viewer) {
            self.viewer_sync(id);
            self.viewer_aim_strip(id, false);
        }
    }

    // ---- per-frame state ----

    /// Advance every viewer's animations by `dt`, run the slideshow clock and make a thumbnail
    /// when one is due. Returns whether anything still moves.
    pub(crate) fn step_viewers(&mut self, dt: f32) -> bool {
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Viewer(_)) && w.shown())
            .map(|w| w.id)
            .collect();
        let now = appui::ticks();
        let mut busy = false;
        for id in ids {
            let (vpw, vph) = self.viewer_vp(id);
            // The slideshow turns the page.
            let due = self
                .viewer_mut(id)
                .is_some_and(|v| v.slideshow.due(now) && v.save.is_none());
            if due {
                self.viewer_step(id, true);
            }
            let Some(v) = self.viewer_mut(id) else {
                continue;
            };
            // A glide pans the picture, stopping at the edge.
            if v.inertia.active() {
                let (dx, dy) = v.inertia.step(dt);
                if let Some((iw, ih)) = v.image.as_ref().map(|i| (i.width(), i.height())) {
                    let before = (v.view.pan_x, v.view.pan_y);
                    v.view.pan_by(dx, dy, iw, ih, vpw, vph);
                    if (v.view.pan_x, v.view.pan_y) == before && (dx != 0 || dy != 0) {
                        v.inertia.stop();
                    }
                    v.pan_x_s.jump(v.view.pan_x as f32);
                    v.pan_y_s.jump(v.view.pan_y as f32);
                } else {
                    v.inertia.stop();
                }
            }
            busy |= v.zoom_s.step(dt);
            busy |= v.pan_x_s.step(dt);
            busy |= v.pan_y_s.step(dt);
            busy |= v.rot.step(dt);
            busy |= v.enter_t.step(dt);
            busy |= v.info_t.step(dt);
            busy |= v.sheet_t.step(dt);
            busy |= v.hover_t.step(dt);
            busy |= v.strip_scroll.step(dt);
            // One thumbnail every so often, nearest the current image first.
            if v.thumbs_pending() && now.saturating_sub(v.thumb_tick) >= THUMB_EVERY {
                v.thumb_tick = now;
                let order = vui::thumb_order(v.list.index(), v.list.len(), THUMB_LIMIT);
                if let Some(i) = order
                    .into_iter()
                    .find(|&i| matches!(v.thumbs.get(i), Some(Thumb::Pending)))
                {
                    let t = v.list.path_at(i).map_or(Thumb::Missing, |p| make_thumb(&p));
                    v.thumbs[i] = t;
                }
                // Images beyond the limit never get one: do not wait for them.
                let keep = vui::thumb_order(v.list.index(), v.list.len(), THUMB_LIMIT);
                for (i, t) in v.thumbs.iter_mut().enumerate() {
                    if matches!(t, Thumb::Pending) && !keep.contains(&i) {
                        *t = Thumb::Missing;
                    }
                }
            }
            busy |= v.animating();
        }
        busy
    }

    // ---- drawing ----

    pub(crate) fn draw_viewer(&self, c: &mut Canvas, r: Rect, v: &ViewerState) {
        let lay = Layout::of(r, v.list.len() > 1, self.viewer_info_rows(v));
        let p = theme::pal();
        self.draw_viewer_canvas(c, &lay, v);
        if let (Some(info), Some(img)) = (lay.info, &v.image) {
            self.draw_viewer_info(c, info, v, img, &lay);
        }
        self.draw_viewer_toolbar(c, &lay, v);
        // Bottom band: the filmstrip and the caption.
        let band_y = lay.canvas.bottom();
        ui::fill(
            c,
            Rect::new(r.x, band_y, r.w, r.bottom() - band_y),
            theme::toolbar(),
        );
        appui::hairline(c, r.x, band_y, r.w);
        if let Some(strip) = lay.strip {
            self.draw_viewer_strip(c, strip, v);
        }
        self.draw_viewer_caption(c, &lay, v);
        if v.save.is_some() {
            self.draw_viewer_sheet(c, r, v);
        }
        let _ = p;
    }

    fn draw_viewer_toolbar(&self, c: &mut Canvas, lay: &Layout, v: &ViewerState) {
        ui::fill(c, lay.toolbar, theme::toolbar());
        appui::hairline(c, lay.toolbar.x, lay.toolbar.bottom() - 1, lay.toolbar.w);
        let has = v.image.is_some();
        let hv = |h: Hit| -> u32 {
            if v.hover == Some(h) {
                level(&v.hover_t)
            } else {
                0
            }
        };
        appui::tool_button(
            c,
            lay.rot_left,
            Tool::RotateLeft,
            has,
            hv(Hit::RotateLeft),
            false,
            false,
        );
        appui::tool_button(
            c,
            lay.rot_right,
            Tool::RotateRight,
            has,
            hv(Hit::RotateRight),
            false,
            false,
        );
        appui::tool_button(c, lay.flip, Tool::FlipH, has, hv(Hit::Flip), false, false);
        let sel = vui::mode_of(&v.view).map_or(usize::MAX, FitMode::index);
        ui::segmented(
            c,
            lay.modes,
            &[t!("viewer.fit"), t!("viewer.fill"), "100%"],
            sel,
        );
        let multi = v.list.len() > 1;
        appui::tool_button(
            c,
            lay.slideshow,
            if v.slideshow.running() {
                Tool::Pause
            } else {
                Tool::Play
            },
            multi,
            hv(Hit::Slideshow),
            false,
            v.slideshow.running(),
        );
        appui::tool_button(
            c,
            lay.info_btn,
            Tool::Info,
            has,
            hv(Hit::Info),
            false,
            v.show_info,
        );
        appui::tool_button(c, lay.save, Tool::Save, has, hv(Hit::Save), false, false);
    }

    fn draw_viewer_canvas(&self, c: &mut Canvas, lay: &Layout, v: &ViewerState) {
        let cv = lay.canvas;
        if cv.w <= 0 || cv.h <= 0 {
            return;
        }
        ui::fill(c, cv, canvas_bg());
        let saved = c.set_clip(
            cv.intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        );
        match &v.image {
            Some(img) => self.paint_image(c, cv, v, img),
            None => {
                let p = theme::pal();
                let [l1, l2] = v.error.clone().unwrap_or_else(|| {
                    [
                        String::from(t!("viewer.empty_title")),
                        String::from(t!("viewer.empty_hint")),
                    ]
                });
                let cx = cv.x + cv.w / 2;
                let top = cv.y + (cv.h - 150) / 2;
                appui::blit_tool_dim(c, Tool::Warning, cx - 22, top, 44, 0xF59E0B, 230);
                let t = text::ellipsize(&l1, CALLOUT, Weight::Semibold, cv.w - 48);
                let tw = text::measure(&t, CALLOUT, Weight::Semibold);
                text::draw(
                    c,
                    cx - tw / 2,
                    top + 60,
                    &t,
                    CALLOUT,
                    Weight::Semibold,
                    theme::solid(p.text),
                );
                let name = String::from_utf8_lossy(vfs::base_name(&v.path)).into_owned();
                let max_w = (cv.w - 64).clamp(120, 380);
                let mut y = top + 86;
                for (a, b) in text::wrap(&l2, FOOTNOTE, Weight::Regular, max_w, 3) {
                    let s = l2[a..b].trim_end();
                    let sw = text::measure(s, FOOTNOTE, Weight::Regular);
                    text::draw(
                        c,
                        cx - sw / 2,
                        y,
                        s,
                        FOOTNOTE,
                        Weight::Regular,
                        theme::solid(p.text_secondary),
                    );
                    y += text::line_height(FOOTNOTE) + 2;
                }
                if !name.is_empty() {
                    let n = text::ellipsize_middle(&name, FOOTNOTE, Weight::Medium, max_w);
                    let nw = text::measure(&n, FOOTNOTE, Weight::Medium);
                    text::draw(
                        c,
                        cx - nw / 2,
                        y + 6,
                        &n,
                        FOOTNOTE,
                        Weight::Medium,
                        theme::solid(p.text_tertiary),
                    );
                }
            }
        }
        c.restore_clip(saved);
    }

    fn draw_viewer_info(
        &self,
        c: &mut Canvas,
        info: Rect,
        v: &ViewerState,
        img: &Image,
        lay: &Layout,
    ) {
        let p = theme::pal();
        let t = level(&v.info_t);
        if t == 0 {
            return;
        }
        let slide = ((256 - t as i32) * 14) / 256;
        let r = Rect::new(info.x + slide, info.y, info.w, info.h);
        c.draw_shadow(
            r,
            Shadow {
                blur: 16,
                dy: 6,
                alpha: 90 * t / 256,
            },
            Rect::new(r.x, r.y + 8, r.w, (r.h - 16).max(0)),
        );
        // Translucent: the picture shows faintly through.
        let (col, a) = theme::tint(p.menu_tint);
        c.fill_rrect(r, 12, Corner::Circle, col, (a as u32 * t / 256) as u16);
        let (ec, ea) = theme::tint(p.separator);
        c.stroke_rrect(
            r,
            12,
            Corner::Circle,
            ec,
            (ea as u32 * t / 256).max(40) as u16,
        );
        let name = vfs::base_name(&v.path);
        let rows = vw::info_rows(
            name,
            img.width(),
            img.height(),
            v.format,
            v.file_bytes,
            v.view.zoom,
            (v.list.index(), v.list.len()),
            !v.opaque,
        );
        let saved = c.set_clip(
            r.intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        );
        let a16 = t as u16;
        let mut y = r.y + vui::INFO_PAD;
        // The widest label sets the column (English labels are not as long as the Portuguese).
        let label_w = rows
            .iter()
            .map(|(k, _)| text::measure(k, FOOTNOTE, Weight::Regular))
            .max()
            .unwrap_or(0)
            .max(60)
            + 14;
        for (k, val) in &rows {
            let row = Rect::new(r.x + 14, y, r.w - 28, vui::INFO_ROW_H);
            text::draw_a(
                c,
                row.x,
                text::center_y(row.y, row.h, FOOTNOTE, Weight::Regular),
                k,
                FOOTNOTE,
                Weight::Regular,
                theme::solid(p.text_secondary),
                a16,
            );
            let room = row.w - label_w;
            let s = text::ellipsize_middle(val, FOOTNOTE, Weight::Medium, room);
            let sw = text::measure(&s, FOOTNOTE, Weight::Medium);
            text::draw_a(
                c,
                row.right() - sw,
                text::center_y(row.y, row.h, FOOTNOTE, Weight::Medium),
                &s,
                FOOTNOTE,
                Weight::Medium,
                theme::solid(p.text),
                a16,
            );
            y += vui::INFO_ROW_H;
        }
        c.restore_clip(saved);
        let _ = lay;
    }

    fn draw_viewer_strip(&self, c: &mut Canvas, strip: Rect, v: &ViewerState) {
        let p = theme::pal();
        let scroll = v.strip_scroll.value() as i32;
        let n = v.list.len();
        let (a, b) = vui::strip_visible(strip.w, n, scroll);
        let saved = c.set_clip(
            strip
                .intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        );
        let acc = theme::accent();
        for i in a..b {
            let r = vui::thumb_rect(strip, scroll, i);
            let current = i == v.list.index();
            let hovered = v.hover == Some(Hit::Thumb(i)) && !current;
            match v.thumbs.get(i) {
                Some(Thumb::Ready(s)) => {
                    c.blit_surface(s, r.x, r.y, if current { 256 } else { 220 });
                }
                other => {
                    let (col, al) = theme::tint(p.hover);
                    c.fill_rrect(r, 8, Corner::Circle, col, al * 2);
                    let _ = other;
                    appui::blit_tool_dim(
                        c,
                        Tool::Photo,
                        r.x + (r.w - 18) / 2,
                        r.y + (r.h - 18) / 2,
                        18,
                        appui::rgb_of(theme::solid(p.text_tertiary)),
                        256,
                    );
                }
            }
            if hovered {
                let (col, al) = theme::tint(p.hover);
                c.fill_rrect(r, 8, Corner::Circle, col, al);
            }
            if current {
                c.stroke_rrect(r.inflated(2), 10, Corner::Circle, acc, 256);
                c.stroke_rrect(r.inflated(3), 11, Corner::Circle, acc, 110);
            }
        }
        c.restore_clip(saved);
    }

    fn draw_viewer_caption(&self, c: &mut Canvas, lay: &Layout, v: &ViewerState) {
        let cap = lay.caption;
        let p = theme::pal();
        let (s, col): (String, Color) = match &v.msg {
            Some((m, true)) => (m.clone(), theme::danger()),
            Some((m, false)) => (m.clone(), theme::ok()),
            None => {
                let name = String::from_utf8_lossy(vfs::base_name(&v.path)).into_owned();
                let mut s = name;
                if let Some(img) = &v.image {
                    s.push_str(&alloc::format!(
                        "  ·  {} × {}  ·  {}",
                        img.width(),
                        img.height(),
                        vw::zoom_label(v.view.zoom)
                    ));
                }
                if v.list.len() > 1 {
                    s.push_str("  ·  ");
                    s.push_str(&t!(
                        "viewer.position",
                        n = v.list.index() + 1,
                        total = v.list.len()
                    ));
                }
                (s, theme::solid(p.text_secondary))
            }
        };
        let shown = text::ellipsize(&s, FOOTNOTE, Weight::Regular, cap.w - 32);
        let w = text::measure(&shown, FOOTNOTE, Weight::Regular);
        text::draw(
            c,
            cap.x + (cap.w - w) / 2,
            text::center_y(cap.y, cap.h, FOOTNOTE, Weight::Regular),
            &shown,
            FOOTNOTE,
            Weight::Regular,
            col,
        );
    }

    fn draw_viewer_sheet(&self, c: &mut Canvas, r: Rect, v: &ViewerState) {
        let Some(input) = &v.save else {
            return;
        };
        let t = level(&v.sheet_t);
        let panel = appui::sheet(c, r, (420, 176), t);
        let saved = c.set_clip(
            panel
                .intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        );
        let p = theme::pal();
        text::draw(
            c,
            panel.x + appui::SHEET_PAD,
            panel.y + appui::SHEET_PAD,
            t!("viewer.save_as"),
            text::TITLE3,
            Weight::Semibold,
            theme::solid(p.text),
        );
        let field = Rect::new(
            panel.x + appui::SHEET_PAD,
            panel.y + appui::SHEET_PAD + 34,
            panel.w - 2 * appui::SHEET_PAD,
            30,
        );
        let s = input.to_string_lossy();
        appui::field(
            c,
            field,
            &appui::FieldText {
                text: &s,
                caret: input.caret(),
                selection: input.selection(),
            },
            t!("viewer.file_name"),
            true,
            256,
            None,
            false,
        );
        let hint = match &v.msg {
            Some((m, true)) => (m.as_str(), theme::danger()),
            _ => (t!("viewer.formats"), theme::solid(p.text_tertiary)),
        };
        text::draw_ellipsis(
            c,
            field.x,
            field.bottom() + 8,
            field.w,
            hint.0,
            FOOTNOTE,
            Weight::Regular,
            hint.1,
        );
        let btns = appui::button_row(
            panel.right() - appui::SHEET_PAD,
            panel.bottom() - appui::SHEET_PAD - appui::BUTTON_H,
            &[t!("common.cancel"), t!("viewer.save")],
        );
        let hover = |b: &Rect| b.contains(self.cursor_x, self.cursor_y);
        appui::sheet_button(
            c,
            btns[0],
            t!("common.cancel"),
            ButtonKind::Secondary,
            hover(&btns[0]),
            false,
        );
        appui::sheet_button(
            c,
            btns[1],
            t!("viewer.save"),
            ButtonKind::Primary,
            hover(&btns[1]),
            false,
        );
        c.restore_clip(saved);
    }

    /// Paint the picture of `v` into the canvas `cv`: at the zoom and pan being drawn, turned
    /// by the animated angle, faded in after a change of image.
    fn paint_image(&self, c: &mut Canvas, cv: Rect, v: &ViewerState, img: &Image) {
        let (vpw, vph) = (cv.w, cv.h);
        let (iw, ih) = (img.width(), img.height());
        let zoom = (v.zoom_s.value() + 0.5).max(1.0) as u32;
        let pan = (
            appui::round(v.pan_x_s.value()),
            appui::round(v.pan_y_s.value()),
        );
        let angle = appui::round(v.rot.value());
        let fade = level(&v.enter_t);
        let bgpx = 0xFF00_0000 | appui::rgb_of(canvas_bg());
        let compose = |p: u32, x: i32, y: i32, ox: i32, oy: i32| -> u32 {
            let px = if v.opaque || image::alpha(p) == 255 {
                p | 0xFF00_0000
            } else {
                let back = checker(vw::checker_dark(x - ox, y - oy));
                image::over(p, back)
            };
            if fade >= 256 {
                px
            } else {
                kitsune_core::raster::lerp(bgpx, px, fade) | 0xFF00_0000
            }
        };
        if angle != 0 {
            // A turning picture: sample it through the rotation map.
            let b = vui::rotated_bounds(vpw, vph, pan, iw, ih, zoom, angle);
            if b.w <= 0 || b.h <= 0 {
                return;
            }
            let map = vui::RotMap::new(vpw, vph, pan, iw, ih, zoom, angle);
            let (su, sv) = map.step_x();
            let mut line: Vec<u32> = alloc::vec![0; b.w as usize];
            for y in b.y..b.bottom() {
                let (mut u, mut vv) = map.at(b.x, y);
                for px in line.iter_mut() {
                    *px = match vui::RotMap::pixel(u, vv, iw, ih) {
                        Some((sx, sy)) => {
                            let p = img.row(sy).get(sx).copied().unwrap_or(0);
                            compose(p, sx as i32, sy as i32, 0, 0)
                        }
                        None => bgpx,
                    };
                    u += su;
                    vv += sv;
                }
                c.put_row((cv.x + b.x) as usize, (cv.y + y) as usize, &line);
            }
            return;
        }
        let view = vw::View {
            zoom,
            fit: false,
            fill: false,
            pan_x: pan.0,
            pan_y: pan.1,
        };
        let (ox, oy) = view.origin(iw, ih, vpw, vph);
        let (sw, sh) = view.scaled(iw, ih);
        // At rest below 100 % the box-filtered copy gives the best quality; in flight and above
        // 100 % the source is sampled directly.
        let (src, szoom) = match &v.scaled {
            Some((z, s)) if *z == zoom && zoom < 1000 && v.zoom_s.at_rest() => (s, 1000),
            _ => (img, zoom),
        };
        let (x0, x1) = (ox.max(0), (ox + sw).min(vpw));
        let (y0, y1) = (oy.max(0), (oy + sh).min(vph));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        // A soft shadow under a picture that sits inside the canvas.
        if ox >= 0 && oy >= 0 && ox + sw <= vpw && oy + sh <= vph {
            let r = Rect::new(cv.x + ox, cv.y + oy, sw, sh);
            c.draw_shadow(
                r,
                Shadow {
                    blur: 14,
                    dy: 5,
                    alpha: if theme::dark() { 150 } else { 70 },
                },
                r,
            );
        }
        let cols = vw::column_map(x0, x1, ox, szoom, src.width());
        let mut line: Vec<u32> = alloc::vec![0; cols.len()];
        for y in y0..y1 {
            let sy = (((y - oy) as u64 * 1000 / szoom.max(1) as u64) as usize)
                .min(src.height().saturating_sub(1));
            let row = src.row(sy);
            for (k, col) in cols.iter().enumerate() {
                let p = col.and_then(|i| row.get(i as usize).copied()).unwrap_or(0);
                line[k] = compose(p, x0 + k as i32, y, ox, oy);
            }
            c.put_row((cv.x + x0) as usize, (cv.y + y) as usize, &line);
        }
    }
}

/// A filmstrip thumbnail of the picture at `path`: the picture scaled to cover a square and
/// cropped, with rounded corners. `Missing` when it is too big or cannot be read.
fn make_thumb(path: &[u8]) -> Thumb {
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
