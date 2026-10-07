//! `Desktop` methods: the image viewer (Kind `Viewer`). Decoding, zoom and pan
//! decisions are `osjeff_core::image` / `osjeff_core::viewer`; this module loads the
//! file through the VFS, keeps the window's state and paints the pixels.
//!
//! Drawing: below 100 % the image is box-filtered once per zoom change (cached in
//! `ViewerState::scaled`); at 100 % and above it is sampled nearest-neighbour
//! straight from the source, so panning a huge zoom costs nothing extra. Rows are
//! converted and written with `Canvas::put_row`; transparency is composited over a
//! checkerboard per pixel.

use super::*;
use osjeff_core::fileman::{self, TextInput};
use osjeff_core::image::{self, Filter, Format, Image};
use osjeff_core::viewer as vw;

/// Height of the bottom status bar.
const STATUS_H: i32 = 24;
/// Largest file the viewer reads (the decoder bounds the pixels on its own; the kernel
/// heap is 64 MiB, so the file and the decoded pixels must both fit).
const MAX_FILE: u64 = 24 * 1024 * 1024;

const BG: Color = Color::rgb(0x1B, 0x1F, 0x27);
const BAR: Color = Color::rgb(0x12, 0x16, 0x22);
const CHECK_A: u32 = 0xFF_C9_CC_D2;
const CHECK_B: u32 = 0xFF_9C_A1_AB;

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
            return Some(String::from("Janelas demais abertas"));
        };
        self.viewer_load(id, path, true);
        None
    }

    /// Viewport (the area the image lives in) of window `id`.
    fn viewer_vp(&self, id: WindowId) -> (i32, i32) {
        self.wm.get(id).map_or((640, 440), |w| {
            (w.rect.w - 2, w.rect.h - TITLE_H - STATUS_H)
        })
    }

    /// Load `path` into window `id`. `rebuild` re-reads the folder for the
    /// previous/next list; a failure shows in the window (never panics).
    pub(crate) fn viewer_load(&mut self, id: WindowId, path: &[u8], rebuild: bool) {
        let loaded: Result<(Image, Option<Format>, u64), [String; 2]> = (|| {
            let info = vfs::stat(path).map_err(|e| [String::from(e.message()), String::new()])?;
            if info.size > MAX_FILE {
                return Err([
                    String::from("Arquivo grande demais para o visualizador"),
                    String::new(),
                ]);
            }
            let bytes =
                vfs::read_file(path).map_err(|e| [String::from(e.message()), String::new()])?;
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
        let title = alloc::format!(
            "IMAGENS - {}",
            String::from_utf8_lossy(&fileman::ellipsize(&fileman::display_ascii(&name), 40))
        );
        let (vpw, vph) = self.viewer_vp(id);
        if let Some(w) = self.wm.get_mut(id) {
            w.app.title = title;
        }
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        v.path = path.to_vec();
        v.scaled = None;
        v.save = None;
        v.msg = None;
        if let Some(l) = list {
            v.list = l;
        }
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
        self.viewer_sync(id);
    }

    /// After a zoom, rotate or resize: refit or re-clamp, and rebuild the
    /// downscaled copy when the zoom is below 100 %.
    pub(crate) fn viewer_sync(&mut self, id: WindowId) {
        let (vpw, vph) = self.viewer_vp(id);
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        let Some(img) = &v.image else {
            return;
        };
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

    fn viewer_step(&mut self, id: WindowId, forward: bool) {
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        let next = if forward {
            v.list.go_next()
        } else {
            v.list.go_prev()
        };
        if let Some(p) = next
            && v.list.len() > 1
        {
            self.viewer_load(id, &p, false);
        }
    }

    pub(crate) fn viewer_save_prompt(&mut self, id: WindowId) {
        if let Some(v) = self.viewer_mut(id)
            && v.image.is_some()
        {
            let sugg = vw::suggest_save_name(vfs::base_name(&v.path));
            v.save = Some(TextInput::new(&sugg, vfs::MAX_NAME));
        }
    }

    /// Enter on the save-as prompt: encode the current (possibly rotated) image.
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
                return Err(String::from("Ja existe um arquivo com esse nome"));
            }
            let bytes =
                image::encode(img, fmt).map_err(|e| String::from(vw::image_error_message(e)))?;
            vfs::write_file(&path, &bytes).map_err(|e| String::from(e.message()))?;
            Ok(path)
        })();
        match result {
            Ok(p) => {
                v.msg = Some((
                    alloc::format!(
                        "Salvo: {}",
                        String::from_utf8_lossy(&fileman::display_ascii(vfs::base_name(&p)))
                    ),
                    false,
                ));
                self.fs_changed();
            }
            Err(m) => {
                // Keep the prompt so the name can be fixed.
                v.save = Some(input);
                v.msg = Some((m, true));
            }
        }
    }

    /// Keys of the viewer (after the global shortcuts).
    pub(crate) fn viewer_key(&mut self, id: WindowId, key: Key) {
        let shift = self.keymap.shift();
        let (vpw, vph) = self.viewer_vp(id);
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        // The save-as prompt owns the keys while it is open.
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
        match key {
            Key::Esc => self.request_close(id),
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
            Key::Char(b'1') => v.view.actual(),
            Key::Char(b'0') => {
                if let Some((iw, ih)) = dims {
                    v.view.fit_to(iw, ih, vpw, vph);
                }
            }
            Key::Char(b'r') | Key::Char(b'R') => {
                if let Some(img) = &v.image {
                    match if shift {
                        img.rotate270()
                    } else {
                        img.rotate90()
                    } {
                        Ok(n) => {
                            v.image = Some(n);
                            v.scaled = None;
                        }
                        Err(e) => v.msg = Some((String::from(vw::image_error_message(e)), true)),
                    }
                }
            }
            Key::Char(b'h') | Key::Char(b'H') => {
                if let Some(img) = v.image.as_mut() {
                    img.flip_horizontal();
                    v.scaled = None;
                }
            }
            Key::Char(b'v') | Key::Char(b'V') => {
                if let Some(img) = v.image.as_mut() {
                    img.flip_vertical();
                    v.scaled = None;
                }
            }
            Key::Char(b'i') | Key::Char(b'I') => v.show_info = !v.show_info,
            Key::Char(b's') | Key::Char(b'S') => self.viewer_save_prompt(id),
            Key::Char(b'w') | Key::Char(b'W') => {
                let path = v.path.clone();
                let r = self.set_wallpaper_path(&path);
                if let Some(v) = self.viewer_mut(id) {
                    v.msg = Some(match r {
                        Some(m) => (m, true),
                        None => (String::from("Papel de parede aplicado"), false),
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
        let (vpw, vph) = self.viewer_vp(id);
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let ax = self.cursor_x - (rect.x + 1 + vpw / 2);
        let ay = self.cursor_y - (rect.y + TITLE_H + vph / 2);
        let Some(v) = self.viewer_mut(id) else {
            return;
        };
        let Some((iw, ih)) = v.image.as_ref().map(|i| (i.width(), i.height())) else {
            return;
        };
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

    /// Pan by a mouse drag step.
    pub(crate) fn viewer_pan(&mut self, id: WindowId, dx: i32, dy: i32) {
        let (vpw, vph) = self.viewer_vp(id);
        if let Some(v) = self.viewer_mut(id)
            && let Some((iw, ih)) = v.image.as_ref().map(|i| (i.width(), i.height()))
        {
            v.view.pan_by(dx, dy, iw, ih, vpw, vph);
        }
    }

    /// A left press inside the viewer window (content area): start a drag, or
    /// toggle fit / 100 % on a double click.
    pub(crate) fn viewer_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        if py >= rect.bottom() - STATUS_H {
            return;
        }
        let double = self.clicks.press(crate::interrupts::ticks(), px, py, id);
        if double {
            let (vpw, vph) = self.viewer_vp(id);
            if let Some(v) = self.viewer_mut(id)
                && let Some((iw, ih)) = v.image.as_ref().map(|i| (i.width(), i.height()))
            {
                if v.view.fit {
                    v.view.actual();
                } else {
                    v.view.fit_to(iw, ih, vpw, vph);
                }
            }
            self.viewer_sync(id);
            return;
        }
        self.drag = Some(Drag {
            win: id,
            mode: DragMode::Pan {
                last_x: px,
                last_y: py,
            },
        });
    }

    /// Refit viewers after a resize or maximize.
    pub(crate) fn relayout_viewer(&mut self, id: WindowId) {
        if self.kind_of(id) == Some(Kind::Viewer) {
            self.viewer_sync(id);
        }
    }

    // ---- drawing ----

    pub(crate) fn draw_viewer(&self, c: &mut Canvas, r: Rect, v: &ViewerState) {
        let top = r.y + TITLE_H;
        let (vpw, vph) = (r.w - 2, r.h - TITLE_H - STATUS_H);
        let (bx, by) = (r.x + 1, top);
        if vpw <= 0 || vph <= 0 {
            return;
        }
        // Viewport background and status bar.
        c.fill_rect(
            bx.max(0) as usize,
            by.max(0) as usize,
            vpw as usize,
            vph as usize,
            BG,
        );
        let sy = r.bottom() - STATUS_H;
        c.fill_rect(
            bx.max(0) as usize,
            sy.max(0) as usize,
            vpw as usize,
            (STATUS_H - 1) as usize,
            BAR,
        );

        let name = vfs::base_name(&v.path);
        match &v.image {
            Some(img) => {
                self.paint_image(c, (bx, by), (vpw, vph), v, img);
                if v.show_info {
                    let lines = vw::info_lines(
                        name,
                        img.width(),
                        img.height(),
                        v.format,
                        v.file_bytes,
                        self.viewer_effective_zoom(v, img, vpw, vph),
                        (v.list.index(), v.list.len()),
                    );
                    let w = 232;
                    let h = 16 + lines.len() as i32 * 20;
                    let x = bx + vpw - w - 12;
                    let y = by + 12;
                    c.fill_round_rect_alpha(
                        x as usize,
                        y as usize,
                        w as usize,
                        h as usize,
                        8,
                        theme::HEADER,
                        220,
                    );
                    for (i, l) in lines.iter().enumerate() {
                        let cols = ((w - 20) / 12) as usize;
                        font::draw_bytes(
                            c,
                            (x + 10) as usize,
                            (y + 10 + i as i32 * 20) as usize,
                            &fileman::ellipsize(l.as_bytes(), cols),
                            theme::HEADER_TEXT,
                            2,
                        );
                    }
                }
            }
            None => {
                let [l1, l2] = v
                    .error
                    .clone()
                    .unwrap_or_else(|| [String::from("Nenhuma imagem"), String::new()]);
                let mid = by + vph / 2;
                let cols = (vpw / 12 - 2).max(8) as usize;
                for (i, l) in [l1, l2].iter().enumerate() {
                    let t = fileman::ellipsize(l.as_bytes(), cols);
                    let x = bx + (vpw - t.len() as i32 * 12) / 2;
                    let col = if i == 0 {
                        theme::CLOSE
                    } else {
                        theme::HEADER_TEXT
                    };
                    font::draw_bytes(
                        c,
                        x.max(0) as usize,
                        (mid - 14 + i as i32 * 24) as usize,
                        &t,
                        col,
                        2,
                    );
                }
            }
        }

        // Status line: name, size, zoom, position; hints or a message on the right.
        let mut left = fileman::display_ascii(name);
        if let Some(img) = &v.image {
            let z = self.viewer_effective_zoom(v, img, vpw, vph);
            let s = alloc::format!("  {}x{}  {}", img.width(), img.height(), vw::zoom_label(z));
            left.extend_from_slice(s.as_bytes());
        }
        if v.list.len() > 1 {
            let s = alloc::format!("  {}/{}", v.list.index() + 1, v.list.len());
            left.extend_from_slice(s.as_bytes());
        }
        let cols = (vpw / 12 - 2).max(8) as usize;
        let ty = (sy + (STATUS_H - 14) / 2) as usize;
        let lt = fileman::ellipsize(&left, cols / 2);
        font::draw_bytes(c, (bx + 10) as usize, ty, &lt, theme::HEADER_TEXT, 2);
        let (right, col): (String, Color) = match &v.msg {
            Some((m, true)) => (m.clone(), theme::CLOSE),
            Some((m, false)) => (m.clone(), theme::accent()),
            None => (
                String::from("+/- zoom  0 ajusta  1 real  R girar  I info  S salvar"),
                theme::TEXT_MUTED,
            ),
        };
        let room = cols.saturating_sub(lt.len() + 2);
        let rt = fileman::ellipsize(right.as_bytes(), room);
        let rx = bx + vpw - 10 - rt.len() as i32 * 12;
        font::draw_bytes(c, rx.max(0) as usize, ty, &rt, col, 2);

        if let Some(input) = &v.save {
            let w = (r.w - 80).clamp(260, 460);
            let (x, y) = (r.x + (r.w - w) / 2, r.y + (r.h - 96) / 2);
            c.fill_round_rect(
                x as usize - 3,
                y as usize - 3,
                (w + 6) as usize,
                102,
                12,
                theme::accent(),
            );
            c.fill_round_rect(
                x as usize,
                y as usize,
                w as usize,
                96,
                10,
                theme::WINDOW_BODY,
            );
            font::draw_text(
                c,
                (x + 14) as usize,
                (y + 10) as usize,
                "Salvar como (png, bmp, ppm)",
                theme::TEXT,
                2,
            );
            c.fill_round_rect(
                (x + 14) as usize,
                (y + 34) as usize,
                (w - 28) as usize,
                26,
                6,
                theme::ACCENT_2,
            );
            c.fill_round_rect(
                (x + 16) as usize,
                (y + 36) as usize,
                (w - 32) as usize,
                22,
                5,
                theme::WHITE,
            );
            let cols = ((w - 44) / 12).max(4) as usize;
            let disp = fileman::display_ascii(input.text());
            let caret = input.caret_column();
            let start = (caret + 1).saturating_sub(cols);
            let end = disp.len().min(start + cols);
            font::draw_bytes(
                c,
                (x + 22) as usize,
                (y + 40) as usize,
                &disp[start.min(disp.len())..end],
                theme::TEXT,
                2,
            );
            c.fill_rect(
                (x + 22) as usize + (caret - start) * 12,
                (y + 39) as usize,
                2,
                16,
                theme::ACCENT_2,
            );
            font::draw_text(
                c,
                (x + 14) as usize,
                (y + 70) as usize,
                "Enter salva   Esc cancela",
                theme::TEXT_MUTED,
                2,
            );
        }
    }

    /// The zoom the window shows right now (fit mode follows the live viewport).
    fn viewer_effective_zoom(&self, v: &ViewerState, img: &Image, vpw: i32, vph: i32) -> u32 {
        let mut view = v.view;
        view.relayout(img.width(), img.height(), vpw, vph);
        view.zoom
    }

    fn paint_image(
        &self,
        c: &mut Canvas,
        vp0: (i32, i32),
        vp: (i32, i32),
        v: &ViewerState,
        img: &Image,
    ) {
        let mut view = v.view;
        view.relayout(img.width(), img.height(), vp.0, vp.1);
        let (ox, oy) = view.origin(img.width(), img.height(), vp.0, vp.1);
        let (sw, sh) = view.scaled(img.width(), img.height());
        let (src, zoom) = match &v.scaled {
            Some((z, s)) if *z == view.zoom && view.zoom < 1000 => (s, 1000),
            _ => (img, view.zoom),
        };
        let (x0, x1) = (ox.max(0), (ox + sw).min(vp.0));
        let (y0, y1) = (oy.max(0), (oy + sh).min(vp.1));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let cols = vw::column_map(x0, x1, ox, zoom, src.width());
        let mut line: Vec<u32> = alloc::vec![0; cols.len()];
        for y in y0..y1 {
            let sy = (((y - oy) as u64 * 1000 / zoom.max(1) as u64) as usize).min(src.height() - 1);
            let row = src.row(sy);
            for (k, col) in cols.iter().enumerate() {
                let p = col.and_then(|i| row.get(i as usize).copied()).unwrap_or(0);
                line[k] = if v.opaque || image::alpha(p) == 255 {
                    p | 0xFF00_0000
                } else {
                    let x = x0 + k as i32;
                    let back = if vw::checker_dark(x - ox, y - oy) {
                        CHECK_B
                    } else {
                        CHECK_A
                    };
                    image::over(p, back)
                };
            }
            c.put_row((vp0.0 + x0) as usize, (vp0.1 + y) as usize, &line);
        }
    }
}
