//! history (split out of `line.rs`).

use super::*;

impl LineEditor {
    pub(super) fn load_history(&mut self, hist: &History, i: usize) {
        self.buf = hist.get(i).unwrap_or("").chars().take(MAX_LINE).collect();
        self.cursor = self.buf.len();
    }

    pub(super) fn history_prev(&mut self, hist: &History) -> LineEvent {
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

    pub(super) fn history_next(&mut self, hist: &History) -> LineEvent {
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

    pub(super) fn search_key(&mut self, ev: KeyEvent, hist: &History) -> LineEvent {
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
}
