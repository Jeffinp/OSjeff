//! The resource monitor: a window with three tabs.
//!
//! * **Processos**: the kernel threads (real CPU share from the scheduler's
//!   tick counters, stack size) and the app windows' process entries (uptime,
//!   the time the compositor spent drawing them, the state they hold), sortable
//!   by column, with "end task".
//! * **Desempenho**: line graphs of the last 60 seconds (CPU per thread and
//!   total, heap, network, compositor draws per second and frame cost) and the
//!   disk usage bar.
//! * **Sistema**: version, uptime, CPU identity (CPUID), memory, screen, boot mode.
//!
//! A [`SysMon`] inside the desktop samples once a second (the compositor loop
//! calls [`Desktop::sample_system`] on the wall-clock tick) and the window only
//! reads it, so an idle desktop with no monitor open does no extra work beyond a
//! few integer operations per second. The data structures and arithmetic live
//! in `osjeff_core::sysmon` (tested); this file is layout and drawing.

use super::ui::*;
use super::*;
use core::fmt::Write as _;
use osjeff_core::klog::FixedBuf;
use osjeff_core::sysif::{DiskUsage, NetStats};
use osjeff_core::sysmon::{
    CpuSample, CpuSampler, MAX_THREADS, ProcRow, RateMeter, RowKind, RowState, Series, SortKey,
    fmt_bytes, fmt_pct10, fmt_rate, fmt_uptime, nice_ceiling, sort_rows,
};

/// Line colours of the scheduler slots in the CPU graph (slot 0 is the compositor).
const THREAD_COLORS: [Color; MAX_THREADS] = [
    Color::rgb(0x7C, 0x6C, 0xFF),
    Color::rgb(0xF5, 0x9E, 0x0B),
    Color::rgb(0x4C, 0xC2, 0xFF),
    Color::rgb(0xF4, 0x72, 0xB6),
    Color::rgb(0xFB, 0x71, 0x85),
    Color::rgb(0xA3, 0xE6, 0x35),
    Color::rgb(0x94, 0xA3, 0xB8),
    Color::rgb(0xE8, 0xED, 0xF7),
];

const ROW_H: i32 = 20;

/// What the compositor loop hands over once a second.
pub struct SysInputs {
    /// Timer ticks since boot.
    pub ticks: u64,
    /// Per-slot cumulative "ticks found running" from the scheduler.
    pub busy: [u64; MAX_THREADS],
    pub heap_used: usize,
    pub heap_total: usize,
    /// Frames drawn in the last second / duration of the last / worst this second.
    pub fps: u32,
    pub frame_us: u64,
    pub max_us: u64,
    pub tsc_khz: u64,
}

/// The sampled history behind the graphs.
pub(crate) struct SysMon {
    sampler: CpuSampler,
    pub last: CpuSample,
    pub cpu_total: Series,
    pub cpu_thr: [Series; MAX_THREADS],
    pub heap: Series,
    pub heap_used: usize,
    pub heap_total: usize,
    rx: RateMeter,
    tx: RateMeter,
    pub rx_rate: u64,
    pub tx_rate: u64,
    pub net_rx: Series,
    pub net_tx: Series,
    disk_r: RateMeter,
    disk_w: RateMeter,
    /// Disk traffic in bytes per second (read, write) and its 60 s history.
    pub disk_rd_rate: u64,
    pub disk_wr_rate: u64,
    pub disk_rd: Series,
    pub disk_wr: Series,
    pub draws: Series,
    pub frame_us: Series,
    pub frame_now_us: u64,
    pub frame_max_us: u64,
    pub uptime_s: u64,
    pub tsc_khz: u64,
}

impl SysMon {
    pub(crate) const fn new() -> Self {
        Self {
            sampler: CpuSampler::new(),
            last: CpuSample {
                thread_pm: [0; MAX_THREADS],
                busy_pm: 0,
                idle_pm: 1000,
            },
            cpu_total: Series::new(),
            cpu_thr: [Series::new(); MAX_THREADS],
            heap: Series::new(),
            heap_used: 0,
            heap_total: 0,
            rx: RateMeter::new(),
            tx: RateMeter::new(),
            rx_rate: 0,
            tx_rate: 0,
            net_rx: Series::new(),
            net_tx: Series::new(),
            disk_r: RateMeter::new(),
            disk_w: RateMeter::new(),
            disk_rd_rate: 0,
            disk_wr_rate: 0,
            disk_rd: Series::new(),
            disk_wr: Series::new(),
            draws: Series::new(),
            frame_us: Series::new(),
            frame_now_us: 0,
            frame_max_us: 0,
            uptime_s: 0,
            tsc_khz: 1,
        }
    }

    /// Take one sample (call once per second).
    pub(crate) fn sample(&mut self, i: &SysInputs) {
        self.uptime_s = i.ticks / crate::interrupts::TIMER_HZ as u64;
        self.tsc_khz = i.tsc_khz.max(1);
        let s = self.sampler.sample(i.ticks, &i.busy);
        self.last = s;
        self.cpu_total.push(s.busy_pm as u32);
        for (series, pm) in self.cpu_thr.iter_mut().zip(s.thread_pm) {
            series.push(pm as u32);
        }
        self.heap_used = i.heap_used;
        self.heap_total = i.heap_total;
        self.heap.push((i.heap_used / 1024) as u32);
        match KernelNetStats.counters() {
            Some(n) => {
                self.rx_rate = self.rx.rate(n.rx_bytes, 1);
                self.tx_rate = self.tx.rate(n.tx_bytes, 1);
            }
            None => {
                self.rx_rate = 0;
                self.tx_rate = 0;
            }
        }
        self.net_rx.push(self.rx_rate.min(u32::MAX as u64) as u32);
        self.net_tx.push(self.tx_rate.min(u32::MAX as u64) as u32);
        let (rd, wr) = crate::ata::io_bytes();
        self.disk_rd_rate = self.disk_r.rate(rd, 1);
        self.disk_wr_rate = self.disk_w.rate(wr, 1);
        self.disk_rd
            .push(self.disk_rd_rate.min(u32::MAX as u64) as u32);
        self.disk_wr
            .push(self.disk_wr_rate.min(u32::MAX as u64) as u32);
        self.draws.push(i.fps);
        self.frame_us.push(i.frame_us.min(u32::MAX as u64) as u32);
        self.frame_now_us = i.frame_us;
        self.frame_max_us = i.max_us;
    }

    /// Peak heap use among the samples kept (KiB-resolution).
    pub(crate) fn heap_peak(&self) -> usize {
        self.heap.max() as usize * 1024
    }
}

/// Per-window state of a resource monitor.
pub(crate) struct MonitorState {
    pub tab: u8,
    pub sort: SortKey,
    pub desc: bool,
    pub sel: usize,
    pub rows: Vec<ProcRow>,
}

impl MonitorState {
    pub(crate) fn new() -> Self {
        Self {
            tab: 0,
            sort: SortKey::Cpu,
            desc: true,
            sel: 0,
            rows: Vec::new(),
        }
    }

    /// Approximate heap this window holds (its row cache).
    pub(crate) fn heap_bytes(&self) -> usize {
        self.rows.capacity() * core::mem::size_of::<ProcRow>()
    }
}

const TABS: [&[u8]; 3] = [b"Processos", b"Desempenho", b"Sistema"];

struct MonLayout {
    tabs: [Rect; 3],
    body: Rect,
}

impl MonLayout {
    fn of(r: Rect) -> MonLayout {
        let x = r.x + 10;
        let ty = r.y + TITLE_H + 8;
        let mut tabs = [Rect::new(0, 0, 0, 0); 3];
        for (i, t) in tabs.iter_mut().enumerate() {
            *t = Rect::new(x + i as i32 * 136, ty, 130, 28);
        }
        let by = ty + 28 + 10;
        MonLayout {
            tabs,
            body: Rect::new(x, by, r.w - 20, (r.bottom() - 10 - by).max(0)),
        }
    }
}

/// Geometry of the process list inside the body.
struct ProcLayout {
    header_y: i32,
    rows_y: i32,
    visible: usize,
    footer: Rect,
    end_btn: Rect,
}

impl ProcLayout {
    fn of(body: Rect) -> ProcLayout {
        let footer = Rect::new(body.x, body.bottom() - 28, body.w, 28);
        let end_btn = Rect::new(footer.right() - 120, footer.y, 120, 28);
        let rows_y = body.y + 24;
        let visible = (((footer.y - 6 - rows_y) / ROW_H).max(1)) as usize;
        ProcLayout {
            header_y: body.y,
            rows_y,
            visible,
            footer,
            end_btn,
        }
    }
}

/// Column positions (in character cells from the body's left edge).
const COL_PID: i32 = 0;
const COL_NAME: i32 = 5;
const COL_ST: i32 = 20;
const COL_CPU: i32 = 25;
const COL_MEM: i32 = 32;
const COL_UP: i32 = 42;

fn column_at(body: Rect, px: i32) -> Option<SortKey> {
    let cell = (px - body.x) / CELL_W;
    match cell {
        c if (COL_NAME..COL_ST - 1).contains(&c) => Some(SortKey::Name),
        c if (COL_CPU..COL_MEM - 1).contains(&c) => Some(SortKey::Cpu),
        c if (COL_MEM..COL_UP - 1).contains(&c) => Some(SortKey::Mem),
        c if (COL_UP..COL_UP + 11).contains(&c) => Some(SortKey::Up),
        _ => None,
    }
}

impl App {
    /// Rough heap held by the app instance, for the monitor's memory column
    /// (`None` when the kernel does not track it).
    pub(crate) fn approx_bytes(&self) -> Option<usize> {
        use core::mem::size_of;
        let n = match self {
            App::Terminal(_) => size_of::<TermState>(),
            App::Editor(_) => size_of::<EditorState>(),
            App::Calculator(_) => size_of::<Calc>(),
            App::Browser(b) => {
                size_of::<BrowserState>()
                    + b.images.bytes()
                    + b.page
                        .as_ref()
                        .map_or(0, |p| p.cmds.len() * size_of::<osjeff_core::web::Cmd>())
            }
            App::Log(l) => size_of::<LogState>() + l.heap_bytes(),
            App::Monitor(m) => size_of::<MonitorState>() + m.heap_bytes(),
            App::Settings(s) => size_of::<SettingsState>() + s.heap_bytes(),
            App::Files(_) | App::Viewer(_) | App::TaskMgr | App::Wasm(_) | App::Gallery(_) => {
                return None;
            }
        };
        Some(n)
    }
}

impl Desktop {
    /// Once-a-second sampling of the system for the monitor graphs, plus the
    /// per-window draw-cost bookkeeping. Cheap: a handful of integer operations
    /// and one pass over the window table.
    pub(crate) fn sample_system(&mut self, i: &SysInputs) {
        self.sysmon.sample(i);
        let khz = self.sysmon.tsc_khz;
        for w in self.wm.windows() {
            // cycles / (cycles per ms) = ms spent this second = permille of the second.
            let cyc = w.app.cost.replace(0);
            w.app.cost_pm.set((cyc / khz).min(1000) as u16);
        }
        self.refresh_monitors();
    }

    /// Rebuild the process rows of every visible monitor window.
    pub(crate) fn refresh_monitors(&mut self) {
        let ids: Vec<(WindowId, SortKey, bool)> = self
            .wm
            .windows()
            .iter()
            .filter(|w| w.shown())
            .filter_map(|w| match &w.app.app {
                App::Monitor(m) => Some((w.id, m.sort, m.desc)),
                _ => None,
            })
            .collect();
        for (id, key, desc) in ids {
            let rows = self.build_rows(key, desc);
            if let Some(App::Monitor(m)) = self.app_mut(id) {
                m.rows = rows;
                m.sel = m.sel.min(m.rows.len().saturating_sub(1));
            }
        }
    }

    /// The process list: kernel threads, app processes, and the idle share.
    fn build_rows(&self, key: SortKey, desc: bool) -> Vec<ProcRow> {
        let mon = &self.sysmon;
        let mut rows = Vec::with_capacity(self.procs.len() + MAX_THREADS + 1);
        for i in 0..self.procs.len() {
            let Some(p) = self.procs.at(i) else { continue };
            // The compositor already appears as a thread.
            if p.kind == ProcKind::System && p.name() == b"compositor" {
                continue;
            }
            let kind = if p.kind == ProcKind::System {
                RowKind::System
            } else {
                RowKind::App
            };
            let state = match p.state {
                ProcState::Running => RowState::Running,
                ProcState::Suspended => RowState::Suspended,
                ProcState::Terminated => RowState::Ended,
            };
            let mut r = ProcRow::new(p.name(), kind, p.pid, state);
            r.up_s = p.ticks;
            if kind == RowKind::App
                && let Some(w) = self.window_of_pid(p.pid).and_then(|id| self.wm.get(id))
            {
                r.cpu_pm = Some(w.app.cost_pm.get());
                r.mem_kib = w.app.app.approx_bytes().map(|b| (b / 1024).max(1) as u32);
            }
            rows.push(r);
        }
        for t in 0..sched::thread_count().min(MAX_THREADS) {
            let pm = mon.last.thread_pm[t];
            let state = if sched::thread_dead(t) {
                RowState::Dead
            } else if pm > 0 {
                RowState::Running
            } else {
                RowState::Idle
            };
            let mut r = ProcRow::new(sched::thread_name(t).as_bytes(), RowKind::Thread, 0, state);
            r.cpu_pm = Some(pm);
            r.up_s = mon.uptime_s.min(u32::MAX as u64) as u32;
            r.mem_kib = Some(sched::thread_stack_kib(t));
            rows.push(r);
        }
        let mut idle = ProcRow::new(b"(ocioso)", RowKind::System, 0, RowState::Idle);
        idle.cpu_pm = Some(mon.last.idle_pm);
        rows.push(idle);
        sort_rows(&mut rows, key, desc);
        rows
    }

    fn monitor_mut(&mut self, id: WindowId) -> Option<&mut MonitorState> {
        match self.app_mut(id) {
            Some(App::Monitor(m)) => Some(m),
            _ => None,
        }
    }

    /// Switch tabs, re-sort or select; returns after refreshing the rows.
    fn monitor_resort(&mut self, id: WindowId) {
        let Some((key, desc)) = self.monitor_mut(id).map(|m| (m.sort, m.desc)) else {
            return;
        };
        let rows = self.build_rows(key, desc);
        if let Some(m) = self.monitor_mut(id) {
            m.rows = rows;
            m.sel = m.sel.min(m.rows.len().saturating_sub(1));
        }
    }

    /// Ask the selected app row's window to close.
    fn monitor_end_task(&mut self, id: WindowId) {
        let Some(row) = self
            .monitor_mut(id)
            .and_then(|m| m.rows.get(m.sel).copied())
        else {
            return;
        };
        if row.kind == RowKind::App
            && let Some(w) = self.window_of_pid(row.pid)
        {
            self.request_close(w);
            crate::notify!(
                Info,
                "Encerrado: {}",
                core::str::from_utf8(row.name()).unwrap_or("?")
            );
        }
    }

    pub(crate) fn monitor_key(&mut self, id: WindowId, key: Key) {
        let Some(m) = self.monitor_mut(id) else {
            return;
        };
        let mut resort = false;
        match key {
            Key::Esc => {
                self.request_close(id);
                return;
            }
            Key::Tab | Key::Right => m.tab = (m.tab + 1) % 3,
            Key::Left => m.tab = (m.tab + 2) % 3,
            Key::Char(b'1') => m.tab = 0,
            Key::Char(b'2') => m.tab = 1,
            Key::Char(b'3') => m.tab = 2,
            Key::Up if m.tab == 0 => m.sel = m.sel.saturating_sub(1),
            Key::Down if m.tab == 0 => m.sel = (m.sel + 1).min(m.rows.len().saturating_sub(1)),
            Key::Char(c @ (b'n' | b'c' | b'm' | b'u')) if m.tab == 0 => {
                let k = match c {
                    b'n' => SortKey::Name,
                    b'c' => SortKey::Cpu,
                    b'm' => SortKey::Mem,
                    _ => SortKey::Up,
                };
                if m.sort == k {
                    m.desc = !m.desc;
                } else {
                    m.sort = k;
                    m.desc = k != SortKey::Name;
                }
                resort = true;
            }
            Key::Delete if m.tab == 0 => {
                self.monitor_end_task(id);
                self.monitor_resort(id);
                return;
            }
            _ => {}
        }
        if resort || key == Key::Tab || matches!(key, Key::Char(b'1'..=b'3')) {
            self.monitor_resort(id);
        }
    }

    pub(crate) fn monitor_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        let lay = MonLayout::of(rect);
        let play = ProcLayout::of(lay.body);
        let Some(m) = self.monitor_mut(id) else {
            return;
        };
        if let Some(i) = lay.tabs.iter().position(|t| t.contains(px, py)) {
            m.tab = i as u8;
            self.monitor_resort(id);
            return;
        }
        if m.tab != 0 {
            return;
        }
        if py >= play.header_y
            && py < play.header_y + ROW_H
            && let Some(k) = column_at(lay.body, px)
        {
            if m.sort == k {
                m.desc = !m.desc;
            } else {
                m.sort = k;
                m.desc = k != SortKey::Name;
            }
            self.monitor_resort(id);
            return;
        }
        if play.end_btn.contains(px, py) {
            self.monitor_end_task(id);
            self.monitor_resort(id);
            return;
        }
        if py >= play.rows_y && py < play.rows_y + play.visible as i32 * ROW_H {
            let first = (m.sel + 1).saturating_sub(play.visible);
            let first = first.min(m.rows.len().saturating_sub(play.visible));
            let i = first + ((py - play.rows_y) / ROW_H) as usize;
            if i < m.rows.len() {
                m.sel = i;
            }
        }
    }

    pub(crate) fn draw_monitor(&self, c: &mut Canvas, r: Rect, m: &MonitorState) {
        let lay = MonLayout::of(r);
        for (i, t) in lay.tabs.iter().enumerate() {
            let st = if m.tab as usize == i {
                Btn::On
            } else {
                Btn::Normal
            };
            button(c, *t, TABS[i], st);
        }
        match m.tab {
            0 => self.draw_processes(c, lay.body, m),
            1 => self.draw_performance(c, lay.body),
            _ => self.draw_system(c, lay.body),
        }
    }

    fn draw_processes(&self, c: &mut Canvas, body: Rect, m: &MonitorState) {
        let pl = ProcLayout::of(body);
        let arrow = if m.desc { b'v' } else { b'^' };
        let mut head = [b' '; 54];
        let put = |h: &mut [u8; 54], col: i32, s: &[u8]| {
            let o = col as usize;
            h[o..o + s.len()].copy_from_slice(s);
        };
        put(&mut head, COL_PID, b"PID");
        put(&mut head, COL_NAME, b"NOME");
        put(&mut head, COL_ST, b"EST");
        put(&mut head, COL_CPU, b"CPU");
        put(&mut head, COL_MEM, b"MEM");
        put(&mut head, COL_UP, b"UPTIME");
        let (sc, sw) = match m.sort {
            SortKey::Name => (COL_NAME, 4),
            SortKey::Cpu => (COL_CPU, 3),
            SortKey::Mem => (COL_MEM, 3),
            SortKey::Up => (COL_UP, 6),
        };
        head[(sc + sw) as usize] = arrow;
        text(
            c,
            body.x,
            pl.header_y + 3,
            body.w,
            &head,
            theme::text_muted(),
        );
        fill(
            c,
            Rect::new(body.x, pl.header_y + ROW_H, body.w, 1),
            theme::line(),
        );

        let first = (m.sel + 1).saturating_sub(pl.visible);
        let first = first.min(m.rows.len().saturating_sub(pl.visible));
        for (k, row) in m.rows.iter().skip(first).take(pl.visible).enumerate() {
            let y = pl.rows_y + k as i32 * ROW_H;
            if first + k == m.sel {
                fill_round(
                    c,
                    Rect::new(body.x - 4, y - 1, body.w + 8, ROW_H),
                    4,
                    theme::selection(),
                );
            }
            let (fg, cpu_fg) = match (row.kind, row.state) {
                (_, RowState::Dead) => (theme::CLOSE, theme::CLOSE),
                (RowKind::Thread, _) => (theme::accent(), theme::accent()),
                (RowKind::System, _) => (theme::text_muted(), theme::text_muted()),
                (RowKind::App, _) => (theme::text(), theme::text_muted()),
            };
            let x = |col: i32| body.x + col * CELL_W;
            let mut b = FixedBuf::<8>::new();
            if row.kind == RowKind::Thread {
                let _ = b.write_str("thr");
            } else if row.pid == 0 {
                let _ = b.write_str("-");
            } else {
                let _ = write!(b, "{}", row.pid);
            }
            text(
                c,
                x(COL_PID),
                y + 3,
                4 * CELL_W,
                b.as_bytes(),
                theme::text_muted(),
            );
            text(c, x(COL_NAME), y + 3, 14 * CELL_W, row.name(), fg);
            text(
                c,
                x(COL_ST),
                y + 3,
                4 * CELL_W,
                row.state.label().as_bytes(),
                fg,
            );
            match row.cpu_pm {
                Some(pm) => {
                    let s = fmt_pct10(pm as u32);
                    let r = Rect::new(x(COL_CPU), y, 6 * CELL_W, ROW_H);
                    text_right(c, r, s.as_bytes(), cpu_fg);
                }
                None => text(c, x(COL_CPU + 5), y + 3, CELL_W, b"-", theme::text_muted()),
            }
            match row.mem_kib {
                Some(k) => {
                    let s = fmt_bytes(k as u64 * 1024);
                    let r = Rect::new(x(COL_MEM), y, 9 * CELL_W, ROW_H);
                    text_right(c, r, s.as_bytes(), fg);
                }
                None => text(c, x(COL_MEM + 8), y + 3, CELL_W, b"-", theme::text_muted()),
            }
            if row.kind == RowKind::App || row.up_s > 0 {
                let s = fmt_uptime(row.up_s as u64);
                text(c, x(COL_UP), y + 3, 11 * CELL_W, s.as_bytes(), fg);
            }
        }
        // Footer: how to read the CPU column, and the end-task button.
        text(
            c,
            pl.footer.x,
            pl.footer.y + 7,
            pl.footer.w - 130,
            b"CPU: threads + ocioso = 100%. Apps: tempo de desenho.",
            theme::text_muted(),
        );
        let can_end = m
            .rows
            .get(m.sel)
            .is_some_and(|r| r.kind == RowKind::App && r.pid != 0);
        let st = if can_end { Btn::Normal } else { Btn::Disabled };
        button(c, pl.end_btn, b"Encerrar", st);
    }

    fn draw_performance(&self, c: &mut Canvas, body: Rect) {
        let mon = &self.sysmon;
        let gap = 10;
        let pw = (body.w - gap) / 2;
        let ph = (body.h - 2 * gap) / 3;
        let cell = |col: i32, row: i32| {
            Rect::new(body.x + col * (pw + gap), body.y + row * (ph + gap), pw, ph)
        };

        // CPU: total plus one line per scheduler slot.
        let mut lines: Vec<Line<'_>> = Vec::with_capacity(MAX_THREADS + 1);
        lines.push(Line {
            series: &mon.cpu_total,
            color: theme::accent(),
            label: b"total",
        });
        let nthr = sched::thread_count().min(MAX_THREADS);
        for t in 0..nthr {
            lines.push(Line {
                series: &mon.cpu_thr[t],
                color: THREAD_COLORS[t],
                label: sched::thread_name(t).as_bytes(),
            });
        }
        let v = fmt_pct10(mon.last.busy_pm as u32);
        let mut sub = FixedBuf::<40>::new();
        let _ = write!(sub, "ocioso {}", fmt_pct10(mon.last.idle_pm as u32));
        graph(
            c,
            cell(0, 0),
            &Graph {
                title: b"CPU",
                value: v.as_bytes(),
                sub: sub.as_bytes(),
                lines: &lines,
                ceiling: 1000,
                ceiling_label: b"100%",
            },
        );

        // Heap.
        let peak = mon.heap_peak();
        // Powers of two, so the axis reads 8.0 MiB / 16.0 MiB rather than 9.7 MiB.
        let ceil = (mon.heap.max() as u64).max(1024).next_power_of_two();
        let used = fmt_bytes(mon.heap_used as u64);
        let mut sub = FixedBuf::<48>::new();
        let _ = write!(
            sub,
            "livre {} pico {}",
            fmt_bytes(mon.heap_total.saturating_sub(mon.heap_used) as u64),
            fmt_bytes(peak as u64)
        );
        let mut top = FixedBuf::<16>::new();
        let _ = write!(top, "{}", fmt_bytes(ceil * 1024));
        graph(
            c,
            cell(1, 0),
            &Graph {
                title: b"Heap do kernel",
                value: used.as_bytes(),
                sub: sub.as_bytes(),
                lines: &[Line {
                    series: &mon.heap,
                    color: theme::accent(),
                    label: b"usado",
                }],
                ceiling: ceil,
                ceiling_label: top.as_bytes(),
            },
        );

        // Network.
        let net = KernelNetStats.counters();
        let ceil = (mon.net_rx.max().max(mon.net_tx.max()) as u64)
            .max(1024)
            .next_power_of_two();
        let mut head = FixedBuf::<24>::new();
        let mut sub = FixedBuf::<48>::new();
        match net {
            Some(n) => {
                let _ = write!(head, "{}", fmt_rate(mon.rx_rate));
                let _ = write!(
                    sub,
                    "rx {} tx {}",
                    fmt_bytes(n.rx_bytes),
                    fmt_bytes(n.tx_bytes)
                );
            }
            None => {
                let _ = write!(head, "sem NIC");
            }
        }
        let mut top = FixedBuf::<16>::new();
        let _ = write!(top, "{}", fmt_rate(ceil));
        graph(
            c,
            cell(0, 1),
            &Graph {
                title: b"Rede",
                value: head.as_bytes(),
                sub: sub.as_bytes(),
                lines: &[
                    Line {
                        series: &mon.net_rx,
                        color: theme::accent(),
                        label: b"rx",
                    },
                    Line {
                        series: &mon.net_tx,
                        color: Color::rgb(0xF5, 0x9E, 0x0B),
                        label: b"tx",
                    },
                ],
                ceiling: ceil,
                ceiling_label: top.as_bytes(),
            },
        );

        // Disk usage bar.
        self.draw_disk_usage(c, cell(1, 1));

        // Compositor: draws per second and frame cost.
        let ceil = nice_ceiling(mon.draws.max() as u64, 10);
        let mut head = FixedBuf::<16>::new();
        let _ = write!(head, "{}/s", mon.draws.last().unwrap_or(0));
        let mut top = FixedBuf::<16>::new();
        let _ = write!(top, "{ceil}/s");
        graph(
            c,
            cell(0, 2),
            &Graph {
                title: b"Quadros desenhados",
                value: head.as_bytes(),
                sub: b"so redesenha quando muda",
                lines: &[Line {
                    series: &mon.draws,
                    color: theme::accent(),
                    label: b"draws/s",
                }],
                ceiling: ceil,
                ceiling_label: top.as_bytes(),
            },
        );
        let ceil = nice_ceiling(mon.frame_us.max() as u64, 1000);
        let mut head = FixedBuf::<16>::new();
        let _ = write!(
            head,
            "{}.{} ms",
            mon.frame_now_us / 1000,
            mon.frame_now_us % 1000 / 100
        );
        let mut sub = FixedBuf::<32>::new();
        let _ = write!(
            sub,
            "pior no segundo {}.{} ms",
            mon.frame_max_us / 1000,
            mon.frame_max_us % 1000 / 100
        );
        let mut top = FixedBuf::<16>::new();
        let _ = write!(top, "{}.{} ms", ceil / 1000, ceil % 1000 / 100);
        graph(
            c,
            cell(1, 2),
            &Graph {
                title: b"Custo do quadro",
                value: head.as_bytes(),
                sub: sub.as_bytes(),
                lines: &[Line {
                    series: &mon.frame_us,
                    color: Color::rgb(0x7C, 0x6C, 0xFF),
                    label: b"ultimo quadro",
                }],
                ceiling: ceil,
                ceiling_label: top.as_bytes(),
            },
        );
    }

    fn draw_disk_usage(&self, c: &mut Canvas, r: Rect) {
        let usage = VfsUsage;
        let u = usage.usage();
        fill_round(c, r, 10, PANEL_DARK);
        text(c, r.x + 10, r.y + 8, r.w - 20, b"Disco", theme::HEADER_TEXT);
        let mut label = FixedBuf::<32>::new();
        let _ = write!(label, "{}", usage.label());
        text(c, r.x + 10, r.y + 26, r.w - 20, label.as_bytes(), DIM_TEXT);
        match (u.used_bytes, u.total_bytes, u.used_permille()) {
            (Some(used), Some(total), Some(pm)) => {
                let mut head = FixedBuf::<16>::new();
                let _ = write!(head, "{}", fmt_pct10(pm));
                text_right(
                    c,
                    Rect::new(r.x, r.y + 8, r.w - 10, CELL_H),
                    head.as_bytes(),
                    theme::accent(),
                );
                let bar = Rect::new(r.x + 10, r.y + 56, r.w - 20, 14);
                usage_bar(c, bar, pm, theme::accent());
                let mut l = FixedBuf::<48>::new();
                let _ = write!(l, "{} de {}", fmt_bytes(used), fmt_bytes(total));
                text(
                    c,
                    r.x + 10,
                    r.y + 80,
                    r.w - 20,
                    l.as_bytes(),
                    theme::HEADER_TEXT,
                );
                if let (Some(a), Some(b)) = (u.items_used, u.items_total) {
                    let mut l = FixedBuf::<48>::new();
                    let _ = write!(l, "{a} de {b} entradas");
                    text(c, r.x + 10, r.y + 100, r.w - 20, l.as_bytes(), DIM_TEXT);
                }
            }
            _ => {
                text(c, r.x + 10, r.y + 60, r.w - 20, b"n/d", DIM_TEXT);
            }
        }
    }

    fn draw_system(&self, c: &mut Canvas, body: Rect) {
        let mon = &self.sysmon;
        let mut y = body.y + 2;
        let mut line = |c: &mut Canvas, key: &[u8], val: &[u8]| {
            text(c, body.x, y, 14 * CELL_W, key, theme::text_muted());
            text(
                c,
                body.x + 14 * CELL_W,
                y,
                body.w - 14 * CELL_W,
                val,
                theme::text(),
            );
            y += 24;
        };
        let mut b = FixedBuf::<80>::new();
        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        let trace = if cfg!(feature = "perf-trace") {
            " +perf-trace"
        } else {
            ""
        };
        let _ = write!(b, "OSjeff {} ({profile}{trace})", env!("CARGO_PKG_VERSION"));
        line(c, b"Sistema", b.as_bytes());
        let mut b = FixedBuf::<80>::new();
        let _ = write!(b, "{}", fmt_uptime(mon.uptime_s));
        line(c, b"Ligado ha", b.as_bytes());
        match crate::sysinfo::get() {
            Some(si) => {
                let mut b = FixedBuf::<80>::new();
                let brand = si.brand.as_bytes();
                if brand.is_empty() {
                    let _ = write!(b, "{}", si.vendor);
                } else {
                    // The brand string is space-padded on some CPUs.
                    let t = brand.iter().skip_while(|&&c| c == b' ').copied();
                    for ch in t {
                        let _ = b.write_char(ch as char);
                    }
                }
                line(c, b"CPU", b.as_bytes());
                let mut b = FixedBuf::<80>::new();
                let _ = write!(b, "{}", si.vendor);
                if !si.hypervisor.as_bytes().is_empty() {
                    let _ = write!(b, "  VM: {}", si.hypervisor);
                }
                line(c, b"Fabricante", b.as_bytes());
                line(c, b"Recursos", si.features.as_bytes());
                line(c, b"Ponto flut.", b"soft-float (o kernel nao usa SSE)");
                let mut b = FixedBuf::<80>::new();
                let _ = write!(b, "{} kHz (calibrado pelo PIT)", mon.tsc_khz);
                line(c, b"TSC", b.as_bytes());
                let mut b = FixedBuf::<80>::new();
                let _ = write!(
                    b,
                    "{} (usavel {} + bootloader {})",
                    fmt_bytes(si.total_ram()),
                    fmt_bytes(si.usable_bytes),
                    fmt_bytes(si.boot_bytes)
                );
                line(c, b"RAM total", b.as_bytes());
                let mut b = FixedBuf::<80>::new();
                let _ = write!(
                    b,
                    "imagem {}  heap {} de {}",
                    fmt_bytes(si.kernel_bytes),
                    fmt_bytes(mon.heap_used as u64),
                    fmt_bytes(mon.heap_total as u64)
                );
                line(c, b"Kernel", b.as_bytes());
                let mut b = FixedBuf::<80>::new();
                let _ = write!(b, "{}x{}", si.width, si.height);
                line(c, b"Tela", b.as_bytes());
                line(c, b"Boot", si.boot_mode.as_bytes());
            }
            None => line(c, b"CPU", b"n/d"),
        }
        let mut b = FixedBuf::<80>::new();
        let n = sched::thread_count();
        let _ = write!(b, "{n}:");
        for t in 0..n {
            let _ = write!(b, " {}", sched::thread_name(t));
        }
        line(c, b"Threads", b.as_bytes());
    }
}
