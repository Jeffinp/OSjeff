//! Ending and restarting a task.

use crate::desktop::*;
use kitsune_core::activity::{TaskKind, TaskRow};
use kitsune_core::{t, tp};

impl Desktop {
    // ------------------------------------------------------------ actions

    /// What "Encerrar" does for `row` (`None`: nothing the user may end).
    pub(super) fn can_end(&self, row: &TaskRow) -> bool {
        match row.kind {
            TaskKind::App => self.window_of_pid(row.pid).is_some(),
            TaskKind::Thread => matches!(row.raw.as_str(), "appd" | "shelld" | "shelld2"),
            TaskKind::System => false,
        }
    }

    pub(super) fn can_restart(&self, row: &TaskRow) -> bool {
        row.kind == TaskKind::App && self.window_of_pid(row.pid).is_some()
    }

    /// Does ending `row` need the user's confirmation first?
    pub(super) fn needs_confirm(row: &TaskRow) -> bool {
        row.kind != TaskKind::App
    }

    /// End the selected row of window `id`. System rows ask first.
    pub(super) fn tarefas_end(&mut self, id: WindowId, confirmed: bool) {
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
                t!("tasks.msg.closed", name = row.name.as_str())
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
                    tp!("tasks.msg.apps_closed", n)
                }
                _ => {
                    // The command line workers: interrupt whatever the terminals run.
                    let mut n = 0;
                    for w in self.wm.windows() {
                        if let App::Terminal(t) = &w.app.app
                            && t.term.is_running()
                        {
                            services::shellhost::cancel(t.uid);
                            n += 1;
                        }
                    }
                    tp!("tasks.msg.cmds_stopped", n)
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
    pub(super) fn tarefas_restart(&mut self, id: WindowId) {
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
            t.msg = Some((t!("tasks.msg.restarted", name = row.name.as_str()), false));
        }
    }
}
