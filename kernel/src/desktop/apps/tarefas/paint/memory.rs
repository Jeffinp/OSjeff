//! The memory tab.

use super::frame::IntoFb;
use crate::desktop::apps::tarefas::layout::Lay;
use crate::desktop::apps::tarefas::layout::tab_split;
use crate::desktop::kit::{self, Chart, Curve};
use crate::desktop::*;
use crate::text::{self, BODY, CALLOUT, FOOTNOTE, TITLE2, Weight};
use core::fmt::Write as _;
use kitsune_core::activity::{self, Pressure, TaskKind, TaskRow, smooth121, snapshot};
use kitsune_core::klog::FixedBuf;
use kitsune_core::sysmon::HIST;
use kitsune_core::t;

impl Desktop {
    pub(super) fn tf_memory(&self, c: &mut Canvas, l: &Lay, st: &TarefasState) {
        let mon = &self.sysmon;
        let sp = tab_split(l.content, 0);
        let t = mon.t_q8();
        let used = mon.heap_now();
        let big = activity::fmt_size(used);
        self.section(c, sp.title, t!("tasks.mem.in_use_title"), kit::fb_str(&big));

        let mut raw = [0u32; HIST];
        let n = snapshot(&mon.heap, &mut raw);
        let mut sm = raw;
        smooth121(&mut sm[..n]);
        // Powers of two, so the axis reads 8,0 MiB / 4,0 MiB rather than 9,7 MiB.
        let peak_kib = (mon.heap.max() as u64).max(1024);
        let ceil = peak_kib.next_power_of_two();
        let top = activity::fmt_size(ceil * 1024);
        let mid = activity::fmt_size(ceil * 512);
        let hover = Self::hovered_sample(st).filter(|&i| i < n);
        let mut tip = FixedBuf::<48>::new();
        if let Some(i) = hover {
            let _ = write!(
                tip,
                "{} · {}",
                activity::fmt_size(raw[i] as u64 * 1024),
                activity::fmt_ago((n - 1 - i) as u32)
            );
        }
        kit::chart(
            c,
            sp.chart,
            &Chart {
                curves: &[Curve {
                    data: &sm[..n],
                    color: theme::accent(),
                }],
                ceiling: ceil,
                y_labels: [kit::fb_str(&top), kit::fb_str(&mid), "0"],
                t_q8: t as i32,
                hover,
                tip: kit::fb_str(&tip),
            },
        );

        // Pressure card.
        let total = mon.heap_total as u64;
        let pm = activity::permille(used, total);
        let pm_prev = activity::permille(mon.heap_prev as u64, total);
        let pm_q8 = kit::lerp((pm_prev as i64) << 8, (pm as i64) << 8, t);
        let pressure = Pressure::of(used, total);
        let col = match pressure {
            Pressure::Normal => kit::green(),
            Pressure::Attention => kit::amber(),
            Pressure::Critical => kit::red(),
        };
        let card = Rect::new(sp.side.x, sp.side.y, sp.side.w, 112);
        kit::card(c, card);
        kit::label(
            c,
            card.x + 16,
            card.y + 12,
            card.w - 32,
            t!("tasks.mem.pressure"),
        );
        text::draw_ellipsis(
            c,
            card.x + 16,
            card.y + 30,
            card.w - 32,
            pressure.label(),
            TITLE2,
            Weight::Semibold,
            col,
        );
        kit::pressure_gauge(
            c,
            Rect::new(card.x + 20, card.y + 68, card.w - 40, 10),
            pm_q8,
        );
        let pct = activity::fmt_pct_int(pm);
        text::draw_right(
            c,
            Rect::new(card.x, card.y + 10, card.w - 16, 20),
            kit::fb_str(&pct),
            BODY,
            Weight::Medium,
            kit::ink2(),
        );
        text::draw_left(
            c,
            Rect::new(card.x + 16, card.bottom() - 26, card.w - 32, 18),
            t!("tasks.mem.of_system"),
            FOOTNOTE,
            Weight::Regular,
            kit::ink3(),
        );

        // Numbers.
        let nums = Rect::new(sp.side.x, card.bottom() + 12, sp.side.w, 5 * 28 + 20);
        kit::card(c, nums);
        let free = total.saturating_sub(used);
        let rows: [(&str, FixedBuf<24>); 5] = [
            (t!("tasks.mem.in_use"), activity::fmt_size(used).into_fb()),
            (t!("tasks.mem.free"), activity::fmt_size(free).into_fb()),
            (t!("tasks.mem.total"), activity::fmt_size(total).into_fb()),
            (
                t!("tasks.mem.peak"),
                activity::fmt_size(mon.heap_peak()).into_fb(),
            ),
            (
                t!("tasks.mem.physical"),
                crate::sysinfo::get().map_or(FixedBuf::<24>::new(), |si| {
                    activity::fmt_size(si.total_ram()).into_fb()
                }),
            ),
        ];
        for (i, (k, v)) in rows.iter().enumerate() {
            kit::kv(
                c,
                Rect::new(nums.x + 16, nums.y + 10 + i as i32 * 28, nums.w - 32, 28),
                k,
                kit::fb_str(v),
            );
        }

        // Per app.
        self.memory_by_app(c, sp.below, st);
    }

    fn memory_by_app(&self, c: &mut Canvas, area: Rect, st: &TarefasState) {
        if area.h < 56 {
            return;
        }
        text::draw_left(
            c,
            Rect::new(area.x, area.y, area.w, 24),
            t!("tasks.mem.by_app"),
            CALLOUT,
            Weight::Semibold,
            kit::ink(),
        );
        text::draw_right(
            c,
            Rect::new(area.x, area.y, area.w, 24),
            t!("tasks.mem.approx"),
            FOOTNOTE,
            Weight::Regular,
            kit::ink3(),
        );
        let apps: Vec<&TaskRow> = st
            .rows
            .iter()
            .filter(|r| r.kind == TaskKind::App && r.mem_kib.is_some())
            .collect();
        let max = apps
            .iter()
            .filter_map(|r| r.mem_kib)
            .max()
            .unwrap_or(1)
            .max(256) as i64;
        let mut y = area.y + 28;
        let fit = ((area.bottom() - y) / 28).max(0) as usize;
        if apps.is_empty() {
            text::draw_left(
                c,
                Rect::new(area.x, y, area.w, 24),
                t!("tasks.mem.no_apps"),
                BODY,
                Weight::Regular,
                kit::ink2(),
            );
        }
        for r in apps.iter().take(fit) {
            let kib = r.mem_kib.unwrap_or(0) as i64;
            let name_w = (area.w / 3).clamp(120, 200);
            if let Some(i) = st.rows.iter().position(|x| x.id == r.id)
                && let Some(Some(icon)) = st.icons.get(i)
            {
                icons::blit(c, *icon, area.x, y + 2, 20, 256);
            }
            text::draw_left(
                c,
                Rect::new(area.x + 28, y, name_w - 28, 24),
                &r.name,
                BODY,
                Weight::Regular,
                kit::ink(),
            );
            let size = activity::fmt_size(kib as u64 * 1024);
            text::draw_right(
                c,
                Rect::new(area.right() - 80, y, 80, 24),
                kit::fb_str(&size),
                BODY,
                Weight::Regular,
                kit::ink2(),
            );
            let bx = area.x + name_w + 12;
            kit::bar(
                c,
                Rect::new(bx, y + 9, (area.right() - 80 - 12 - bx).max(20), 6),
                kib * 1000 * 256 / max,
                theme::accent(),
            );
            y += 28;
        }
    }
}
