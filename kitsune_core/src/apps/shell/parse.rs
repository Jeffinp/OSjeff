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

mod grammar;

mod words;

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
