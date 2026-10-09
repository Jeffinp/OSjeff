//! Geometry of the Tarefas window.

use super::state::FOOT_H;
use super::state::HEAD_H;
use super::state::PAD;
use crate::desktop::*;

// ------------------------------------------------------------------ layout

pub(super) struct Lay {
    pub(super) tabs: Rect,
    pub(super) search: Rect,
    pub(super) content: Rect,
    // Processos
    pub(super) head: Rect,
    pub(super) list: Rect,
    pub(super) foot: Rect,
    pub(super) btn_end: Rect,
    pub(super) btn_restart: Rect,
    // The confirmation sheet.
    pub(super) sheet: Rect,
    pub(super) sheet_cancel: Rect,
    pub(super) sheet_ok: Rect,
}

pub(super) fn lay(r: Rect) -> Lay {
    let body = r.body();
    let tabs_w = (5 * 92 + 4).min(body.w - 2 * PAD);
    let tabs = Rect::new(body.x + PAD, body.y + 12, tabs_w, 28);
    let sx = tabs.right() + 12;
    let search = Rect::new(sx, body.y + 12, (body.right() - PAD - sx).clamp(0, 220), 28);
    let content = Rect::new(
        body.x + PAD,
        tabs.bottom() + 12,
        body.w - 2 * PAD,
        (body.bottom() - PAD - (tabs.bottom() + 12)).max(0),
    );
    let head = Rect::new(content.x, content.y, content.w, HEAD_H);
    let foot = Rect::new(content.x, content.bottom() - FOOT_H, content.w, FOOT_H);
    let list = Rect::new(
        content.x,
        head.bottom(),
        content.w,
        (foot.y - head.bottom()).max(0),
    );
    let btn_end = Rect::new(foot.right() - 96, foot.y + 12, 96, 28);
    let btn_restart = Rect::new(btn_end.x - 8 - 96, foot.y + 12, 96, 28);
    let sheet = Rect::new(
        r.x + (r.w - 360) / 2,
        r.body().y + (r.body().h - 168) / 2,
        360,
        168,
    );
    Lay {
        tabs,
        search,
        content,
        head,
        list,
        foot,
        btn_end,
        btn_restart,
        sheet,
        sheet_cancel: Rect::new(
            sheet.right() - 20 - 104 - 8 - 104,
            sheet.bottom() - 48,
            104,
            28,
        ),
        sheet_ok: Rect::new(sheet.right() - 20 - 104, sheet.bottom() - 48, 104, 28),
    }
}

/// Column rectangles of the process table, left to right.
pub(super) fn columns(list: Rect) -> [Rect; 6] {
    let fixed = [56, 0, 92, 72, 92, 108];
    let flex = (list.w - fixed.iter().sum::<i32>()).max(80);
    let mut out = [Rect::new(0, 0, 0, 0); 6];
    let mut x = list.x;
    for (i, f) in fixed.iter().enumerate() {
        let w = if *f == 0 { flex } else { *f };
        out[i] = Rect::new(x, list.y, w, list.h);
        x += w;
    }
    out
}

/// Chart card and its right-hand column on the CPU / Memória / Disco / Rede tabs.
pub(super) struct Split {
    /// Section title row of the chart.
    pub(super) title: Rect,
    pub(super) chart: Rect,
    /// Space under the chart.
    pub(super) below: Rect,
    /// The right-hand column.
    pub(super) side: Rect,
}

fn split(content: Rect, top: i32, fill: bool) -> Split {
    let side_w = 224;
    let lw = (content.w - side_w - 16).max(200);
    let y = content.y + top;
    let ch = if fill {
        (content.h - top - 28).clamp(120, 320)
    } else {
        ((content.h - top - 28) * 45 / 100).clamp(120, 232)
    };
    Split {
        title: Rect::new(content.x, y, lw, 24),
        chart: Rect::new(content.x, y + 28, lw, ch),
        below: Rect::new(
            content.x,
            y + 28 + ch + 16,
            lw,
            (content.bottom() - (y + 28 + ch + 16)).max(0),
        ),
        side: Rect::new(content.x + lw + 16, y, side_w, content.bottom() - y),
    }
}

/// Height reserved above the chart on the Disco and Rede tabs (the volume / interface card).
pub(super) const NET_TOP: i32 = 150;
pub(super) const DISK_TOP: i32 = 140;

pub(super) fn tab_split(content: Rect, tab: u8) -> Split {
    match tab {
        2 => split(content, DISK_TOP + 16, true),
        3 => split(content, NET_TOP + 16, true),
        _ => split(content, 0, false),
    }
}
