//! The "find in page" bar (Ctrl+F): the query being typed, the matches in the
//! laid-out page and the current one. Pure: the kernel only draws the bar and
//! the highlights and scrolls to [`FindBar::current_y`].

use super::layout::Page;
use super::metrics::TextMetrics;
use super::textops::Span;
use crate::Key;
use alloc::string::String;
use alloc::vec::Vec;

/// Longest query typed.
pub const MAX_QUERY: usize = 60;

/// What a key did to the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindOutcome {
    /// The bar is closed or the key means nothing to it.
    Ignored,
    /// The query or the current match changed (repaint, maybe scroll).
    Changed,
    /// Esc: the bar closed.
    Closed,
}

/// State of the find bar.
#[derive(Default)]
pub struct FindBar {
    open: bool,
    query: String,
    matches: Vec<Vec<Span>>,
    cur: usize,
}

impl FindBar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the bar (the old query is kept so Ctrl+F, Enter repeats a search).
    pub fn open(&mut self, page: Option<&Page>, m: &dyn TextMetrics) {
        self.open = true;
        self.refresh(page, m);
    }

    /// Hide the bar and forget the matches.
    pub fn close(&mut self) {
        self.open = false;
        self.matches.clear();
        self.cur = 0;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// Recompute the matches against `page` (after typing, a relayout, a new page),
    /// keeping the current match index when it still exists.
    pub fn refresh(&mut self, page: Option<&Page>, m: &dyn TextMetrics) {
        self.matches = match (self.open, page) {
            (true, Some(p)) => p.find(&self.query, m),
            _ => Vec::new(),
        };
        if self.cur >= self.matches.len() {
            self.cur = 0;
        }
    }

    /// Number of matches.
    pub fn count(&self) -> usize {
        self.matches.len()
    }

    /// 1-based index of the current match (0 when there is none).
    pub fn position(&self) -> usize {
        if self.matches.is_empty() {
            0
        } else {
            self.cur + 1
        }
    }

    /// The boxes of the current match.
    pub fn current_spans(&self) -> &[Span] {
        self.matches.get(self.cur).map_or(&[], Vec::as_slice)
    }

    /// The boxes of every other match.
    pub fn other_spans(&self) -> impl Iterator<Item = &Span> {
        let cur = self.cur;
        self.matches
            .iter()
            .enumerate()
            .filter(move |(i, _)| *i != cur)
            .flat_map(|(_, m)| m.iter())
    }

    /// Page-space `y` of the current match (to scroll to it).
    pub fn current_y(&self) -> Option<i32> {
        self.current_spans().first().map(|s| s.y)
    }

    /// Go to the next match (wraps).
    pub fn next(&mut self) {
        if !self.matches.is_empty() {
            self.cur = (self.cur + 1) % self.matches.len();
        }
    }

    /// Go to the previous match (wraps).
    pub fn prev(&mut self) {
        if !self.matches.is_empty() {
            self.cur = (self.cur + self.matches.len() - 1) % self.matches.len();
        }
    }

    /// Handle a key while the bar is open. `shift` turns Enter into "previous".
    pub fn on_key(
        &mut self,
        key: Key,
        shift: bool,
        page: Option<&Page>,
        m: &dyn TextMetrics,
    ) -> FindOutcome {
        if !self.open {
            return FindOutcome::Ignored;
        }
        match key {
            Key::Char(c) if (b' '..0x7f).contains(&c) => {
                if self.query.len() < MAX_QUERY {
                    self.query.push(c as char);
                    self.cur = 0;
                    self.refresh(page, m);
                }
                FindOutcome::Changed
            }
            Key::Backspace => {
                self.query.pop();
                self.cur = 0;
                self.refresh(page, m);
                FindOutcome::Changed
            }
            Key::Enter | Key::Down => {
                if shift {
                    self.prev();
                } else {
                    self.next();
                }
                FindOutcome::Changed
            }
            Key::Up => {
                self.prev();
                FindOutcome::Changed
            }
            Key::Esc => {
                self.close();
                FindOutcome::Closed
            }
            _ => FindOutcome::Ignored,
        }
    }
}

#[cfg(test)]
mod tests;
