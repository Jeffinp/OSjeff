//! words (split out of `parse.rs`).

use super::*;

impl<'a> Lexer<'a> {
    pub(super) fn lex_word(&mut self) -> Result<Word, ParseError> {
        let mut parts: Word = Vec::new();
        let mut text = String::new();
        let first = self.i;
        while let Some(c) = self.peek(0) {
            if is_delim(c) {
                break;
            }
            match c {
                '\'' => {
                    push_lit(&mut parts, &mut text, false);
                    let open = self.i;
                    self.i += 1;
                    let mut s = String::new();
                    loop {
                        match self.peek(0) {
                            None => {
                                return Err(ParseError::new(
                                    ParseErrorKind::UnterminatedSingleQuote,
                                    self.pos_at(open),
                                ));
                            }
                            Some('\'') => {
                                self.i += 1;
                                break;
                            }
                            Some(ch) => {
                                s.push(ch);
                                self.i += 1;
                            }
                        }
                    }
                    parts.push(Part::Lit {
                        text: s,
                        quoted: true,
                    });
                }
                '"' => {
                    push_lit(&mut parts, &mut text, false);
                    let open = self.i;
                    self.i += 1;
                    let mut inner = self.lex_dq(Some(open))?;
                    if inner.is_empty() {
                        inner.push(Part::Lit {
                            text: String::new(),
                            quoted: true,
                        });
                    }
                    parts.extend(inner);
                }
                '\\' => {
                    self.i += 1;
                    match self.peek(0) {
                        Some('\n') => self.i += 1,
                        Some(n) => {
                            push_lit(&mut parts, &mut text, false);
                            parts.push(Part::Lit {
                                text: n.to_string(),
                                quoted: true,
                            });
                            self.i += 1;
                        }
                        None => text.push('\\'),
                    }
                }
                '$' => {
                    push_lit(&mut parts, &mut text, false);
                    match self.lex_dollar(false)? {
                        Some(p) => parts.push(p),
                        None => text.push('$'),
                    }
                }
                '~' if self.i == first && self.peek(1).is_none_or(|n| n == '/' || is_delim(n)) => {
                    parts.push(Part::Var {
                        name: "HOME".to_string(),
                        quoted: false,
                    });
                    self.i += 1;
                }
                _ => {
                    text.push(c);
                    self.i += 1;
                }
            }
        }
        push_lit(&mut parts, &mut text, false);
        Ok(parts)
    }

    /// Lex the inside of double quotes. `open` is the index of the opening
    /// quote (`None` reads to the end of input, used for `$(( ))` bodies).
    pub(super) fn lex_dq(&mut self, open: Option<usize>) -> Result<Word, ParseError> {
        let mut parts: Word = Vec::new();
        let mut text = String::new();
        loop {
            match self.peek(0) {
                None => {
                    return match open {
                        Some(o) => Err(ParseError::new(
                            ParseErrorKind::UnterminatedDoubleQuote,
                            self.pos_at(o),
                        )),
                        None => {
                            push_lit(&mut parts, &mut text, true);
                            Ok(parts)
                        }
                    };
                }
                Some('"') if open.is_some() => {
                    self.i += 1;
                    push_lit(&mut parts, &mut text, true);
                    return Ok(parts);
                }
                Some('\\') => {
                    match self.peek(1) {
                        Some(n @ ('$' | '"' | '\\' | '`')) => {
                            text.push(n);
                            self.i += 2;
                        }
                        Some('\n') => self.i += 2,
                        _ => {
                            text.push('\\');
                            self.i += 1;
                        }
                    };
                }
                Some('$') => {
                    push_lit(&mut parts, &mut text, true);
                    match self.lex_dollar(true)? {
                        Some(p) => parts.push(p),
                        None => text.push('$'),
                    }
                }
                Some(c) => {
                    text.push(c);
                    self.i += 1;
                }
            }
        }
    }

    /// At a `$`. Returns the part, or `None` when the `$` is a plain character
    /// (the cursor then sits after it).
    pub(super) fn lex_dollar(&mut self, quoted: bool) -> Result<Option<Part>, ParseError> {
        let at = self.i;
        self.i += 1;
        let Some(c) = self.peek(0) else {
            return Ok(None);
        };
        match c {
            '(' => {
                if self.peek(1) == Some('(') {
                    self.lex_arith(at, quoted).map(Some)
                } else {
                    self.lex_cmd(at, quoted).map(Some)
                }
            }
            '{' => {
                self.i += 1;
                let mut name = String::new();
                loop {
                    match self.peek(0) {
                        Some('}') => {
                            self.i += 1;
                            break;
                        }
                        Some(ch) if ch.is_ascii_alphanumeric() || ch == '_' || ch == '?' => {
                            name.push(ch);
                            self.i += 1;
                        }
                        _ => {
                            return Err(ParseError::new(
                                ParseErrorKind::BadSubstitution,
                                self.pos_at(at),
                            ));
                        }
                    }
                }
                let ok = super::super::env::valid_name(&name)
                    || (name.len() == 1
                        && (name == "?" || name.chars().all(|c| c.is_ascii_digit())))
                    || (!name.is_empty() && name.chars().all(|c| c.is_ascii_digit()));
                if !ok {
                    return Err(ParseError::new(
                        ParseErrorKind::BadSubstitution,
                        self.pos_at(at),
                    ));
                }
                Ok(Some(if name == "?" {
                    Part::Status { quoted }
                } else {
                    Part::Var { name, quoted }
                }))
            }
            '?' => {
                self.i += 1;
                Ok(Some(Part::Status { quoted }))
            }
            '#' => {
                self.i += 1;
                Ok(Some(Part::Count { quoted }))
            }
            '@' | '*' => {
                self.i += 1;
                Ok(Some(Part::Args {
                    at: c == '@',
                    quoted,
                }))
            }
            d if d.is_ascii_digit() => {
                self.i += 1;
                Ok(Some(Part::Var {
                    name: d.to_string(),
                    quoted,
                }))
            }
            a if a.is_ascii_alphabetic() || a == '_' => {
                let mut name = String::new();
                while let Some(ch) = self.peek(0) {
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        name.push(ch);
                        self.i += 1;
                    } else {
                        break;
                    }
                }
                Ok(Some(Part::Var { name, quoted }))
            }
            _ => Ok(None),
        }
    }

    /// Find the index of the `)` matching an already-open `(` (cursor just
    /// after it), skipping quotes and escapes.
    pub(super) fn find_close(&self, mut j: usize) -> Option<usize> {
        let mut depth = 1usize;
        while let Some(&(_, c)) = self.cs.get(j) {
            match c {
                '\'' => {
                    j += 1;
                    while let Some(&(_, d)) = self.cs.get(j) {
                        if d == '\'' {
                            break;
                        }
                        j += 1;
                    }
                }
                '"' => {
                    j += 1;
                    while let Some(&(_, d)) = self.cs.get(j) {
                        if d == '\\' {
                            j += 1;
                        } else if d == '"' {
                            break;
                        }
                        j += 1;
                    }
                }
                '\\' => j += 1,
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(j);
                    }
                }
                _ => {}
            }
            j += 1;
        }
        None
    }

    pub(super) fn byte_at(&self, j: usize) -> usize {
        self.cs.get(j).map_or(self.src.len(), |c| c.0)
    }

    pub(super) fn lex_cmd(&mut self, at: usize, quoted: bool) -> Result<Part, ParseError> {
        // Cursor is on the `(`.
        let open = self.i + 1;
        let Some(close) = self.find_close(open) else {
            return Err(ParseError::new(
                ParseErrorKind::UnterminatedSubstitution,
                self.pos_at(at),
            ));
        };
        let inner = &self.src[self.byte_at(open)..self.byte_at(close)];
        let base = self.base + self.byte_at(open);
        let body = parse_with(inner, base, self.depth + 1)?;
        self.i = close + 1;
        Ok(Part::Cmd {
            body: Arc::new(body),
            quoted,
        })
    }

    pub(super) fn lex_arith(&mut self, at: usize, quoted: bool) -> Result<Part, ParseError> {
        // Cursor is on the first `(` of `$((`.
        let open = self.i + 2;
        let Some(close) = self.find_close(self.i + 1) else {
            return Err(ParseError::new(
                ParseErrorKind::UnterminatedSubstitution,
                self.pos_at(at),
            ));
        };
        // The expression ends at `))`: the char before `close` must be `)`.
        if close == 0 || self.cs.get(close - 1).map(|c| c.1) != Some(')') || close - 1 < open {
            return Err(ParseError::new(
                ParseErrorKind::UnterminatedSubstitution,
                self.pos_at(at),
            ));
        }
        let inner = &self.src[self.byte_at(open)..self.byte_at(close - 1)];
        if self.depth + 1 > MAX_DEPTH {
            return Err(ParseError::new(ParseErrorKind::TooDeep, self.pos_at(at)));
        }
        let mut sub = Lexer::new(inner, self.base + self.byte_at(open), self.depth + 1);
        let expr = sub.lex_dq(None)?;
        self.i = close + 1;
        Ok(Part::Arith { expr, quoted })
    }
}
