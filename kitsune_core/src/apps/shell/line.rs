//! The terminal side of the shell: a line editor with cursor movement,
//! kill/yank, history navigation, Ctrl+R reverse search and Tab completion
//! of commands, variables and paths.
//!
//! [`LineEditor`] owns only the text being typed. History lives in the shell
//! ([`super::Shell::history`]) and completion data comes through the
//! [`Completer`] trait ([`ShellCompleter`] implements it for a shell plus its
//! filesystem), so the editor is testable on its own.

use super::exec::Shell;
use super::fs::{Kind, ShellFs};
use super::history::History;
use crate::system::input::{KeyCode, KeyEvent};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

mod completion;
mod history;
mod keys;

/// Longest line the editor accepts, in characters.
pub const MAX_LINE: usize = 4096;

/// What the caller should do after a key.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LineEvent {
    /// Nothing changed on screen.
    None,
    /// Redraw the input line.
    Changed,
    /// Enter: run this text (the editor is already cleared).
    Submit(String),
    /// Ctrl+C: the line was discarded; print a fresh prompt.
    Interrupt,
    /// Ctrl+D on an empty line: the user wants to leave.
    Eof,
    /// Ctrl+L: clear the screen and redraw the prompt and line.
    ClearScreen,
    /// Tab with several possible completions: print these names.
    Candidates(Vec<String>),
}

/// One completion: the full replacement for the word and whether it names a
/// directory.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Completion {
    pub text: String,
    pub is_dir: bool,
}

/// Source of completions.
pub trait Completer {
    /// Command names starting with `prefix`.
    fn commands(&self, prefix: &str) -> Vec<Completion>;
    /// Paths starting with `prefix` (as typed, relative or absolute).
    fn paths(&self, prefix: &str) -> Vec<Completion>;
    /// Variable names starting with `prefix` (without the `$`).
    fn variables(&self, prefix: &str) -> Vec<Completion>;
}

/// A completer that offers nothing.
pub struct NoCompleter;

impl Completer for NoCompleter {
    fn commands(&self, _: &str) -> Vec<Completion> {
        Vec::new()
    }

    fn paths(&self, _: &str) -> Vec<Completion> {
        Vec::new()
    }

    fn variables(&self, _: &str) -> Vec<Completion> {
        Vec::new()
    }
}

/// Completion backed by a [`Shell`] (builtins, aliases, functions, scripts on
/// `PATH`, variables) and a [`ShellFs`] (paths).
pub struct ShellCompleter<'a> {
    pub shell: &'a Shell,
    pub fs: &'a dyn ShellFs,
}

fn list_matches(fs: &dyn ShellFs, dir: &str, base: &str, dir_prefix: &str) -> Vec<Completion> {
    let Ok(entries) = fs.list(if dir.is_empty() { "." } else { dir }) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries {
        if !e.name.starts_with(base) || (e.name.starts_with('.') && !base.starts_with('.')) {
            continue;
        }
        let is_dir = e.kind == Kind::Dir;
        let mut text = alloc::format!("{dir_prefix}{}", e.name);
        if is_dir {
            text.push('/');
        }
        out.push(Completion { text, is_dir });
    }
    out
}

impl Completer for ShellCompleter<'_> {
    fn commands(&self, prefix: &str) -> Vec<Completion> {
        let mut names: Vec<String> = Vec::new();
        names.extend(self.shell.registry().names().map(String::from));
        names.extend(self.shell.aliases().keys().cloned());
        names.extend(self.shell.function_names().map(String::from));
        for dir in self.shell.env.path_dirs() {
            if let Ok(entries) = self.fs.list(&dir) {
                for e in entries {
                    if e.kind == Kind::File {
                        names.push(e.name.strip_suffix(".sh").unwrap_or(&e.name).to_string());
                    }
                }
            }
        }
        names.retain(|n| n.starts_with(prefix));
        names.sort();
        names.dedup();
        names
            .into_iter()
            .map(|text| Completion {
                text,
                is_dir: false,
            })
            .collect()
    }

    fn paths(&self, prefix: &str) -> Vec<Completion> {
        let (dir, base) = match prefix.rfind('/') {
            Some(i) => (&prefix[..=i], &prefix[i + 1..]),
            None => ("", prefix),
        };
        list_matches(self.fs, dir, base, dir)
    }

    fn variables(&self, prefix: &str) -> Vec<Completion> {
        self.shell
            .env
            .iter()
            .filter(|(n, _, _)| n.starts_with(prefix))
            .map(|(n, _, _)| Completion {
                text: n.to_string(),
                is_dir: false,
            })
            .collect()
    }
}

struct SearchState {
    query: String,
    found: Option<usize>,
    saved: Vec<char>,
    saved_cursor: usize,
}

/// The line being edited.
pub struct LineEditor {
    buf: Vec<char>,
    cursor: usize,
    prompt: String,
    hist_idx: Option<usize>,
    draft: Vec<char>,
    kill: Vec<char>,
    search: Option<SearchState>,
}

impl Default for LineEditor {
    fn default() -> Self {
        Self::new("$ ")
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl LineEditor {
    pub fn new(prompt: &str) -> Self {
        Self {
            buf: Vec::new(),
            cursor: 0,
            prompt: prompt.to_string(),
            hist_idx: None,
            draft: Vec::new(),
            kill: Vec::new(),
            search: None,
        }
    }

    pub fn set_prompt(&mut self, prompt: &str) {
        self.prompt = prompt.to_string();
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    /// The text typed so far.
    pub fn text(&self) -> String {
        self.buf.iter().collect()
    }

    /// Cursor position in characters.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn is_searching(&self) -> bool {
        self.search.is_some()
    }

    pub fn set_text(&mut self, s: &str) {
        self.buf = s.chars().take(MAX_LINE).collect();
        self.cursor = self.buf.len();
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.cursor = 0;
        self.hist_idx = None;
        self.search = None;
    }

    /// What to draw: the prompt plus text and the cursor column (in
    /// characters from the start of that string). In Ctrl+R mode it shows the
    /// search line instead.
    pub fn display(&self) -> (String, usize) {
        if let Some(s) = &self.search {
            let head = alloc::format!("{} ", crate::t!("sh.line.rsearch", q = s.query.as_str()));
            let col = head.chars().count();
            let tail: String = self.buf.iter().collect();
            return (alloc::format!("{head}{tail}"), col);
        }
        let mut t = self.prompt.clone();
        let base = t.chars().count();
        t.extend(self.buf.iter());
        (t, base + self.cursor)
    }

    fn insert(&mut self, c: char) {
        if self.buf.len() >= MAX_LINE || c.is_control() {
            return;
        }
        self.buf.insert(self.cursor, c);
        self.cursor += 1;
    }

    fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            self.insert(c);
        }
    }

    fn word_left(&self) -> usize {
        let mut p = self.cursor;
        while p > 0 && !is_word_char(self.buf[p - 1]) {
            p -= 1;
        }
        while p > 0 && is_word_char(self.buf[p - 1]) {
            p -= 1;
        }
        p
    }

    fn word_right(&self) -> usize {
        let mut p = self.cursor;
        while p < self.buf.len() && !is_word_char(self.buf[p]) {
            p += 1;
        }
        while p < self.buf.len() && is_word_char(self.buf[p]) {
            p += 1;
        }
        p
    }

    fn kill_range(&mut self, a: usize, b: usize) {
        if a < b {
            self.kill = self.buf.drain(a..b).collect();
            self.cursor = a;
        }
    }
}

fn common_prefix(items: &[&str]) -> String {
    let Some(first) = items.first() else {
        return String::new();
    };
    let mut len = first.chars().count();
    for it in &items[1..] {
        let n = first
            .chars()
            .zip(it.chars())
            .take_while(|(a, b)| a == b)
            .count();
        len = len.min(n);
    }
    first.chars().take(len).collect()
}

#[cfg(test)]
mod tests;
