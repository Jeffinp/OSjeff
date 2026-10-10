//! grammar (split out of `parse.rs`).

use super::*;

impl Parser {
    /// Parse statements until EOF or one of the `stop` keywords (not
    /// consumed).
    pub(super) fn parse_list(&mut self, stop: &[&str]) -> Result<Vec<Stmt>, ParseError> {
        let mut out = Vec::new();
        loop {
            self.skip_separators();
            if self.peek().is_none() {
                break;
            }
            if let Some(k) = self.keyword()
                && stop.contains(&k)
            {
                break;
            }
            let pos = self.pos();
            let and_or = self.parse_and_or()?;
            out.push(Stmt { and_or, pos });
            match self.peek() {
                None | Some(Tok::Newline | Tok::Op(Op::Semi)) => {}
                Some(_) => return self.err(ParseErrorKind::UnexpectedToken),
            }
        }
        Ok(out)
    }

    pub(super) fn parse_and_or(&mut self) -> Result<AndOr, ParseError> {
        let first = self.parse_pipeline()?;
        let mut rest = Vec::new();
        loop {
            let c = match self.peek() {
                Some(Tok::Op(Op::AndIf)) => Connector::And,
                Some(Tok::Op(Op::OrIf)) => Connector::Or,
                _ => break,
            };
            self.i += 1;
            self.skip_newlines();
            if self.peek().is_none() {
                return self.err(ParseErrorKind::UnexpectedEof);
            }
            rest.push((c, self.parse_pipeline()?));
        }
        Ok(AndOr { first, rest })
    }

    pub(super) fn parse_pipeline(&mut self) -> Result<Pipeline, ParseError> {
        let mut cmds = alloc::vec![self.parse_command()?];
        while matches!(self.peek(), Some(Tok::Op(Op::Pipe))) {
            self.i += 1;
            self.skip_newlines();
            if self.peek().is_none() {
                return self.err(ParseErrorKind::UnexpectedEof);
            }
            cmds.push(self.parse_command()?);
        }
        Ok(Pipeline { cmds })
    }

    pub(super) fn parse_command(&mut self) -> Result<Command, ParseError> {
        self.enter()?;
        let r = self.parse_command_inner();
        self.leave();
        r
    }

    pub(super) fn parse_command_inner(&mut self) -> Result<Command, ParseError> {
        match self.keyword() {
            Some("if") => return self.parse_if(),
            Some("for") => return self.parse_for(),
            Some("while") => return self.parse_while(false),
            Some("until") => return self.parse_while(true),
            Some("{") => {
                self.i += 1;
                let body = self.parse_list(&["}"])?;
                self.expect_keyword("}")?;
                return Ok(Command::Group(body));
            }
            Some(_) => return self.err(ParseErrorKind::UnexpectedToken),
            None => {}
        }
        if let Some(Tok::Word(w)) = self.peek()
            && lit_of(w) == Some("function")
        {
            return self.parse_function_kw();
        }
        // `name ( ) { ... }`
        if let (Some(Tok::Word(w)), Some((Tok::Op(Op::LParen), _))) =
            (self.peek(), self.toks.get(self.i + 1))
        {
            let name = match lit_of(w) {
                Some(n) if super::super::env::valid_name(n) => n.to_string(),
                _ => return self.err(ParseErrorKind::BadFunctionName),
            };
            self.i += 2;
            if !matches!(self.peek(), Some(Tok::Op(Op::RParen))) {
                return self.err(ParseErrorKind::Expected(")"));
            }
            self.i += 1;
            return self.parse_function_body(name);
        }
        self.parse_simple()
    }

    pub(super) fn parse_function_kw(&mut self) -> Result<Command, ParseError> {
        self.i += 1;
        let name = match self.peek() {
            Some(Tok::Word(w)) => match lit_of(w) {
                Some(n) if super::super::env::valid_name(n) => n.to_string(),
                _ => return self.err(ParseErrorKind::BadFunctionName),
            },
            _ => return self.err(ParseErrorKind::BadFunctionName),
        };
        self.i += 1;
        if matches!(self.peek(), Some(Tok::Op(Op::LParen))) {
            self.i += 1;
            if !matches!(self.peek(), Some(Tok::Op(Op::RParen))) {
                return self.err(ParseErrorKind::Expected(")"));
            }
            self.i += 1;
        }
        self.parse_function_body(name)
    }

    pub(super) fn parse_function_body(&mut self, name: String) -> Result<Command, ParseError> {
        self.skip_newlines();
        if !self.at_keyword("{") {
            return self.err(ParseErrorKind::Expected("{"));
        }
        self.i += 1;
        let body = self.parse_list(&["}"])?;
        self.expect_keyword("}")?;
        Ok(Command::Func {
            name,
            body: Arc::new(body),
        })
    }

    pub(super) fn parse_if(&mut self) -> Result<Command, ParseError> {
        self.i += 1; // if
        let mut arms = Vec::new();
        let mut else_body = None;
        loop {
            let cond = self.parse_list(&["then"])?;
            if cond.is_empty() {
                return self.err(ParseErrorKind::UnexpectedToken);
            }
            self.expect_keyword("then")?;
            let body = self.parse_list(&["elif", "else", "fi"])?;
            arms.push((cond, body));
            if self.at_keyword("elif") {
                self.i += 1;
                continue;
            }
            if self.at_keyword("else") {
                self.i += 1;
                else_body = Some(self.parse_list(&["fi"])?);
            }
            self.expect_keyword("fi")?;
            break;
        }
        Ok(Command::If { arms, else_body })
    }

    pub(super) fn parse_for(&mut self) -> Result<Command, ParseError> {
        self.i += 1; // for
        let var = match self.peek() {
            Some(Tok::Word(w)) => match lit_of(w) {
                Some(n) if super::super::env::valid_name(n) => n.to_string(),
                _ => return self.err(ParseErrorKind::UnexpectedToken),
            },
            None => return self.err(ParseErrorKind::UnexpectedEof),
            _ => return self.err(ParseErrorKind::UnexpectedToken),
        };
        self.i += 1;
        self.skip_newlines();
        let mut words = Vec::new();
        if self.at_keyword("in") {
            self.i += 1;
            while let Some(Tok::Word(w)) = self.peek() {
                if lit_of(w) == Some("do") {
                    break;
                }
                words.push(w.clone());
                self.i += 1;
            }
        } else {
            // `for x; do` iterates over the positional parameters.
            words.push(alloc::vec![Part::Args {
                at: true,
                quoted: true
            }]);
        }
        self.skip_separators();
        self.expect_keyword("do")?;
        let body = self.parse_list(&["done"])?;
        self.expect_keyword("done")?;
        Ok(Command::For { var, words, body })
    }

    pub(super) fn parse_while(&mut self, until: bool) -> Result<Command, ParseError> {
        self.i += 1;
        let cond = self.parse_list(&["do"])?;
        if cond.is_empty() {
            return self.err(ParseErrorKind::UnexpectedToken);
        }
        self.expect_keyword("do")?;
        let body = self.parse_list(&["done"])?;
        self.expect_keyword("done")?;
        Ok(Command::While { cond, body, until })
    }

    pub(super) fn parse_simple(&mut self) -> Result<Command, ParseError> {
        let mut s = Simple {
            pos: self.pos(),
            ..Simple::default()
        };
        loop {
            match self.peek() {
                Some(Tok::Word(w)) => {
                    let w = w.clone();
                    if s.words.is_empty()
                        && let Some((name, value)) = split_assignment(&w)
                    {
                        s.assigns.push((name, value));
                    } else {
                        s.words.push(w);
                    }
                    self.i += 1;
                }
                Some(Tok::Op(op @ (Op::Lt | Op::Gt | Op::GtGt))) => {
                    let kind = match op {
                        Op::Lt => RedirKind::In,
                        Op::Gt => RedirKind::Out,
                        _ => RedirKind::Append,
                    };
                    let pos = self.pos();
                    self.i += 1;
                    match self.peek() {
                        Some(Tok::Word(w)) => {
                            s.redirs.push(Redir {
                                kind,
                                target: w.clone(),
                                pos,
                            });
                            self.i += 1;
                        }
                        _ => return self.err(ParseErrorKind::MissingRedirectTarget),
                    }
                }
                _ => break,
            }
        }
        if s.words.is_empty() && s.assigns.is_empty() && s.redirs.is_empty() {
            return match self.peek() {
                None => self.err(ParseErrorKind::UnexpectedEof),
                Some(_) => self.err(ParseErrorKind::UnexpectedToken),
            };
        }
        Ok(Command::Simple(s))
    }
}
