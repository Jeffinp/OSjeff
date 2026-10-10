//! history (split out of `mod.rs`).

use super::*;

impl Editor {
    /// Undo the last group. Returns false when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        let text = &mut self.text;
        let r = self.hist.undo_with(|g| {
            for e in g.edits.iter().rev() {
                text.replace(e.pos, e.inserted.len(), &e.removed);
            }
            g.cursor_before
        });
        self.finish_history_move(r)
    }

    /// Redo the last undone group.
    pub fn redo(&mut self) -> bool {
        let text = &mut self.text;
        let r = self.hist.redo_with(|g| {
            for e in &g.edits {
                text.replace(e.pos, e.removed.len(), &e.inserted);
            }
            g.cursor_after
        });
        self.finish_history_move(r)
    }

    pub(super) fn finish_history_move(&mut self, r: Option<usize>) -> bool {
        match r {
            Some(c) => {
                self.cursor = self.normalize(c);
                self.anchor = None;
                self.after_move(false);
                true
            }
            None => false,
        }
    }

    // ---- clipboard -----------------------------------------------------

    /// Copy the selection into `clip` (truncated to its capacity at a
    /// character boundary). Returns false without a selection.
    pub fn copy(&self, clip: &mut Clipboard) -> bool {
        let sel = self.selected_bytes();
        if sel.is_empty() {
            return false;
        }
        clip.set(truncate_utf8(&sel, clipboard::CAP));
        true
    }

    /// Copy then delete the selection. If the selection is bigger than the
    /// clipboard, nothing is deleted (no silent data loss).
    pub fn cut(&mut self, clip: &mut Clipboard) -> bool {
        let sel = self.selected_bytes();
        if sel.is_empty() || sel.len() > clipboard::CAP {
            return false;
        }
        clip.set(&sel);
        self.backspace();
        true
    }

    /// Paste the clipboard at the cursor (replacing the selection).
    pub fn paste(&mut self, clip: &Clipboard) -> bool {
        if clip.is_empty() {
            return false;
        }
        let data = clip.get().to_vec();
        self.insert_bytes(&data);
        true
    }
}
