//! The processes tab and the end-task confirmation.

use crate::desktop::apps::tarefas::layout::Lay;
use crate::desktop::apps::tarefas::layout::columns;
use crate::desktop::apps::tarefas::state::H_CANCEL;
use crate::desktop::apps::tarefas::state::H_DOWN;
use crate::desktop::apps::tarefas::state::H_END;
use crate::desktop::apps::tarefas::state::H_HEAD;
use crate::desktop::apps::tarefas::state::H_OK;
use crate::desktop::apps::tarefas::state::H_RESTART;
use crate::desktop::apps::tarefas::state::H_ROW;
use crate::desktop::apps::tarefas::state::HEAD_H;
use crate::desktop::kit::ui::{self, ButtonKind};
use crate::desktop::kit::{self};
use crate::desktop::*;
use crate::text::{self, BODY, FOOTNOTE, TITLE3, Weight};
use core::fmt::Write as _;
use kitsune_core::activity::{self, Column, TaskKind, TaskState};
use kitsune_core::fileman::ui::ROW_H;
use kitsune_core::klog::FixedBuf;
use kitsune_core::{t, tp};

impl Desktop {
    pub(super) fn tf_processes(&self, c: &mut Canvas, l: &Lay, st: &TarefasState, hv: u32) {
        let p = theme::pal();
        let hv_key = hv & !H_DOWN;
        let down = hv & H_DOWN != 0;
        // The search field.
        if l.search.w >= 80 {
            kit::search_field(
                c,
                l.search,
                &st.query,
                t!("tasks.proc.search"),
                st.search_focus,
                st.search_focus,
            );
        }
        // Column headers.
        let cols = columns(l.head);
        for (i, col) in Column::ALL.iter().enumerate() {
            let cr = cols[i];
            if hv_key == H_HEAD + i as u32 {
                ui::fill_token(c, Rect::new(cr.x, cr.y + 2, cr.w, cr.h - 4), 6, p.hover);
            }
            let right = matches!(col, Column::Cpu | Column::Mem | Column::Up);
            let active = st.sort == *col;
            let color = if active { kit::ink() } else { kit::ink2() };
            let title = col.title();
            let tw = text::measure(title, FOOTNOTE, Weight::Medium);
            let (tx, aw) = if right {
                let aw = if active { 14 } else { 0 };
                (cr.right() - 12 - aw - tw, aw)
            } else {
                (cr.x + 12, 14)
            };
            let ty = text::center_y(cr.y, HEAD_H, FOOTNOTE, Weight::Medium);
            text::draw(c, tx, ty, title, FOOTNOTE, Weight::Medium, color);
            if active {
                kit::sort_arrow(c, tx + tw + 3, cr.y + (HEAD_H - 10) / 2, st.desc, color);
            }
            let _ = aw;
        }
        ui::separator(c, Rect::new(l.head.x, l.head.bottom() - 1, l.head.w, 1));

        // Rows.
        let saved = kit::clip_to(c, l.list);
        let scroll = st.scroll.value();
        let first = (scroll / ROW_H).max(0) as usize;
        let mut tip: Option<(i32, i32, String)> = None;
        for (k, row) in st.rows.iter().enumerate().skip(first) {
            let y = l.list.y + k as i32 * ROW_H - scroll;
            if y >= l.list.bottom() {
                break;
            }
            let rr = Rect::new(l.list.x, y, l.list.w, ROW_H);
            let selected = st.sel == Some(row.id);
            let hovered = hv_key == H_ROW + k as u32;
            let inner = Rect::new(rr.x - 4, rr.y + 1, rr.w + 8, rr.h - 2);
            let fg = if selected {
                c.fill_rrect(inner, 8, Corner::Circle, theme::accent(), 256);
                theme::ACCENT_TEXT
            } else {
                if hovered {
                    ui::fill_token(c, inner, 8, p.hover);
                } else if k % 2 == 1 {
                    ui::fill_token(
                        c,
                        inner,
                        8,
                        if theme::dark() {
                            0x0AFF_FFFF
                        } else {
                            0x0600_0000
                        },
                    );
                }
                kit::ink()
            };
            let fg2 = if selected {
                Color::rgb(0xE6, 0xE6, 0xFF)
            } else {
                kit::ink2()
            };
            // PID.
            let mut pid = FixedBuf::<8>::new();
            if row.pid == 0 {
                let _ = write!(pid, "—");
            } else {
                let _ = write!(pid, "{}", row.pid);
            }
            text::draw_left(
                c,
                Rect::new(cols[0].x + 12, y, cols[0].w - 12, ROW_H),
                kit::fb_str(&pid),
                BODY,
                Weight::Regular,
                fg2,
            );
            // Name, with the app's icon.
            let mut nx = cols[1].x + 4;
            if let Some(Some(icon)) = st.icons.get(k) {
                icons::blit(c, *icon, nx, y + 4, 20, 256);
                nx += 28;
            } else {
                // Services and system entries carry the mark of the system.
                let tile = Rect::new(nx, y + 4, 20, 20);
                c.fill_rrect(
                    tile,
                    6,
                    Corner::Circle,
                    if selected {
                        Color::rgb(0xFF, 0xFF, 0xFF)
                    } else {
                        kit::ink3()
                    },
                    if selected { 60 } else { 70 },
                );
                ui::draw_glyph(
                    c,
                    iconart::Glyph::Brand,
                    tile.x + 3,
                    tile.y + 3,
                    14,
                    kit::argb(if selected {
                        Color::rgb(0xFF, 0xFF, 0xFF)
                    } else {
                        kit::ink2()
                    }),
                );
                nx += 28;
            }
            text::draw_left(
                c,
                Rect::new(nx, y, cols[1].right() - nx - 8, ROW_H),
                &row.name,
                BODY,
                if selected {
                    Weight::Medium
                } else {
                    Weight::Regular
                },
                fg,
            );
            if hovered && !selected {
                let cx = self.cursor_x;
                if cols[1].contains(cx, y + 1) && !row.raw.is_empty() && row.raw != row.name {
                    tip = Some((cx, y, row.raw.clone()));
                }
            }
            // State, with a coloured dot.
            let sc = match row.state {
                TaskState::Running => kit::green(),
                TaskState::Waiting => Color::rgb(0xA0, 0xA0, 0xA8),
                TaskState::Suspended => kit::amber(),
                TaskState::Ended | TaskState::Stopped => kit::red(),
            };
            c.fill_rrect(
                Rect::new(cols[2].x + 12, y + 11, 6, 6),
                3,
                Corner::Circle,
                sc,
                256,
            );
            text::draw_left(
                c,
                Rect::new(cols[2].x + 24, y, cols[2].w - 24, ROW_H),
                row.state.label(),
                BODY,
                Weight::Regular,
                fg2,
            );
            // CPU (glides), memory, uptime: right-aligned so the digits line up.
            let cpu = match row.cpu_pm {
                Some(cur) => {
                    let prev = st
                        .prev_cpu
                        .iter()
                        .find(|(id, _)| *id == row.id)
                        .map_or(cur as i64, |(_, v)| *v as i64);
                    let pm = kit::lerp(prev << 8, (cur as i64) << 8, self.sysmon.t_q8()) >> 8;
                    let s = activity::fmt_pct(pm as u32);
                    String::from(kit::fb_str(&s))
                }
                None => String::from("—"),
            };
            text::draw_right(
                c,
                Rect::new(cols[3].x, y, cols[3].w - 12, ROW_H),
                &cpu,
                BODY,
                Weight::Regular,
                fg,
            );
            let mem = match row.mem_kib {
                Some(k) => String::from(kit::fb_str(&activity::fmt_size(k as u64 * 1024))),
                None => String::from("—"),
            };
            text::draw_right(
                c,
                Rect::new(cols[4].x, y, cols[4].w - 12, ROW_H),
                &mem,
                BODY,
                Weight::Regular,
                fg,
            );
            let up = if row.kind == TaskKind::App || row.up_s > 0 {
                String::from(kit::fb_str(&activity::fmt_elapsed(row.up_s as u64)))
            } else {
                String::from("—")
            };
            text::draw_right(
                c,
                Rect::new(cols[5].x, y, cols[5].w - 12, ROW_H),
                &up,
                BODY,
                Weight::Regular,
                fg2,
            );
        }
        if st.rows.is_empty() {
            let msg = if st.query.is_empty() {
                t!("tasks.proc.empty")
            } else {
                t!("tasks.proc.no_match")
            };
            text::draw_left(
                c,
                Rect::new(l.list.x + 12, l.list.y + 8, l.list.w - 24, 24),
                msg,
                BODY,
                Weight::Regular,
                kit::ink2(),
            );
        }
        c.restore_clip(saved);
        ui::overlay_scrollbar(
            c,
            Rect::new(l.list.right() - 10, l.list.y, 10, l.list.h),
            (scroll / ROW_H).max(0) as usize,
            st.rows.len(),
            (l.list.h / ROW_H).max(1) as usize,
            st.sb.alpha(crate::desktop::shell::toasts::now_ms()),
        );

        // Footer: the summary, the selected row's internal name, and the buttons.
        ui::separator(c, Rect::new(l.foot.x, l.foot.y, l.foot.w, 1));
        let totals = activity::totals(&st.rows);
        let mon = &self.sysmon;
        let cpu = activity::fmt_pct_int(mon.last.busy_pm as u32);
        let mem_pct = activity::fmt_pct_int(activity::permille(
            mon.heap_used as u64,
            mon.heap_total as u64,
        ));
        let disk_pct = activity::fmt_pct_int(vfs::statfs().used_permille());
        let summary = t!(
            "tasks.proc.summary",
            processes = &tp!("tasks.proc.processes", totals.processes),
            threads = &tp!("tasks.proc.threads", totals.threads),
            cpu = kit::fb_str(&cpu),
            mem = kit::fb_str(&mem_pct),
            disk = kit::fb_str(&disk_pct)
        );
        let btn_x = l.btn_restart.x;
        text::draw_left(
            c,
            Rect::new(l.foot.x, l.foot.y + 8, btn_x - l.foot.x - 12, 20),
            &summary,
            BODY,
            Weight::Regular,
            kit::ink2(),
        );
        let sel_row = st.sel_index().and_then(|i| st.rows.get(i));
        let detail = match (&st.msg, sel_row) {
            (Some((m, _)), _) => m.clone(),
            (None, Some(r)) => t!(
                "tasks.proc.detail",
                raw = if r.raw.is_empty() {
                    "—"
                } else {
                    r.raw.as_str()
                },
                kind = match r.kind {
                    TaskKind::App => t!("tasks.kind.app"),
                    TaskKind::Thread => t!("tasks.kind.service"),
                    TaskKind::System => t!("tasks.kind.system"),
                }
            ),
            _ => String::new(),
        };
        text::draw_left(
            c,
            Rect::new(l.foot.x, l.foot.y + 28, btn_x - l.foot.x - 12, 18),
            &detail,
            FOOTNOTE,
            Weight::Regular,
            kit::ink3(),
        );
        let can_end = sel_row.is_some_and(|r| self.can_end(r));
        let can_restart = sel_row.is_some_and(|r| self.can_restart(r));
        ui::push_button(
            c,
            l.btn_restart,
            t!("tasks.restart"),
            ButtonKind::Secondary,
            kit::control_state(hv_key == H_RESTART, down, can_restart),
        );
        ui::push_button(
            c,
            l.btn_end,
            t!("tasks.end"),
            if can_end && sel_row.is_some_and(Self::needs_confirm) {
                ButtonKind::Destructive
            } else {
                ButtonKind::Secondary
            },
            kit::control_state(hv_key == H_END, down, can_end),
        );
        if let Some((cx, y, raw)) = tip {
            ui::tooltip(c, cx, y - 2, &raw);
        }
    }

    /// The confirmation sheet for ending a system service.
    pub(super) fn tf_confirm(&self, c: &mut Canvas, r: Rect, l: &Lay, st: &TarefasState, hv: u32) {
        let p = theme::pal();
        let body = r.body();
        c.blend_rect(
            body,
            Color::rgb(0, 0, 0),
            if theme::dark() { 120 } else { 70 },
        );
        let s = l.sheet;
        c.draw_shadow(
            s,
            Shadow {
                blur: 24,
                dy: 12,
                alpha: 90,
            },
            Rect::new(s.x, s.y + 14, s.w, s.h - 28),
        );
        ui::fill_token(c, s, 14, p.window_bg);
        ui::stroke_token(c, s, 14, p.separator);
        let row = st
            .confirm
            .and_then(|id| st.rows.iter().find(|r| r.id == id));
        let name = row.map_or("", |r| r.name.as_str());
        let title = t!("tasks.confirm.title", name = name);
        text::draw_left(
            c,
            Rect::new(s.x + 20, s.y + 18, s.w - 40, 24),
            &title,
            TITLE3,
            Weight::Semibold,
            kit::ink(),
        );
        let msg = match row.map(|r| r.raw.as_str()) {
            Some("appd") => t!("tasks.confirm.appd"),
            Some(_) => t!("tasks.confirm.shell"),
            None => "",
        };
        let body_txt = t!("tasks.confirm.body", msg = msg);
        let lines = text::wrap(&body_txt, BODY, Weight::Regular, s.w - 40, 3);
        for (k, (a, b)) in lines.into_iter().enumerate() {
            text::draw(
                c,
                s.x + 20,
                s.y + 52 + k as i32 * 20,
                &body_txt[a..b],
                BODY,
                Weight::Regular,
                kit::ink2(),
            );
        }
        let hv_key = hv & !H_DOWN;
        let down = hv & H_DOWN != 0;
        ui::push_button(
            c,
            l.sheet_cancel,
            t!("common.cancel"),
            ButtonKind::Secondary,
            kit::control_state(hv_key == H_CANCEL, down, true),
        );
        ui::push_button(
            c,
            l.sheet_ok,
            t!("tasks.end"),
            ButtonKind::Destructive,
            kit::control_state(hv_key == H_OK, down, true),
        );
    }
}
