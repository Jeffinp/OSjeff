//! The disk tab.

use super::frame::IntoFb;
use crate::desktop::apps::tarefas::layout::DISK_TOP;
use crate::desktop::apps::tarefas::layout::Lay;
use crate::desktop::apps::tarefas::layout::tab_split;
use crate::desktop::kit::{self, Chart, Curve};
use crate::desktop::*;
use crate::text::{self, CALLOUT, FOOTNOTE, TITLE2, TITLE3, Weight};
use core::fmt::Write as _;
use kitsune_core::activity::{self, smooth121, snapshot};
use kitsune_core::klog::FixedBuf;
use kitsune_core::sysif::DiskUsage;
use kitsune_core::sysmon::{HIST, nice_ceiling};
use kitsune_core::t;

impl Desktop {
    pub(super) fn tf_disk(&self, c: &mut Canvas, l: &Lay, st: &TarefasState) {
        let mon = &self.sysmon;
        let sp = tab_split(l.content, 2);
        let t = mon.t_q8();
        // The volume card.
        let card = Rect::new(l.content.x, l.content.y, l.content.w, DISK_TOP);
        kit::card(c, card);
        let usage = VfsUsage;
        let u = vfs::statfs();
        let (title, sub) = match vfs::volume() {
            vfs::Volume::Disk => (
                t!("tasks.disk.main"),
                usage.label().trim_end_matches(" (IDE)"),
            ),
            vfs::Volume::Memory => (t!("tasks.disk.ram"), t!("tasks.disk.ram_sub")),
        };
        text::draw_left(
            c,
            Rect::new(card.x + 20, card.y + 14, card.w / 2, 24),
            title,
            TITLE3,
            Weight::Semibold,
            kit::ink(),
        );
        let model = self.disks[1]
            .map(|d| alloc::format!("{} · {}", d.model_name(), activity::fmt_size(d.mib() << 20)));
        let line2 = match model {
            Some(m) => alloc::format!("{sub} · {m}"),
            None => String::from(sub),
        };
        text::draw_left(
            c,
            Rect::new(card.x + 20, card.y + 40, card.w - 160, 20),
            &line2,
            FOOTNOTE,
            Weight::Regular,
            kit::ink2(),
        );
        let pm = u.used_permille();
        let pct = activity::fmt_pct(pm);
        text::draw_right(
            c,
            Rect::new(card.x, card.y + 14, card.w - 20, 32),
            kit::fb_str(&pct),
            TITLE2,
            Weight::Semibold,
            theme::accent(),
        );
        kit::bar(
            c,
            Rect::new(card.x + 20, card.y + 70, card.w - 40, 12),
            (pm as i64) << 8,
            if pm >= 900 {
                kit::red()
            } else {
                theme::accent()
            },
        );
        let cols = [
            (
                t!("tasks.disk.used"),
                activity::fmt_size(u.used()).into_fb(),
            ),
            (t!("tasks.disk.free"), activity::fmt_size(u.free).into_fb()),
            (
                t!("tasks.disk.total"),
                activity::fmt_size(u.total).into_fb(),
            ),
            (
                t!("tasks.disk.items"),
                if u.inodes_total > 0 {
                    activity::fmt_count(u.inodes_used() as u64).into_fb()
                } else {
                    let mut b = FixedBuf::<24>::new();
                    let _ = write!(b, "—");
                    b
                },
            ),
        ];
        let cw = (card.w - 40) / 4;
        for (i, (k, v)) in cols.iter().enumerate() {
            kit::stat(
                c,
                card.x + 20 + i as i32 * cw,
                card.y + 92,
                cw - 8,
                k,
                kit::fb_str(v),
            );
        }

        // Throughput chart.
        let io_title = t!("tasks.disk.io");
        self.section(c, sp.title, io_title, "");
        // The legend starts after the section title, whatever its width.
        let mut lx = sp.title.x + text::measure(io_title, CALLOUT, Weight::Semibold) + 24;
        for (name, col) in [
            (t!("tasks.disk.read"), theme::accent()),
            (t!("tasks.disk.write"), kit::amber()),
        ] {
            c.fill_rrect(
                Rect::new(lx, sp.title.y + 8, 8, 8),
                4,
                Corner::Circle,
                col,
                256,
            );
            text::draw_left(
                c,
                Rect::new(lx + 14, sp.title.y, 80, 24),
                name,
                FOOTNOTE,
                Weight::Regular,
                kit::ink2(),
            );
            lx += 14 + text::measure(name, FOOTNOTE, Weight::Regular) + 18;
        }
        let mut rd = [0u32; HIST];
        let n = snapshot(&mon.disk_rd, &mut rd);
        let mut wr = [0u32; HIST];
        snapshot(&mon.disk_wr, &mut wr);
        let (rraw, wraw) = (rd, wr);
        smooth121(&mut rd[..n]);
        smooth121(&mut wr[..n]);
        let peak = mon.disk_rd.max().max(mon.disk_wr.max()) as u64;
        let ceil = nice_ceiling(peak, 4096);
        let top = activity::fmt_speed(ceil);
        let mid = activity::fmt_speed(ceil / 2);
        let hover = Self::hovered_sample(st).filter(|&i| i < n);
        let mut tip = FixedBuf::<64>::new();
        if let Some(i) = hover {
            let _ = tip.write_str(&t!(
                "tasks.disk.tip",
                r = kit::fb_str(&activity::fmt_speed(rraw[i] as u64)),
                w = kit::fb_str(&activity::fmt_speed(wraw[i] as u64)),
                ago = kit::fb_str(&activity::fmt_ago((n - 1 - i) as u32))
            ));
        }
        kit::chart(
            c,
            sp.chart,
            &Chart {
                curves: &[
                    Curve {
                        data: &rd[..n],
                        color: theme::accent(),
                    },
                    Curve {
                        data: &wr[..n],
                        color: kit::amber(),
                    },
                ],
                ceiling: ceil,
                y_labels: [kit::fb_str(&top), kit::fb_str(&mid), "0"],
                t_q8: t as i32,
                hover,
                tip: kit::fb_str(&tip),
            },
        );
        // The side column.
        let (rd_total, wr_total) = crate::ata::io_bytes();
        let ch = 96;
        let rate_rd = kit::lerp(mon.disk_rd_prev as i64, mon.disk_rd_rate as i64, t) as u64;
        let rate_wr = kit::lerp(mon.disk_wr_prev as i64, mon.disk_wr_rate as i64, t) as u64;
        let a = activity::fmt_speed(rate_rd);
        let sub_a = t!(
            "tasks.disk.since_boot",
            size = kit::fb_str(&activity::fmt_size(rd_total))
        );
        self.stat_card(
            c,
            Rect::new(sp.side.x, sp.side.y, sp.side.w, ch),
            t!("tasks.disk.read"),
            kit::fb_str(&a),
            &sub_a,
        );
        let b = activity::fmt_speed(rate_wr);
        let sub_b = t!(
            "tasks.disk.since_boot",
            size = kit::fb_str(&activity::fmt_size(wr_total))
        );
        self.stat_card(
            c,
            Rect::new(sp.side.x, sp.side.y + ch + 12, sp.side.w, ch),
            t!("tasks.disk.write"),
            kit::fb_str(&b),
            &sub_b,
        );
    }
}
