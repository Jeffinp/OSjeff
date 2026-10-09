//! Hand-off of command lines to the `shelld` thread and pickup of the results.

use crate::desktop::services::shellhost::{self, Ctx, Job, Snap, UiReq};
use crate::desktop::*;
use kitsune_core::shell::TermAction;
use kitsune_core::shell::sys::{MemInfo, ProcInfo};
use kitsune_core::t;

impl Desktop {
    /// Hand `line` to the command thread.
    pub(super) fn term_run(&mut self, id: WindowId, line: String) {
        let snap = self.make_snap();
        let dead = shellhost::worker_dead();
        let Some(ts) = self.term_state_mut(id) else {
            return;
        };
        let uid = ts.uid;
        let problem = match ts.ctx.take() {
            Some(ctx) if !dead => match shellhost::post(Job {
                uid,
                ctx,
                line,
                snap,
            }) {
                Ok(()) => return,
                Err(job) => {
                    ts.ctx = Some(job.ctx);
                    t!("term.err.queue_full")
                }
            },
            Some(ctx) => {
                ts.ctx = Some(ctx);
                t!("term.err.thread_stopped")
            }
            None => t!("term.err.no_shell"),
        };
        let ctx = ts.ctx.get_or_insert_with(Ctx::new);
        let prompt = shellhost::prompt_of(ctx);
        let res = kitsune_core::shell::RunResult {
            status: 1,
            output: alloc::format!("sh: {problem}\n").into_bytes(),
            ..Default::default()
        };
        ts.term.finish(&res, &prompt);
    }

    /// What `ps`, `kill` and `free` see, copied from the compositor's tables.
    fn make_snap(&self) -> Snap {
        let mut procs = Vec::new();
        for i in 0..self.procs.len() {
            let Some(p) = self.procs.at(i) else { continue };
            let mem = self
                .window_of_pid(p.pid)
                .and_then(|w| self.wm.get(w))
                .and_then(|w| w.app.app.approx_bytes())
                .unwrap_or(0) as u64;
            procs.push((
                ProcInfo {
                    pid: u32::from(p.pid),
                    name: String::from_utf8_lossy(p.name()).into_owned(),
                    state: String::from(match p.state {
                        ProcState::Running => "run",
                        ProcState::Suspended => "susp",
                        ProcState::Terminated => "end",
                    }),
                    mem_bytes: mem,
                },
                p.kind == ProcKind::App,
            ));
        }
        // The kernel threads, after the processes (not killable).
        for i in 0..sched::thread_count() {
            procs.push((
                ProcInfo {
                    pid: 1000 + i as u32,
                    name: alloc::format!("[{}]", sched::thread_name(i)),
                    state: String::from(if sched::thread_dead(i) { "dead" } else { "run" }),
                    mem_bytes: u64::from(sched::thread_stack_kib(i)) * 1024,
                },
                false,
            ));
        }
        Snap {
            procs,
            mem: MemInfo {
                total: self.sysmon.heap_total as u64,
                used: self.sysmon.heap_used as u64,
            },
            disks: self.disks,
        }
    }

    /// Collect finished command lines and the requests commands left for the
    /// desktop. Runs every tick from `animate`.
    pub(crate) fn step_shell_jobs(&mut self) {
        for d in shellhost::take_done() {
            // Closing a terminal mid-command just drops its shell here.
            let id = self
                .wm
                .windows()
                .iter()
                .find(|w| matches!(&w.app.app, App::Terminal(t) if t.uid == d.uid))
                .map(|w| w.id);
            for &(pid, _) in &d.kills {
                if let Ok(pid) = u16::try_from(pid)
                    && let Some(w) = self.window_of_pid(pid)
                {
                    self.request_close(w);
                }
            }
            let Some(id) = id else { continue };
            let Some(ts) = self.term_state_mut(id) else {
                continue;
            };
            let prompt = if d.prompt.is_empty() {
                shellhost::prompt_of(&d.ctx)
            } else {
                d.prompt
            };
            ts.ctx = Some(d.ctx);
            ts.sel = None;
            if ts.term.finish(&d.result, &prompt) == TermAction::Exit {
                self.request_close(id);
            }
            // The command may have changed files.
            self.fs_changed();
        }
        for req in shellhost::take_ui() {
            match req {
                UiReq::Edit(path) => {
                    self.open_editor_for(path);
                }
                UiReq::Files => {
                    self.launch(Kind::Files);
                }
                UiReq::Tasks => {
                    self.launch(Kind::TaskMgr);
                }
                UiReq::Calc => {
                    self.launch(Kind::Calculator);
                }
                UiReq::Reboot => {
                    if !self.guard_unsaved() {
                        crate::power::reboot();
                    }
                }
                UiReq::Shutdown => {
                    if !self.guard_unsaved() {
                        crate::power::shutdown();
                    }
                }
            }
        }
        // The command thread died with command lines in flight: free those terminals.
        if shellhost::worker_dead() {
            let ids: Vec<WindowId> = self
                .wm
                .windows()
                .iter()
                .filter(|w| matches!(&w.app.app, App::Terminal(t) if t.term.is_running()))
                .map(|w| w.id)
                .collect();
            for id in ids {
                if let Some(ts) = self.term_state_mut(id) {
                    let ctx = ts.ctx.get_or_insert_with(Ctx::new);
                    let prompt = shellhost::prompt_of(ctx);
                    let res = kitsune_core::shell::RunResult {
                        status: 1,
                        output: alloc::format!("sh: {}\n", t!("term.err.thread_stopped"))
                            .into_bytes(),
                        ..Default::default()
                    };
                    ts.term.finish(&res, &prompt);
                }
            }
        }
    }
}
