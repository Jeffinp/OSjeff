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

    /// Handle one key.
    pub fn handle_key(&mut self, ev: KeyEvent, hist: &History, comp: &dyn Completer) -> LineEvent {
        if self.search.is_some() {
            return self.search_key(ev, hist);
        }
        let (ctrl, alt) = (ev.mods.ctrl, ev.mods.alt);
        match ev.code {
            KeyCode::Char(c) if ctrl && !alt => self.ctrl_key(c, hist),
            KeyCode::Char(_) if alt => LineEvent::None,
            KeyCode::Char(c) => {
                self.insert(c);
                self.hist_idx = None;
                LineEvent::Changed
            }
            KeyCode::Enter => {
                let line = self.text();
                self.clear();
                LineEvent::Submit(line)
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.buf.remove(self.cursor);
                    LineEvent::Changed
                } else {
                    LineEvent::None
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.buf.len() {
                    self.buf.remove(self.cursor);
                    LineEvent::Changed
                } else {
                    LineEvent::None
                }
            }
            KeyCode::Left => {
                if ctrl {
                    self.cursor = self.word_left();
                } else {
                    self.cursor = self.cursor.saturating_sub(1);
                }
                LineEvent::Changed
            }
            KeyCode::Right => {
                if ctrl {
                    self.cursor = self.word_right();
                } else if self.cursor < self.buf.len() {
                    self.cursor += 1;
                }
                LineEvent::Changed
            }
            KeyCode::Home => {
                self.cursor = 0;
                LineEvent::Changed
            }
            KeyCode::End => {
                self.cursor = self.buf.len();
                LineEvent::Changed
            }
            KeyCode::Up => self.history_prev(hist),
            KeyCode::Down => self.history_next(hist),
            KeyCode::Tab => self.complete(comp),
            _ => LineEvent::None,
        }
    }

    fn ctrl_key(&mut self, c: char, hist: &History) -> LineEvent {
        match c.to_ascii_lowercase() {
            'a' => self.cursor = 0,
            'e' => self.cursor = self.buf.len(),
            'b' => self.cursor = self.cursor.saturating_sub(1),
            'f' => {
                if self.cursor < self.buf.len() {
                    self.cursor += 1;
                }
            }
            'k' => {
                let n = self.buf.len();
                self.kill_range(self.cursor, n);
            }
            'u' => {
                let c = self.cursor;
                self.kill_range(0, c);
            }
            'w' => {
                let w = self.word_left();
                let c = self.cursor;
                self.kill_range(w, c);
            }
            'y' => {
                let k = self.kill.clone();
                for ch in k {
                    self.insert(ch);
                }
            }
            'd' => {
                if self.buf.is_empty() {
                    return LineEvent::Eof;
                }
                if self.cursor < self.buf.len() {
                    self.buf.remove(self.cursor);
                }
            }
            'l' => return LineEvent::ClearScreen,
            'c' => {
                self.clear();
                return LineEvent::Interrupt;
            }
            'p' => return self.history_prev(hist),
            'n' => return self.history_next(hist),
            'r' => {
                self.search = Some(SearchState {
                    query: String::new(),
                    found: None,
                    saved: self.buf.clone(),
                    saved_cursor: self.cursor,
                });
            }
            _ => return LineEvent::None,
        }
        LineEvent::Changed
    }

    fn load_history(&mut self, hist: &History, i: usize) {
        self.buf = hist.get(i).unwrap_or("").chars().take(MAX_LINE).collect();
        self.cursor = self.buf.len();
    }

    fn history_prev(&mut self, hist: &History) -> LineEvent {
        if hist.is_empty() {
            return LineEvent::None;
        }
        let i = match self.hist_idx {
            None => {
                self.draft = self.buf.clone();
                hist.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.hist_idx = Some(i.min(hist.len() - 1));
        self.load_history(hist, i.min(hist.len() - 1));
        LineEvent::Changed
    }

    fn history_next(&mut self, hist: &History) -> LineEvent {
        match self.hist_idx {
            None => LineEvent::None,
            Some(i) if i + 1 < hist.len() => {
                self.hist_idx = Some(i + 1);
                self.load_history(hist, i + 1);
                LineEvent::Changed
            }
            Some(_) => {
                self.hist_idx = None;
                self.buf = core::mem::take(&mut self.draft);
                self.cursor = self.buf.len();
                LineEvent::Changed
            }
        }
    }

    fn search_key(&mut self, ev: KeyEvent, hist: &History) -> LineEvent {
        let Some(mut st) = self.search.take() else {
            return LineEvent::None;
        };
        let (ctrl, alt) = (ev.mods.ctrl, ev.mods.alt);
        let mut result = LineEvent::Changed;
        let mut keep = true;
        match ev.code {
            KeyCode::Char(c) if ctrl && !alt => match c.to_ascii_lowercase() {
                'r' => {
                    let before = st.found.unwrap_or(hist.len());
                    if let Some(i) = hist.search_rev(&st.query, before) {
                        st.found = Some(i);
                    }
                }
                'g' | 'c' => {
                    self.buf = st.saved.clone();
                    self.cursor = st.saved_cursor;
                    keep = false;
                    if c.eq_ignore_ascii_case(&'c') {
                        self.clear();
                        result = LineEvent::Interrupt;
                    }
                }
                _ => {}
            },
            KeyCode::Char(c) if !alt => {
                if !c.is_control() && st.query.len() < 256 {
                    st.query.push(c);
                    let before = st.found.map_or(hist.len(), |i| i + 1);
                    st.found = hist.search_rev(&st.query, before);
                }
            }
            KeyCode::Backspace => {
                st.query.pop();
                st.found = hist.search_rev(&st.query, hist.len());
            }
            KeyCode::Esc => {
                self.buf = st.saved.clone();
                self.cursor = st.saved_cursor;
                keep = false;
            }
            KeyCode::Enter => {
                keep = false;
                match st.found.and_then(|i| hist.get(i)) {
                    Some(line) => {
                        let line = line.to_string();
                        self.clear();
                        result = LineEvent::Submit(line);
                    }
                    None => {
                        self.buf = st.saved.clone();
                        self.cursor = st.saved_cursor;
                    }
                }
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End | KeyCode::Tab => {
                // Accept the match into the buffer and keep editing it.
                keep = false;
                if let Some(i) = st.found {
                    self.load_history(hist, i);
                } else {
                    self.buf = st.saved.clone();
                    self.cursor = st.saved_cursor;
                }
            }
            _ => {}
        }
        if keep {
            if let Some(i) = st.found {
                self.load_history(hist, i);
            } else if st.query.is_empty() {
                self.buf = st.saved.clone();
                self.cursor = st.saved_cursor;
            }
            self.search = Some(st);
        }
        result
    }

    // ---- completion -------------------------------------------------------

    /// Find the word under the cursor: `(start, quote, command_position)`.
    fn current_word(&self) -> (usize, Option<char>, bool) {
        let mut start = 0usize;
        let mut quote: Option<char> = None;
        let mut i = 0usize;
        let mut cmd_pos = true;
        let mut cur = String::new();
        let end = self.cursor;
        let mut in_word = false;
        while i < end {
            let c = self.buf[i];
            match quote {
                Some(q) => {
                    if c == q {
                        quote = None;
                    }
                    cur.push(c);
                }
                None => match c {
                    '\\' if i + 1 < end => {
                        if !in_word {
                            start = i;
                            in_word = true;
                        }
                        cur.push(self.buf[i + 1]);
                        i += 1;
                    }
                    '\'' | '"' => {
                        if !in_word {
                            start = i;
                            in_word = true;
                        }
                        quote = Some(c);
                    }
                    ' ' | '\t' => {
                        if in_word {
                            let prev_word = core::mem::take(&mut cur);
                            in_word = false;
                            cmd_pos = matches!(
                                prev_word.as_str(),
                                "then" | "do" | "else" | "elif" | "if" | "while" | "until" | "{"
                            );
                        }
                    }
                    '|' | ';' | '&' | '(' => {
                        if in_word {
                            in_word = false;
                            cur.clear();
                        }
                        cmd_pos = true;
                        start = i + 1;
                    }
                    _ => {
                        if !in_word {
                            start = i;
                            in_word = true;
                        }
                        cur.push(c);
                    }
                },
            }
            i += 1;
        }
        if !in_word {
            start = end;
        }
        (start, quote, cmd_pos)
    }

    fn unescape(raw: &[char]) -> String {
        let mut out = String::new();
        let mut i = 0;
        let mut quote: Option<char> = None;
        while i < raw.len() {
            let c = raw[i];
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => out.push(c),
                None => match c {
                    '\\' if i + 1 < raw.len() => {
                        i += 1;
                        out.push(raw[i]);
                    }
                    '\'' | '"' => quote = Some(c),
                    _ => out.push(c),
                },
            }
            i += 1;
        }
        out
    }

    fn escape(s: &str, first_is_word_start: bool) -> String {
        let mut out = String::new();
        for (i, c) in s.chars().enumerate() {
            let special = matches!(
                c,
                ' ' | '\t'
                    | '\''
                    | '"'
                    | '$'
                    | '*'
                    | '?'
                    | ';'
                    | '&'
                    | '|'
                    | '<'
                    | '>'
                    | '('
                    | ')'
                    | '\\'
                    | '#'
            ) || (c == '~' && i == 0 && first_is_word_start);
            if special {
                out.push('\\');
            }
            out.push(c);
        }
        out
    }

    fn complete(&mut self, comp: &dyn Completer) -> LineEvent {
        let (start, quote, cmd_pos) = self.current_word();
        let raw: Vec<char> = self.buf[start..self.cursor].to_vec();
        let word = Self::unescape(&raw);
        let mut cands: Vec<Completion>;
        let mut var_mode = false;
        if let Some(v) = word.strip_prefix('$') {
            var_mode = true;
            cands = comp.variables(v);
        } else if cmd_pos && !word.contains('/') {
            cands = comp.commands(&word);
        } else {
            cands = comp.paths(&word);
        }
        cands.sort_by(|a, b| a.text.cmp(&b.text));
        cands.dedup();
        if cands.is_empty() {
            return LineEvent::None;
        }
        let prefix_len = word.chars().count() - usize::from(var_mode);
        let texts: Vec<&str> = cands.iter().map(|c| c.text.as_str()).collect();
        let common = common_prefix(&texts);
        let replacement_plain = if cands.len() == 1 {
            cands[0].text.clone()
        } else {
            common
        };
        if cands.len() > 1 && replacement_plain.chars().count() <= prefix_len {
            let names = cands
                .iter()
                .map(|c| {
                    // Show the last component only.
                    let t = c.text.trim_end_matches('/');
                    let base = t.rsplit('/').next().unwrap_or(t);
                    if c.is_dir {
                        alloc::format!("{base}/")
                    } else {
                        base.to_string()
                    }
                })
                .collect();
            return LineEvent::Candidates(names);
        }
        let mut text = if var_mode {
            alloc::format!("${}", Self::escape(&replacement_plain, false))
        } else {
            match quote {
                Some(q) => alloc::format!("{q}{replacement_plain}"),
                None => Self::escape(&replacement_plain, true),
            }
        };
        if cands.len() == 1 {
            if let Some(q) = quote {
                text.push(q);
            }
            if !cands[0].is_dir {
                text.push(' ');
            }
        }
        let tail: Vec<char> = self.buf[self.cursor..].to_vec();
        self.buf.truncate(start);
        self.cursor = start;
        self.insert_str(&text);
        let new_cursor = self.cursor;
        for c in tail {
            if self.buf.len() < MAX_LINE {
                self.buf.push(c);
            }
        }
        self.cursor = new_cursor;
        LineEvent::Changed
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
