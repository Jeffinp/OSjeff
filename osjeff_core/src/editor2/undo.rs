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
mod tests {
    use super::*;

    fn ins(pos: usize, s: &[u8]) -> Edit {
        Edit {
            pos,
            removed: Vec::new(),
            inserted: s.to_vec(),
        }
    }

    #[test]
    fn starts_clean_and_empty() {
        let h = History::new();
        assert!(!h.modified());
        assert!(!h.can_undo() && !h.can_redo());
    }

    #[test]
    fn typing_merges_into_one_group() {
        let mut h = History::new();
        for (i, c) in b"abc".iter().enumerate() {
            h.begin(Kind::Typing, i, false);
            h.push(ins(i, &[*c]));
            h.finish(i + 1, Kind::Typing);
        }
        assert_eq!(h.undo_depth(), 1);
        let g = h.undo_with(Clone::clone).unwrap();
        assert_eq!(g.edits.len(), 1);
        assert_eq!(g.edits[0].inserted, b"abc");
    }

    #[test]
    fn other_kind_never_merges() {
        let mut h = History::new();
        h.begin(Kind::Other, 0, false);
        h.push(ins(0, b"a"));
        h.finish(1, Kind::Other);
        h.begin(Kind::Other, 1, false);
        h.push(ins(1, b"b"));
        h.finish(2, Kind::Other);
        assert_eq!(h.undo_depth(), 2);
    }

    #[test]
    fn break_group_splits_typing() {
        let mut h = History::new();
        h.begin(Kind::Typing, 0, false);
        h.push(ins(0, b"a"));
        h.finish(1, Kind::Typing);
        h.break_group();
        h.begin(Kind::Typing, 1, false);
        h.push(ins(1, b"b"));
        h.finish(2, Kind::Typing);
        assert_eq!(h.undo_depth(), 2);
    }

    #[test]
    fn word_boundary_splits_typing() {
        let mut h = History::new();
        for (i, c) in b"ab cd".iter().enumerate() {
            h.begin(Kind::Typing, i, *c == b' ');
            h.push(ins(i, &[*c]));
            h.finish(i + 1, Kind::Typing);
        }
        // "ab " then "cd".
        assert_eq!(h.undo_depth(), 2);
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut h = History::new();
        h.begin(Kind::Other, 0, false);
        h.push(ins(0, b"a"));
        h.finish(1, Kind::Other);
        h.undo_with(|_| ());
        assert!(h.can_redo());
        h.begin(Kind::Other, 0, false);
        h.push(ins(0, b"b"));
        h.finish(1, Kind::Other);
        assert!(!h.can_redo());
    }

    #[test]
    fn modified_tracks_save_point() {
        let mut h = History::new();
        h.begin(Kind::Other, 0, false);
        h.push(ins(0, b"a"));
        h.finish(1, Kind::Other);
        assert!(h.modified());
        h.mark_saved();
        assert!(!h.modified());
        h.undo_with(|_| ());
        assert!(h.modified());
        h.redo_with(|_| ());
        assert!(!h.modified());
    }

    #[test]
    fn save_point_lost_when_redo_discarded() {
        let mut h = History::new();
        h.begin(Kind::Other, 0, false);
        h.push(ins(0, b"a"));
        h.finish(1, Kind::Other);
        h.mark_saved();
        h.undo_with(|_| ());
        h.begin(Kind::Other, 0, false);
        h.push(ins(0, b"b"));
        h.finish(1, Kind::Other);
        // Depth equals the saved depth (1) but the content differs.
        assert!(h.modified());
        h.undo_with(|_| ());
        assert!(h.modified());
    }

    #[test]
    fn typing_after_save_is_a_new_group() {
        let mut h = History::new();
        h.begin(Kind::Typing, 0, false);
        h.push(ins(0, b"a"));
        h.finish(1, Kind::Typing);
        h.mark_saved();
        h.begin(Kind::Typing, 1, false);
        h.push(ins(1, b"b"));
        h.finish(2, Kind::Typing);
        assert!(h.modified());
        assert_eq!(h.undo_depth(), 2);
    }

    #[test]
    fn backspace_merges_in_reverse() {
        let mut h = History::new();
        // Text "abc": backspace at 3, then 2.
        h.begin(Kind::Backspacing, 3, false);
        h.push(Edit {
            pos: 2,
            removed: b"c".to_vec(),
            inserted: Vec::new(),
        });
        h.finish(2, Kind::Backspacing);
        h.begin(Kind::Backspacing, 2, false);
        h.push(Edit {
            pos: 1,
            removed: b"b".to_vec(),
            inserted: Vec::new(),
        });
        h.finish(1, Kind::Backspacing);
        let g = h.undo_with(Clone::clone).unwrap();
        assert_eq!(g.edits.len(), 1);
        assert_eq!(g.edits[0].pos, 1);
        assert_eq!(g.edits[0].removed, b"bc");
    }

    #[test]
    fn delete_merges_forward() {
        let mut h = History::new();
        for _ in 0..2 {
            h.begin(Kind::Deleting, 1, false);
            h.push(Edit {
                pos: 1,
                removed: b"x".to_vec(),
                inserted: Vec::new(),
            });
            h.finish(1, Kind::Deleting);
        }
        let g = h.undo_with(Clone::clone).unwrap();
        assert_eq!(g.edits[0].removed, b"xx");
    }

    #[test]
    fn empty_group_is_dropped() {
        let mut h = History::new();
        h.begin(Kind::Other, 0, false);
        h.finish(0, Kind::Other);
        assert_eq!(h.undo_depth(), 0);
        assert!(!h.modified());
    }

    #[test]
    fn limit_discards_oldest() {
        let mut h = History::new();
        for i in 0..10 {
            h.begin(Kind::Other, i, false);
            h.push(ins(i, &[b'x'; 100]));
            h.finish(i + 1, Kind::Other);
        }
        h.set_limit(500);
        assert!(h.undo_depth() < 10);
        assert!(h.undo_depth() >= 1);
        assert!(h.modified());
    }

    #[test]
    fn clear_resets() {
        let mut h = History::new();
        h.begin(Kind::Other, 0, false);
        h.push(ins(0, b"a"));
        h.finish(1, Kind::Other);
        h.clear();
        assert!(!h.modified());
        assert!(!h.can_undo());
    }
}
