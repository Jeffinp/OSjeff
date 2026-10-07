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
use crate::input::{KeyCode, KeyEvent};
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

    /// Characters of the prompt on the live line (to colour them).
    pub fn prompt_chars(&self) -> usize {
        self.line.prompt().chars().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{KeyCode, KeyEvent, Mods};
    use crate::shell::{Host, MemFs, MockSys};
    use alloc::string::ToString;
    use alloc::vec::Vec;

    /// The kernel's loop in miniature: keys in, commands run inline.
    struct Rig {
        term: Term,
        shell: Shell,
        fs: MemFs,
        sys: MockSys,
        exited: bool,
    }

    impl Rig {
        fn new() -> Self {
            let fs = MemFs::new()
                .with_file("/alpha.txt", b"a\n")
                .with_file("/alpine.md", b"b\n")
                .with_dir("/docs")
                .with_file("/docs/readme.txt", b"hi\n");
            let mut shell = Shell::new();
            shell.env.set("PS1", "\\w\\$ ");
            let mut term = Term::new("");
            term.set_prompt(&shell.prompt(&fs, &MockSys::default()));
            term.resize(40, 10);
            Self {
                term,
                shell,
                fs,
                sys: MockSys::default(),
                exited: false,
            }
        }

        fn press(&mut self, ev: KeyEvent) -> TermAction {
            let a = self
                .term
                .key(ev, Some((&self.shell, &self.fs as &dyn ShellFs)));
            if let TermAction::Run(line) = &a {
                let r = {
                    let mut h = Host {
                        fs: &mut self.fs,
                        sys: &mut self.sys,
                    };
                    self.shell.run_line(line, &mut h)
                };
                let p = self.shell.prompt(&self.fs, &self.sys);
                if self.term.finish(&r, &p) == TermAction::Exit {
                    self.exited = true;
                }
            }
            if a == TermAction::Exit {
                self.exited = true;
            }
            a
        }

        fn type_text(&mut self, s: &str) {
            for c in s.chars() {
                self.press(KeyEvent::ch(c));
            }
        }

        fn enter(&mut self) -> TermAction {
            self.press(KeyEvent::plain(KeyCode::Enter))
        }

        fn run(&mut self, line: &str) {
            self.type_text(line);
            self.enter();
        }

        fn rows(&self) -> Vec<String> {
            self.term.view().rows
        }
    }

    #[test]
    fn a_command_echoes_runs_and_shows_the_next_prompt() {
        let mut r = Rig::new();
        r.run("echo hello");
        assert_eq!(r.rows(), ["/$ echo hello", "hello", "/$ "]);
        r.run("cd docs");
        assert_eq!(r.rows().last().unwrap(), "/docs$ ");
        r.run("cat readme.txt");
        let rows = r.rows();
        assert_eq!(
            &rows[rows.len() - 3..],
            ["/docs$ cat readme.txt", "hi", "/docs$ "]
        );
    }

    #[test]
    fn an_empty_line_only_moves_the_prompt() {
        let mut r = Rig::new();
        assert_eq!(r.enter(), TermAction::Redraw);
        assert_eq!(r.rows(), ["/$ ", "/$ "]);
        assert!(!r.term.is_running());
    }

    #[test]
    fn up_and_down_walk_the_history() {
        let mut r = Rig::new();
        r.run("echo one");
        r.run("echo two");
        r.press(KeyEvent::plain(KeyCode::Up));
        assert_eq!(r.term.input(), "echo two");
        r.press(KeyEvent::plain(KeyCode::Up));
        assert_eq!(r.term.input(), "echo one");
        r.press(KeyEvent::plain(KeyCode::Down));
        assert_eq!(r.term.input(), "echo two");
        r.press(KeyEvent::plain(KeyCode::Down));
        assert_eq!(r.term.input(), "");
    }

    #[test]
    fn tab_completes_commands_and_paths() {
        let mut r = Rig::new();
        r.type_text("hist");
        r.press(KeyEvent::plain(KeyCode::Tab));
        assert_eq!(r.term.input(), "history ");
        r.press(KeyEvent::ctrl('u'));
        r.type_text("cat docs/re");
        r.press(KeyEvent::plain(KeyCode::Tab));
        assert_eq!(r.term.input(), "cat docs/readme.txt ");
        // Several candidates: the common prefix first, then the list.
        r.press(KeyEvent::ctrl('u'));
        r.type_text("cat al");
        r.press(KeyEvent::plain(KeyCode::Tab));
        assert_eq!(r.term.input(), "cat alp");
        r.press(KeyEvent::plain(KeyCode::Tab));
        let rows = r.rows();
        assert!(
            rows.iter()
                .any(|l| l.contains("alpha.txt") && l.contains("alpine.md")),
            "{rows:?}"
        );
        // The typed line is echoed above the candidates and the line is intact.
        assert!(rows.iter().any(|l| l == "/$ cat alp"));
        assert_eq!(r.term.input(), "cat alp");
    }

    #[test]
    fn ctrl_c_discards_the_line_and_shows_it_crossed_out() {
        let mut r = Rig::new();
        r.type_text("echo nope");
        r.press(KeyEvent::ctrl('c'));
        assert_eq!(r.term.input(), "");
        assert_eq!(r.rows(), ["/$ echo nope^C", "/$ "]);
        assert!(!r.term.is_running());
    }

    #[test]
    fn ctrl_c_while_running_asks_to_cancel_and_other_keys_wait() {
        let mut r = Rig::new();
        r.type_text("sleep 5");
        let a = r.term.key(
            KeyEvent::plain(KeyCode::Enter),
            Some((&r.shell, &r.fs as &dyn ShellFs)),
        );
        assert_eq!(a, TermAction::Run("sleep 5".to_string()));
        assert!(r.term.is_running());
        // Keys are not queued; Ctrl+C asks the kernel to stop the command.
        assert_eq!(r.term.key(KeyEvent::ch('x'), None), TermAction::None);
        assert_eq!(r.term.key(KeyEvent::ctrl('c'), None), TermAction::Cancel);
        assert_eq!(r.term.input(), "");
        // While it runs there is no prompt on screen.
        assert_eq!(r.rows(), ["/$ sleep 5"]);
        let res = RunResult {
            status: 130,
            output: b"sh: interrupted\n".to_vec(),
            ..RunResult::default()
        };
        assert_eq!(r.term.finish(&res, "/$ "), TermAction::Redraw);
        assert!(!r.term.is_running());
        assert_eq!(r.rows(), ["/$ sleep 5", "sh: interrupted", "/$ "]);
    }

    #[test]
    fn ctrl_l_and_clear_wipe_the_screen() {
        let mut r = Rig::new();
        r.run("echo a");
        r.type_text("par");
        r.press(KeyEvent::ctrl('l'));
        assert_eq!(r.rows(), ["/$ par"], "the typed text stays");
        r.run("echo b");
        r.run("clear");
        assert_eq!(r.rows(), ["/$ "]);
    }

    #[test]
    fn exit_and_ctrl_d_close_the_window() {
        let mut r = Rig::new();
        r.run("exit");
        assert!(r.exited);
        let mut r = Rig::new();
        r.type_text("x");
        assert_eq!(
            r.press(KeyEvent::ctrl('d')),
            TermAction::Redraw,
            "not empty: delete"
        );
        let mut r = Rig::new();
        assert_eq!(r.press(KeyEvent::ctrl('d')), TermAction::Exit);
    }

    #[test]
    fn page_keys_scroll_the_scrollback_and_typing_snaps_back() {
        let mut r = Rig::new();
        r.run("seq 100");
        let bottom = r.rows();
        assert_eq!(bottom.len(), 10);
        assert_eq!(bottom.last().unwrap(), "/$ ");
        r.press(KeyEvent::plain(KeyCode::PageUp));
        let up = r.rows();
        assert_ne!(up, bottom);
        assert!(r.term.screen.is_scrolled());
        assert_eq!(
            up.last().unwrap(),
            "92",
            "9 rows older than the bottom view"
        );
        r.press(KeyEvent::plain(KeyCode::PageDown));
        assert_eq!(r.rows(), bottom);
        // Ctrl+Home goes to the oldest output, Ctrl+End back; a typed key too.
        r.press(KeyEvent::new(KeyCode::Home, Mods::CTRL));
        assert_eq!(r.rows()[0], "/$ seq 100");
        r.press(KeyEvent::new(KeyCode::End, Mods::CTRL));
        assert_eq!(r.rows(), bottom);
        r.press(KeyEvent::plain(KeyCode::PageUp));
        r.type_text("x");
        assert!(!r.term.screen.is_scrolled());
        assert_eq!(r.rows().last().unwrap(), "/$ x");
        // The wheel path.
        r.term.scroll_rows(3);
        assert!(r.term.screen.is_scrolled());
    }

    #[test]
    fn paste_never_submits() {
        let mut r = Rig::new();
        r.term.paste("echo a\necho b\r\n\x07x");
        assert!(!r.term.is_running());
        assert_eq!(r.term.input(), "echo a echo b   x");
        // Pasting while a command runs is ignored.
        let mut r = Rig::new();
        r.type_text("echo q");
        r.term.key(
            KeyEvent::plain(KeyCode::Enter),
            Some((&r.shell, &r.fs as &dyn ShellFs)),
        );
        r.term.paste("zzz");
        assert_eq!(r.term.input(), "");
    }

    #[test]
    fn a_ten_thousand_line_output_stays_bounded_and_scrollable() {
        let mut r = Rig::new();
        r.run("seq 10000");
        assert!(r.term.screen.line_count() <= crate::shell::screen::MAX_LINES);
        let rows = r.rows();
        assert_eq!(rows[rows.len() - 2], "10000");
        r.press(KeyEvent::new(KeyCode::Home, Mods::CTRL));
        assert!(
            r.rows().iter().any(|l| l.parse::<u32>().is_ok()),
            "oldest kept line is a number"
        );
    }

    #[test]
    fn narrow_windows_wrap_the_live_line_and_keep_the_caret_visible() {
        let mut r = Rig::new();
        r.term.resize(8, 4);
        r.type_text("echo 0123456789");
        let v = r.term.view();
        assert_eq!(v.rows, ["/$ echo ", "01234567", "89"].map(String::from));
        assert_eq!(v.cursor, Some((2, 2)));
        r.term.resize(1, 1);
        let v = r.term.view();
        assert_eq!(v.rows.len(), 1);
    }

    #[test]
    fn finish_with_stale_scroll_returns_to_the_bottom() {
        let mut r = Rig::new();
        r.run("seq 30");
        r.press(KeyEvent::plain(KeyCode::PageUp));
        r.run("echo done");
        assert!(!r.term.screen.is_scrolled());
        assert_eq!(r.rows()[r.rows().len() - 2], "done");
    }
}
