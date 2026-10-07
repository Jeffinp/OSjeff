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
use crate::input::{KeyCode, KeyEvent};
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
            let head = alloc::format!("(reverse-i-search)`{}': ", s.query);
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
mod tests {
    use super::*;
    use crate::input::Mods;
    use crate::shell::fs::MemFs;
    use crate::shell::{Host, MockSys};

    fn key(
        e: &mut LineEditor,
        h: &History,
        c: &dyn Completer,
        code: KeyCode,
        m: Mods,
    ) -> LineEvent {
        e.handle_key(KeyEvent::new(code, m), h, c)
    }

    fn type_str(e: &mut LineEditor, s: &str) {
        let h = History::new();
        for c in s.chars() {
            key(e, &h, &NoCompleter, KeyCode::Char(c), Mods::NONE);
        }
    }

    fn press(e: &mut LineEditor, code: KeyCode) -> LineEvent {
        key(e, &History::new(), &NoCompleter, code, Mods::NONE)
    }

    fn ctrl(e: &mut LineEditor, c: char) -> LineEvent {
        key(
            e,
            &History::new(),
            &NoCompleter,
            KeyCode::Char(c),
            Mods::CTRL,
        )
    }

    #[test]
    fn typing_and_submit() {
        let mut e = LineEditor::new("$ ");
        type_str(&mut e, "ls -l");
        assert_eq!(e.text(), "ls -l");
        assert_eq!(e.cursor(), 5);
        assert_eq!(
            press(&mut e, KeyCode::Enter),
            LineEvent::Submit("ls -l".into())
        );
        assert!(e.is_empty());
    }

    #[test]
    fn insert_in_the_middle() {
        let mut e = LineEditor::new("");
        type_str(&mut e, "ac");
        press(&mut e, KeyCode::Left);
        type_str(&mut e, "b");
        assert_eq!(e.text(), "abc");
        assert_eq!(e.cursor(), 2);
    }

    #[test]
    fn backspace_and_delete() {
        let mut e = LineEditor::new("");
        type_str(&mut e, "abcd");
        press(&mut e, KeyCode::Backspace);
        assert_eq!(e.text(), "abc");
        press(&mut e, KeyCode::Home);
        press(&mut e, KeyCode::Delete);
        assert_eq!(e.text(), "bc");
        assert_eq!(press(&mut e, KeyCode::Backspace), LineEvent::None);
        press(&mut e, KeyCode::End);
        assert_eq!(press(&mut e, KeyCode::Delete), LineEvent::None);
    }

    #[test]
    fn home_end_and_ctrl_a_e() {
        let mut e = LineEditor::new("");
        type_str(&mut e, "hello");
        ctrl(&mut e, 'a');
        assert_eq!(e.cursor(), 0);
        ctrl(&mut e, 'e');
        assert_eq!(e.cursor(), 5);
        press(&mut e, KeyCode::Home);
        assert_eq!(e.cursor(), 0);
        press(&mut e, KeyCode::End);
        assert_eq!(e.cursor(), 5);
    }

    #[test]
    fn ctrl_b_f_move_by_one() {
        let mut e = LineEditor::new("");
        type_str(&mut e, "ab");
        ctrl(&mut e, 'b');
        assert_eq!(e.cursor(), 1);
        ctrl(&mut e, 'f');
        ctrl(&mut e, 'f');
        assert_eq!(e.cursor(), 2);
    }

    #[test]
    fn kill_and_yank() {
        let mut e = LineEditor::new("");
        type_str(&mut e, "one two three");
        ctrl(&mut e, 'w');
        assert_eq!(e.text(), "one two ");
        ctrl(&mut e, 'u');
        assert_eq!(e.text(), "");
        ctrl(&mut e, 'y');
        assert_eq!(e.text(), "one two ");
        press(&mut e, KeyCode::Home);
        for _ in 0..3 {
            press(&mut e, KeyCode::Right);
        }
        ctrl(&mut e, 'k');
        assert_eq!(e.text(), "one");
        ctrl(&mut e, 'y');
        assert_eq!(e.text(), "one two ");
    }

    #[test]
    fn word_movement_with_ctrl_arrows() {
        let mut e = LineEditor::new("");
        type_str(&mut e, "foo bar baz");
        key(
            &mut e,
            &History::new(),
            &NoCompleter,
            KeyCode::Left,
            Mods::CTRL,
        );
        assert_eq!(e.cursor(), 8);
        key(
            &mut e,
            &History::new(),
            &NoCompleter,
            KeyCode::Left,
            Mods::CTRL,
        );
        assert_eq!(e.cursor(), 4);
        key(
            &mut e,
            &History::new(),
            &NoCompleter,
            KeyCode::Right,
            Mods::CTRL,
        );
        assert_eq!(e.cursor(), 7);
    }

    #[test]
    fn ctrl_d_semantics() {
        let mut e = LineEditor::new("");
        assert_eq!(ctrl(&mut e, 'd'), LineEvent::Eof);
        type_str(&mut e, "ab");
        press(&mut e, KeyCode::Home);
        assert_eq!(ctrl(&mut e, 'd'), LineEvent::Changed);
        assert_eq!(e.text(), "b");
    }

    #[test]
    fn ctrl_c_and_ctrl_l() {
        let mut e = LineEditor::new("");
        type_str(&mut e, "abc");
        assert_eq!(ctrl(&mut e, 'l'), LineEvent::ClearScreen);
        assert_eq!(e.text(), "abc");
        assert_eq!(ctrl(&mut e, 'c'), LineEvent::Interrupt);
        assert!(e.is_empty());
    }

    #[test]
    fn unicode_editing() {
        let mut e = LineEditor::new("");
        type_str(&mut e, "ação€");
        assert_eq!(e.cursor(), 5);
        press(&mut e, KeyCode::Backspace);
        press(&mut e, KeyCode::Backspace);
        assert_eq!(e.text(), "açã");
    }

    #[test]
    fn control_characters_are_not_inserted() {
        let mut e = LineEditor::new("");
        type_str(&mut e, "a\u{7}b\n");
        assert_eq!(e.text(), "ab");
    }

    #[test]
    fn line_length_is_capped() {
        let mut e = LineEditor::new("");
        for _ in 0..MAX_LINE + 50 {
            e.insert('x');
        }
        assert_eq!(e.text().len(), MAX_LINE);
    }

    #[test]
    fn display_shows_prompt_and_cursor_column() {
        let mut e = LineEditor::new("user@osjeff:/$ ");
        type_str(&mut e, "ls");
        let (t, col) = e.display();
        assert_eq!(t, "user@osjeff:/$ ls");
        assert_eq!(col, 17);
        press(&mut e, KeyCode::Left);
        assert_eq!(e.display().1, 16);
        e.set_prompt("> ");
        assert_eq!(e.display().0, "> ls");
    }

    fn hist(items: &[&str]) -> History {
        let mut h = History::new();
        for i in items {
            h.add(i);
        }
        h
    }

    #[test]
    fn history_up_down_with_draft() {
        let h = hist(&["first", "second", "third"]);
        let mut e = LineEditor::new("");
        type_str(&mut e, "dra");
        key(&mut e, &h, &NoCompleter, KeyCode::Up, Mods::NONE);
        assert_eq!(e.text(), "third");
        key(&mut e, &h, &NoCompleter, KeyCode::Up, Mods::NONE);
        assert_eq!(e.text(), "second");
        key(&mut e, &h, &NoCompleter, KeyCode::Up, Mods::NONE);
        key(&mut e, &h, &NoCompleter, KeyCode::Up, Mods::NONE);
        assert_eq!(e.text(), "first");
        key(&mut e, &h, &NoCompleter, KeyCode::Down, Mods::NONE);
        assert_eq!(e.text(), "second");
        key(&mut e, &h, &NoCompleter, KeyCode::Down, Mods::NONE);
        key(&mut e, &h, &NoCompleter, KeyCode::Down, Mods::NONE);
        assert_eq!(e.text(), "dra");
        assert_eq!(
            key(&mut e, &h, &NoCompleter, KeyCode::Down, Mods::NONE),
            LineEvent::None
        );
    }

    #[test]
    fn history_with_empty_history_does_nothing() {
        let mut e = LineEditor::new("");
        assert_eq!(press(&mut e, KeyCode::Up), LineEvent::None);
    }

    #[test]
    fn history_entry_can_be_edited_and_submitted() {
        let h = hist(&["echo a"]);
        let mut e = LineEditor::new("");
        key(&mut e, &h, &NoCompleter, KeyCode::Up, Mods::NONE);
        type_str(&mut e, "b");
        assert_eq!(
            key(&mut e, &h, &NoCompleter, KeyCode::Enter, Mods::NONE),
            LineEvent::Submit("echo ab".into())
        );
    }

    #[test]
    fn ctrl_p_n_navigate_history() {
        let h = hist(&["x", "y"]);
        let mut e = LineEditor::new("");
        key(&mut e, &h, &NoCompleter, KeyCode::Char('p'), Mods::CTRL);
        assert_eq!(e.text(), "y");
        key(&mut e, &h, &NoCompleter, KeyCode::Char('n'), Mods::CTRL);
        assert_eq!(e.text(), "");
    }

    #[test]
    fn reverse_search_finds_and_cycles() {
        let h = hist(&["echo one", "ls", "echo two", "pwd"]);
        let mut e = LineEditor::new("$ ");
        let k = |e: &mut LineEditor, c: KeyCode, m: Mods| key(e, &h, &NoCompleter, c, m);
        k(&mut e, KeyCode::Char('r'), Mods::CTRL);
        assert!(e.is_searching());
        for c in "echo".chars() {
            k(&mut e, KeyCode::Char(c), Mods::NONE);
        }
        assert_eq!(e.text(), "echo two");
        assert!(
            e.display()
                .0
                .starts_with("(reverse-i-search)`echo': echo two")
        );
        k(&mut e, KeyCode::Char('r'), Mods::CTRL);
        assert_eq!(e.text(), "echo one");
        k(&mut e, KeyCode::Char('r'), Mods::CTRL);
        assert_eq!(e.text(), "echo one");
        assert_eq!(
            k(&mut e, KeyCode::Enter, Mods::NONE),
            LineEvent::Submit("echo one".into())
        );
        assert!(!e.is_searching());
    }

    #[test]
    fn reverse_search_cancel_restores_the_line() {
        let h = hist(&["alpha", "beta"]);
        let mut e = LineEditor::new("");
        type_str(&mut e, "draft");
        let k = |e: &mut LineEditor, c: KeyCode, m: Mods| key(e, &h, &NoCompleter, c, m);
        k(&mut e, KeyCode::Char('r'), Mods::CTRL);
        k(&mut e, KeyCode::Char('a'), Mods::NONE);
        assert_eq!(e.text(), "beta");
        k(&mut e, KeyCode::Esc, Mods::NONE);
        assert_eq!(e.text(), "draft");
        assert!(!e.is_searching());
        k(&mut e, KeyCode::Char('r'), Mods::CTRL);
        k(&mut e, KeyCode::Char('g'), Mods::CTRL);
        assert_eq!(e.text(), "draft");
    }

    #[test]
    fn reverse_search_accept_with_arrow_keeps_editing() {
        let h = hist(&["make all"]);
        let mut e = LineEditor::new("");
        let k = |e: &mut LineEditor, c: KeyCode, m: Mods| key(e, &h, &NoCompleter, c, m);
        k(&mut e, KeyCode::Char('r'), Mods::CTRL);
        k(&mut e, KeyCode::Char('m'), Mods::NONE);
        k(&mut e, KeyCode::Left, Mods::NONE);
        assert!(!e.is_searching());
        assert_eq!(e.text(), "make all");
        k(&mut e, KeyCode::Char('!'), Mods::NONE);
        assert_eq!(e.text(), "make all!");
    }

    #[test]
    fn reverse_search_backspace_and_no_match() {
        let h = hist(&["foo", "bar"]);
        let mut e = LineEditor::new("");
        let k = |e: &mut LineEditor, c: KeyCode, m: Mods| key(e, &h, &NoCompleter, c, m);
        k(&mut e, KeyCode::Char('r'), Mods::CTRL);
        k(&mut e, KeyCode::Char('f'), Mods::NONE);
        assert_eq!(e.text(), "foo");
        k(&mut e, KeyCode::Char('z'), Mods::NONE);
        k(&mut e, KeyCode::Backspace, Mods::NONE);
        assert_eq!(e.text(), "foo");
        k(&mut e, KeyCode::Backspace, Mods::NONE);
        assert_eq!(e.text(), "");
        k(&mut e, KeyCode::Char('q'), Mods::NONE);
        assert_eq!(k(&mut e, KeyCode::Enter, Mods::NONE), LineEvent::Changed);
        assert!(!e.is_searching());
    }

    #[test]
    fn ctrl_c_in_search_interrupts() {
        let h = hist(&["a"]);
        let mut e = LineEditor::new("");
        let k = |e: &mut LineEditor, c: KeyCode, m: Mods| key(e, &h, &NoCompleter, c, m);
        k(&mut e, KeyCode::Char('r'), Mods::CTRL);
        assert_eq!(
            k(&mut e, KeyCode::Char('c'), Mods::CTRL),
            LineEvent::Interrupt
        );
        assert!(!e.is_searching());
    }

    // ---- completion -------------------------------------------------------------

    struct World {
        sh: Shell,
        fs: MemFs,
        sys: MockSys,
        ed: LineEditor,
    }

    impl World {
        fn new() -> Self {
            let fs = MemFs::new()
                .with_file("/notes.txt", b"")
                .with_file("/note2.txt", b"")
                .with_file("/.hidden", b"")
                .with_dir("/docs")
                .with_dir("/docs/sub")
                .with_file("/docs/readme.md", b"")
                .with_file("/docs/my file.txt", b"")
                .with_dir("/bin")
                .with_file("/bin/hello.sh", b"echo hi\n");
            Self {
                sh: Shell::new(),
                fs,
                sys: MockSys::default(),
                ed: LineEditor::new("$ "),
            }
        }

        fn tab(&mut self, typed: &str) -> LineEvent {
            self.ed.set_text(typed);
            let comp = ShellCompleter {
                shell: &self.sh,
                fs: &self.fs,
            };
            self.ed
                .handle_key(KeyEvent::plain(KeyCode::Tab), &History::new(), &comp)
        }
    }

    #[test]
    fn complete_unique_command() {
        let mut w = World::new();
        assert_eq!(w.tab("whi"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "which ");
        assert_eq!(w.tab("unal"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "unalias ");
    }

    #[test]
    fn complete_command_common_prefix_then_list() {
        let mut w = World::new();
        // "hi" matches only `history`; "he" matches help/head.
        assert_eq!(w.tab("his"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "history ");
        match w.tab("he") {
            LineEvent::Candidates(c) => assert_eq!(c, ["head", "hello", "help"]),
            e => panic!("{e:?}"),
        }
        assert_eq!(w.ed.text(), "he");
        assert_eq!(w.tab("cl"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "clear ");
    }

    #[test]
    fn complete_extends_to_common_prefix() {
        let mut w = World::new();
        assert_eq!(w.tab("mk"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "mkdir ");
        assert_eq!(w.tab("tou"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "touch ");
        // "ec" is unique too; use a real shared prefix: "un" -> uniq/unset/unalias share only "un".
        match w.tab("un") {
            LineEvent::Candidates(c) => assert_eq!(c, ["unalias", "uniq", "unset"]),
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn complete_script_from_path_without_extension() {
        let mut w = World::new();
        assert_eq!(w.tab("hell"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "hello ");
    }

    #[test]
    fn complete_aliases_and_functions() {
        let mut w = World::new();
        {
            let mut h = Host {
                fs: &mut w.fs,
                sys: &mut w.sys,
            };
            w.sh.run_line("alias zzalias='echo a'", &mut h);
            w.sh.run_line("zzfunc() { :; }", &mut h);
        }
        assert_eq!(w.tab("zza"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "zzalias ");
        assert_eq!(w.tab("zzf"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "zzfunc ");
    }

    #[test]
    fn complete_paths_files_and_dirs() {
        let mut w = World::new();
        assert_eq!(w.tab("cat not"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "cat note");
        match w.tab("cat note") {
            LineEvent::Candidates(c) => assert_eq!(c, ["note2.txt", "notes.txt"]),
            e => panic!("{e:?}"),
        }
        assert_eq!(w.tab("cd do"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "cd docs/");
        assert_eq!(w.tab("cat docs/re"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "cat docs/readme.md ");
        assert_eq!(w.tab("ls /do"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "ls /docs/");
    }

    #[test]
    fn complete_hides_dotfiles_unless_asked() {
        let mut w = World::new();
        assert_eq!(w.tab("cat ."), LineEvent::Changed);
        assert_eq!(w.ed.text(), "cat .hidden ");
        match w.tab("cat ") {
            LineEvent::Candidates(c) => assert!(!c.iter().any(|n| n.starts_with('.'))),
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn complete_escapes_spaces() {
        let mut w = World::new();
        assert_eq!(w.tab("cat docs/my"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "cat docs/my\\ file.txt ");
    }

    #[test]
    fn complete_inside_quotes() {
        let mut w = World::new();
        assert_eq!(w.tab("cat \"docs/my"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "cat \"docs/my file.txt\" ");
    }

    #[test]
    fn complete_command_position_after_operators() {
        let mut w = World::new();
        assert_eq!(w.tab("echo hi | whi"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "echo hi | which ");
        assert_eq!(w.tab("true && unal"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "true && unalias ");
        assert_eq!(w.tab("echo a; whi"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "echo a; which ");
        assert_eq!(w.tab("if whi"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "if which ");
    }

    #[test]
    fn complete_argument_position_uses_paths_not_commands() {
        let mut w = World::new();
        assert_eq!(w.tab("echo whi"), LineEvent::None);
        assert_eq!(w.ed.text(), "echo whi");
    }

    #[test]
    fn complete_variables() {
        let mut w = World::new();
        assert_eq!(w.tab("echo $HO"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "echo $HOME ");
        assert_eq!(w.tab("echo $PA"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "echo $PATH ");
    }

    #[test]
    fn complete_in_the_middle_of_a_line_keeps_the_tail() {
        let mut w = World::new();
        w.ed.set_text("cat not rest");
        for _ in 0..5 {
            w.ed.handle_key(
                KeyEvent::plain(KeyCode::Left),
                &History::new(),
                &NoCompleter,
            );
        }
        let comp = ShellCompleter {
            shell: &w.sh,
            fs: &w.fs,
        };
        w.ed.handle_key(KeyEvent::plain(KeyCode::Tab), &History::new(), &comp);
        assert_eq!(w.ed.text(), "cat note rest");
        assert_eq!(w.ed.cursor(), "cat note".len());
    }

    #[test]
    fn complete_nothing_is_a_noop() {
        let mut w = World::new();
        assert_eq!(w.tab("cat zzz"), LineEvent::None);
        assert_eq!(w.ed.text(), "cat zzz");
        assert_eq!(w.tab("zzzz"), LineEvent::None);
    }

    #[test]
    fn complete_relative_to_cwd() {
        let mut w = World::new();
        w.fs.set_cwd("/docs").unwrap();
        assert_eq!(w.tab("cat re"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "cat readme.md ");
        assert_eq!(w.tab("cd ../no"), LineEvent::Changed);
        assert_eq!(w.ed.text(), "cd ../note");
    }

    #[test]
    fn full_session_through_the_shell() {
        let mut w = World::new();
        let mut submitted = None;
        let typed = "echo hi";
        for c in typed.chars() {
            w.ed.handle_key(KeyEvent::ch(c), w.sh.history(), &NoCompleter);
        }
        if let LineEvent::Submit(line) = w.ed.handle_key(
            KeyEvent::plain(KeyCode::Enter),
            w.sh.history(),
            &NoCompleter,
        ) {
            let mut h = Host {
                fs: &mut w.fs,
                sys: &mut w.sys,
            };
            submitted = Some(w.sh.run_line(&line, &mut h));
        }
        assert_eq!(submitted.unwrap().text(), "hi\n");
        // History now has the line: Up recalls it.
        w.ed.handle_key(KeyEvent::plain(KeyCode::Up), w.sh.history(), &NoCompleter);
        assert_eq!(w.ed.text(), "echo hi");
    }

    #[test]
    fn common_prefix_helper() {
        assert_eq!(common_prefix(&["abc", "abd", "ab"]), "ab");
        assert_eq!(common_prefix(&["x"]), "x");
        assert_eq!(common_prefix(&[]), "");
        assert_eq!(common_prefix(&["é1", "é2"]), "é");
    }

    #[test]
    fn random_keys_never_panic() {
        let codes = [
            KeyCode::Char('a'),
            KeyCode::Char(' '),
            KeyCode::Char('\''),
            KeyCode::Char('"'),
            KeyCode::Char('$'),
            KeyCode::Char('/'),
            KeyCode::Char('|'),
            KeyCode::Char('r'),
            KeyCode::Char('g'),
            KeyCode::Enter,
            KeyCode::Backspace,
            KeyCode::Delete,
            KeyCode::Tab,
            KeyCode::Esc,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::PageUp,
            KeyCode::F(3),
        ];
        let mut w = World::new();
        let mut hist = History::new();
        for c in ["echo a", "ls /docs", "cat notes.txt"] {
            hist.add(c);
        }
        let mut x: u64 = 31337;
        for _ in 0..5000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let code = codes[(x % codes.len() as u64) as usize];
            let m = Mods {
                ctrl: (x >> 8) & 3 == 0,
                shift: (x >> 10) & 3 == 0,
                alt: (x >> 12) & 7 == 0,
            };
            let comp = ShellCompleter {
                shell: &w.sh,
                fs: &w.fs,
            };
            let _ = w.ed.handle_key(KeyEvent::new(code, m), &hist, &comp);
            assert!(w.ed.cursor() <= w.ed.text().chars().count());
            let (_, col) = w.ed.display();
            let _ = col;
        }
    }
}
