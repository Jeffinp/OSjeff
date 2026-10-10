//! keys (split out of `line.rs`).

use super::*;

impl LineEditor {
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

    pub(super) fn ctrl_key(&mut self, c: char, hist: &History) -> LineEvent {
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
}
