//! Tarefas: the activity monitor. One window with five tabs (CPU, Memória, Disco, Rede,
//! Processos) that replaces the old Task Manager and the Resource Monitor.
//!
//! * [`SysMon`] samples the machine once a second (the compositor loop calls
//!   [`Desktop::sample_system`] on the wall-clock tick): CPU shares from the scheduler's
//!   tick counters, heap, disk and network rates, load averages. It keeps 60 s of history
//!   per signal and the previous value, so the window can glide between two samples.
//! * [`TarefasState`] is the window: the tab, the sort column, the selection, the search
//!   text. The pure parts (names, formatting, sorting, interpolation, rates) are in
//!   `osjeff_core::activity`; this file is layout, drawing and the glue to the kernel.
//!
//! The window repaints itself once a second like the other live windows, and for about half
//! a second after each sample it animates (the curves scroll by one step, the bars slide to
//! their new length). Hidden and minimised windows cost nothing beyond the sampler's
//! handful of integer operations.

use super::kit::{self, Chart, Curve};
use super::ui::{self, ButtonKind};
use super::*;
use crate::text::{self, BODY, CALLOUT, FOOTNOTE, TITLE2, TITLE3, Weight};
use core::cell::Cell;
use core::fmt::Write as _;
use osjeff_core::activity::{
    self, Column, Glide, LoadAvg, Pressure, Rate, TaskKind, TaskRow, TaskState, fold, sample_under,
    smooth121, snapshot, sort_tasks,
};
use osjeff_core::klog::FixedBuf;
use osjeff_core::sysif::{DiskUsage, NetStats};
use osjeff_core::sysmon::{CpuSample, CpuSampler, HIST, MAX_THREADS, Series, nice_ceiling};

/// Milliseconds the glide between two samples takes.
pub(crate) const ANIM_MS: u32 = 450;

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
    /// The sample before `last` (the animation starts from it).
    pub prev: CpuSample,
    pub cpu_total: Series,
    pub heap: Series,
    pub heap_used: usize,
    pub heap_prev: usize,
    pub heap_total: usize,
    rx: Rate,
    tx: Rate,
    pub rx_rate: u64,
    pub tx_rate: u64,
    pub rx_prev: u64,
    pub tx_prev: u64,
    pub net_rx: Series,
    pub net_tx: Series,
    disk_r: Rate,
    disk_w: Rate,
    pub disk_rd_rate: u64,
    pub disk_wr_rate: u64,
    pub disk_rd_prev: u64,
    pub disk_wr_prev: u64,
    pub disk_rd: Series,
    pub disk_wr: Series,
    pub load: LoadAvg,
    pub frame_us: Series,
    pub frame_now_us: u64,
    pub frame_max_us: u64,
    pub fps: u32,
    pub uptime_s: u64,
    pub tsc_khz: u64,
    /// Milliseconds since the last sample (stepped by the animation clock).
    pub age_ms: u32,
    /// Samples taken so far.
    pub samples: u32,
}

impl SysMon {
    pub(crate) const fn new() -> Self {
        let idle = CpuSample {
            thread_pm: [0; MAX_THREADS],
            busy_pm: 0,
            idle_pm: 1000,
        };
        Self {
            sampler: CpuSampler::new(),
            last: idle,
            prev: idle,
            cpu_total: Series::new(),
            heap: Series::new(),
            heap_used: 0,
            heap_prev: 0,
            heap_total: 0,
            rx: Rate::new(64),
            tx: Rate::new(64),
            rx_rate: 0,
            tx_rate: 0,
            rx_prev: 0,
            tx_prev: 0,
            net_rx: Series::new(),
            net_tx: Series::new(),
            disk_r: Rate::new(64),
            disk_w: Rate::new(64),
            disk_rd_rate: 0,
            disk_wr_rate: 0,
            disk_rd_prev: 0,
            disk_wr_prev: 0,
            disk_rd: Series::new(),
            disk_wr: Series::new(),
            load: LoadAvg::new(),
            frame_us: Series::new(),
            frame_now_us: 0,
            frame_max_us: 0,
            fps: 0,
            uptime_s: 0,
            tsc_khz: 1,
            age_ms: u32::MAX / 2,
            samples: 0,
        }
    }

    /// Take one sample (call once per second).
    pub(crate) fn sample(&mut self, i: &SysInputs) {
        let t_ms = i.ticks.saturating_mul(1000) / crate::interrupts::TIMER_HZ as u64;
        self.uptime_s = t_ms / 1000;
        self.tsc_khz = i.tsc_khz.max(1);
        self.prev = self.last;
        self.last = self.sampler.sample(i.ticks, &i.busy);
        self.cpu_total.push(self.last.busy_pm as u32);
        self.load.feed(self.last.busy_pm as u32);
        self.heap_prev = self.heap_used;
        self.heap_used = i.heap_used;
        self.heap_total = i.heap_total;
        self.heap.push((i.heap_used / 1024) as u32);
        self.rx_prev = self.rx_rate;
        self.tx_prev = self.tx_rate;
        match KernelNetStats.counters() {
            Some(n) => {
                self.rx_rate = self.rx.feed(n.rx_bytes, t_ms);
                self.tx_rate = self.tx.feed(n.tx_bytes, t_ms);
            }
            None => {
                self.rx_rate = 0;
                self.tx_rate = 0;
            }
        }
        self.net_rx.push(self.rx_rate.min(u32::MAX as u64) as u32);
        self.net_tx.push(self.tx_rate.min(u32::MAX as u64) as u32);
        let (rd, wr) = crate::ata::io_bytes();
        self.disk_rd_prev = self.disk_rd_rate;
        self.disk_wr_prev = self.disk_wr_rate;
        self.disk_rd_rate = self.disk_r.feed(rd, t_ms);
        self.disk_wr_rate = self.disk_w.feed(wr, t_ms);
        self.disk_rd
            .push(self.disk_rd_rate.min(u32::MAX as u64) as u32);
        self.disk_wr
            .push(self.disk_wr_rate.min(u32::MAX as u64) as u32);
        self.fps = i.fps;
        self.frame_us.push(i.frame_us.min(u32::MAX as u64) as u32);
        self.frame_now_us = i.frame_us;
        self.frame_max_us = i.max_us;
        self.age_ms = 0;
        self.samples = self.samples.saturating_add(1);
    }

    /// Progress of the glide since the last sample, 0..=256 (256 = settled), eased out.
    pub(crate) fn t_q8(&self) -> i64 {
        if osjeff_core::anim::reduce_motion() || self.samples < 2 {
            return 256;
        }
        kit::ease_out((self.age_ms.min(ANIM_MS) as i64 * 256) / ANIM_MS as i64)
    }

    /// Is the glide still running?
    pub(crate) fn gliding(&self) -> bool {
        !osjeff_core::anim::reduce_motion() && self.samples >= 2 && self.age_ms < ANIM_MS
    }

    /// Peak heap use among the samples kept (KiB resolution).
    pub(crate) fn heap_peak(&self) -> u64 {
        self.heap.max() as u64 * 1024
    }

    /// Heap use now, glided from the previous sample.
    fn heap_now(&self) -> u64 {
        kit::lerp(self.heap_prev as i64, self.heap_used as i64, self.t_q8()) as u64
    }
}

// ------------------------------------------------------------------ state

pub(crate) const TAB_NAMES: [&str; 5] = ["CPU", "Memória", "Disco", "Rede", "Processos"];
pub(crate) const TAB_PROCESSES: u8 = 4;

/// Words Busca also knows Tarefas by, and the tab each opens (the old Monitor lives on as these).
pub(crate) const SEARCH_ALIASES: [(&str, u8); 6] = [
    ("Monitor", 0),
    ("Desempenho", 0),
    ("Processador", 0),
    ("Memória", 1),
    ("Disco", 2),
    ("Rede", 3),
];

const ROW_H: i32 = 28;
const HEAD_H: i32 = 28;
const FOOT_H: i32 = 52;
const PAD: i32 = 16;

/// Per-window state.
pub(crate) struct TarefasState {
    pub tab: u8,
    pub sort: Column,
    pub desc: bool,
    /// Rows of the current tab (filtered, sorted).
    pub rows: Vec<TaskRow>,
    /// The app icon of each row, when it has one.
    icons: Vec<Option<Icon>>,
    /// CPU share of each row at the previous sample, for the bars to glide from.
    prev_cpu: Vec<(u32, u16)>,
    pub sel: Option<u32>,
    pub query: String,
    pub search_focus: bool,
    /// A row waiting for the user's "Encerrar" confirmation.
    pub confirm: Option<u32>,
    /// What the pointer is over (see the `H_*` keys); the top bit is the pressed state.
    pub hover: Cell<u32>,
    /// Scroll position of the process table in pixels.
    scroll: Glide,
    sb: osjeff_core::widgets::ScrollbarFade,
    /// A short message after an action (`(text, error)`), cleared by the next sample.
    pub msg: Option<(String, bool)>,
}

impl TarefasState {
    pub(crate) fn new(tab: u8) -> Self {
        Self {
            tab: tab.min(TAB_PROCESSES),
            sort: Column::Cpu,
            desc: true,
            rows: Vec::new(),
            icons: Vec::new(),
            prev_cpu: Vec::new(),
            sel: None,
            query: String::new(),
            search_focus: false,
            confirm: None,
            hover: Cell::new(0),
            scroll: Glide::at(0),
            sb: osjeff_core::widgets::ScrollbarFade::new(),
            msg: None,
        }
    }

    /// Approximate heap this window holds (its row cache).
    pub(crate) fn heap_bytes(&self) -> usize {
        self.rows.capacity() * core::mem::size_of::<TaskRow>() + self.icons.capacity() * 2
    }

    fn sel_index(&self) -> Option<usize> {
        let id = self.sel?;
        self.rows.iter().position(|r| r.id == id)
    }
}

// Hover keys: what the pointer is over, so the window repaints only when it changes.
const H_DOWN: u32 = 1 << 31;
const H_END: u32 = 0x10;
const H_RESTART: u32 = 0x11;
const H_CANCEL: u32 = 0x12;
const H_OK: u32 = 0x13;
const H_SEARCH: u32 = 0x14;
const H_TAB: u32 = 0x20; // + tab
const H_HEAD: u32 = 0x100; // + column
const H_ROW: u32 = 0x1000; // + row index
const H_SAMPLE: u32 = 0x10000; // + sample index

// ------------------------------------------------------------------ layout

struct Lay {
    tabs: Rect,
    search: Rect,
    content: Rect,
    // Processos
    head: Rect,
    list: Rect,
    foot: Rect,
    btn_end: Rect,
    btn_restart: Rect,
    // The confirmation sheet.
    sheet: Rect,
    sheet_cancel: Rect,
    sheet_ok: Rect,
}

fn lay(r: Rect) -> Lay {
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
fn columns(list: Rect) -> [Rect; 6] {
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
struct Split {
    /// Section title row of the chart.
    title: Rect,
    chart: Rect,
    /// Space under the chart.
    below: Rect,
    /// The right-hand column.
    side: Rect,
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
const NET_TOP: i32 = 150;
const DISK_TOP: i32 = 140;

fn tab_split(content: Rect, tab: u8) -> Split {
    match tab {
        2 => split(content, DISK_TOP + 16, true),
        3 => split(content, NET_TOP + 16, true),
        _ => split(content, 0, false),
    }
}

// ------------------------------------------------------------------ the Desktop side

impl App {
    /// Rough heap held by the app instance, for the memory tab (`None` when the kernel
    /// does not track it).
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
            App::Tarefas(m) => size_of::<TarefasState>() + m.heap_bytes(),
            App::Settings(s) => size_of::<SettingsState>() + s.heap_bytes(),
            App::Files(_) | App::Viewer(_) | App::Wasm(_) | App::Gallery(_) => {
                return None;
            }
        };
        Some(n)
    }
}

impl Desktop {
    /// Once-a-second sampling of the system for the graphs, plus the per-window draw-cost
    /// bookkeeping. Cheap: a handful of integer operations and one pass over the window table.
    pub(crate) fn sample_system(&mut self, i: &SysInputs) {
        self.sysmon.sample(i);
        let khz = self.sysmon.tsc_khz;
        for w in self.wm.windows() {
            // cycles / (cycles per ms) = ms spent this second = permille of the second.
            let cyc = w.app.cost.replace(0);
            w.app.cost_pm.set((cyc / khz).min(1000) as u16);
        }
        self.refresh_tarefas();
    }

    /// The ids of the visible Tarefas windows.
    fn tarefas_ids(&self) -> Vec<WindowId> {
        self.wm
            .windows()
            .iter()
            .filter(|w| w.shown() && matches!(w.app.app, App::Tarefas(_)))
            .map(|w| w.id)
            .collect()
    }

    /// Rebuild the rows of every visible Tarefas window (once a second and on changes).
    pub(crate) fn refresh_tarefas(&mut self) {
        for id in self.tarefas_ids() {
            self.tarefas_rebuild(id);
        }
    }

    fn tarefas_mut(&mut self, id: WindowId) -> Option<&mut TarefasState> {
        match self.app_mut(id) {
            Some(App::Tarefas(t)) => Some(t),
            _ => None,
        }
    }

    /// Open (or focus) Tarefas on `tab`.
    pub(crate) fn open_tarefas_tab(&mut self, tab: u8) {
        let Some(id) = self.launch(Kind::TaskMgr) else {
            return;
        };
        if let Some(t) = self.tarefas_mut(id) {
            t.tab = tab.min(TAB_PROCESSES);
        }
        self.tarefas_rebuild(id);
    }

    fn tarefas_rebuild(&mut self, id: WindowId) {
        let Some((tab, sort, desc, query)) = self.tarefas_mut(id).map(|t| {
            let (s, d) = match t.tab {
                0 => (Column::Cpu, true),
                1 => (Column::Mem, true),
                _ => (t.sort, t.desc),
            };
            (t.tab, s, d, fold(&t.query))
        }) else {
            return;
        };
        if tab == 2 || tab == 3 {
            return;
        }
        let (rows, icons) = self.build_rows(sort, desc, &query);
        if let Some(t) = self.tarefas_mut(id) {
            t.prev_cpu = t
                .rows
                .iter()
                .map(|r| (r.id, r.cpu_pm.unwrap_or(0)))
                .collect();
            t.rows = rows;
            t.icons = icons;
            if let Some(s) = t.sel
                && !t.rows.iter().any(|r| r.id == s)
            {
                t.sel = None;
            }
            t.msg = None;
        }
    }

    /// The process list: the process table, the kernel threads and the idle share.
    fn build_rows(
        &self,
        sort: Column,
        desc: bool,
        query: &str,
    ) -> (Vec<TaskRow>, Vec<Option<Icon>>) {
        let mon = &self.sysmon;
        let wasm = crate::wasm::statuses();
        let kernel_kib = crate::sysinfo::get().map(|si| (si.kernel_bytes / 1024).max(1) as u32);
        let mut rows: Vec<(TaskRow, Option<Icon>)> = Vec::with_capacity(self.procs.len() + 9);
        for i in 0..self.procs.len() {
            let Some(p) = self.procs.at(i) else { continue };
            // The compositor already appears as a thread.
            if p.kind == ProcKind::System && p.name() == b"compositor" {
                continue;
            }
            let kind = if p.kind == ProcKind::System {
                TaskKind::System
            } else {
                TaskKind::App
            };
            let state = match p.state {
                ProcState::Running => TaskState::Running,
                ProcState::Suspended => TaskState::Suspended,
                ProcState::Terminated => TaskState::Ended,
            };
            let mut r = TaskRow::new(p.pid as u32, p.name(), kind, p.pid, state);
            r.up_s = p.ticks;
            let mut icon = None;
            if kind == TaskKind::App
                && let Some(w) = self.window_of_pid(p.pid).and_then(|id| self.wm.get(id))
            {
                icon = Some(w.app.kind().icon());
                r.cpu_pm = Some(w.app.cost_pm.get());
                r.mem_kib = w.app.app.approx_bytes().map(|b| (b / 1024).max(1) as u32);
                if let App::Wasm(ww) = &w.app.app {
                    r.name = w.app.title.clone();
                    if let Some(s) = wasm.iter().find(|s| s.id == ww.id) {
                        r.cpu_pm = Some(s.cpu_pct as u16 * 10);
                        r.mem_kib = Some(s.mem_kib);
                        r.raw = s.app_id.clone();
                        r.state = match s.state {
                            crate::wasm::State::Starting => TaskState::Waiting,
                            crate::wasm::State::Running => TaskState::Running,
                            crate::wasm::State::Suspended => TaskState::Suspended,
                            crate::wasm::State::Exited => TaskState::Ended,
                            crate::wasm::State::Crashed => TaskState::Stopped,
                        };
                    }
                }
            } else if kind == TaskKind::System {
                r.mem_kib = kernel_kib;
            }
            rows.push((r, icon));
        }
        for t in 0..sched::thread_count().min(MAX_THREADS) {
            let pm = mon.last.thread_pm[t];
            let state = if sched::thread_dead(t) {
                TaskState::Stopped
            } else if pm > 0 {
                TaskState::Running
            } else {
                TaskState::Waiting
            };
            let mut r = TaskRow::new(
                0x1_0000 + t as u32,
                sched::thread_name(t).as_bytes(),
                TaskKind::Thread,
                0,
                state,
            );
            r.cpu_pm = Some(pm);
            r.up_s = mon.uptime_s.min(u32::MAX as u64) as u32;
            r.mem_kib = Some(sched::thread_stack_kib(t));
            rows.push((r, None));
        }
        let mut idle = TaskRow::new(
            0x2_0000,
            b"(ocioso)",
            TaskKind::System,
            0,
            TaskState::Waiting,
        );
        idle.cpu_pm = Some(mon.last.idle_pm);
        rows.push((idle, None));
        rows.retain(|(r, _)| r.matches(query));
        // Sort rows and icons together: sort the rows, then look each icon up by id.
        let icon_of: Vec<(u32, Option<Icon>)> = rows.iter().map(|(r, i)| (r.id, *i)).collect();
        let mut only: Vec<TaskRow> = rows.into_iter().map(|(r, _)| r).collect();
        sort_tasks(&mut only, sort, desc);
        let icons = only
            .iter()
            .map(|r| {
                icon_of
                    .iter()
                    .find(|(id, _)| *id == r.id)
                    .and_then(|(_, i)| *i)
            })
            .collect();
        (only, icons)
    }

    // ------------------------------------------------------------ actions

    /// What "Encerrar" does for `row` (`None`: nothing the user may end).
    fn can_end(&self, row: &TaskRow) -> bool {
        match row.kind {
            TaskKind::App => self.window_of_pid(row.pid).is_some(),
            TaskKind::Thread => matches!(row.raw.as_str(), "appd" | "shelld" | "shelld2"),
            TaskKind::System => false,
        }
    }

    fn can_restart(&self, row: &TaskRow) -> bool {
        row.kind == TaskKind::App && self.window_of_pid(row.pid).is_some()
    }

    /// Does ending `row` need the user's confirmation first?
    fn needs_confirm(row: &TaskRow) -> bool {
        row.kind != TaskKind::App
    }

    /// End the selected row of window `id`. System rows ask first.
    fn tarefas_end(&mut self, id: WindowId, confirmed: bool) {
        let Some(row) = self
            .tarefas_mut(id)
            .and_then(|t| t.sel_index().and_then(|i| t.rows.get(i).cloned()))
        else {
            return;
        };
        if !self.can_end(&row) {
            return;
        }
        if Self::needs_confirm(&row) && !confirmed {
            if let Some(t) = self.tarefas_mut(id) {
                t.confirm = Some(row.id);
            }
            return;
        }
        if let Some(t) = self.tarefas_mut(id) {
            t.confirm = None;
        }
        let said = match row.kind {
            TaskKind::App => {
                if let Some(w) = self.window_of_pid(row.pid) {
                    self.request_close(w);
                }
                alloc::format!("{} encerrado.", row.name)
            }
            _ => match row.raw.as_str() {
                "appd" => {
                    let wasm: Vec<WindowId> = self
                        .wm
                        .windows()
                        .iter()
                        .filter(|w| matches!(w.app.app, App::Wasm(_)))
                        .map(|w| w.id)
                        .collect();
                    let n = wasm.len();
                    for w in wasm {
                        self.request_close(w);
                    }
                    alloc::format!("{n} aplicativo(s) encerrado(s).")
                }
                _ => {
                    // The command line workers: interrupt whatever the terminals run.
                    let mut n = 0;
                    for w in self.wm.windows() {
                        if let App::Terminal(t) = &w.app.app
                            && t.term.is_running()
                        {
                            shellhost::cancel(t.uid);
                            n += 1;
                        }
                    }
                    alloc::format!("{n} comando(s) interrompido(s).")
                }
            },
        };
        if let Some(t) = self.tarefas_mut(id) {
            t.msg = Some((said, false));
        }
        self.tarefas_rebuild(id);
    }

    /// Restart the selected app: a fresh window of the same kind (or a fresh instance of a
    /// WASM package).
    fn tarefas_restart(&mut self, id: WindowId) {
        let Some(row) = self
            .tarefas_mut(id)
            .and_then(|t| t.sel_index().and_then(|i| t.rows.get(i).cloned()))
        else {
            return;
        };
        if !self.can_restart(&row) {
            return;
        }
        let Some(w) = self.window_of_pid(row.pid) else {
            return;
        };
        if let Some(h) = self.wasm_handle(w) {
            crate::wasm::restart(h);
        } else if let Some(kind) = self.kind_of(w) {
            self.request_close(w);
            self.open_new(kind);
        }
        if let Some(t) = self.tarefas_mut(id) {
            t.msg = Some((alloc::format!("{} reiniciado.", row.name), false));
        }
    }

    // ------------------------------------------------------------ input

    pub(crate) fn tarefas_key(&mut self, id: WindowId, key: Key) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let l = lay(rect);
        let visible = (l.list.h / ROW_H).max(1) as usize;
        let Some(t) = self.tarefas_mut(id) else {
            return;
        };
        if t.confirm.is_some() {
            match key {
                Key::Esc => t.confirm = None,
                Key::Enter => self.tarefas_end(id, true),
                _ => {}
            }
            return;
        }
        if t.search_focus {
            match key {
                Key::Esc => {
                    t.query.clear();
                    t.search_focus = false;
                }
                Key::Enter | Key::Down => t.search_focus = false,
                Key::Backspace => {
                    t.query.pop();
                }
                Key::Char(b) if (0x20..0x7F).contains(&b) && t.query.len() < 32 => {
                    t.query.push(b as char);
                }
                _ => return,
            }
            self.tarefas_rebuild(id);
            return;
        }
        let tab = t.tab;
        match key {
            Key::Esc => {
                if !t.query.is_empty() {
                    t.query.clear();
                    self.tarefas_rebuild(id);
                } else {
                    self.request_close(id);
                }
                return;
            }
            Key::Tab | Key::Right => t.tab = (t.tab + 1) % 5,
            Key::Left => t.tab = (t.tab + 4) % 5,
            Key::Char(c @ b'1'..=b'5') => t.tab = c - b'1',
            Key::Char(b'/') | Key::Char(b'f') if tab == TAB_PROCESSES => t.search_focus = true,
            Key::Up | Key::Down if tab == TAB_PROCESSES => {
                let n = t.rows.len();
                if n == 0 {
                    return;
                }
                let cur = t.sel_index();
                let next = match (key, cur) {
                    (Key::Up, Some(i)) => i.saturating_sub(1),
                    (Key::Up, None) => n - 1,
                    (_, Some(i)) => (i + 1).min(n - 1),
                    (_, None) => 0,
                };
                t.sel = t.rows.get(next).map(|r| r.id);
                // Keep the selection in view.
                let top = t.scroll.value();
                let (y0, y1) = (next as i32 * ROW_H, (next as i32 + 1) * ROW_H);
                if y0 < top {
                    t.scroll.set(y0);
                } else if y1 > top + visible as i32 * ROW_H {
                    t.scroll.set(y1 - visible as i32 * ROW_H);
                }
                t.sb.touch(super::toasts_ui::now_ms());
                return;
            }
            Key::Delete | Key::Backspace if tab == TAB_PROCESSES => {
                self.tarefas_end(id, false);
                return;
            }
            Key::Enter if tab == TAB_PROCESSES => {
                // Bring the selected app to the front.
                if let Some(pid) = t.sel_index().and_then(|i| t.rows.get(i)).map(|r| r.pid)
                    && let Some(w) = self.window_of_pid(pid)
                {
                    self.wm.activate(w);
                }
                return;
            }
            Key::Char(b'r' | b'R') if tab == TAB_PROCESSES => {
                self.tarefas_restart(id);
                return;
            }
            _ => return,
        }
        self.tarefas_rebuild(id);
    }

    pub(crate) fn tarefas_wheel(&mut self, id: WindowId, notches: i32) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let l = lay(rect);
        let Some(t) = self.tarefas_mut(id) else {
            return;
        };
        if t.tab != TAB_PROCESSES {
            return;
        }
        let max = (t.rows.len() as i32 * ROW_H - l.list.h).max(0);
        let target = (t.scroll_target() + notches * ROW_H * 2).clamp(0, max);
        t.scroll.set(target);
        t.sb.touch(super::toasts_ui::now_ms());
    }

    pub(crate) fn tarefas_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        let l = lay(rect);
        let Some(t) = self.tarefas_mut(id) else {
            return;
        };
        if t.confirm.is_some() {
            if l.sheet_ok.contains(px, py) {
                self.tarefas_end(id, true);
            } else if l.sheet_cancel.contains(px, py) || !l.sheet.contains(px, py) {
                t.confirm = None;
            }
            return;
        }
        t.search_focus = false;
        if let Some(i) = osjeff_core::widgets::segmented_hit(l.tabs, 5, px, py) {
            if t.tab != i as u8 {
                t.tab = i as u8;
                self.tarefas_rebuild(id);
            }
            return;
        }
        if t.tab != TAB_PROCESSES {
            return;
        }
        if l.search.contains(px, py) {
            t.search_focus = true;
            return;
        }
        if l.btn_end.contains(px, py) {
            self.tarefas_end(id, false);
            return;
        }
        if l.btn_restart.contains(px, py) {
            self.tarefas_restart(id);
            return;
        }
        if l.head.contains(px, py) {
            if let Some(i) = columns(l.head).iter().position(|c| c.contains(px, py)) {
                let col = Column::ALL[i];
                if t.sort == col {
                    t.desc = !t.desc;
                } else {
                    t.sort = col;
                    t.desc = col.default_desc();
                }
                self.tarefas_rebuild(id);
            }
            return;
        }
        if l.list.contains(px, py) {
            let i = ((py - l.list.y + t.scroll.value()) / ROW_H) as usize;
            t.sel = t.rows.get(i).map(|r| r.id);
        }
    }

    // ------------------------------------------------------------ hover

    /// What the pointer is over in window `rect` (0 for nothing special).
    fn tarefas_hover_key(&self, rect: Rect, st: &TarefasState, cx: i32, cy: i32) -> u32 {
        let l = lay(rect);
        if st.confirm.is_some() {
            return if l.sheet_ok.contains(cx, cy) {
                H_OK
            } else if l.sheet_cancel.contains(cx, cy) {
                H_CANCEL
            } else {
                0
            };
        }
        if !rect.body().contains(cx, cy) {
            return 0;
        }
        if let Some(i) = osjeff_core::widgets::segmented_hit(l.tabs, 5, cx, cy) {
            return H_TAB + i as u32;
        }
        match st.tab {
            TAB_PROCESSES => {
                if l.search.contains(cx, cy) {
                    return H_SEARCH;
                }
                if l.btn_end.contains(cx, cy) {
                    return H_END;
                }
                if l.btn_restart.contains(cx, cy) {
                    return H_RESTART;
                }
                if l.head.contains(cx, cy) {
                    return columns(l.head)
                        .iter()
                        .position(|c| c.contains(cx, cy))
                        .map_or(0, |i| H_HEAD + i as u32);
                }
                if l.list.contains(cx, cy) {
                    let i = (cy - l.list.y + st.scroll.value()) / ROW_H;
                    if (i as usize) < st.rows.len() {
                        return H_ROW + i as u32;
                    }
                }
                0
            }
            0..=3 => {
                let sp = tab_split(l.content, st.tab);
                let plot = kit::plot_of(sp.chart);
                let n = self.sysmon.cpu_total.len();
                sample_under(cx, plot.x, plot.w, HIST, n)
                    .filter(|_| cy >= plot.y - 8 && cy < plot.bottom() + 8)
                    .map_or(0, |i| H_SAMPLE + i as u32)
            }
            _ => 0,
        }
    }

    /// Update the hover key of every Tarefas window for a pointer at `(cx, cy)` with the
    /// primary button `down`. Returns whether a window needs repainting.
    pub(crate) fn tarefas_hover(&mut self, cx: i32, cy: i32, down: bool) -> bool {
        let top = if self.overlay_open() {
            None
        } else {
            self.topmost_at(cx, cy)
        };
        let mut changed = false;
        let mut dirty = Vec::new();
        for w in self.wm.windows() {
            let App::Tarefas(st) = &w.app.app else {
                continue;
            };
            if !w.shown() {
                continue;
            }
            let mut key = if Some(w.id) == top {
                self.tarefas_hover_key(w.rect, st, cx, cy)
            } else {
                0
            };
            if key != 0 && down {
                key |= H_DOWN;
            }
            if key != st.hover.get() {
                st.hover.set(key);
                changed = true;
                dirty.push(self.window_box(w));
            }
        }
        for r in dirty {
            self.mark_dirty(r);
        }
        changed
    }

    /// Advance the animations of the Tarefas windows (called by `Desktop::live_step`).
    pub(crate) fn tarefas_step(&mut self, dt_ms: u32) {
        if !self
            .wm
            .windows()
            .iter()
            .any(|w| matches!(w.app.app, App::Tarefas(_)))
        {
            return;
        }
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Tarefas(_)))
            .map(|w| w.id)
            .collect();
        for id in ids {
            if let Some(t) = self.tarefas_mut(id) {
                t.scroll.step(dt_ms, 90);
            }
        }
    }

    /// What window `w` repaints while it animates: the heading and chart of the graph tabs
    /// (the curve scrolls, the headline number glides), the table of Processos (scrolling).
    /// Everything else waits for the settle frame at the end of the glide.
    pub(crate) fn tarefas_live_rect(&self, w: &Win) -> Rect {
        let App::Tarefas(t) = &w.app.app else {
            return w.rect;
        };
        let l = lay(w.rect);
        if t.tab == TAB_PROCESSES {
            l.head.union(&l.list)
        } else {
            let sp = tab_split(l.content, t.tab);
            sp.title.union(&sp.chart)
        }
    }

    pub(crate) fn tarefas_busy_one(&self, w: &Win) -> bool {
        let App::Tarefas(t) = &w.app.app else {
            return false;
        };
        w.shown()
            && ((matches!(t.tab, 0..=3) && self.sysmon.gliding())
                || t.scroll.moving()
                || t.sb.active(super::toasts_ui::now_ms()))
    }

    // ------------------------------------------------------------ drawing

    pub(crate) fn draw_tarefas(&self, c: &mut Canvas, r: Rect, st: &TarefasState) {
        let l = lay(r);
        let hv = st.hover.get();
        // The tab bar.
        ui::segmented(c, l.tabs, &TAB_NAMES, st.tab as usize);
        match st.tab {
            0 => self.tf_cpu(c, &l, st),
            1 => self.tf_memory(c, &l, st),
            2 => self.tf_disk(c, &l, st),
            3 => self.tf_network(c, &l, st),
            _ => self.tf_processes(c, &l, st, hv),
        }
        if st.confirm.is_some() {
            self.tf_confirm(c, r, &l, st, hv);
        }
    }

    /// The hovered sample index on a chart tab, if any.
    fn hovered_sample(st: &TarefasState) -> Option<usize> {
        let hv = st.hover.get() & !H_DOWN;
        (hv >= H_SAMPLE).then(|| (hv - H_SAMPLE) as usize)
    }

    /// A section title row: the title on the left, `value` on the right in the accent.
    fn section(&self, c: &mut Canvas, r: Rect, title: &str, value: &str) {
        text::draw_left(c, r, title, CALLOUT, Weight::Semibold, kit::ink());
        if !value.is_empty() {
            text::draw_right(
                c,
                Rect::new(r.x, r.y, r.w, r.h),
                value,
                TITLE3,
                Weight::Semibold,
                theme::accent(),
            );
        }
    }

    /// A small card with a label and a value, for the right-hand column.
    fn stat_card(&self, c: &mut Canvas, r: Rect, name: &str, value: &str, sub: &str) {
        kit::card(c, r);
        kit::label(c, r.x + 16, r.y + 12, r.w - 32, name);
        text::draw_ellipsis(
            c,
            r.x + 16,
            r.y + 28,
            r.w - 32,
            value,
            TITLE2,
            Weight::Semibold,
            kit::ink(),
        );
        if !sub.is_empty() && r.h >= 84 {
            text::draw_ellipsis(
                c,
                r.x + 16,
                r.bottom() - 24,
                r.w - 32,
                sub,
                FOOTNOTE,
                Weight::Regular,
                kit::ink2(),
            );
        }
    }

    fn tf_cpu(&self, c: &mut Canvas, l: &Lay, st: &TarefasState) {
        let mon = &self.sysmon;
        let sp = tab_split(l.content, 0);
        let t = mon.t_q8();
        let now_pm = kit::lerp(mon.prev.busy_pm as i64, mon.last.busy_pm as i64, t) as u32;
        let big = activity::fmt_pct(now_pm);
        self.section(c, sp.title, "Uso do processador", kit::fb_str(&big));

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
            "Tempo ligado",
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
            "Carga média",
            kit::fb_str(&ld),
            "1 min   5 min   15 min",
        );
        y += ch + gap;
        let mut th = FixedBuf::<24>::new();
        let _ = write!(th, "{}", sched::thread_count());
        let mut sub = FixedBuf::<40>::new();
        let _ = write!(sub, "{} processos", self.procs.len().saturating_sub(1));
        self.stat_card(
            c,
            Rect::new(sp.side.x, y, sp.side.w, ch),
            "Threads",
            kit::fb_str(&th),
            kit::fb_str(&sub),
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
            kit::label(c, r.x + 16, r.y + 10, r.w - 32, "Processador");
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
                let vm = if si.hypervisor.as_bytes().is_empty() {
                    ""
                } else {
                    "Virtualizado · "
                };
                let sub = alloc::format!("{vm}{feats} recursos");
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
            "Por processo",
            CALLOUT,
            Weight::Semibold,
            kit::ink(),
        );
        let t = self.sysmon.t_q8();
        let mut y = area.y + 28;
        let fit = ((area.bottom() - y) / 28).max(0) as usize;
        let mut shown = 0;
        for r in st.rows.iter().filter(|r| r.raw != "(ocioso)") {
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

    fn tf_memory(&self, c: &mut Canvas, l: &Lay, st: &TarefasState) {
        let mon = &self.sysmon;
        let sp = tab_split(l.content, 0);
        let t = mon.t_q8();
        let used = mon.heap_now();
        let big = activity::fmt_size(used);
        self.section(c, sp.title, "Memória em uso", kit::fb_str(&big));

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
            "Pressão da memória",
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
            "do espaço do sistema",
            FOOTNOTE,
            Weight::Regular,
            kit::ink3(),
        );

        // Numbers.
        let nums = Rect::new(sp.side.x, card.bottom() + 12, sp.side.w, 5 * 28 + 20);
        kit::card(c, nums);
        let free = total.saturating_sub(used);
        let rows: [(&str, FixedBuf<24>); 5] = [
            ("Em uso", activity::fmt_size(used).into_fb()),
            ("Livre", activity::fmt_size(free).into_fb()),
            ("Total", activity::fmt_size(total).into_fb()),
            ("Pico (60 s)", activity::fmt_size(mon.heap_peak()).into_fb()),
            (
                "Memória física",
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
            "Por aplicativo",
            CALLOUT,
            Weight::Semibold,
            kit::ink(),
        );
        text::draw_right(
            c,
            Rect::new(area.x, area.y, area.w, 24),
            "valores aproximados",
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
                "Nenhum aplicativo aberto.",
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

    fn tf_disk(&self, c: &mut Canvas, l: &Lay, st: &TarefasState) {
        let mon = &self.sysmon;
        let sp = tab_split(l.content, 2);
        let t = mon.t_q8();
        // The volume card.
        let card = Rect::new(l.content.x, l.content.y, l.content.w, DISK_TOP);
        kit::card(c, card);
        let usage = VfsUsage;
        let u = vfs::statfs();
        let (title, sub) = match vfs::volume() {
            vfs::Volume::Disk => ("Disco principal", usage.label().trim_end_matches(" (IDE)")),
            vfs::Volume::Memory => ("Memória (sem disco)", "Os arquivos somem ao desligar"),
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
            ("Usado", activity::fmt_size(u.used()).into_fb()),
            ("Livre", activity::fmt_size(u.free).into_fb()),
            ("Total", activity::fmt_size(u.total).into_fb()),
            (
                "Arquivos e pastas",
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
        self.section(c, sp.title, "Leitura e gravação", "");
        let mut lx = sp.title.x + 170;
        for (name, col) in [("Leitura", theme::accent()), ("Gravação", kit::amber())] {
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
            let _ = write!(
                tip,
                "Leitura {} · Gravação {} · {}",
                activity::fmt_speed(rraw[i] as u64),
                activity::fmt_speed(wraw[i] as u64),
                activity::fmt_ago((n - 1 - i) as u32)
            );
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
        let sub_a = alloc::format!("{} desde a inicialização", activity::fmt_size(rd_total));
        self.stat_card(
            c,
            Rect::new(sp.side.x, sp.side.y, sp.side.w, ch),
            "Leitura",
            kit::fb_str(&a),
            &sub_a,
        );
        let b = activity::fmt_speed(rate_wr);
        let sub_b = alloc::format!("{} desde a inicialização", activity::fmt_size(wr_total));
        self.stat_card(
            c,
            Rect::new(sp.side.x, sp.side.y + ch + 12, sp.side.w, ch),
            "Gravação",
            kit::fb_str(&b),
            &sub_b,
        );
    }

    fn tf_network(&self, c: &mut Canvas, l: &Lay, st: &TarefasState) {
        use osjeff_core::netstats::NicKind;
        let mon = &self.sysmon;
        let sp = tab_split(l.content, 3);
        let t = mon.t_q8();
        let snap = crate::netd::stats();
        let has_nic = snap.nic != NicKind::None;
        let card = Rect::new(l.content.x, l.content.y, l.content.w, NET_TOP);
        kit::card(c, card);
        // Status line.
        let (word, col) = if !has_nic {
            ("Sem placa de rede", kit::ink3())
        } else if !snap.link_up {
            ("Sem sinal", kit::red())
        } else if snap.config.is_none() {
            ("Procurando endereço", kit::amber())
        } else {
            ("Conectado", kit::green())
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
                if cfg == osjeff_core::net::NetConfig::STATIC_FALLBACK {
                    let _ = write!(lease, "Estático");
                } else if let Some(ms) = snap.lease_remaining_ms {
                    let _ = write!(lease, "{} restantes", activity::fmt_elapsed(ms / 1000));
                } else {
                    let _ = write!(lease, "Sem expiração");
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
        let left = [("Endereço IP", &ip), ("Máscara", &mask)];
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
            "Roteador",
            kit::fb_str(&gw),
        );
        kit::kv(
            c,
            Rect::new(card.x + 20 + colw + 24, card.y + 52, colw, 26),
            "DNS",
            kit::fb_str(&dns),
        );
        kit::kv(
            c,
            Rect::new(card.x + 20 + colw + 24, card.y + 52 + 26, colw, 26),
            "Concessão",
            kit::fb_str(&lease),
        );
        let _ = st;

        // Throughput chart.
        let rx_now = kit::lerp(mon.rx_prev as i64, mon.rx_rate as i64, t) as u64;
        let tx_now = kit::lerp(mon.tx_prev as i64, mon.tx_rate as i64, t) as u64;
        self.section(c, sp.title, "Tráfego", "");
        let mut lx = sp.title.x + 80;
        for (name, col) in [("Recebido", theme::accent()), ("Enviado", kit::amber())] {
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
            let _ = write!(
                tip,
                "Recebido {} · Enviado {} · {}",
                activity::fmt_speed(rraw[i] as u64),
                activity::fmt_speed(traw[i] as u64),
                activity::fmt_ago((n - 1 - i) as u32)
            );
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
        let sub_a = alloc::format!(
            "{} · {} pacotes",
            activity::fmt_size(snap.rx_bytes),
            activity::fmt_count(snap.rx_packets)
        );
        self.stat_card(
            c,
            Rect::new(sp.side.x, sp.side.y, sp.side.w, ch),
            "Recebido",
            kit::fb_str(&a),
            &sub_a,
        );
        let b = activity::fmt_speed(tx_now);
        let sub_b = alloc::format!(
            "{} · {} pacotes",
            activity::fmt_size(snap.tx_bytes),
            activity::fmt_count(snap.tx_packets)
        );
        self.stat_card(
            c,
            Rect::new(sp.side.x, sp.side.y + ch + 12, sp.side.w, ch),
            "Enviado",
            kit::fb_str(&b),
            &sub_b,
        );
        let errs = snap.rx_errors + snap.tx_errors + snap.rx_dropped + snap.tx_dropped;
        if errs > 0 && sp.side.h > 2 * ch + 12 + 12 + 60 {
            let e = alloc::format!("{}", activity::fmt_count(errs));
            self.stat_card(
                c,
                Rect::new(sp.side.x, sp.side.y + 2 * (ch + 12), sp.side.w, 72),
                "Erros e descartes",
                &e,
                "",
            );
        }
    }

    fn tf_processes(&self, c: &mut Canvas, l: &Lay, st: &TarefasState, hv: u32) {
        let p = theme::pal();
        let hv_key = hv & !H_DOWN;
        let down = hv & H_DOWN != 0;
        // The search field.
        if l.search.w >= 80 {
            kit::search_field(
                c,
                l.search,
                &st.query,
                "Buscar",
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
                "Nada para mostrar."
            } else {
                "Nenhum processo corresponde à busca."
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
            st.sb.alpha(super::toasts_ui::now_ms()),
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
        let summary = alloc::format!(
            "{} processos · {} threads · CPU {} · Memória {} · Disco {}",
            totals.processes,
            totals.threads,
            kit::fb_str(&cpu),
            kit::fb_str(&mem_pct),
            kit::fb_str(&disk_pct)
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
            (None, Some(r)) => alloc::format!(
                "Nome interno: {} · {}",
                if r.raw.is_empty() { "—" } else { &r.raw },
                match r.kind {
                    TaskKind::App => "aplicativo",
                    TaskKind::Thread => "serviço do sistema",
                    TaskKind::System => "sistema",
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
            "Reiniciar",
            ButtonKind::Secondary,
            kit::control_state(hv_key == H_RESTART, down, can_restart),
        );
        ui::push_button(
            c,
            l.btn_end,
            "Encerrar",
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
    fn tf_confirm(&self, c: &mut Canvas, r: Rect, l: &Lay, st: &TarefasState, hv: u32) {
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
        let title = alloc::format!("Encerrar “{name}”?");
        text::draw_left(
            c,
            Rect::new(s.x + 20, s.y + 18, s.w - 40, 24),
            &title,
            TITLE3,
            Weight::Semibold,
            kit::ink(),
        );
        let msg = match row.map(|r| r.raw.as_str()) {
            Some("appd") => "Todos os aplicativos instalados serão fechados.",
            Some(_) => "Os comandos em execução nos terminais serão interrompidos.",
            None => "",
        };
        let body_txt = alloc::format!("É um serviço do sistema. {msg}");
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
            "Cancelar",
            ButtonKind::Secondary,
            kit::control_state(hv_key == H_CANCEL, down, true),
        );
        ui::push_button(
            c,
            l.sheet_ok,
            "Encerrar",
            ButtonKind::Destructive,
            kit::control_state(hv_key == H_OK, down, true),
        );
    }
}

impl TarefasState {
    fn scroll_target(&self) -> i32 {
        self.scroll.target()
    }
}

/// `fmt_*` buffers of different sizes, converted to one size for tables of values.
trait IntoFb {
    fn into_fb(self) -> FixedBuf<24>;
}

impl<const N: usize> IntoFb for FixedBuf<N> {
    fn into_fb(self) -> FixedBuf<24> {
        let mut b = FixedBuf::<24>::new();
        let _ = b.write_str(core::str::from_utf8(self.as_bytes()).unwrap_or(""));
        b
    }
}
