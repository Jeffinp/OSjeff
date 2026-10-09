//! Drawing the log viewer.

use super::state::H_CLEAR;
use super::state::H_DOWN;
use super::state::H_ROW;
use super::state::H_SAVE;
use super::state::HEAD_H;
use super::state::LEVELS;
use super::state::LogLayout;
use super::state::MONO;
use crate::desktop::kit;
use crate::desktop::kit::ui::{self, ButtonKind};
use crate::desktop::*;
use crate::text::{self, BODY, FOOTNOTE, Weight};
use kitsune_core::activity::{self};
use kitsune_core::fileman::ui::ROW_H;
use kitsune_core::i18n;
use kitsune_core::klog::Level;
use kitsune_core::{t, tp};

fn level_color(l: Level) -> Color {
    match l {
        Level::Trace => Color::rgb(0x8E, 0x8E, 0x93),
        Level::Debug => Color::rgb(0x64, 0x8F, 0xD8),
        Level::Info => theme::accent(),
        Level::Warn => kit::amber(),
        Level::Error => kit::red(),
        Level::Fatal => Color::rgb(0xFF, 0x2D, 0x92),
    }
}

impl Desktop {
    pub(crate) fn draw_log(&self, c: &mut Canvas, r: Rect, l: &LogState) {
        let p = theme::pal();
        let lay = LogLayout::of(r);
        let hv = l.hover.get();
        let key = hv & !H_DOWN;
        let down = hv & H_DOWN != 0;

        // Toolbar.
        let focused = true;
        let needle = String::from_utf8_lossy(l.filter.needle()).into_owned();
        kit::search_field(c, lay.search, &needle, t!("log.search"), focused, focused);
        let levels = LEVELS.map(i18n::tr);
        ui::segmented(c, lay.seg, &levels, l.seg_index());
        text::draw_right(
            c,
            lay.follow_label,
            t!("log.follow"),
            BODY,
            Weight::Regular,
            kit::ink(),
        );
        ui::switch(c, lay.follow_sw, l.knob.value(), true);
        kit::icon_button(
            c,
            lay.clear,
            kitsune_core::iconart::Glyph::Trash,
            t!("log.clear"),
            ButtonKind::Secondary,
            kit::control_state(key == H_CLEAR, down, true),
        );
        kit::icon_button(
            c,
            lay.save,
            kitsune_core::iconart::Glyph::Save,
            t!("log.save"),
            ButtonKind::Secondary,
            kit::control_state(key == H_SAVE, down, true),
        );

        // The table.
        kit::card(c, lay.card);
        let heads = [
            t!("log.col.time"),
            t!("log.col.level"),
            t!("log.col.source"),
            t!("log.col.message"),
        ];
        for (i, h) in heads.iter().enumerate() {
            let col = lay.cols[i];
            if col.w == 0 {
                continue;
            }
            text::draw_left(
                c,
                Rect::new(col.x, lay.head.y, col.w, HEAD_H),
                h,
                FOOTNOTE,
                Weight::Medium,
                kit::ink2(),
            );
        }
        ui::separator(
            c,
            Rect::new(lay.head.x, lay.head.bottom() - 1, lay.head.w, 1),
        );

        let saved = kit::clip_to(c, lay.list);
        let scroll = l.scroll.value().clamp(0, l.max_px(lay.list.h));
        let first = (scroll / ROW_H).max(0) as usize;
        let frac = scroll % ROW_H;
        let mono_pitch = text::measure("0", MONO, Weight::Mono).max(1);
        let msg_cols = (lay.cols[3].w / mono_pitch).max(0) as usize;
        let mut y = lay.list.y - frac;
        for (k, e) in l
            .view
            .visible_from(&l.snap, first, lay.rows + 2)
            .enumerate()
        {
            let idx = first + k;
            let row = Rect::new(lay.list.x, y, lay.list.w, ROW_H);
            if key == H_ROW + idx as u32 {
                ui::fill_token(c, row.inflated(-4), 6, p.hover);
            } else if idx % 2 == 1 {
                ui::fill_token(
                    c,
                    row.inflated(-4),
                    6,
                    if theme::dark() {
                        0x0AFF_FFFF
                    } else {
                        0x0600_0000
                    },
                );
            }
            // Time, in the mono font so the digits line up.
            let t = activity::fmt_log_time(e.ts_ms);
            text::draw_mono(
                c,
                lay.cols[0].x,
                text::center_y(y, ROW_H, MONO, Weight::Mono),
                kit::fb_str(&t),
                MONO,
                kit::ink2(),
            );
            // The level chip.
            kit::chip(
                c,
                lay.cols[1].x,
                y + (ROW_H - 18) / 2,
                18,
                activity::level_name(e.level),
                level_color(e.level),
            );
            // Who wrote it.
            if lay.cols[2].w > 0 {
                let raw = crate::sched::thread_name(e.origin as usize);
                let name = if raw.is_empty() {
                    String::from("—")
                } else {
                    activity::friendly_name(raw.as_bytes())
                };
                text::draw_left(
                    c,
                    Rect::new(lay.cols[2].x, y, lay.cols[2].w - 8, ROW_H),
                    &name,
                    BODY,
                    Weight::Regular,
                    kit::ink2(),
                );
            }
            // The message.
            let msg = text::from_bytes(e.text);
            let shown: String = if msg.chars().count() > msg_cols && msg_cols > 1 {
                let mut s: String = msg.chars().take(msg_cols - 1).collect();
                s.push('…');
                s
            } else {
                msg.into_owned()
            };
            text::draw_mono(
                c,
                lay.cols[3].x,
                text::center_y(y, ROW_H, MONO, Weight::Mono),
                &shown,
                MONO,
                kit::ink(),
            );
            y += ROW_H;
        }
        if l.view.is_empty() {
            let msg = if l.snap.is_empty() {
                t!("log.empty")
            } else {
                t!("log.no_match")
            };
            text::draw_left(
                c,
                Rect::new(lay.list.x + 12, lay.list.y + 8, lay.list.w - 24, 24),
                msg,
                BODY,
                Weight::Regular,
                kit::ink2(),
            );
        }
        c.restore_clip(saved);
        ui::overlay_scrollbar(
            c,
            Rect::new(lay.list.right() - 10, lay.list.y, 10, lay.list.h),
            (scroll / ROW_H).max(0) as usize,
            l.view.len(),
            lay.rows,
            l.sb.alpha(crate::desktop::shell::toasts::now_ms()),
        );

        // Status line: counts on the left, the last action on the right.
        let total = kitsune_core::klog::records(&l.snap).count();
        let counts = if l.view.len() == total {
            tp!("log.count", total)
        } else {
            tp!("log.count_of", total, shown = l.view.len())
        };
        text::draw_left(
            c,
            lay.status,
            &counts,
            FOOTNOTE,
            Weight::Regular,
            kit::ink2(),
        );
        if let Some((m, err)) = &l.status {
            text::draw_right(
                c,
                lay.status,
                i18n::tr(m),
                FOOTNOTE,
                Weight::Regular,
                if *err { kit::red() } else { kit::ink2() },
            );
        }
    }
}
