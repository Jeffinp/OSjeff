//! Drawing the viewer: toolbar, canvas, info panel, filmstrip, caption and sheets.

use crate::desktop::kit::appui::{self, level};
use crate::desktop::kit::ui::ButtonKind;
use crate::desktop::*;
use crate::text::{self, CALLOUT, FOOTNOTE, Weight};
use kitsune_core::appart::Tool;
use kitsune_core::image::{self, Image};
use kitsune_core::t;
use kitsune_core::viewer as vw;
use kitsune_core::viewer::ui::{self as vui, FitMode, Hit, Layout};

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
