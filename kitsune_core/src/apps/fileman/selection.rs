//! selection (split out of `fileman.rs`).

use super::*;

/// Multi-selection over `n` rows with a cursor and a range anchor.
#[derive(Clone, Debug, Default)]
pub struct Selection {
    mask: Vec<bool>,
    count: usize,
    cursor: usize,
    anchor: usize,
}

impl Selection {
    /// Empty selection over zero rows.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start over with `n` unselected rows (cursor on the first).
    pub fn reset(&mut self, n: usize) {
        self.mask.clear();
        self.mask.resize(n, false);
        self.count = 0;
        self.cursor = 0;
        self.anchor = 0;
    }

    /// Number of rows covered.
    pub fn len(&self) -> usize {
        self.mask.len()
    }

    /// True when there are no rows.
    pub fn is_empty(&self) -> bool {
        self.mask.is_empty()
    }

    /// Number of selected rows.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Whether row `i` is selected.
    pub fn is_selected(&self, i: usize) -> bool {
        self.mask.get(i).copied().unwrap_or(false)
    }

    /// The keyboard cursor row.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Deselect everything (the cursor stays).
    pub fn clear(&mut self) {
        self.mask.iter_mut().for_each(|m| *m = false);
        self.count = 0;
    }

    /// Select every row (Ctrl+A).
    pub fn select_all(&mut self) {
        self.mask.iter_mut().for_each(|m| *m = true);
        self.count = self.mask.len();
    }

    pub(super) fn set(&mut self, i: usize, v: bool) {
        if let Some(m) = self.mask.get_mut(i)
            && *m != v
        {
            *m = v;
            if v {
                self.count += 1;
            } else {
                self.count -= 1;
            }
        }
    }

    /// Select exactly row `i` (plain click or arrow key).
    pub fn only(&mut self, i: usize) {
        if self.mask.is_empty() {
            return;
        }
        let i = i.min(self.mask.len() - 1);
        self.clear();
        self.set(i, true);
        self.cursor = i;
        self.anchor = i;
    }

    /// Select exactly the rows `idx` (ascending, in range), cursor and anchor on the first.
    pub fn select_set(&mut self, idx: &[usize]) {
        self.clear();
        for &i in idx {
            self.set(i, true);
        }
        if let Some(&f) = idx.first() {
            self.cursor = f.min(self.mask.len().saturating_sub(1));
            self.anchor = self.cursor;
        }
    }

    /// A mouse click on row `i` with the modifier state: plain selects only it,
    /// Ctrl toggles it, Shift selects the range from the anchor (Ctrl+Shift adds the
    /// range to what is selected).
    pub fn click(&mut self, i: usize, ctrl: bool, shift: bool) {
        if i >= self.mask.len() {
            return;
        }
        match (ctrl, shift) {
            (false, false) => self.only(i),
            (true, false) => {
                let v = !self.is_selected(i);
                self.set(i, v);
                self.cursor = i;
                self.anchor = i;
            }
            (_, true) => {
                if !ctrl {
                    self.clear();
                }
                let (a, b) = (self.anchor.min(i), self.anchor.max(i));
                for k in a..=b {
                    self.set(k, true);
                }
                self.cursor = i;
            }
        }
    }

    /// Move the cursor by `delta` rows; with Shift the selection grows from the
    /// anchor, without it only the new row is selected.
    pub fn move_cursor(&mut self, delta: isize, shift: bool) {
        if self.mask.is_empty() {
            return;
        }
        let target = (self.cursor as isize + delta).clamp(0, self.mask.len() as isize - 1) as usize;
        if shift {
            self.clear();
            let (a, b) = (self.anchor.min(target), self.anchor.max(target));
            for k in a..=b {
                self.set(k, true);
            }
            self.cursor = target;
        } else {
            self.only(target);
        }
    }

    /// Indices of the selected rows, ascending.
    pub fn selected(&self) -> Vec<usize> {
        self.mask
            .iter()
            .enumerate()
            .filter(|&(_, &m)| m)
            .map(|(i, _)| i)
            .collect()
    }

    /// Select the rows in `names` (by index of `rows`), cursor on `cursor_name`.
    pub(super) fn restore(&mut self, rows: &[Row], names: &[Vec<u8>], cursor_name: Option<&[u8]>) {
        self.reset(rows.len());
        let wanted: alloc::collections::BTreeSet<&[u8]> = names.iter().map(|n| &n[..]).collect();
        for (i, r) in rows.iter().enumerate() {
            if wanted.contains(&r.name[..]) {
                self.set(i, true);
            }
            if cursor_name == Some(&r.name[..]) {
                self.cursor = i;
                self.anchor = i;
            }
        }
    }
}
