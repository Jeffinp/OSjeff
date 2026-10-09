//! Lexer and parser: text to a syntax tree, never panicking.
//!
//! Supported syntax:
//!
//! * words with `'single'` and `"double"` quotes and `\` escapes;
//! * `$VAR`, `${VAR}`, `$?`, `$#`, `$@`, `$*`, `$0..$9`, `$(command)`,
//!   `$((arithmetic))` and a leading `~` for `$HOME`;
//! * `*` and `?` globbing (resolved at run time against the filesystem);
//! * `|`, `>`, `>>`, `<`, `&&`, `||`, `;`, newlines, `#` comments;
//! * `if/elif/else/fi`, `for x in ...; do ...; done`, `while`/`until`,
//!   `name() { ...; }` and `function name { ...; }`, `{ ...; }` groups;
//! * `NAME=value` assignments.
//!
//! Rejected with a precise error: `&` (background jobs), `<<` (here-documents),
//! `2>` and `>&` (descriptor redirections), unbalanced quotes and
//! substitutions, and nesting deeper than [`MAX_DEPTH`].

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

/// Maximum nesting of compound commands and `$( )`.
pub const MAX_DEPTH: usize = 24;

/// Why parsing failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParseErrorKind {
    UnterminatedSingleQuote,
    UnterminatedDoubleQuote,
    UnterminatedSubstitution,
    BadSubstitution,
    /// A token that cannot start or continue a command here.
    UnexpectedToken,
    /// Input ended while a construct was open (`&&` with nothing after it).
    UnexpectedEof,
    /// A specific keyword or token was required (`fi`, `done`, `then`, ...).
    Expected(&'static str),
    MissingRedirectTarget,
    /// `&`: background jobs are not supported.
    BackgroundNotSupported,
    /// `<<`: here-documents are not supported.
    HeredocNotSupported,
    /// `2>`, `>&`: descriptor redirections are not supported.
    FdRedirectNotSupported,
    BadFunctionName,
    TooDeep,
}

/// A parse error with the byte offset in the input.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub pos: usize,
}

impl ParseError {
    fn new(kind: ParseErrorKind, pos: usize) -> Self {
        Self { kind, pos }
    }

    /// Human-readable description (no position).
    pub fn message(&self) -> String {
        use ParseErrorKind::*;
        match self.kind {
            UnterminatedSingleQuote => crate::t!("sh.parse.single_quote").to_string(),
            UnterminatedDoubleQuote => crate::t!("sh.parse.double_quote").to_string(),
            UnterminatedSubstitution => crate::t!("sh.parse.substitution").to_string(),
            BadSubstitution => crate::t!("sh.parse.bad_subst").to_string(),
            UnexpectedToken => crate::t!("sh.parse.unexpected").to_string(),
            UnexpectedEof => crate::t!("sh.parse.eof").to_string(),
            Expected(what) => crate::t!("sh.parse.expected", what = what),
            MissingRedirectTarget => crate::t!("sh.parse.redirect").to_string(),
            BackgroundNotSupported => crate::t!("sh.parse.background").to_string(),
            HeredocNotSupported => crate::t!("sh.parse.heredoc").to_string(),
            FdRedirectNotSupported => crate::t!("sh.parse.fd_redirect").to_string(),
            BadFunctionName => crate::t!("sh.parse.func_name").to_string(),
            TooDeep => crate::t!("sh.parse.too_deep").to_string(),
        }
    }

    /// 1-based `(line, column)` of the error in `src`.
    pub fn line_col(&self, src: &str) -> (usize, usize) {
        let mut line = 1;
        let mut col = 1;
        for (i, c) in src.char_indices() {
            if i >= self.pos {
                break;
            }
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (line, col)
    }
}

/// One piece of a word.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Part {
    /// Literal text. `quoted` literals never glob or split.
    Lit { text: String, quoted: bool },
    /// `$NAME`, `${NAME}`, `$1`.
    Var { name: String, quoted: bool },
    /// `$?`.
    Status { quoted: bool },
    /// `$#`.
    Count { quoted: bool },
    /// `$@` (`at`) or `$*`.
    Args { at: bool, quoted: bool },
    /// `$( ... )`.
    Cmd { body: Arc<Vec<Stmt>>, quoted: bool },
    /// `$(( ... ))`; the expression is itself a word so `$x` works inside.
    Arith { expr: Vec<Part>, quoted: bool },
}

/// A shell word: concatenated parts.
pub type Word = Vec<Part>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedirKind {
    /// `< file`
    In,
    /// `> file`
    Out,
    /// `>> file`
    Append,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Redir {
    pub kind: RedirKind,
    pub target: Word,
    pub pos: usize,
}

/// A simple command: assignments, words, redirections.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Simple {
    pub assigns: Vec<(String, Word)>,
    pub words: Vec<Word>,
    pub redirs: Vec<Redir>,
    pub pos: usize,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Command {
    Simple(Simple),
    If {
        arms: Vec<(Vec<Stmt>, Vec<Stmt>)>,
        else_body: Option<Vec<Stmt>>,
    },
    For {
        var: String,
        words: Vec<Word>,
        body: Vec<Stmt>,
    },
    While {
        cond: Vec<Stmt>,
        body: Vec<Stmt>,
        until: bool,
    },
    Func {
        name: String,
        body: Arc<Vec<Stmt>>,
    },
    Group(Vec<Stmt>),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Pipeline {
    pub cmds: Vec<Command>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Connector {
    And,
    Or,
}

/// `a && b || c ...`
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AndOr {
    pub first: Pipeline,
    pub rest: Vec<(Connector, Pipeline)>,
}

/// One statement of a list.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stmt {
    pub and_or: AndOr,
    pub pos: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Op {
    Pipe,
    AndIf,
    OrIf,
    Semi,
    Lt,
    Gt,
    GtGt,
    LParen,
    RParen,
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Tok {
    Word(Word),
    Op(Op),
    Newline,
}

struct Lexer<'a> {
    src: &'a str,
    cs: Vec<(usize, char)>,
    i: usize,
    base: usize,
    depth: usize,
}

fn is_delim(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t' | '\n' | '\r' | '|' | '&' | ';' | '<' | '>' | '(' | ')'
    )
}

fn push_lit(parts: &mut Word, text: &mut String, quoted: bool) {
    if !text.is_empty() {
        parts.push(Part::Lit {
            text: core::mem::take(text),
            quoted,
        });
    }
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str, base: usize, depth: usize) -> Self {
        Self {
            src,
            cs: src.char_indices().collect(),
            i: 0,
            base,
            depth,
        }
    }

    fn pos_at(&self, i: usize) -> usize {
        self.base + self.cs.get(i).map_or(self.src.len(), |c| c.0)
    }

    fn peek(&self, off: usize) -> Option<char> {
        self.cs.get(self.i + off).map(|c| c.1)
    }

    fn lex(mut self) -> Result<Vec<(Tok, usize)>, ParseError> {
        let mut toks: Vec<(Tok, usize)> = Vec::new();
        while let Some(c) = self.peek(0) {
            let start = self.pos_at(self.i);
            match c {
                ' ' | '\t' | '\r' => self.i += 1,
                '\n' => {
                    toks.push((Tok::Newline, start));
                    self.i += 1;
                }
                '#' => {
                    while self.peek(0).is_some_and(|c| c != '\n') {
                        self.i += 1;
                    }
                }
                '|' => {
                    if self.peek(1) == Some('|') {
                        toks.push((Tok::Op(Op::OrIf), start));
                        self.i += 2;
                    } else {
                        toks.push((Tok::Op(Op::Pipe), start));
                        self.i += 1;
                    }
                }
                '&' => {
                    if self.peek(1) == Some('&') {
                        toks.push((Tok::Op(Op::AndIf), start));
                        self.i += 2;
                    } else {
                        return Err(ParseError::new(
                            ParseErrorKind::BackgroundNotSupported,
                            self.pos_at(self.i),
                        ));
                    }
                }
                ';' => {
                    toks.push((Tok::Op(Op::Semi), start));
                    self.i += 1;
                }
                '<' => {
                    if self.peek(1) == Some('<') {
                        return Err(ParseError::new(
                            ParseErrorKind::HeredocNotSupported,
                            self.pos_at(self.i),
                        ));
                    }
                    toks.push((Tok::Op(Op::Lt), start));
                    self.i += 1;
                }
                '>' => {
                    if self.peek(1) == Some('&') {
                        return Err(ParseError::new(
                            ParseErrorKind::FdRedirectNotSupported,
                            self.pos_at(self.i),
                        ));
                    }
                    if self.peek(1) == Some('>') {
                        toks.push((Tok::Op(Op::GtGt), start));
                        self.i += 2;
                    } else {
                        toks.push((Tok::Op(Op::Gt), start));
                        self.i += 1;
                    }
                }
                '(' => {
                    toks.push((Tok::Op(Op::LParen), start));
                    self.i += 1;
                }
                ')' => {
                    toks.push((Tok::Op(Op::RParen), start));
                    self.i += 1;
                }
                _ => {
                    let start_i = self.i;
                    let w = self.lex_word()?;
                    // `2>file` / `1<file`: a bare number glued to a redirect.
                    if let [
                        Part::Lit {
                            text,
                            quoted: false,
                        },
                    ] = w.as_slice()
                        && text.chars().all(|c| c.is_ascii_digit())
                        && matches!(self.peek(0), Some('<' | '>'))
                    {
                        return Err(ParseError::new(
                            ParseErrorKind::FdRedirectNotSupported,
                            self.pos_at(start_i),
                        ));
                    }
                    toks.push((Tok::Word(w), start));
                }
            }
        }
        Ok(toks)
    }

    fn lex_word(&mut self) -> Result<Word, ParseError> {
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
    fn lex_dq(&mut self, open: Option<usize>) -> Result<Word, ParseError> {
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
    fn lex_dollar(&mut self, quoted: bool) -> Result<Option<Part>, ParseError> {
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
                let ok = super::env::valid_name(&name)
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
    fn find_close(&self, mut j: usize) -> Option<usize> {
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

    fn byte_at(&self, j: usize) -> usize {
        self.cs.get(j).map_or(self.src.len(), |c| c.0)
    }

    fn lex_cmd(&mut self, at: usize, quoted: bool) -> Result<Part, ParseError> {
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

    fn lex_arith(&mut self, at: usize, quoted: bool) -> Result<Part, ParseError> {
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

/// Parse a whole script or command line.
pub fn parse(src: &str) -> Result<Vec<Stmt>, ParseError> {
    parse_with(src, 0, 0)
}

fn parse_with(src: &str, base: usize, depth: usize) -> Result<Vec<Stmt>, ParseError> {
    if depth > MAX_DEPTH {
        return Err(ParseError::new(ParseErrorKind::TooDeep, base));
    }
    let toks = Lexer::new(src, base, depth).lex()?;
    let mut p = Parser {
        toks,
        i: 0,
        depth,
        eof: base + src.len(),
    };
    let list = p.parse_list(&[])?;
    if p.i < p.toks.len() {
        return Err(ParseError::new(ParseErrorKind::UnexpectedToken, p.pos()));
    }
    Ok(list)
}

struct Parser {
    toks: Vec<(Tok, usize)>,
    i: usize,
    depth: usize,
    eof: usize,
}

fn lit_of(w: &Word) -> Option<&str> {
    match w.as_slice() {
        [
            Part::Lit {
                text,
                quoted: false,
            },
        ] => Some(text.as_str()),
        _ => None,
    }
}

const RESERVED: [&str; 13] = [
    "if", "then", "elif", "else", "fi", "for", "in", "do", "done", "while", "until", "{", "}",
];

impl Parser {
    fn pos(&self) -> usize {
        self.toks.get(self.i).map_or(self.eof, |t| t.1)
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i).map(|t| &t.0)
    }

    fn err<T>(&self, kind: ParseErrorKind) -> Result<T, ParseError> {
        Err(ParseError::new(kind, self.pos()))
    }

    /// The reserved word at the cursor, if any.
    fn keyword(&self) -> Option<&str> {
        match self.peek() {
            Some(Tok::Word(w)) => lit_of(w).filter(|s| RESERVED.contains(s)),
            _ => None,
        }
    }

    fn at_keyword(&self, kw: &str) -> bool {
        self.keyword() == Some(kw)
    }

    fn expect_keyword(&mut self, kw: &'static str) -> Result<(), ParseError> {
        if self.at_keyword(kw) {
            self.i += 1;
            Ok(())
        } else {
            self.err(ParseErrorKind::Expected(kw))
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Some(Tok::Newline)) {
            self.i += 1;
        }
    }

    fn skip_separators(&mut self) {
        while matches!(self.peek(), Some(Tok::Newline | Tok::Op(Op::Semi))) {
            self.i += 1;
        }
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.err(ParseErrorKind::TooDeep);
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    /// Parse statements until EOF or one of the `stop` keywords (not
    /// consumed).
    fn parse_list(&mut self, stop: &[&str]) -> Result<Vec<Stmt>, ParseError> {
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

    fn parse_and_or(&mut self) -> Result<AndOr, ParseError> {
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

    fn parse_pipeline(&mut self) -> Result<Pipeline, ParseError> {
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

    fn parse_command(&mut self) -> Result<Command, ParseError> {
        self.enter()?;
        let r = self.parse_command_inner();
        self.leave();
        r
    }

    fn parse_command_inner(&mut self) -> Result<Command, ParseError> {
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
                Some(n) if super::env::valid_name(n) => n.to_string(),
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

    fn parse_function_kw(&mut self) -> Result<Command, ParseError> {
        self.i += 1;
        let name = match self.peek() {
            Some(Tok::Word(w)) => match lit_of(w) {
                Some(n) if super::env::valid_name(n) => n.to_string(),
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

    fn parse_function_body(&mut self, name: String) -> Result<Command, ParseError> {
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

    fn parse_if(&mut self) -> Result<Command, ParseError> {
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

    fn parse_for(&mut self) -> Result<Command, ParseError> {
        self.i += 1; // for
        let var = match self.peek() {
            Some(Tok::Word(w)) => match lit_of(w) {
                Some(n) if super::env::valid_name(n) => n.to_string(),
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

    fn parse_while(&mut self, until: bool) -> Result<Command, ParseError> {
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

    fn parse_simple(&mut self) -> Result<Command, ParseError> {
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

/// `NAME=value...` at the start of a word: split into the name and the value
/// word.
fn split_assignment(w: &Word) -> Option<(String, Word)> {
    let Some(Part::Lit {
        text,
        quoted: false,
    }) = w.first()
    else {
        return None;
    };
    let eq = text.find('=')?;
    let name = &text[..eq];
    if !super::env::valid_name(name) {
        return None;
    }
    let mut value: Word = Vec::new();
    let rest = &text[eq + 1..];
    if !rest.is_empty() {
        value.push(Part::Lit {
            text: rest.to_string(),
            quoted: false,
        });
    }
    value.extend(w[1..].iter().cloned());
    Some((name.to_string(), value))
}

#[cfg(test)]
mod tests;
