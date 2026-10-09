//! The terminal window: `Desktop` methods around [`TermState`].
//!
//! The behaviour (line editing, history, Tab, Ctrl+C / Ctrl+L, scrollback) is
//! `osjeff_core::shell::Term`, tested on the host. This file connects it to the
//! window: the character grid that fits the window, keys in, pixels out, and the
//! hand-off of command lines to the `shelld` thread
//! ([`shellhost`](super::shellhost)). A command that waits (`sleep`, `ping`,
//! `curl`) never blocks the compositor: the terminal shows "executando" and the
//! result is picked up by [`Desktop::step_shell_jobs`] on a later frame.

use super::appui;
use super::shellhost::{self, Ctx, Job, Snap, UiReq};
use super::sysstore::VfsStore;
use super::*;
use crate::text::{self, BODY, FOOTNOTE, Weight};
use alloc::boxed::Box;
use core::sync::atomic::{AtomicU32, Ordering};
use osjeff_core::appart::Tool;
use osjeff_core::input::{KeyCode, KeyEvent, Mods};
use osjeff_core::settings::{FONT_MAX, FONT_MIN, font_step};
use osjeff_core::shell::sys::{MemInfo, ProcInfo};
use osjeff_core::shell::{ShellFs, Term, TermAction};
use osjeff_core::sysif::SettingsStore;
use osjeff_core::termui::{self, Grid, Metrics, Selection};
use osjeff_core::widgets::ScrollbarFade;

static NEXT_UID: AtomicU32 = AtomicU32::new(1);

/// A terminal window: the interactive state and the shell it talks to.
pub(crate) struct TermState {
    /// Identity the worker thread knows this terminal by.
    pub uid: u32,
    pub term: Term,
    /// The shell and its working directory; `None` while a command line runs
    /// (the worker owns them then).
    pub ctx: Option<Box<Ctx>>,
    /// The text selected with the mouse, as cells of the visible rows. It lives until the next
    /// key or output moves the rows.
    pub sel: Option<Selection>,
    /// Tick of the last key or click: the caret holds still, then blinks (and rests again).
    pub last_input: u64,
    pub scroll_fade: ScrollbarFade,
    /// The window's grid as of the last input (what a copy reads the rows with).
    grid: (usize, usize),
    /// Last press: tick, cell and how many in a row (double: word, triple: line).
    last_click: (u64, (usize, usize), u8),
    press_at: (i32, i32),
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
            sel: None,
            last_input: 0,
            scroll_fade: ScrollbarFade::new(),
            grid: (80, 24),
            last_click: (0, (0, 0), 0),
            press_at: (-1, -1),
        }
    }

    /// Text on the live line (what Ctrl+Shift+C copies when nothing is selected).
    pub(crate) fn input(&self) -> String {
        self.term.input()
    }

    /// The text selected with the mouse, if any.
    pub(crate) fn selection_text(&self) -> Option<String> {
        let sel = self.sel.filter(|s| !s.is_empty())?;
        let v = self.term.view_in(self.grid.0, self.grid.1);
        let t = termui::extract(&v.rows, &sel);
        (!t.is_empty()).then_some(t)
    }

    /// Whether the window needs frames beyond a running command: a blinking caret, a fading
    /// scrollbar.
    pub(crate) fn animating(&self, focused: bool) -> bool {
        (focused && appui::caret_animating(self.last_input))
            || self.scroll_fade.active(appui::now_ms())
    }

    fn click_count(&mut self, ticks: u64, cell: (usize, usize)) -> u8 {
        let (t0, c0, n0) = self.last_click;
        let n = if n0 > 0 && c0 == cell && ticks.saturating_sub(t0) <= 125 {
            (n0 % 3) + 1
        } else {
            1
        };
        self.last_click = (ticks, cell, n);
        n
    }
}

/// The text size in pixels (a setting shared by all terminals).
fn font_px() -> u16 {
    crate::settings::get()
        .terminal_font
        .clamp(FONT_MIN, FONT_MAX) as u16
}

/// The character cell: the face's pitch and a line a little taller than its natural height.
fn metrics() -> Metrics {
    let (cw, lh) = text::mono_cell_px(font_px());
    Metrics { cw, lh: lh + 2 }
}

/// The geometry of a terminal window of rectangle `r`.
pub(crate) fn term_grid(r: Rect) -> Grid {
    termui::layout(r, metrics())
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
        let g = term_grid(rect);
        let ctrl = ev.mods.ctrl && !ev.mods.alt;
        if ctrl && let KeyCode::Char(c) = ev.code {
            match c {
                '+' | '=' => {
                    self.term_zoom(1);
                    return true;
                }
                '-' | '_' => {
                    self.term_zoom(-1);
                    return true;
                }
                '0' => {
                    self.term_zoom(0);
                    return true;
                }
                _ => {}
            }
        }
        let Some(ts) = self.term_state_mut(id) else {
            return false;
        };
        ts.term.resize(g.cols, g.rows);
        ts.grid = (g.cols, g.rows);
        ts.sel = None;
        ts.last_input = appui::ticks();
        if matches!(
            ev.code,
            KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End
        ) {
            ts.scroll_fade.touch(appui::now_ms());
        }
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

    /// Ctrl +, Ctrl - and Ctrl 0: the text size of every terminal, kept in the settings file.
    fn term_zoom(&mut self, dir: i32) {
        let mut s = crate::settings::get();
        let n = font_step(s.terminal_font, dir);
        if n == s.terminal_font {
            return;
        }
        s.terminal_font = n;
        crate::settings::set(s);
        let _ = VfsStore.save(&s.to_text());
        // Every terminal redraws at the new size, with a grid to match.
        self.force_full = true;
        let ids: Vec<(WindowId, Rect)> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Terminal(_)))
            .map(|w| (w.id, w.rect))
            .collect();
        for (id, rect) in ids {
            let g = term_grid(rect);
            if let Some(ts) = self.term_state_mut(id) {
                ts.term.resize(g.cols, g.rows);
                ts.grid = (g.cols, g.rows);
                ts.sel = None;
                ts.last_input = appui::ticks();
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
        let g = term_grid(rect);
        if let Some(ts) = self.term_state_mut(id) {
            ts.term.resize(g.cols, g.rows);
            ts.grid = (g.cols, g.rows);
            ts.sel = None;
            ts.term.scroll_rows(notches as isize * 3);
            ts.scroll_fade.touch(appui::now_ms());
        }
    }

    /// A press in terminal `id`: start a selection (a double press takes the word, a triple
    /// press the line).
    pub(crate) fn term_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        let g = term_grid(rect);
        if !g.in_text(px, py) {
            return;
        }
        let cell = g.cell_at(px, py);
        let ticks = crate::interrupts::ticks();
        let Some(ts) = self.term_state_mut(id) else {
            return;
        };
        ts.grid = (g.cols, g.rows);
        ts.last_input = appui::ticks();
        let n = ts.click_count(ticks, cell);
        let rows = ts.term.view_in(g.cols, g.rows).rows;
        let row_text = rows.get(cell.0).map(String::as_str).unwrap_or("");
        ts.sel = match n {
            1 => Some(Selection::new(cell)),
            2 => {
                let (a, b) = termui::word_bounds(row_text, cell.1);
                Some(Selection {
                    anchor: (cell.0, a),
                    head: (cell.0, b),
                })
            }
            _ => Some(Selection {
                anchor: (cell.0, 0),
                head: (cell.0, row_text.chars().count().saturating_sub(1)),
            }),
        };
        ts.press_at = (px, py);
        self.drag = Some(Drag {
            win: id,
            mode: DragMode::Select,
        });
    }

    /// The pointer moved with the button held after a press: extend the selection.
    pub(crate) fn term_drag(&mut self, id: WindowId, px: i32, py: i32) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let g = term_grid(rect);
        let cell = g.cell_at(px, py);
        let Some(ts) = self.term_state_mut(id) else {
            return;
        };
        // A press that has not moved keeps the word or line a double press took.
        if (px, py) == ts.press_at || ts.last_click.2 > 1 {
            return;
        }
        if let Some(sel) = &mut ts.sel {
            sel.head = cell;
        }
    }

    /// Whether the pointer at `(cx, cy)` is over the text of terminal `id` (an I-beam).
    pub(crate) fn term_text_at(&self, id: WindowId, cx: i32, cy: i32) -> bool {
        self.wm
            .get(id)
            .is_some_and(|w| term_grid(w.rect).in_text(cx, cy))
    }

    /// Paste text into terminal `id` (never runs a command by itself).
    pub(crate) fn term_paste(&mut self, id: WindowId, text: &[u8]) {
        let s = String::from_utf8_lossy(text).into_owned();
        if let Some(ts) = self.term_state_mut(id) {
            ts.term.paste(&s);
        }
    }

    // ---- drawing ----

    /// The terminal: a strip with the session's tab under the title bar, as many columns and
    /// rows as the window holds, the newest output at the bottom, the prompt in the accent
    /// colour, a selection, a block or bar cursor and an overlay scrollbar.
    pub(crate) fn draw_terminal(&self, c: &mut Canvas, r: Rect, t: &TermState, focused: bool) {
        let g = term_grid(r);
        let p = theme::pal();
        let body = Rect::new(r.x, r.y + TITLE_H, r.w, (r.h - TITLE_H).max(0));
        let saved = c.set_clip(
            body.intersection(&c.clip_rect())
                .unwrap_or(Rect::new(0, 0, 0, 0)),
        );
        ui::fill(c, body, theme::surface());
        self.draw_term_strip(c, &g, t);
        let v = t.term.view_in(g.cols, g.rows);
        let px = font_px();
        let (_, mono_lh) = text::mono_cell_px(px);
        let dy = (g.m.lh - mono_lh) / 2;
        let (path, sym) = termui::split_prompt(t.term.prompt());
        let path_n = path.chars().count();
        let sym_n = sym.chars().count();
        let acc = theme::accent();
        let (sel_col, sel_a) = if focused {
            theme::tint(0x59_00_00_00 | appui::rgb_of(acc))
        } else {
            theme::tint(if theme::dark() {
                0x40_80_80_88
            } else {
                0x40_70_70_78
            })
        };
        for (i, row) in v.rows.iter().enumerate() {
            let y = g.y + i as i32 * g.m.lh;
            let len = row.chars().count();
            if let Some((first, n)) = t.sel.and_then(|s| s.span(i, len)) {
                let rx = g.x + first as i32 * g.m.cw;
                c.blend_rect(Rect::new(rx, y, n as i32 * g.m.cw, g.m.lh), sel_col, sel_a);
            }
            // The prompt part of the live line: the path in the accent, the symbol quieter.
            let (a, b) = match v.live_first {
                Some(lf) if i >= lf => {
                    let start = (i - lf) * g.cols;
                    (
                        path_n.saturating_sub(start).min(len),
                        (path_n + sym_n).saturating_sub(start).min(len),
                    )
                }
                _ => (0, 0),
            };
            let cut = |n: usize| row.char_indices().nth(n).map_or(row.len(), |(b, _)| b);
            let (ia, ib) = (cut(a), cut(b));
            let mut x = g.x;
            for (seg, col) in [
                (&row[..ia], acc),
                (&row[ia..ib], theme::text_muted()),
                (&row[ib..], theme::text()),
            ] {
                if !seg.is_empty() {
                    text::draw_mono(c, x, y + dy, seg, px, col);
                }
                x += seg.chars().count() as i32 * g.m.cw;
            }
        }
        if let Some((row, col)) = v.cursor {
            let cell = g.cell_rect(row, col);
            let on_text = v.rows.get(row).is_some_and(|s| s.chars().count() > col);
            if !focused {
                c.stroke_rrect(cell, 2, Corner::Circle, acc, 220);
            } else {
                let alpha = appui::caret_alpha(t.last_input);
                if on_text {
                    // Between characters: a bar.
                    c.fill_rrect(
                        Rect::new(cell.x, cell.y + 2, 2, cell.h - 4),
                        1,
                        Corner::Circle,
                        acc,
                        alpha as u16,
                    );
                } else if alpha > 0 {
                    // At the end of the line: a block.
                    c.fill_rrect(
                        Rect::new(cell.x, cell.y + 2, cell.w, cell.h - 4),
                        2,
                        Corner::Circle,
                        acc,
                        (alpha * 200 / 256) as u16,
                    );
                }
            }
        }
        // The overlay scrollbar.
        let total = v.above + v.rows.len() + v.below;
        ui::overlay_scrollbar(
            c,
            g.track(),
            v.above,
            total,
            v.rows.len().max(1),
            t.scroll_fade.alpha(appui::now_ms()),
        );
        if t.term.is_running() {
            let msg = "Executando · Ctrl+C cancela";
            let w = text::measure(msg, FOOTNOTE, Weight::Medium) + 24;
            let pill = Rect::new(r.right() - 14 - w, r.bottom() - 14 - 24, w, 24);
            ui::fill_token(c, pill, 12, p.control_bg);
            ui::stroke_token(c, pill, 12, p.control_border);
            text::draw_centered(c, pill, msg, FOOTNOTE, Weight::Medium, theme::text_muted());
        }
        c.restore_clip(saved);
    }

    /// The strip under the title bar: the session's tab with its folder.
    fn draw_term_strip(&self, c: &mut Canvas, g: &Grid, t: &TermState) {
        let p = theme::pal();
        let strip = g.strip;
        ui::fill(c, strip, theme::toolbar());
        appui::hairline(c, strip.x, strip.bottom() - 1, strip.w);
        let label = termui::tab_label(t.term.prompt());
        let room = (strip.w - 2 * termui::PAD - 40).clamp(40, 260);
        let label = text::ellipsize_middle(&label, BODY, Weight::Medium, room);
        let w = text::measure(&label, BODY, Weight::Medium) + 40;
        let tab = Rect::new(strip.x + termui::PAD - 4, strip.y + 5, w, strip.h - 11);
        ui::fill_token(c, tab, 7, p.control_bg);
        ui::stroke_token(c, tab, 7, p.control_border);
        appui::blit_tool_dim(
            c,
            Tool::Folder,
            tab.x + 10,
            tab.y + (tab.h - 14) / 2,
            14,
            appui::rgb_of(theme::accent()),
            256,
        );
        let ty = text::center_y(tab.y, tab.h, BODY, Weight::Medium);
        text::draw(
            c,
            tab.x + 30,
            ty,
            &label,
            BODY,
            Weight::Medium,
            theme::text(),
        );
    }
}
