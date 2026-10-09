//! The process rows and the once-a-second refresh of Tarefas windows.

use super::state::TAB_PROCESSES;
use crate::desktop::*;
use kitsune_core::activity::{Column, TaskKind, TaskRow, TaskState, fold, sort_tasks};
use kitsune_core::sysmon::MAX_THREADS;

impl App {
    /// Rough heap held by the app instance, for the memory tab (`None` when the kernel
    /// does not track it).
    pub(crate) fn approx_bytes(&self) -> Option<usize> {
        use core::mem::size_of;
        let n = match self {
            App::Terminal(_) => size_of::<TermState>(),
            App::Editor(_) => size_of::<EditorState>(),
            App::Calculator(_) => size_of::<apps::calculadora::logic::CalcState>(),
            App::Browser(b) => {
                size_of::<BrowserState>()
                    + b.images.bytes()
                    + b.page
                        .as_ref()
                        .map_or(0, |p| p.cmds.len() * size_of::<kitsune_core::web::Cmd>())
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

    pub(super) fn tarefas_mut(&mut self, id: WindowId) -> Option<&mut TarefasState> {
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

    /// The language changed: the rows hold friendly names and the footer message holds text, so
    /// build the rows again and drop the message.
    pub(crate) fn tarefas_language_changed(&mut self) {
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Tarefas(_)))
            .map(|w| w.id)
            .collect();
        for id in ids {
            if let Some(t) = self.tarefas_mut(id) {
                t.msg = None;
            }
            self.tarefas_rebuild(id);
        }
    }

    pub(super) fn tarefas_rebuild(&mut self, id: WindowId) {
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
        let mut idle = TaskRow::new(0x2_0000, b"(idle)", TaskKind::System, 0, TaskState::Waiting);
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
}
