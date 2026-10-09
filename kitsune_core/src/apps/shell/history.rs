//! Command history: bounded, de-duplicated, persistable as plain text.

use alloc::string::String;
use alloc::vec::Vec;

/// Default maximum number of remembered commands.
pub const DEFAULT_MAX: usize = 500;

/// The list of commands typed so far, oldest first.
#[derive(Clone, Debug)]
pub struct History {
    entries: Vec<String>,
    max: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    pub fn new() -> Self {
        Self::with_max(DEFAULT_MAX)
    }

    pub fn with_max(max: usize) -> Self {
        Self {
            entries: Vec::new(),
            max: max.max(1),
        }
    }

    /// Remember `line`. Empty lines, lines starting with a space and a
    /// repeat of the previous entry are ignored. Returns whether it was
    /// added.
    pub fn add(&mut self, line: &str) -> bool {
        let t = line.trim_end();
        if t.trim().is_empty() || t.starts_with(' ') || t.contains('\n') {
            return false;
        }
        if self.entries.last().is_some_and(|l| l == t) {
            return false;
        }
        self.entries.push(String::from(t));
        if self.entries.len() > self.max {
            let drop = self.entries.len() - self.max;
            self.entries.drain(..drop);
        }
        true
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&str> {
        self.entries.get(i).map(String::as_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(String::as_str)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Index of the newest entry before `before` that contains `query`
    /// (reverse incremental search). An empty query matches nothing.
    pub fn search_rev(&self, query: &str, before: usize) -> Option<usize> {
        if query.is_empty() {
            return None;
        }
        let end = before.min(self.entries.len());
        (0..end).rev().find(|&i| self.entries[i].contains(query))
    }

    /// Serialize as one command per line (for saving to a file).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for e in &self.entries {
            out.extend_from_slice(e.as_bytes());
            out.push(b'\n');
        }
        out
    }

    /// Load from [`History::to_bytes`] output, keeping the newest `max`
    /// entries. Invalid UTF-8 is replaced, never a panic.
    pub fn load(&mut self, data: &[u8]) {
        for line in String::from_utf8_lossy(data).lines() {
            self.add(line);
        }
    }
}

#[cfg(test)]
mod tests;
