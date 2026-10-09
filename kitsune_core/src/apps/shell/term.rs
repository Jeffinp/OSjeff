//! A terminal session: scrollback ([`Screen`]) plus the line being typed
//! ([`LineEditor`]) and the rules that connect keys to commands.
//!
//! The kernel owns the [`Shell`] and the filesystem (it may run commands on
//! another thread), so this type never executes anything: [`Term::key`]
//! answers with a [`TermAction`] and the kernel calls [`Term::finish`] with the
//! result. That keeps the whole interaction (history, completion, Ctrl+C,
//! Ctrl+L, scrolling, echo of the typed line) testable on the host.
//!
//! ```text
//! key ──► Term::key ──► TermAction::Run(line) ──► kernel runs it ──► Term::finish(result)
//!                  └──► Redraw / Cancel / Exit
//! ```

use super::exec::{RunResult, Shell};
use super::fs::ShellFs;
use super::history::History;
use super::line::{LineEditor, LineEvent, NoCompleter, ShellCompleter};
use super::screen::{self, Screen, View};
use crate::system::input::{KeyCode, KeyEvent};
use alloc::string::String;

/// What the kernel must do after a key.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TermAction {
    /// Nothing changed.
    None,
    /// Redraw the window.
    Redraw,
    /// Run this command line, then call [`Term::finish`].
    Run(String),
    /// Ctrl+C while a command runs: ask it to stop (it still finishes normally).
    Cancel,
    /// Close the window (Ctrl+D on an empty line, or the `exit` command).
    Exit,
}

/// One terminal window's interactive state.
pub struct Term {
    pub screen: Screen,
    line: LineEditor,
    running: bool,
    cols: usize,
    rows: usize,
}

impl Default for Term {
    fn default() -> Self {
        Self::new("$ ")
    }
}

impl Term {
    /// A fresh terminal showing `banner` (may be empty) above the first prompt.
    pub fn new(prompt: &str) -> Self {
        Self {
            screen: Screen::new(),
            line: LineEditor::new(prompt),
            running: false,
            cols: 80,
            rows: 24,
        }
    }

    /// The window now shows `rows` x `cols` characters (call before `key`).
    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols.max(1);
        self.rows = rows.max(1);
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn set_prompt(&mut self, prompt: &str) {
        self.line.set_prompt(prompt);
    }

    /// The text typed on the live line.
    pub fn input(&self) -> String {
        self.line.text()
    }

    /// Print text as if a command wrote it (banners, errors from the kernel).
    pub fn print(&mut self, text: &str) {
        self.screen.print(text.as_bytes());
        self.screen.ensure_newline();
    }

    fn live_len(&self) -> usize {
        let (text, _) = self.line.display();
        text.chars().count() + 1
    }

    fn scroll(&mut self, delta: isize) {
        let live = self.live_len();
        self.screen.scroll(delta, self.cols, self.rows, live);
    }

    /// Scroll back (positive) or forward by `delta` rows (the mouse wheel).
    pub fn scroll_rows(&mut self, delta: isize) {
        self.scroll(delta);
    }

    fn echo(&mut self, suffix: &str) {
        let (text, _) = self.line.display();
        self.screen.print(text.as_bytes());
        self.screen.print(suffix.as_bytes());
        self.screen.ensure_newline();
    }

    /// Handle a key. `ctx` is the shell and filesystem for history and Tab
    /// completion; it is `None` while a command runs (the kernel owns them).
    pub fn key(&mut self, ev: KeyEvent, ctx: Option<(&Shell, &dyn ShellFs)>) -> TermAction {
        let page = (self.rows.saturating_sub(1)).max(1) as isize;
        match ev.code {
            KeyCode::PageUp => {
                self.scroll(page);
                return TermAction::Redraw;
            }
            KeyCode::PageDown => {
                self.scroll(-page);
                return TermAction::Redraw;
            }
            KeyCode::Home if ev.mods.ctrl => {
                self.scroll(isize::MAX / 2);
                return TermAction::Redraw;
            }
            KeyCode::End if ev.mods.ctrl => {
                self.screen.to_bottom();
                return TermAction::Redraw;
            }
            _ => {}
        }
        let ctrl_c = ev.mods.ctrl
            && !ev.mods.alt
            && matches!(ev.code, KeyCode::Char(c) if c.eq_ignore_ascii_case(&'c'));
        if self.running {
            return if ctrl_c {
                self.screen.to_bottom();
                TermAction::Cancel
            } else {
                TermAction::None
            };
        }
        let Some((shell, fs)) = ctx else {
            return TermAction::None;
        };
        self.screen.to_bottom();
        let typed = self.line.text();
        let comp = ShellCompleter { shell, fs };
        match self.line.handle_key(ev, shell.history(), &comp) {
            LineEvent::None => TermAction::None,
            LineEvent::Changed => TermAction::Redraw,
            LineEvent::Submit(text) => {
                // Echo the line with the prompt it was typed at.
                let prompt = String::from(self.line.prompt());
                self.screen.print(prompt.as_bytes());
                self.screen.print(text.as_bytes());
                self.screen.ensure_newline();
                if text.trim().is_empty() {
                    TermAction::Redraw
                } else {
                    self.running = true;
                    TermAction::Run(text)
                }
            }
            LineEvent::Interrupt => {
                let prompt = String::from(self.line.prompt());
                self.screen.print(prompt.as_bytes());
                self.screen.print(typed.as_bytes());
                self.screen.print(b"^C");
                self.screen.ensure_newline();
                TermAction::Redraw
            }
            LineEvent::Eof => TermAction::Exit,
            LineEvent::ClearScreen => {
                self.screen.clear();
                TermAction::Redraw
            }
            LineEvent::Candidates(names) => {
                self.echo("");
                let cols = self.cols;
                self.screen
                    .print(screen::columnize(&names, cols).as_bytes());
                TermAction::Redraw
            }
        }
    }

    /// Insert pasted text at the caret: newlines and other control characters
    /// become spaces (a paste never runs a command by itself).
    pub fn paste(&mut self, text: &str) {
        if self.running {
            return;
        }
        let hist = History::new();
        for c in text.chars() {
            let c = if c.is_control() { ' ' } else { c };
            self.line.handle_key(KeyEvent::ch(c), &hist, &NoCompleter);
        }
        self.screen.to_bottom();
    }

    /// A command finished: show its output and the next prompt. Returns
    /// [`TermAction::Exit`] if the command was `exit`.
    pub fn finish(&mut self, result: &RunResult, next_prompt: &str) -> TermAction {
        self.running = false;
        if result.clear {
            self.screen.clear();
        }
        self.screen.print(&result.output);
        self.screen.ensure_newline();
        self.screen.to_bottom();
        self.line.set_prompt(next_prompt);
        if result.exit.is_some() {
            TermAction::Exit
        } else {
            TermAction::Redraw
        }
    }

    /// What to draw for the current geometry. While a command runs the prompt is
    /// hidden (the output so far is all there is).
    pub fn view(&self) -> View {
        self.view_in(self.cols, self.rows)
    }

    /// [`Term::view`] for an explicit window size (the window may have been resized since
    /// the last key).
    pub fn view_in(&self, cols: usize, rows: usize) -> View {
        if self.running {
            return self.screen.view_history(cols, rows);
        }
        let (text, cur) = self.line.display();
        self.screen.view(cols, rows, &text, cur)
    }

    /// The prompt text of the live line (the window's tab shows its directory).
    pub fn prompt(&self) -> &str {
        self.line.prompt()
    }

    /// Characters of the prompt on the live line (to colour them).
    pub fn prompt_chars(&self) -> usize {
        self.line.prompt().chars().count()
    }
}

#[cfg(test)]
mod tests;
