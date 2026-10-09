//! Pointer handling of the terminal: wheel, selection and paste.

use crate::desktop::apps::terminal::term_grid;
use crate::desktop::kit::appui;
use crate::desktop::*;
use kitsune_core::termui::{self, Selection};

impl Desktop {
    /// Wheel over a terminal: scroll the history (`notches` > 0 = older).
    pub(crate) fn term_wheel(&mut self, id: WindowId, notches: i32) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let g = term_grid(rect);
        if let Some(ts) = self.term_state_mut(id) {
            ts.term.resize(g.cols, g.rows);
            ts.grid = (g.cols, g.rows);
            ts.sel = None;
            ts.term.scroll_rows(notches as isize * 3);
            ts.scroll_fade.touch(appui::now_ms());
        }
    }

    /// A press in terminal `id`: start a selection (a double press takes the word, a triple
    /// press the line).
    pub(crate) fn term_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        let g = term_grid(rect);
        if !g.in_text(px, py) {
            return;
        }
        let cell = g.cell_at(px, py);
        let ticks = crate::interrupts::ticks();
        let Some(ts) = self.term_state_mut(id) else {
            return;
        };
        ts.grid = (g.cols, g.rows);
        ts.last_input = appui::ticks();
        let n = ts.click_count(ticks, cell);
        let rows = ts.term.view_in(g.cols, g.rows).rows;
        let row_text = rows.get(cell.0).map(String::as_str).unwrap_or("");
        ts.sel = match n {
            1 => Some(Selection::new(cell)),
            2 => {
                let (a, b) = termui::word_bounds(row_text, cell.1);
                Some(Selection {
                    anchor: (cell.0, a),
                    head: (cell.0, b),
                })
            }
            _ => Some(Selection {
                anchor: (cell.0, 0),
                head: (cell.0, row_text.chars().count().saturating_sub(1)),
            }),
        };
        ts.press_at = (px, py);
        self.drag = Some(Drag {
            win: id,
            mode: DragMode::Select,
        });
    }

    /// The pointer moved with the button held after a press: extend the selection.
    pub(crate) fn term_drag(&mut self, id: WindowId, px: i32, py: i32) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let g = term_grid(rect);
        let cell = g.cell_at(px, py);
        let Some(ts) = self.term_state_mut(id) else {
            return;
        };
        // A press that has not moved keeps the word or line a double press took.
        if (px, py) == ts.press_at || ts.last_click.2 > 1 {
            return;
        }
        if let Some(sel) = &mut ts.sel {
            sel.head = cell;
        }
    }

    /// Whether the pointer at `(cx, cy)` is over the text of terminal `id` (an I-beam).
    pub(crate) fn term_text_at(&self, id: WindowId, cx: i32, cy: i32) -> bool {
        self.wm
            .get(id)
            .is_some_and(|w| term_grid(w.rect).in_text(cx, cy))
    }

    /// Paste text into terminal `id` (never runs a command by itself).
    pub(crate) fn term_paste(&mut self, id: WindowId, text: &[u8]) {
        let s = String::from_utf8_lossy(text).into_owned();
        if let Some(ts) = self.term_state_mut(id) {
            ts.term.paste(&s);
        }
    }
}
