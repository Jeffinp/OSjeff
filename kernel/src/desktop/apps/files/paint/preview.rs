//! The preview pane and the status bar.

use super::helpers::tertiary;
use crate::desktop::kit::appui::{self};
use crate::desktop::*;
use crate::text::{self, BODY, CALLOUT, FOOTNOTE, Weight};
use kitsune_core::fileman::ui::Layout;
use kitsune_core::{t, tp};

impl Desktop {
    // ------------------------------------------------------------------ preview

    pub(super) fn draw_files_preview(&self, c: &mut Canvas, pane: Rect, st: &FilesState) {
        let p = theme::pal();
        ui::fill(c, pane, theme::sidebar());
        ui::fill_token(c, Rect::new(pane.x, pane.y, 1, pane.h), 0, p.separator);
        let saved = c.set_clip(
            pane.intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        );
        let inner = Rect::new(pane.x + 16, pane.y + 16, pane.w - 32, pane.h - 32);
        let Some(d) = st.preview.as_ref() else {
            c.restore_clip(saved);
            return;
        };
        if d.name.is_empty() {
            text::draw_centered(
                c,
                Rect::new(pane.x, pane.y, pane.w, pane.h),
                t!("files.preview.select"),
                BODY,
                Weight::Regular,
                tertiary(),
            );
            c.restore_clip(saved);
            return;
        }
        let mut y = inner.y;
        // Picture area.
        let box_h = 156;
        let card = Rect::new(inner.x, y, inner.w, box_h);
        if let Some(img) = &d.image {
            ui::fill_token(c, card, 8, p.content_bg);
            let ix = card.x + (card.w - img.w as i32) / 2;
            let iy = card.y + (card.h - img.h as i32) / 2;
            let clip = Rect::new(ix, iy, img.w as i32, img.h as i32);
            let saved2 = c.set_clip(
                clip.intersection(&c.clip_rect())
                    .unwrap_or(Rect::new(0, 0, 0, 0)),
            );
            c.blit_surface(img, ix, iy, 256);
            c.restore_clip(saved2);
            ui::stroke_token(c, card, 8, p.separator);
        } else if !d.lines.is_empty() {
            ui::fill_token(c, card, 8, p.content_bg);
            ui::stroke_token(c, card, 8, p.separator);
            let saved2 = c.set_clip(
                Rect::new(card.x + 1, card.y + 1, card.w - 2, card.h - 2)
                    .intersection(&c.clip_rect())
                    .unwrap_or(Rect::new(0, 0, 0, 0)),
            );
            let lh = 11;
            for (k, l) in d.lines.iter().take(12).enumerate() {
                text::draw_mono(
                    c,
                    card.x + 8,
                    card.y + 8 + k as i32 * lh,
                    l,
                    9,
                    theme::text_muted(),
                );
            }
            c.restore_clip(saved2);
        } else {
            kit::appart::blit_file(
                c,
                d.kind,
                card.x + (card.w - 64) / 2,
                card.y + (card.h - 64) / 2,
                64,
                256,
            );
        }
        y = card.bottom() + 14;
        // Title and kind.
        let lines = text::wrap(&d.name, CALLOUT, Weight::Semibold, inner.w, 2);
        for (s, e) in &lines {
            let piece = d.name[*s..*e].trim_end();
            let tw = text::measure(piece, CALLOUT, Weight::Semibold);
            text::draw(
                c,
                inner.x + (inner.w - tw.min(inner.w)) / 2,
                y,
                &text::ellipsize(piece, CALLOUT, Weight::Semibold, inner.w),
                CALLOUT,
                Weight::Semibold,
                theme::text(),
            );
            y += text::line_height(CALLOUT) + 1;
        }
        let kw = text::measure(&d.kind_label, FOOTNOTE, Weight::Regular);
        text::draw(
            c,
            inner.x + (inner.w - kw.min(inner.w)) / 2,
            y + 2,
            &text::ellipsize(&d.kind_label, FOOTNOTE, Weight::Regular, inner.w),
            FOOTNOTE,
            Weight::Regular,
            theme::text_muted(),
        );
        y += 26;
        if let Some(note) = &d.note {
            text::draw_ellipsis(
                c,
                inner.x,
                y,
                inner.w,
                note,
                FOOTNOTE,
                Weight::Regular,
                tertiary(),
            );
            y += 22;
        }
        appui::hairline(c, inner.x, y, inner.w);
        y += 8;
        for (label, value) in &d.info {
            let lw = text::measure(label, FOOTNOTE, Weight::Regular) + 8;
            text::draw(c, inner.x, y, label, FOOTNOTE, Weight::Regular, tertiary());
            let room = (inner.w - lw).max(10);
            let v = text::ellipsize(value, FOOTNOTE, Weight::Regular, room);
            let vw = text::measure(&v, FOOTNOTE, Weight::Regular);
            text::draw(
                c,
                inner.right() - vw,
                y,
                &v,
                FOOTNOTE,
                Weight::Regular,
                theme::text(),
            );
            y += 22;
        }
        c.restore_clip(saved);
    }

    // ------------------------------------------------------------------- status

    pub(super) fn draw_files_status(&self, c: &mut Canvas, lay: &Layout, st: &FilesState) {
        let s = lay.status;
        appui::hairline(c, s.x, s.y, s.w);
        let ty = text::center_y(s.y, s.h, FOOTNOTE, Weight::Regular);
        let mut summary = st.view.summary();
        if !st.view.filter().is_empty() {
            summary = tp!(
                "files.filtered",
                st.view.total_rows(),
                shown = st.view.rows.len()
            );
        }
        let w = text::draw(
            c,
            s.x + 16,
            ty,
            &summary,
            FOOTNOTE,
            Weight::Regular,
            theme::text_muted(),
        );
        if let Some((m, err)) = &st.msg {
            let x = s.x + 16 + w + 14;
            let col = if *err { theme::danger() } else { theme::ok() };
            text::draw_ellipsis(
                c,
                x,
                ty,
                (s.right() - 16 - x).max(0),
                m,
                FOOTNOTE,
                Weight::Medium,
                col,
            );
        } else if st.view.in_apps() {
            let hint = t!("files.hint.apps");
            let x = s.x + 16 + w + 14;
            text::draw_ellipsis(
                c,
                x,
                ty,
                (s.right() - 16 - x).max(0),
                hint,
                FOOTNOTE,
                Weight::Regular,
                tertiary(),
            );
        }
    }
}
