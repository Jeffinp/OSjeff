//! The CPU tab.

use crate::desktop::apps::tarefas::layout::Lay;
use crate::desktop::apps::tarefas::layout::tab_split;
use crate::desktop::kit::{self, Chart, Curve};
use crate::desktop::*;
use crate::text::{self, BODY, CALLOUT, FOOTNOTE, Weight};
use core::fmt::Write as _;
use kitsune_core::activity::{self, smooth121, snapshot};
use kitsune_core::klog::FixedBuf;
use kitsune_core::sysmon::HIST;
use kitsune_core::{t, tp};

impl Desktop {
    pub(super) fn tf_cpu(&self, c: &mut Canvas, l: &Lay, st: &TarefasState) {
        let mon = &self.sysmon;
        let sp = tab_split(l.content, 0);
        let t = mon.t_q8();
        let now_pm = kit::lerp(mon.prev.busy_pm as i64, mon.last.busy_pm as i64, t) as u32;
        let big = activity::fmt_pct(now_pm);
        self.section(c, sp.title, t!("tasks.cpu.usage"), kit::fb_str(&big));

        let mut raw = [0u32; HIST];
        let n = snapshot(&mon.cpu_total, &mut raw);
        let mut sm = raw;
        smooth121(&mut sm[..n]);
        let hover = Self::hovered_sample(st).filter(|&i| i < n);
        let mut tip = FixedBuf::<40>::new();
        if let Some(i) = hover {
            let _ = write!(
                tip,
                "{} · {}",
                activity::fmt_pct(raw[i]),
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
                ceiling: 1000,
                y_labels: ["100%", "50%", "0%"],
                t_q8: t as i32,
                hover,
                tip: kit::fb_str(&tip),
            },
        );

        // The right-hand column.
        let gap = 12;
        let ch = if sp.side.h >= 4 * 88 + 3 * gap + 72 - 88 {
            88
        } else {
            72
        };
        let mut y = sp.side.y;
        let up = activity::fmt_clock(mon.uptime_s);
        self.stat_card(
            c,
            Rect::new(sp.side.x, y, sp.side.w, ch),
            t!("tasks.cpu.uptime"),
            kit::fb_str(&up),
            "",
        );
        y += ch + gap;
        let (a, b, d) = mon.load.get();
        let mut ld = FixedBuf::<40>::new();
        let _ = write!(
            ld,
            "{}  {}  {}",
            activity::fmt_milli(a),
            activity::fmt_milli(b),
            activity::fmt_milli(d)
        );
        self.stat_card(
            c,
            Rect::new(sp.side.x, y, sp.side.w, ch),
            t!("tasks.cpu.load"),
            kit::fb_str(&ld),
            t!("tasks.cpu.load_sub"),
        );
        y += ch + gap;
        let mut th = FixedBuf::<24>::new();
        let _ = write!(th, "{}", sched::thread_count());
        let sub = tp!("tasks.cpu.processes", self.procs.len().saturating_sub(1));
        self.stat_card(
            c,
            Rect::new(sp.side.x, y, sp.side.w, ch),
            t!("tasks.cpu.threads"),
            kit::fb_str(&th),
            &sub,
        );
        y += ch + gap;
        if y + 72 <= sp.side.bottom() {
            let brand = crate::sysinfo::get()
                .map(|si| {
                    let b = si.brand.as_bytes();
                    let s = core::str::from_utf8(b).unwrap_or("").trim();
                    if s.is_empty() {
                        String::from(core::str::from_utf8(si.vendor.as_bytes()).unwrap_or(""))
                    } else {
                        String::from(s)
                    }
                })
                .unwrap_or_default();
            let r = Rect::new(sp.side.x, y, sp.side.w, 72.min(sp.side.bottom() - y));
            kit::card(c, r);
            kit::label(c, r.x + 16, r.y + 10, r.w - 32, t!("tasks.cpu.processor"));
            text::draw_ellipsis(
                c,
                r.x + 16,
                r.y + 28,
                r.w - 32,
                &brand,
                BODY,
                Weight::Medium,
                kit::ink(),
            );
            if let Some(si) = crate::sysinfo::get() {
                let feats = core::str::from_utf8(si.features.as_bytes())
                    .unwrap_or("")
                    .split_whitespace()
                    .count();
                let sub = if si.hypervisor.as_bytes().is_empty() {
                    tp!("tasks.cpu.features", feats)
                } else {
                    tp!("tasks.cpu.features_vm", feats)
                };
                text::draw_ellipsis(
                    c,
                    r.x + 16,
                    r.y + 48,
                    r.w - 32,
                    &sub,
                    FOOTNOTE,
                    Weight::Regular,
                    kit::ink2(),
                );
            }
        }

        // Below the chart: who uses the CPU.
        self.cpu_by_process(c, sp.below, st);
    }

    /// Bars of the busiest processes under the CPU chart.
    fn cpu_by_process(&self, c: &mut Canvas, area: Rect, st: &TarefasState) {
        if area.h < 56 {
            return;
        }
        text::draw_left(
            c,
            Rect::new(area.x, area.y, area.w, 24),
            t!("tasks.cpu.by_process"),
            CALLOUT,
            Weight::Semibold,
            kit::ink(),
        );
        let t = self.sysmon.t_q8();
        let mut y = area.y + 28;
        let fit = ((area.bottom() - y) / 28).max(0) as usize;
        let mut shown = 0;
        for r in st.rows.iter().filter(|r| r.raw != "(idle)") {
            if shown >= fit {
                break;
            }
            let cur = r.cpu_pm.unwrap_or(0) as i64;
            if cur == 0 && shown >= 4 {
                continue;
            }
            let prev = st
                .prev_cpu
                .iter()
                .find(|(id, _)| *id == r.id)
                .map_or(cur, |(_, v)| *v as i64);
            let v_pm_q8 = kit::lerp(prev << 8, cur << 8, t);
            let name_w = (area.w / 3).clamp(120, 200);
            text::draw_left(
                c,
                Rect::new(area.x, y, name_w, 24),
                &r.name,
                BODY,
                Weight::Regular,
                kit::ink(),
            );
            let pct = activity::fmt_pct((v_pm_q8 >> 8) as u32);
            text::draw_right(
                c,
                Rect::new(area.right() - 64, y, 64, 24),
                kit::fb_str(&pct),
                BODY,
                Weight::Regular,
                kit::ink2(),
            );
            let bx = area.x + name_w + 12;
            kit::bar(
                c,
                Rect::new(bx, y + 9, (area.right() - 64 - 12 - bx).max(20), 6),
                v_pm_q8,
                theme::accent(),
            );
            y += 28;
            shown += 1;
        }
    }
}
