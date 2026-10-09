//! Keyboard handling of the terminal.

use crate::desktop::apps::terminal::term_grid;
use crate::desktop::kit::appui;
use crate::desktop::services::shellhost::{self, Ctx};
use crate::desktop::services::sysstore::VfsStore;
use crate::desktop::*;
use kitsune_core::input::{KeyCode, KeyEvent, Mods};
use kitsune_core::settings::font_step;
use kitsune_core::shell::{ShellFs, TermAction};
use kitsune_core::sysif::SettingsStore;

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
}
