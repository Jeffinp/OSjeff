//! The network tab.

use crate::desktop::apps::tarefas::layout::Lay;
use crate::desktop::apps::tarefas::layout::NET_TOP;
use crate::desktop::apps::tarefas::layout::tab_split;
use crate::desktop::kit::{self, Chart, Curve};
use crate::desktop::*;
use crate::text::{self, CALLOUT, FOOTNOTE, Weight};
use core::fmt::Write as _;
use kitsune_core::activity::{self, smooth121, snapshot};
use kitsune_core::klog::FixedBuf;
use kitsune_core::sysmon::{HIST, nice_ceiling};
use kitsune_core::{t, tp};

impl Desktop {
    pub(super) fn tf_network(&self, c: &mut Canvas, l: &Lay, st: &TarefasState) {
        use kitsune_core::netstats::NicKind;
        let mon = &self.sysmon;
        let sp = tab_split(l.content, 3);
        let t = mon.t_q8();
        let snap = crate::netd::stats();
        let has_nic = snap.nic != NicKind::None;
        let card = Rect::new(l.content.x, l.content.y, l.content.w, NET_TOP);
        kit::card(c, card);
        // Status line.
        let (word, col) = if !has_nic {
            (t!("tasks.net.no_nic"), kit::ink3())
        } else if !snap.link_up {
            (t!("tasks.net.no_link"), kit::red())
        } else if snap.config.is_none() {
            (t!("tasks.net.searching"), kit::amber())
        } else {
            (t!("tasks.net.connected"), kit::green())
        };
        let w = kit::chip(c, card.x + 20, card.y + 16, 22, word, col);
        if has_nic {
            let m = crate::nic::mac();
            let mac = alloc::format!(
                "{} · {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                snap.nic.name(),
                m[0],
                m[1],
                m[2],
                m[3],
                m[4],
                m[5]
            );
            text::draw_left(
                c,
                Rect::new(card.x + 20 + w + 12, card.y + 16, card.w - 60 - w, 22),
                &mac,
                FOOTNOTE,
                Weight::Regular,
                kit::ink2(),
            );
        }
        let mut ip = FixedBuf::<24>::new();
        let mut mask = FixedBuf::<24>::new();
        let mut gw = FixedBuf::<24>::new();
        let mut dns = FixedBuf::<64>::new();
        let mut lease = FixedBuf::<48>::new();
        match snap.config {
            Some(cfg) => {
                let _ = write!(ip, "{}", cfg.ip);
                let m = if cfg.prefix == 0 {
                    0
                } else {
                    u32::MAX << (32 - cfg.prefix as u32)
                };
                let _ = write!(
                    mask,
                    "{}.{}.{}.{}",
                    m >> 24,
                    (m >> 16) & 255,
                    (m >> 8) & 255,
                    m & 255
                );
                match cfg.gateway {
                    Some(g) => {
                        let _ = write!(gw, "{g}");
                    }
                    None => {
                        let _ = write!(gw, "—");
                    }
                }
                if cfg.dns.is_empty() {
                    let _ = write!(dns, "—");
                }
                for (i, d) in cfg.dns.as_slice().iter().enumerate() {
                    if i > 0 {
                        let _ = write!(dns, ", ");
                    }
                    let _ = write!(dns, "{d}");
                }
                if cfg == kitsune_core::net::NetConfig::STATIC_FALLBACK {
                    let _ = lease.write_str(t!("tasks.net.static"));
                } else if let Some(ms) = snap.lease_remaining_ms {
                    let _ = lease.write_str(&t!(
                        "tasks.net.lease_left",
                        time = kit::fb_str(&activity::fmt_elapsed(ms / 1000))
                    ));
                } else {
                    let _ = lease.write_str(t!("tasks.net.lease_none"));
                }
            }
            None => {
                for b in [&mut ip, &mut mask, &mut gw] {
                    let _ = write!(b, "—");
                }
                let _ = write!(dns, "—");
                let _ = write!(lease, "—");
            }
        }
        let colw = (card.w - 40 - 24) / 2;
        let left = [(t!("tasks.net.ip"), &ip), (t!("tasks.net.mask"), &mask)];
        for (i, (k, v)) in left.iter().enumerate() {
            kit::kv(
                c,
                Rect::new(card.x + 20, card.y + 52 + i as i32 * 26, colw, 26),
                k,
                kit::fb_str(v),
            );
        }
        kit::kv(
            c,
            Rect::new(card.x + 20, card.y + 52 + 2 * 26, colw, 26),
            t!("tasks.net.router"),
            kit::fb_str(&gw),
        );
        kit::kv(
            c,
            Rect::new(card.x + 20 + colw + 24, card.y + 52, colw, 26),
            t!("tasks.net.dns"),
            kit::fb_str(&dns),
        );
        kit::kv(
            c,
            Rect::new(card.x + 20 + colw + 24, card.y + 52 + 26, colw, 26),
            t!("tasks.net.lease"),
            kit::fb_str(&lease),
        );
        let _ = st;

        // Throughput chart.
        let rx_now = kit::lerp(mon.rx_prev as i64, mon.rx_rate as i64, t) as u64;
        let tx_now = kit::lerp(mon.tx_prev as i64, mon.tx_rate as i64, t) as u64;
        let traffic = t!("tasks.net.traffic");
        self.section(c, sp.title, traffic, "");
        let mut lx = sp.title.x + text::measure(traffic, CALLOUT, Weight::Semibold) + 24;
        for (name, col) in [
            (t!("tasks.net.received"), theme::accent()),
            (t!("tasks.net.sent"), kit::amber()),
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
        let mut rx = [0u32; HIST];
        let n = snapshot(&mon.net_rx, &mut rx);
        let mut tx = [0u32; HIST];
        snapshot(&mon.net_tx, &mut tx);
        let (rraw, traw) = (rx, tx);
        smooth121(&mut rx[..n]);
        smooth121(&mut tx[..n]);
        let peak = mon.net_rx.max().max(mon.net_tx.max()) as u64;
        let ceil = nice_ceiling(peak, 4096);
        let top = activity::fmt_speed(ceil);
        let mid = activity::fmt_speed(ceil / 2);
        let hover = Self::hovered_sample(st).filter(|&i| i < n);
        let mut tip = FixedBuf::<64>::new();
        if let Some(i) = hover {
            let _ = tip.write_str(&t!(
                "tasks.net.tip",
                rx = kit::fb_str(&activity::fmt_speed(rraw[i] as u64)),
                tx = kit::fb_str(&activity::fmt_speed(traw[i] as u64)),
                ago = kit::fb_str(&activity::fmt_ago((n - 1 - i) as u32))
            ));
        }
        kit::chart(
            c,
            sp.chart,
            &Chart {
                curves: &[
                    Curve {
                        data: &rx[..n],
                        color: theme::accent(),
                    },
                    Curve {
                        data: &tx[..n],
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
        // Side cards.
        let ch = 96;
        let a = activity::fmt_speed(rx_now);
        let sub_a = tp!(
            "tasks.net.packets",
            snap.rx_packets,
            size = kit::fb_str(&activity::fmt_size(snap.rx_bytes))
        );
        self.stat_card(
            c,
            Rect::new(sp.side.x, sp.side.y, sp.side.w, ch),
            t!("tasks.net.received"),
            kit::fb_str(&a),
            &sub_a,
        );
        let b = activity::fmt_speed(tx_now);
        let sub_b = tp!(
            "tasks.net.packets",
            snap.tx_packets,
            size = kit::fb_str(&activity::fmt_size(snap.tx_bytes))
        );
        self.stat_card(
            c,
            Rect::new(sp.side.x, sp.side.y + ch + 12, sp.side.w, ch),
            t!("tasks.net.sent"),
            kit::fb_str(&b),
            &sub_b,
        );
        let errs = snap.rx_errors + snap.tx_errors + snap.rx_dropped + snap.tx_dropped;
        if errs > 0 && sp.side.h > 2 * ch + 12 + 12 + 60 {
            let e = alloc::format!("{}", activity::fmt_count(errs));
            self.stat_card(
                c,
                Rect::new(sp.side.x, sp.side.y + 2 * (ch + 12), sp.side.w, 72),
                t!("tasks.net.errors"),
                &e,
                "",
            );
        }
    }
}
