//! Undo/redo history made of *groups* of byte-level edits.
//!
//! Every edit stores exactly what it removed and inserted, so undo is the exact
//! inverse and the history is lossless. Consecutive typing, backspacing and
//! forward deleting are coalesced into one group (see [`Kind`]) so that Ctrl+Z
//! undoes a burst of typing at once instead of one character.

use alloc::collections::VecDeque;
use alloc::vec::Vec;

/// What a group is made of; decides whether the next edit may join it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Never merged with its neighbours.
    Other,
    /// Typed characters.
    Typing,
    /// Backspace.
    Backspacing,
    /// Forward delete.
    Deleting,
}

/// One primitive change: at `pos`, `removed` was replaced by `inserted`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub pos: usize,
    pub removed: Vec<u8>,
    pub inserted: Vec<u8>,
}

impl Edit {
    fn cost(&self) -> usize {
        self.removed.len() + self.inserted.len() + 32
    }
}

/// A set of edits undone/redone together, with the cursor around them.
#[derive(Clone, Debug)]
pub struct Group {
    pub edits: Vec<Edit>,
    pub cursor_before: usize,
    pub cursor_after: usize,
    kind: Kind,
}

/// Undo and redo stacks.
#[derive(Default)]
pub struct History {
    undo: VecDeque<Group>,
    redo: Vec<Group>,
    /// The top group still accepts merged edits.
    open: bool,
    /// Undo depth at the last save; `None` when that state is unreachable.
    saved: Option<usize>,
    bytes: usize,
    /// Memory cap in bytes (`usize::MAX` = unlimited).
    limit: usize,
    last_ws: bool,
}

impl History {
    pub fn new() -> Self {
        Self {
            saved: Some(0),
            limit: usize::MAX,
            ..Self::default()
        }
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.open = false;
        self.saved = Some(0);
        self.bytes = 0;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// Approximate memory held by the history.
    pub fn memory(&self) -> usize {
        self.bytes
    }

    pub fn set_limit(&mut self, bytes: usize) {
        self.limit = bytes;
        self.trim();
    }

    /// Stop merging: the next edit starts a new group.
    pub fn break_group(&mut self) {
        self.open = false;
    }

    /// Record the current state as the saved one.
    pub fn mark_saved(&mut self) {
        self.open = false;
        self.saved = Some(self.undo.len());
    }

    /// True when the text differs from the last saved state.
    pub fn modified(&self) -> bool {
        self.saved != Some(self.undo.len())
    }

    /// Force the "modified" state (used when the buffer was edited outside
    /// the history, e.g. a failed load).
    pub fn mark_modified(&mut self) {
        self.saved = None;
    }

    /// Begin (or continue) a group of `kind`. `ws` tells whether a typed
    /// character is whitespace (a word boundary starts a new typing group).
    pub fn begin(&mut self, kind: Kind, cursor: usize, ws: bool) {
        let merge = self.open
            && kind != Kind::Other
            && self.undo.back().is_some_and(|g| g.kind == kind)
            && !(kind == Kind::Typing && self.last_ws && !ws);
        self.last_ws = ws;
        if merge {
            return;
        }
        if let Some(s) = self.saved
            && s > self.undo.len()
        {
            self.saved = None;
        }
        self.redo.clear();
        self.undo.push_back(Group {
            edits: Vec::new(),
            cursor_before: cursor,
            cursor_after: cursor,
            kind,
        });
        self.open = true;
    }

    /// Append `edit` to the current group, coalescing with the previous edit
    /// when the group kind allows it.
    pub fn push(&mut self, edit: Edit) {
        let Some(g) = self.undo.back_mut() else {
            return;
        };
        if edit.removed.is_empty() && edit.inserted.is_empty() {
            return;
        }
        if let Some(last) = g.edits.last_mut() {
            let merged = match g.kind {
                Kind::Typing
                    if edit.removed.is_empty()
                        && last.removed.is_empty()
                        && edit.pos == last.pos + last.inserted.len() =>
                {
                    last.inserted.extend_from_slice(&edit.inserted);
                    true
                }
                Kind::Backspacing
                    if edit.inserted.is_empty()
                        && last.inserted.is_empty()
                        && edit.pos + edit.removed.len() == last.pos =>
                {
                    let mut r = edit.removed.clone();
                    r.extend_from_slice(&last.removed);
                    last.removed = r;
                    last.pos = edit.pos;
                    true
                }
                Kind::Deleting
                    if edit.inserted.is_empty()
                        && last.inserted.is_empty()
                        && edit.pos == last.pos =>
                {
                    last.removed.extend_from_slice(&edit.removed);
                    true
                }
                _ => false,
            };
            if merged {
                self.bytes += edit.removed.len() + edit.inserted.len();
                return;
            }
        }
        self.bytes += edit.cost();
        g.edits.push(edit);
    }

    /// Set where the cursor ends up after the current group.
    pub fn finish(&mut self, cursor_after: usize, kind: Kind) {
        if let Some(g) = self.undo.back_mut() {
            g.cursor_after = cursor_after;
            if g.edits.is_empty() {
                // Nothing was recorded: drop the empty group.
                self.undo.pop_back();
                self.open = false;
                if let Some(s) = self.saved
                    && s > self.undo.len()
                {
                    self.saved = None;
                }
                return;
            }
        }
        if kind == Kind::Other {
            self.open = false;
        }
        self.trim();
    }

    fn trim(&mut self) {
        while self.bytes > self.limit && self.undo.len() > 1 {
            if let Some(g) = self.undo.pop_front() {
                let c: usize = g.edits.iter().map(Edit::cost).sum();
                self.bytes = self.bytes.saturating_sub(c);
                self.saved = match self.saved {
                    Some(s) if s > 0 => Some(s - 1),
                    _ => None,
                };
            }
        }
    }

    /// Undo the newest group: `f` applies its inverse; the group then moves to
    /// the redo stack (without copying its edits).
    pub fn undo_with<R>(&mut self, f: impl FnOnce(&Group) -> R) -> Option<R> {
        self.open = false;
        let g = self.undo.pop_back()?;
        let r = f(&g);
        self.redo.push(g);
        Some(r)
    }

    /// Redo the newest undone group: `f` re-applies it; it returns to the undo
    /// stack.
    pub fn redo_with<R>(&mut self, f: impl FnOnce(&Group) -> R) -> Option<R> {
        self.open = false;
        let g = self.redo.pop()?;
        let r = f(&g);
        self.undo.push_back(g);
        Some(r)
    }
}

#[cfg(test)]
mod tests;
