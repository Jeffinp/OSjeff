//! completion (split out of `line.rs`).

use super::*;

impl LineEditor {
    /// Find the word under the cursor: `(start, quote, command_position)`.
    pub(super) fn current_word(&self) -> (usize, Option<char>, bool) {
        let mut start = 0usize;
        let mut quote: Option<char> = None;
        let mut i = 0usize;
        let mut cmd_pos = true;
        let mut cur = String::new();
        let end = self.cursor;
        let mut in_word = false;
        while i < end {
            let c = self.buf[i];
            match quote {
                Some(q) => {
                    if c == q {
                        quote = None;
                    }
                    cur.push(c);
                }
                None => match c {
                    '\\' if i + 1 < end => {
                        if !in_word {
                            start = i;
                            in_word = true;
                        }
                        cur.push(self.buf[i + 1]);
                        i += 1;
                    }
                    '\'' | '"' => {
                        if !in_word {
                            start = i;
                            in_word = true;
                        }
                        quote = Some(c);
                    }
                    ' ' | '\t' => {
                        if in_word {
                            let prev_word = core::mem::take(&mut cur);
                            in_word = false;
                            cmd_pos = matches!(
                                prev_word.as_str(),
                                "then" | "do" | "else" | "elif" | "if" | "while" | "until" | "{"
                            );
                        }
                    }
                    '|' | ';' | '&' | '(' => {
                        if in_word {
                            in_word = false;
                            cur.clear();
                        }
                        cmd_pos = true;
                        start = i + 1;
                    }
                    _ => {
                        if !in_word {
                            start = i;
                            in_word = true;
                        }
                        cur.push(c);
                    }
                },
            }
            i += 1;
        }
        if !in_word {
            start = end;
        }
        (start, quote, cmd_pos)
    }

    pub(super) fn unescape(raw: &[char]) -> String {
        let mut out = String::new();
        let mut i = 0;
        let mut quote: Option<char> = None;
        while i < raw.len() {
            let c = raw[i];
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => out.push(c),
                None => match c {
                    '\\' if i + 1 < raw.len() => {
                        i += 1;
                        out.push(raw[i]);
                    }
                    '\'' | '"' => quote = Some(c),
                    _ => out.push(c),
                },
            }
            i += 1;
        }
        out
    }

    pub(super) fn escape(s: &str, first_is_word_start: bool) -> String {
        let mut out = String::new();
        for (i, c) in s.chars().enumerate() {
            let special = matches!(
                c,
                ' ' | '\t'
                    | '\''
                    | '"'
                    | '$'
                    | '*'
                    | '?'
                    | ';'
                    | '&'
                    | '|'
                    | '<'
                    | '>'
                    | '('
                    | ')'
                    | '\\'
                    | '#'
            ) || (c == '~' && i == 0 && first_is_word_start);
            if special {
                out.push('\\');
            }
            out.push(c);
        }
        out
    }

    pub(super) fn complete(&mut self, comp: &dyn Completer) -> LineEvent {
        let (start, quote, cmd_pos) = self.current_word();
        let raw: Vec<char> = self.buf[start..self.cursor].to_vec();
        let word = Self::unescape(&raw);
        let mut cands: Vec<Completion>;
        let mut var_mode = false;
        if let Some(v) = word.strip_prefix('$') {
            var_mode = true;
            cands = comp.variables(v);
        } else if cmd_pos && !word.contains('/') {
            cands = comp.commands(&word);
        } else {
            cands = comp.paths(&word);
        }
        cands.sort_by(|a, b| a.text.cmp(&b.text));
        cands.dedup();
        if cands.is_empty() {
            return LineEvent::None;
        }
        let prefix_len = word.chars().count() - usize::from(var_mode);
        let texts: Vec<&str> = cands.iter().map(|c| c.text.as_str()).collect();
        let common = common_prefix(&texts);
        let replacement_plain = if cands.len() == 1 {
            cands[0].text.clone()
        } else {
            common
        };
        if cands.len() > 1 && replacement_plain.chars().count() <= prefix_len {
            let names = cands
                .iter()
                .map(|c| {
                    // Show the last component only.
                    let t = c.text.trim_end_matches('/');
                    let base = t.rsplit('/').next().unwrap_or(t);
                    if c.is_dir {
                        alloc::format!("{base}/")
                    } else {
                        base.to_string()
                    }
                })
                .collect();
            return LineEvent::Candidates(names);
        }
        let mut text = if var_mode {
            alloc::format!("${}", Self::escape(&replacement_plain, false))
        } else {
            match quote {
                Some(q) => alloc::format!("{q}{replacement_plain}"),
                None => Self::escape(&replacement_plain, true),
            }
        };
        if cands.len() == 1 {
            if let Some(q) = quote {
                text.push(q);
            }
            if !cands[0].is_dir {
                text.push(' ');
            }
        }
        let tail: Vec<char> = self.buf[self.cursor..].to_vec();
        self.buf.truncate(start);
        self.cursor = start;
        self.insert_str(&text);
        let new_cursor = self.cursor;
        for c in tail {
            if self.buf.len() < MAX_LINE {
                self.buf.push(c);
            }
        }
        self.cursor = new_cursor;
        LineEvent::Changed
    }
}
