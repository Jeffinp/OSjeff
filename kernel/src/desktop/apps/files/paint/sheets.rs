//! The confirmation, properties and copy-progress sheets.

use super::super::props::{SheetKind, files_sheet_kind};
use crate::desktop::kit::appui::{self};
use crate::desktop::kit::ui::ButtonKind;
use crate::desktop::*;
use crate::text::{self, BODY, FOOTNOTE, Weight};
use kitsune_core::{t, tp};

impl Desktop {
    // -------------------------------------------------------------------- sheets

    pub(super) fn draw_files_sheet(&self, c: &mut Canvas, r: Rect, st: &FilesState) {
        let (kind, size) = files_sheet_kind(st);
        let t = appui::level(&st.sheet_t);
        let panel = appui::sheet(c, r, size, t);
        let labels = kind.buttons();
        let btns = appui::button_row(
            panel.right() - appui::SHEET_PAD,
            panel.bottom() - appui::SHEET_PAD - appui::BUTTON_H,
            &labels,
        );
        let hover = |b: &Rect| b.contains(self.cursor_x, self.cursor_y);
        let saved = c.set_clip(
            panel
                .intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        );
        match kind {
            SheetKind::Confirm => {
                let what = |n: usize| tp!("files.confirm.purge_body", n);
                let (title, msg) = match &st.confirm {
                    Some(Confirm::Purge(p)) => (t!("files.confirm.purge_title"), what(p.len())),
                    Some(Confirm::PurgeTrash(p)) => {
                        (t!("files.confirm.purge_trash_title"), what(p.len()))
                    }
                    _ => (
                        t!("files.confirm.empty_title"),
                        String::from(t!("files.confirm.empty_body")),
                    ),
                };
                appui::sheet_text(c, panel, title, &msg, false);
                appui::sheet_button(
                    c,
                    btns[0],
                    labels[0],
                    ButtonKind::Secondary,
                    hover(&btns[0]),
                    false,
                );
                appui::sheet_button(
                    c,
                    btns[1],
                    labels[1],
                    ButtonKind::Destructive,
                    hover(&btns[1]),
                    false,
                );
            }
            SheetKind::Info => {
                let lines = st.props.as_deref().unwrap_or(&[]);
                let p = theme::pal();
                let tw = text::TITLE3;
                text::draw(
                    c,
                    panel.x + appui::SHEET_PAD,
                    panel.y + appui::SHEET_PAD,
                    t!("files.sheet.properties"),
                    tw,
                    Weight::Semibold,
                    theme::text(),
                );
                let mut y = panel.y + appui::SHEET_PAD + text::line_height(tw) + 10;
                // The widest label sets the column (English and Portuguese labels differ).
                let label_w = lines
                    .iter()
                    .filter_map(|l| l.split_once(": "))
                    .map(|(k, _)| text::measure(k, BODY, Weight::Regular))
                    .max()
                    .unwrap_or(0)
                    .clamp(64, panel.w / 2);
                for l in lines {
                    let (k, v) = match l.split_once(": ") {
                        Some((k, v)) => (k, v),
                        None => ("", l.as_str()),
                    };
                    let row = Rect::new(
                        panel.x + appui::SHEET_PAD,
                        y,
                        panel.w - 2 * appui::SHEET_PAD,
                        24,
                    );
                    text::draw_right(
                        c,
                        Rect::new(row.x, row.y, label_w, row.h),
                        k,
                        BODY,
                        Weight::Regular,
                        theme::solid(p.text_secondary),
                    );
                    let vx = row.x + label_w + 12;
                    let shown = text::ellipsize_middle(v, BODY, Weight::Regular, row.right() - vx);
                    text::draw(
                        c,
                        vx,
                        text::center_y(row.y, row.h, BODY, Weight::Regular),
                        &shown,
                        BODY,
                        Weight::Regular,
                        theme::text(),
                    );
                    y += 24;
                }
                appui::sheet_button(
                    c,
                    btns[1],
                    labels[1],
                    ButtonKind::Primary,
                    hover(&btns[1]),
                    false,
                );
            }
            SheetKind::Copy => {
                let (title, name, pm) = match &st.job {
                    Some(j) => {
                        let (n, _) = j.copy.files();
                        let label = if n > 1 {
                            tp!("files.job.copying_n", n)
                        } else {
                            String::from(kitsune_core::i18n::tr(j.label))
                        };
                        (
                            label,
                            String::from_utf8_lossy(j.copy.current_name()).into_owned(),
                            j.copy.permille(),
                        )
                    }
                    None => (String::new(), String::new(), 0),
                };
                text::draw(
                    c,
                    panel.x + appui::SHEET_PAD,
                    panel.y + appui::SHEET_PAD,
                    &title,
                    text::TITLE3,
                    Weight::Semibold,
                    theme::text(),
                );
                let y = panel.y + appui::SHEET_PAD + text::line_height(text::TITLE3) + 8;
                text::draw_ellipsis(
                    c,
                    panel.x + appui::SHEET_PAD,
                    y,
                    panel.w - 2 * appui::SHEET_PAD,
                    &name,
                    BODY,
                    Weight::Regular,
                    theme::text_muted(),
                );
                let bar = Rect::new(
                    panel.x + appui::SHEET_PAD,
                    y + 28,
                    panel.w - 2 * appui::SHEET_PAD - 44,
                    6,
                );
                ui::progress(c, bar, pm);
                let pct = alloc::format!("{}%", pm / 10);
                text::draw_right(
                    c,
                    Rect::new(bar.right() + 8, bar.y - 6, 36, 18),
                    &pct,
                    FOOTNOTE,
                    Weight::Medium,
                    theme::text_muted(),
                );
                appui::sheet_button(
                    c,
                    btns[1],
                    labels[1],
                    ButtonKind::Secondary,
                    hover(&btns[1]),
                    false,
                );
            }
        }
        c.restore_clip(saved);
    }
}
