//! The terminal window: `Desktop` methods around [`TermState`].
//!
//! The behaviour (line editing, history, Tab, Ctrl+C / Ctrl+L, scrollback) is
//! `osjeff_core::shell::Term`, tested on the host. This file connects it to the
//! window: the character grid that fits the window, keys in, pixels out, and the
//! hand-off of command lines to the `shelld` thread
//! ([`shellhost`](super::shellhost)). A command that waits (`sleep`, `ping`,
//! `curl`) never blocks the compositor: the terminal shows "executando" and the
//! result is picked up by [`Desktop::step_shell_jobs`] on a later frame.

use super::shellhost::{self, Ctx, Job, Snap, UiReq};
use super::*;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicU32, Ordering};
use osjeff_core::input::{KeyEvent, Mods};
use osjeff_core::shell::sys::{MemInfo, ProcInfo};
use osjeff_core::shell::{ShellFs, Term, TermAction};

/// Side padding of the text area and the gap under the title bar.
const PAD: i32 = 12;
const TOP: i32 = TITLE_H + 8;
/// Glyph cell at text scale 2.
const CELL_W: i32 = 12;
const LINE_H: i32 = 18;
/// Room kept at the right edge for the scroll indicator.
const BAR_W: i32 = 8;

static NEXT_UID: AtomicU32 = AtomicU32::new(1);

/// A terminal window: the interactive state and the shell it talks to.
pub(crate) struct TermState {
    /// Identity the worker thread knows this terminal by.
    pub uid: u32,
    pub term: Term,
    /// The shell and its working directory; `None` while a command line runs
    /// (the worker owns them then).
    pub ctx: Option<Box<Ctx>>,
}

impl TermState {
    pub(crate) fn new() -> Self {
        let ctx = Ctx::new();
        let mut term = Term::new(&shellhost::prompt_of(&ctx));
        term.print("OSjeff shell: digite help para ver os comandos.");
        term.print("Tab completa, Ctrl+C interrompe, PageUp rola.");
        Self {
            uid: NEXT_UID.fetch_add(1, Ordering::Relaxed),
            term,
            ctx: Some(ctx),
        }
    }

    /// Text on the live line (what Ctrl+Shift+C copies).
    pub(crate) fn input(&self) -> String {
        self.term.input()
    }
}

/// The byte the bitmap font draws for `c` (Latin-1; anything else is `?`).
pub(crate) fn latin1(c: char) -> u8 {
    u8::try_from(u32::from(c)).unwrap_or(b'?')
}

/// Characters and rows that fit a terminal window of rectangle `r`.
pub(crate) fn term_grid(r: Rect) -> (usize, usize) {
    let cols = ((r.w - 2 * PAD - BAR_W) / CELL_W).max(1) as usize;
    let rows = ((r.h - TOP - 8) / LINE_H).max(1) as usize;
    (cols, rows)
}

impl Desktop {
    pub(crate) fn term_state_mut(&mut self, id: WindowId) -> Option<&mut TermState> {
        match self.app_mut(id) {
            Some(App::Terminal(t)) => Some(t),
            _ => None,
        }
    }

    /// The modifier state of the keyboard right now.
    pub(crate) fn mods(&self) -> Mods {
        Mods {
            ctrl: self.keymap.ctrl(),
            shift: self.keymap.shift(),
            alt: self.keymap.alt(),
        }
    }

    pub(crate) fn term_key(&mut self, id: WindowId, key: Key) -> bool {
        let ev = KeyEvent::from_key(key, self.mods());
        self.term_event(id, ev)
    }

    /// Give a key to terminal `id`. `true` when the window needs a repaint.
    pub(crate) fn term_event(&mut self, id: WindowId, ev: KeyEvent) -> bool {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return false;
        };
        let (cols, rows) = term_grid(rect);
        let Some(ts) = self.term_state_mut(id) else {
            return false;
        };
        ts.term.resize(cols, rows);
        // The shell died with its command line (the worker thread was killed): start over.
        if ts.ctx.is_none() && !ts.term.is_running() {
            ts.ctx = Some(Ctx::new());
        }
        let uid = ts.uid;
        let action = {
            let ctx = ts.ctx.as_deref();
            ts.term
                .key(ev, ctx.map(|c| (&c.shell, &c.fs as &dyn ShellFs)))
        };
        match action {
            TermAction::None => false,
            TermAction::Redraw => true,
            TermAction::Cancel => {
                shellhost::cancel(uid);
                true
            }
            TermAction::Exit => {
                self.request_close(id);
                true
            }
            TermAction::Run(line) => {
                self.term_run(id, line);
                true
            }
        }
    }

    /// Hand `line` to the command thread.
    fn term_run(&mut self, id: WindowId, line: String) {
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
                    "sh: too many commands waiting"
                }
            },
            Some(ctx) => {
                ts.ctx = Some(ctx);
                "sh: the command thread stopped"
            }
            None => "sh: no shell",
        };
        let ctx = ts.ctx.get_or_insert_with(Ctx::new);
        let prompt = shellhost::prompt_of(ctx);
        let res = osjeff_core::shell::RunResult {
            status: 1,
            output: alloc::format!("{problem}\n").into_bytes(),
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
                    let res = osjeff_core::shell::RunResult {
                        status: 1,
                        output: b"sh: the command thread stopped\n".to_vec(),
                        ..Default::default()
                    };
                    ts.term.finish(&res, &prompt);
                }
            }
        }
    }

    /// Wheel over a terminal: scroll the history (`notches` > 0 = older).
    pub(crate) fn term_wheel(&mut self, id: WindowId, notches: i32) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let (cols, rows) = term_grid(rect);
        if let Some(ts) = self.term_state_mut(id) {
            ts.term.resize(cols, rows);
            ts.term.scroll_rows(notches as isize * 3);
        }
    }

    /// Paste text into terminal `id` (never runs a command by itself).
    pub(crate) fn term_paste(&mut self, id: WindowId, text: &[u8]) {
        let s = String::from_utf8_lossy(text).into_owned();
        if let Some(ts) = self.term_state_mut(id) {
            ts.term.paste(&s);
        }
    }

    // ---- drawing ----

    /// The terminal: as many columns and rows as the window holds (text scale 2),
    /// the newest output at the bottom, the prompt in the accent colour, a thin
    /// scroll indicator while looking at older output.
    pub(crate) fn draw_terminal(&self, c: &mut Canvas, r: Rect, t: &TermState, focused: bool) {
        let (cols, rows) = term_grid(r);
        let v = t.term.view_in(cols, rows);
        let tx = r.x.max(0) as usize + PAD as usize;
        let ty0 = r.y.max(0) as usize + TOP as usize;
        let plen = t.term.prompt_chars();
        for (i, row) in v.rows.iter().enumerate() {
            let y = ty0 + i * LINE_H as usize;
            let bytes: Vec<u8> = row.chars().map(latin1).collect();
            // The prompt part of the live line is drawn in the accent colour.
            let split = match v.live_first {
                Some(lf) if i >= lf => plen.saturating_sub((i - lf) * cols).min(bytes.len()),
                _ => 0,
            };
            if split > 0 {
                font::draw_bytes(c, tx, y, &bytes[..split], theme::TERM_PROMPT, 2);
            }
            font::draw_bytes(
                c,
                tx + split * CELL_W as usize,
                y,
                &bytes[split..],
                theme::TEXT,
                2,
            );
        }
        if focused && let Some((row, col)) = v.cursor {
            c.fill_rect(
                tx + col * CELL_W as usize,
                ty0 + row * LINE_H as usize,
                3,
                14,
                theme::accent(),
            );
        }
        // Scroll indicator.
        if v.above > 0 || v.below > 0 {
            let total = v.above + v.rows.len() + v.below;
            let track_h = rows as i32 * LINE_H;
            let track = Rect::new(r.x + r.w - PAD / 2 - 4, r.y + TOP, 4, track_h);
            ui::fill_round(c, track, 2, ui::BORDER);
            let thumb_h = ((track_h as i64 * v.rows.len() as i64) / total as i64).max(14) as i32;
            let thumb_y = track.y
                + (((track_h - thumb_h) as i64 * v.above as i64)
                    / (total - v.rows.len()).max(1) as i64) as i32;
            ui::fill_round(
                c,
                Rect::new(track.x, thumb_y, 4, thumb_h.min(track_h)),
                2,
                theme::accent(),
            );
        }
        if t.term.is_running() {
            let msg = "executando... Ctrl+C cancela";
            let w = font::text_width(msg, 1) as i32;
            font::draw_text(
                c,
                (r.x + r.w - PAD - w).max(0) as usize,
                (r.y + r.h - 14).max(0) as usize,
                msg,
                theme::TEXT_MUTED,
                1,
            );
        }
    }
}
