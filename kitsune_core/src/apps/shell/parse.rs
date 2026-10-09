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
mod tests {
    use super::*;

    fn ok(src: &str) -> Vec<Stmt> {
        parse(src).unwrap_or_else(|e| panic!("{src:?}: {e:?}"))
    }

    fn err(src: &str) -> ParseError {
        parse(src).expect_err(src)
    }

    fn simple(src: &str) -> Simple {
        match &ok(src)[0].and_or.first.cmds[0] {
            Command::Simple(s) => s.clone(),
            other => panic!("not simple: {other:?}"),
        }
    }

    fn lit(w: &Word) -> String {
        w.iter()
            .map(|p| match p {
                Part::Lit { text, .. } => text.clone(),
                _ => "?".to_string(),
            })
            .collect()
    }

    #[test]
    fn empty_input_is_empty_list() {
        assert!(ok("").is_empty());
        assert!(ok("   \n\n  ").is_empty());
        assert!(ok("# just a comment").is_empty());
    }

    #[test]
    fn simple_words() {
        let s = simple("echo hello  world");
        let w: Vec<String> = s.words.iter().map(lit).collect();
        assert_eq!(w, ["echo", "hello", "world"]);
    }

    #[test]
    fn single_quotes_are_literal() {
        let s = simple("echo 'a $b * c'");
        assert_eq!(
            s.words[1],
            [Part::Lit {
                text: "a $b * c".into(),
                quoted: true
            }]
        );
    }

    #[test]
    fn double_quotes_keep_variables() {
        let s = simple("echo \"x $V y\"");
        let w = &s.words[1];
        assert_eq!(w.len(), 3);
        assert!(matches!(&w[1], Part::Var { name, quoted: true } if name == "V"));
    }

    #[test]
    fn escapes() {
        // echo a\ b "q\"r" \$X
        let s = simple("echo a\\ b \"q\\\"r\" \\$X");
        assert_eq!(s.words.len(), 4);
        assert_eq!(lit(&s.words[1]), "a b");
        assert_eq!(lit(&s.words[2]), "q\"r");
        assert_eq!(lit(&s.words[3]), "$X");
    }

    #[test]
    fn backslash_in_double_quotes_only_escapes_specials() {
        let s = simple("echo \"a\\nb\"");
        assert_eq!(lit(&s.words[1]), "a\\nb");
    }

    #[test]
    fn variables_forms() {
        let s = simple("echo $A ${B} $? $1 $# $@ $*");
        assert!(matches!(&s.words[1][0], Part::Var { name, .. } if name == "A"));
        assert!(matches!(&s.words[2][0], Part::Var { name, .. } if name == "B"));
        assert!(matches!(&s.words[3][0], Part::Status { .. }));
        assert!(matches!(&s.words[4][0], Part::Var { name, .. } if name == "1"));
        assert!(matches!(&s.words[5][0], Part::Count { .. }));
        assert!(matches!(&s.words[6][0], Part::Args { at: true, .. }));
        assert!(matches!(&s.words[7][0], Part::Args { at: false, .. }));
    }

    #[test]
    fn lone_dollar_is_literal() {
        let s = simple("echo $ a$ $-");
        assert_eq!(lit(&s.words[1]), "$");
        assert_eq!(lit(&s.words[2]), "a$");
    }

    #[test]
    fn command_substitution_parses_inner() {
        let s = simple("echo $(echo hi | wc -c)");
        match &s.words[1][0] {
            Part::Cmd { body, .. } => assert_eq!(body[0].and_or.first.cmds.len(), 2),
            p => panic!("{p:?}"),
        }
    }

    #[test]
    fn nested_substitution_with_quotes_and_parens() {
        let s = simple("echo \"$(echo \")\" $(echo 'a)b'))\"");
        assert!(matches!(&s.words[1][0], Part::Cmd { .. }));
    }

    #[test]
    fn arithmetic_expansion() {
        let s = simple("echo $((1 + $X * 2))");
        match &s.words[1][0] {
            Part::Arith { expr, .. } => {
                assert!(expr.iter().any(|p| matches!(p, Part::Var { .. })));
            }
            p => panic!("{p:?}"),
        }
    }

    #[test]
    fn tilde_expands_to_home_only_at_word_start() {
        let s = simple("cd ~ ~/x a~b");
        assert!(matches!(&s.words[1][0], Part::Var { name, .. } if name == "HOME"));
        assert!(matches!(&s.words[2][0], Part::Var { name, .. } if name == "HOME"));
        assert_eq!(lit(&s.words[3]), "a~b");
    }

    #[test]
    fn pipelines_and_lists() {
        let l = ok("a | b | c; d && e || f\ng");
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].and_or.first.cmds.len(), 3);
        assert_eq!(l[1].and_or.rest.len(), 2);
        assert_eq!(l[1].and_or.rest[0].0, Connector::And);
        assert_eq!(l[1].and_or.rest[1].0, Connector::Or);
    }

    #[test]
    fn redirections() {
        let s = simple("cat < in > out >> log");
        assert_eq!(s.redirs.len(), 3);
        assert_eq!(s.redirs[0].kind, RedirKind::In);
        assert_eq!(s.redirs[1].kind, RedirKind::Out);
        assert_eq!(s.redirs[2].kind, RedirKind::Append);
    }

    #[test]
    fn redirect_before_command_word() {
        let s = simple("> out echo hi");
        assert_eq!(s.words.len(), 2);
        assert_eq!(s.redirs.len(), 1);
    }

    #[test]
    fn assignments() {
        let s = simple("A=1 B=\"x y\" cmd arg");
        assert_eq!(s.assigns.len(), 2);
        assert_eq!(s.assigns[0].0, "A");
        assert_eq!(s.words.len(), 2);
        let only = simple("X=5");
        assert!(only.words.is_empty());
        assert_eq!(only.assigns.len(), 1);
    }

    #[test]
    fn equals_after_command_is_an_argument() {
        let s = simple("echo A=1");
        assert!(s.assigns.is_empty());
        assert_eq!(lit(&s.words[1]), "A=1");
    }

    #[test]
    fn comments_end_at_newline() {
        let l = ok("echo a # comment ; echo b\necho c");
        assert_eq!(l.len(), 2);
        let s = simple("echo a#b");
        assert_eq!(lit(&s.words[1]), "a#b");
    }

    #[test]
    fn if_forms() {
        let l = ok("if a; then b; elif c; then d; else e; fi");
        match &l[0].and_or.first.cmds[0] {
            Command::If { arms, else_body } => {
                assert_eq!(arms.len(), 2);
                assert!(else_body.is_some());
            }
            c => panic!("{c:?}"),
        }
        ok("if a\nthen\n b\nfi");
    }

    #[test]
    fn for_and_while() {
        let l = ok("for x in a b c; do echo $x; done");
        match &l[0].and_or.first.cmds[0] {
            Command::For { var, words, body } => {
                assert_eq!(var, "x");
                assert_eq!(words.len(), 3);
                assert_eq!(body.len(), 1);
            }
            c => panic!("{c:?}"),
        }
        ok("while true; do break; done");
        ok("until false; do break; done");
        ok("for x; do echo $x; done");
        ok("for x in; do echo; done");
    }

    #[test]
    fn functions() {
        let l = ok("greet() { echo hi; }\nfunction bye { echo bye; }\nfunction f() { :; }");
        assert_eq!(l.len(), 3);
        assert!(
            matches!(&l[0].and_or.first.cmds[0], Command::Func { name, .. } if name == "greet")
        );
    }

    #[test]
    fn groups() {
        let l = ok("{ echo a; echo b; } | wc -l");
        assert_eq!(l[0].and_or.first.cmds.len(), 2);
        assert!(matches!(&l[0].and_or.first.cmds[0], Command::Group(b) if b.len() == 2));
    }

    #[test]
    fn keywords_are_plain_words_in_arguments() {
        let s = simple("echo if then fi done");
        assert_eq!(s.words.len(), 5);
        let s = simple("echo {}");
        assert_eq!(lit(&s.words[1]), "{}");
    }

    #[test]
    fn quoted_keyword_is_a_command_name() {
        let s = simple("'if' x");
        assert_eq!(lit(&s.words[0]), "if");
    }

    #[test]
    fn error_unterminated_quotes() {
        assert_eq!(
            err("echo 'abc").kind,
            ParseErrorKind::UnterminatedSingleQuote
        );
        assert_eq!(err("echo 'abc").pos, 5);
        assert_eq!(
            err("echo \"abc").kind,
            ParseErrorKind::UnterminatedDoubleQuote
        );
        assert_eq!(
            err("echo $(abc").kind,
            ParseErrorKind::UnterminatedSubstitution
        );
        assert_eq!(
            err("echo $((1+2)").kind,
            ParseErrorKind::UnterminatedSubstitution
        );
    }

    #[test]
    fn error_background_job() {
        let e = err("sleep 5 &");
        assert_eq!(e.kind, ParseErrorKind::BackgroundNotSupported);
        assert_eq!(e.pos, 8);
        let _lang = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::En);
        assert!(e.message().contains("background"));
        let _pt = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
        assert!(e.message().contains("segundo plano"));
    }

    #[test]
    fn error_unsupported_redirections() {
        assert_eq!(
            err("cmd 2>err").kind,
            ParseErrorKind::FdRedirectNotSupported
        );
        assert_eq!(err("cmd >&2").kind, ParseErrorKind::FdRedirectNotSupported);
        assert_eq!(err("cat <<EOF").kind, ParseErrorKind::HeredocNotSupported);
        assert_eq!(err("cmd &> f").kind, ParseErrorKind::BackgroundNotSupported);
    }

    #[test]
    fn number_arguments_are_fine() {
        let s = simple("echo 2 3 > out");
        assert_eq!(s.words.len(), 3);
    }

    #[test]
    fn error_missing_redirect_target() {
        assert_eq!(err("echo >").kind, ParseErrorKind::MissingRedirectTarget);
        assert_eq!(err("echo > ;").kind, ParseErrorKind::MissingRedirectTarget);
    }

    #[test]
    fn error_dangling_operators() {
        assert_eq!(err("echo a &&").kind, ParseErrorKind::UnexpectedEof);
        assert_eq!(err("echo a |").kind, ParseErrorKind::UnexpectedEof);
        assert_eq!(err("| echo").kind, ParseErrorKind::UnexpectedToken);
        assert_eq!(err("&& echo").kind, ParseErrorKind::UnexpectedToken);
        assert_eq!(err(")").kind, ParseErrorKind::UnexpectedToken);
    }

    #[test]
    fn error_unclosed_constructs() {
        assert_eq!(
            err("if true; then echo").kind,
            ParseErrorKind::Expected("fi")
        );
        assert_eq!(
            err("for x in a; do echo").kind,
            ParseErrorKind::Expected("done")
        );
        assert_eq!(err("while true; do").kind, ParseErrorKind::Expected("done"));
        assert_eq!(err("if true echo").kind, ParseErrorKind::Expected("then"));
        assert_eq!(err("f() { echo").kind, ParseErrorKind::Expected("}"));
        assert_eq!(err("{ echo").kind, ParseErrorKind::Expected("}"));
    }

    #[test]
    fn error_stray_closing_keywords() {
        assert_eq!(err("fi").kind, ParseErrorKind::UnexpectedToken);
        assert_eq!(err("done").kind, ParseErrorKind::UnexpectedToken);
        assert_eq!(err("echo a; }").kind, ParseErrorKind::UnexpectedToken);
        assert_eq!(err("then").kind, ParseErrorKind::UnexpectedToken);
    }

    #[test]
    fn error_bad_function_and_for() {
        assert_eq!(err("1bad() { :; }").kind, ParseErrorKind::BadFunctionName);
        assert_eq!(
            err("function 1x { :; }").kind,
            ParseErrorKind::BadFunctionName
        );
        assert_eq!(
            err("for 1x in a; do :; done").kind,
            ParseErrorKind::UnexpectedToken
        );
    }

    #[test]
    fn error_bad_brace_substitution() {
        assert_eq!(err("echo ${}").kind, ParseErrorKind::BadSubstitution);
        assert_eq!(err("echo ${a-b}").kind, ParseErrorKind::BadSubstitution);
        assert_eq!(err("echo ${abc").kind, ParseErrorKind::BadSubstitution);
    }

    #[test]
    fn error_depth_limit() {
        let mut s = String::new();
        for _ in 0..40 {
            s.push_str("if true; then ");
        }
        s.push_str("echo");
        for _ in 0..40 {
            s.push_str("; fi");
        }
        assert_eq!(err(&s).kind, ParseErrorKind::TooDeep);
        let mut c = String::from("echo ");
        for _ in 0..40 {
            c.push_str("$(echo ");
        }
        for _ in 0..40 {
            c.push(')');
        }
        assert_eq!(err(&c).kind, ParseErrorKind::TooDeep);
    }

    #[test]
    fn error_positions_inside_substitution_are_absolute() {
        let e = err("echo $(echo 'x)");
        assert_eq!(e.kind, ParseErrorKind::UnterminatedSubstitution);
        let e = err("echo $(a &)");
        assert_eq!(e.kind, ParseErrorKind::BackgroundNotSupported);
        assert_eq!(e.pos, 9);
    }

    #[test]
    fn line_and_column() {
        let src = "echo a\necho 'b";
        let e = err(src);
        assert_eq!(e.line_col(src), (2, 6));
    }

    #[test]
    fn multiline_script() {
        let l = ok("A=1\nif [ $A = 1 ]; then\n  echo yes\nfi\n\n# end\n");
        assert_eq!(l.len(), 2);
    }

    #[test]
    fn line_continuation() {
        let s = simple("echo a\\\nb");
        assert_eq!(lit(&s.words[1]), "ab");
    }

    #[test]
    fn unicode_input_is_fine() {
        let s = simple("echo 'ação' é€");
        assert_eq!(s.words.len(), 3);
        let e = err("echo é 'x");
        assert_eq!(e.kind, ParseErrorKind::UnterminatedSingleQuote);
        assert_eq!(e.pos, "echo é ".len());
    }

    fn xorshift(x: &mut u64) -> u64 {
        *x ^= *x << 13;
        *x ^= *x >> 7;
        *x ^= *x << 17;
        *x
    }

    #[test]
    fn never_panics_on_token_soup() {
        let alphabet: &[u8] = b"abc $(){}[]'\"\\|&;<>#~*?=\n\t12${ifthenfidodonefor";
        let mut x: u64 = 0x1234_5678_9abc_def1;
        for _ in 0..4000 {
            let mut s = String::new();
            let len = (xorshift(&mut x) % 40) as usize;
            for _ in 0..len {
                let k = (xorshift(&mut x) % alphabet.len() as u64) as usize;
                s.push(alphabet[k] as char);
            }
            let _ = parse(&s);
        }
    }

    #[test]
    fn never_panics_on_arbitrary_bytes() {
        let mut x: u64 = 99;
        for _ in 0..3000 {
            let mut bytes = Vec::new();
            for _ in 0..(xorshift(&mut x) % 60) {
                bytes.push((xorshift(&mut x) >> 5) as u8);
            }
            let s = String::from_utf8_lossy(&bytes).into_owned();
            let _ = parse(&s);
        }
    }

    #[test]
    fn never_panics_on_every_prefix_of_a_complex_script() {
        let script = "f() { for x in a*b $(ls | head -n 2) \"$@\"; do if [ $x = 1 ] && echo ${x} > out; then echo $((1+2)); fi; done; }\nwhile true; do break; done # end\n";
        for end in 0..=script.len() {
            if script.is_char_boundary(end) {
                let _ = parse(&script[..end]);
            }
        }
    }
}
