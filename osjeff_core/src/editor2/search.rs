//! Find / replace / go-to-line, including the prompt state machine the key
//! handler drives.
//!
//! Matching works on bytes over the (made contiguous) buffer, so a 2 MB file is
//! searched at memory speed. Case-insensitive matching folds ASCII letters
//! byte-wise; when the needle contains non-ASCII characters it compares each
//! character by its simple lower-case mapping instead.

use super::Editor;
use super::buffer::{decode, is_cont};
use super::undo::Kind;
use alloc::string::String;
use alloc::vec::Vec;

/// Feedback for the status line after a search command.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Notice {
    None,
    /// No match for the query.
    NotFound,
    /// The search wrapped past the end (or start) of the file.
    Wrapped,
    /// `n` replacements were made.
    Replaced(usize),
    /// Go-to-line got something that is not a line number.
    InvalidLine,
}

/// Which prompt is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromptKind {
    Find,
    Replace,
    Goto,
}

#[derive(Default)]
pub(crate) struct Search {
    pub(crate) query: String,
    pub(crate) repl: String,
}

pub(crate) struct Prompt {
    pub(crate) kind: PromptKind,
    pub(crate) fields: [String; 2],
    pub(crate) active: usize,
    /// Where incremental search starts from.
    pub(crate) origin: usize,
}

/// What to draw for an open prompt.
#[derive(Clone, Copy, Debug)]
pub struct PromptView<'a> {
    pub kind: PromptKind,
    pub label: &'static str,
    /// The main field (search text or line number).
    pub text: &'a str,
    /// The replacement field (replace prompt only).
    pub text2: Option<&'a str>,
    /// 0 = main field, 1 = replacement field.
    pub active: usize,
    pub case_sensitive: bool,
    pub notice: Notice,
}

enum Mode {
    Exact,
    AsciiFold,
    UniFold(Vec<char>),
}

struct Matcher {
    needle: Vec<u8>,
    mode: Mode,
}

fn fold(c: char) -> char {
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

impl Matcher {
    fn new(query: &str, case_sensitive: bool) -> Option<Self> {
        if query.is_empty() {
            return None;
        }
        let (needle, mode) = if case_sensitive {
            (query.as_bytes().to_vec(), Mode::Exact)
        } else if query.is_ascii() {
            (query.to_ascii_lowercase().into_bytes(), Mode::AsciiFold)
        } else {
            (
                query.as_bytes().to_vec(),
                Mode::UniFold(query.chars().map(fold).collect()),
            )
        };
        Some(Self { needle, mode })
    }

    /// End of a match starting exactly at `s`.
    fn at(&self, hay: &[u8], s: usize) -> Option<usize> {
        match &self.mode {
            Mode::Exact => {
                let e = s.checked_add(self.needle.len())?;
                (hay.get(s..e)? == self.needle.as_slice()).then_some(e)
            }
            Mode::AsciiFold => {
                let e = s.checked_add(self.needle.len())?;
                let w = hay.get(s..e)?;
                w.iter()
                    .zip(&self.needle)
                    .all(|(a, b)| a.to_ascii_lowercase() == *b)
                    .then_some(e)
            }
            Mode::UniFold(chars) => {
                let mut p = s;
                for &nc in chars {
                    if p >= hay.len() {
                        return None;
                    }
                    let (c, n) = decode(&hay[p..hay.len().min(p + 4)]);
                    if fold(c) != nc {
                        return None;
                    }
                    p += n;
                }
                Some(p)
            }
        }
    }

    /// Is `s` a place a match may start (a character boundary)?
    fn can_start(&self, hay: &[u8], s: usize) -> bool {
        !matches!(self.mode, Mode::UniFold(_)) || hay.get(s).is_some_and(|&b| !is_cont(b))
    }

    /// First match whose start is in `lo..hi`.
    fn find(&self, hay: &[u8], lo: usize, hi: usize) -> Option<(usize, usize)> {
        let hi = hi.min(hay.len());
        let mut i = lo;
        let n0 = self.needle[0];
        while i < hi {
            // Skip to the next candidate first byte.
            let rest = &hay[i..hi];
            let off = match self.mode {
                Mode::Exact => rest.iter().position(|&b| b == n0)?,
                Mode::AsciiFold => rest.iter().position(|&b| b.to_ascii_lowercase() == n0)?,
                Mode::UniFold(_) => 0,
            };
            i += off;
            if self.can_start(hay, i)
                && let Some(e) = self.at(hay, i)
            {
                return Some((i, e));
            }
            i += 1;
        }
        None
    }

    /// Last match that starts at `>= lo` and ends at `<= end_max`.
    fn rfind(&self, hay: &[u8], lo: usize, end_max: usize) -> Option<(usize, usize)> {
        let end_max = end_max.min(hay.len());
        let mut s = end_max;
        let n0 = self.needle[0];
        loop {
            if s < lo {
                return None;
            }
            let hit = s < hay.len()
                && match self.mode {
                    Mode::Exact => hay[s] == n0,
                    Mode::AsciiFold => hay[s].to_ascii_lowercase() == n0,
                    Mode::UniFold(_) => true,
                }
                && self.can_start(hay, s);
            if hit
                && let Some(e) = self.at(hay, s)
                && e <= end_max
            {
                return Some((s, e));
            }
            if s == 0 {
                return None;
            }
            s -= 1;
        }
    }
}

impl Editor {
    // ---- programmatic search API ------------------------------------------

    /// Set the search text (and case mode).
    pub fn set_search(&mut self, query: &str, case_sensitive: bool) {
        self.srch.query.clear();
        self.srch.query.push_str(query);
        self.cfg.case_sensitive = case_sensitive;
    }

    pub fn search_query(&self) -> &str {
        &self.srch.query
    }

    pub fn set_replacement(&mut self, repl: &str) {
        self.srch.repl.clear();
        self.srch.repl.push_str(repl);
    }

    /// Feedback of the last search/replace command.
    pub fn notice(&self) -> Notice {
        self.notice
    }

    fn matcher(&self) -> Option<Matcher> {
        Matcher::new(&self.srch.query, self.cfg.case_sensitive)
    }

    fn select_match(&mut self, s: usize, e: usize) {
        self.hist.break_group();
        self.anchor = Some(s);
        self.cursor = e;
        self.pref_dc = self.text.dc_of(e, self.cfg.tab_width);
        self.ensure_visible();
    }

    fn find_from(&mut self, from: usize, forward: bool) -> bool {
        let Some(m) = self.matcher() else {
            self.notice = Notice::None;
            return false;
        };
        let hay = self.text.make_contiguous();
        let len = hay.len();
        let (hit, wrapped) = if forward {
            match m.find(hay, from, len) {
                Some(r) => (Some(r), false),
                None => (m.find(hay, 0, len), true),
            }
        } else {
            match m.rfind(hay, 0, from) {
                Some(r) => (Some(r), false),
                None => (m.rfind(hay, 0, len), true),
            }
        };
        match hit {
            Some((s, e)) => {
                self.select_match(s, e);
                self.notice = if wrapped {
                    Notice::Wrapped
                } else {
                    Notice::None
                };
                true
            }
            None => {
                self.notice = Notice::NotFound;
                false
            }
        }
    }

    /// Select the next match after the selection/cursor, wrapping around.
    pub fn find_next(&mut self) -> bool {
        let from = self.sel_range().map_or(self.cursor, |r| r.1);
        self.find_from(from, true)
    }

    /// Select the previous match before the selection/cursor, wrapping around.
    pub fn find_prev(&mut self) -> bool {
        let from = self.sel_range().map_or(self.cursor, |r| r.0);
        self.find_from(from, false)
    }

    /// Number of (non-overlapping) matches in the whole buffer.
    pub fn count_matches(&mut self) -> usize {
        let Some(m) = self.matcher() else { return 0 };
        let hay = self.text.make_contiguous();
        let mut n = 0;
        let mut p = 0;
        while let Some((_, e)) = m.find(hay, p, hay.len()) {
            n += 1;
            p = e;
        }
        n
    }

    fn selection_is_match(&mut self) -> bool {
        let Some((a, b)) = self.sel_range() else {
            return false;
        };
        let Some(m) = self.matcher() else {
            return false;
        };
        let hay = self.text.make_contiguous();
        m.can_start(hay, a) && m.at(hay, a) == Some(b)
    }

    /// Replace the selected match (if the selection is one) and move to the
    /// next match. Returns whether a replacement happened.
    pub fn replace_current(&mut self) -> bool {
        if !self.selection_is_match() {
            self.find_next();
            return false;
        }
        let repl = self.srch.repl.clone().into_bytes();
        self.edit_cmd(Kind::Other, false, |e| {
            e.delete_selection_raw();
            e.raw_insert_at_cursor(&repl);
        });
        self.notice = Notice::Replaced(1);
        let wrapped_before = self.notice;
        self.find_next();
        if self.notice == Notice::NotFound {
            self.notice = wrapped_before;
        }
        true
    }

    /// Replace every match in one undoable step. Returns the count.
    pub fn replace_all(&mut self) -> usize {
        let Some(m) = self.matcher() else { return 0 };
        let repl = self.srch.repl.clone().into_bytes();
        let hay = self.text.make_contiguous();
        let mut hits: Vec<(usize, usize)> = Vec::new();
        let mut p = 0;
        while let Some((s, e)) = m.find(hay, p, hay.len()) {
            hits.push((s, e));
            p = e;
        }
        let (Some(&(first, _)), Some(&(_, last))) = (hits.first(), hits.last()) else {
            self.notice = Notice::NotFound;
            return 0;
        };
        let mut mid: Vec<u8> = Vec::with_capacity(last - first);
        let mut p = first;
        for &(s, e) in &hits {
            mid.extend_from_slice(&hay[p..s]);
            mid.extend_from_slice(&repl);
            p = e;
        }
        let n = hits.len();
        let keep = self.cursor;
        self.edit_cmd(Kind::Other, false, |e| {
            e.raw_edit(first, last - first, &mid);
            e.cursor = keep.min(e.text.len());
            e.anchor = None;
        });
        self.notice = Notice::Replaced(n);
        n
    }

    // ---- prompts ---------------------------------------------------------

    /// The open prompt, for drawing.
    pub fn prompt(&self) -> Option<PromptView<'_>> {
        let p = self.prompt.as_ref()?;
        Some(PromptView {
            kind: p.kind,
            label: match p.kind {
                PromptKind::Find => crate::t!("edit.find.search"),
                PromptKind::Replace => {
                    if p.active == 0 {
                        crate::t!("edit.find.search")
                    } else {
                        crate::t!("edit.find.replace_with")
                    }
                }
                PromptKind::Goto => crate::t!("edit.find.goto"),
            },
            text: &p.fields[0],
            text2: (p.kind == PromptKind::Replace).then_some(p.fields[1].as_str()),
            active: p.active,
            case_sensitive: self.cfg.case_sensitive,
            notice: self.notice,
        })
    }

    pub fn is_prompt_open(&self) -> bool {
        self.prompt.is_some()
    }

    /// Open the find prompt (Ctrl+F), pre-filled with a short one-line
    /// selection.
    pub fn open_find(&mut self) {
        self.open_search_prompt(PromptKind::Find);
    }

    /// Open the find-and-replace prompt (Ctrl+H).
    pub fn open_replace(&mut self) {
        self.open_search_prompt(PromptKind::Replace);
    }

    fn open_search_prompt(&mut self, kind: PromptKind) {
        let mut q = self.srch.query.clone();
        if let Some((a, b)) = self.sel_range() {
            let sel = self.text.copy_range(a, b);
            if sel.len() <= 64
                && !sel.contains(&b'\n')
                && let Ok(s) = core::str::from_utf8(&sel)
            {
                q = String::from(s);
            }
        }
        self.srch.query = q.clone();
        let origin = self.sel_range().map_or(self.cursor, |r| r.0);
        self.prompt = Some(Prompt {
            kind,
            fields: [q, self.srch.repl.clone()],
            active: 0,
            origin,
        });
        self.notice = Notice::None;
    }

    /// Open the go-to-line prompt (Ctrl+G).
    pub fn open_goto(&mut self) {
        self.prompt = Some(Prompt {
            kind: PromptKind::Goto,
            fields: [String::new(), String::new()],
            active: 0,
            origin: self.cursor,
        });
        self.notice = Notice::None;
    }

    pub fn close_prompt(&mut self) {
        self.prompt = None;
    }

    /// A character typed into the prompt.
    pub(crate) fn prompt_type(&mut self, c: char) {
        let Some(p) = self.prompt.as_mut() else {
            return;
        };
        if c.is_control() {
            return;
        }
        let f = &mut p.fields[p.active];
        match p.kind {
            PromptKind::Goto => {
                if c.is_ascii_digit() && f.len() < 9 {
                    f.push(c);
                }
            }
            _ => {
                if f.len() < 256 {
                    f.push(c);
                }
            }
        }
        self.prompt_changed();
    }

    pub(crate) fn prompt_backspace(&mut self) {
        let Some(p) = self.prompt.as_mut() else {
            return;
        };
        p.fields[p.active].pop();
        self.prompt_changed();
    }

    pub(crate) fn prompt_clear_field(&mut self) {
        let Some(p) = self.prompt.as_mut() else {
            return;
        };
        p.fields[p.active].clear();
        self.prompt_changed();
    }

    fn prompt_changed(&mut self) {
        let Some(p) = self.prompt.as_ref() else {
            return;
        };
        match p.kind {
            PromptKind::Goto => {}
            _ => {
                self.srch.query = p.fields[0].clone();
                self.srch.repl = p.fields[1].clone();
                if p.active == 0 {
                    // Incremental: stay on/near the match as the query grows.
                    let origin = p.origin;
                    self.find_from(origin, true);
                }
            }
        }
    }

    /// Put the focus on field `i` of the open prompt (0 = search, 1 = replacement): a click on
    /// a field of the bar. The replacement field exists only in the replace prompt.
    pub fn prompt_focus(&mut self, i: usize) {
        if let Some(p) = self.prompt.as_mut()
            && (i == 0 || (i == 1 && p.kind == PromptKind::Replace))
        {
            p.active = i;
        }
    }

    pub(crate) fn prompt_switch_field(&mut self) {
        if let Some(p) = self.prompt.as_mut()
            && p.kind == PromptKind::Replace
        {
            p.active = 1 - p.active;
        }
    }

    /// Enter in the prompt. Returns true when the prompt should close.
    pub(crate) fn prompt_enter(&mut self, shift: bool) -> bool {
        let Some(p) = self.prompt.as_ref() else {
            return true;
        };
        match p.kind {
            PromptKind::Goto => {
                let txt = p.fields[0].clone();
                match txt.parse::<usize>() {
                    Ok(n) if n > 0 => {
                        self.goto_line(n);
                        true
                    }
                    _ => {
                        self.notice = Notice::InvalidLine;
                        false
                    }
                }
            }
            PromptKind::Find => {
                if shift {
                    self.find_prev();
                } else {
                    self.find_next();
                }
                false
            }
            PromptKind::Replace => {
                if p.active == 0 {
                    self.find_next();
                } else {
                    self.replace_current();
                }
                false
            }
        }
    }
}
