//! A small backtracking regular-expression engine for `grep`.
//!
//! Syntax (extended): literals, `.`, `[abc]`, `[a-z]`, `[^x]`, `*`, `+`, `?`,
//! `|`, `( )`, `^`, `$`, and the escapes `\d \w \s` plus `\` before any
//! punctuation. Matching is bounded by a step budget so a pathological
//! pattern fails the match instead of hanging the shell.

use alloc::vec::Vec;

/// Steps one `is_match` call may spend before giving up (returns false).
pub const STEP_BUDGET: usize = 200_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegexErr {
    UnbalancedParen,
    UnterminatedClass,
    NothingToRepeat,
    TrailingBackslash,
    TooLong,
}

impl RegexErr {
    /// The catalog key of the text for this error.
    pub const fn key(self) -> &'static str {
        match self {
            RegexErr::UnbalancedParen => crate::tk!("sh.regex.unbalanced"),
            RegexErr::UnterminatedClass => crate::tk!("sh.regex.class"),
            RegexErr::NothingToRepeat => crate::tk!("sh.regex.nothing"),
            RegexErr::TrailingBackslash => crate::tk!("sh.regex.backslash"),
            RegexErr::TooLong => crate::tk!("sh.regex.too_long"),
        }
    }

    /// The text of this error in the language in effect.
    pub fn message(self) -> &'static str {
        crate::i18n::tr(self.key())
    }
}

#[derive(Clone, Debug)]
enum Node {
    Char(char),
    Any,
    Class(Vec<(char, char)>, bool),
    Start,
    End,
    Cat(Vec<Node>),
    Alt(Vec<Node>),
    Star(alloc::boxed::Box<Node>),
    Plus(alloc::boxed::Box<Node>),
    Opt(alloc::boxed::Box<Node>),
}

#[derive(Clone, Debug)]
enum Inst {
    Char(char),
    Any,
    Class(Vec<(char, char)>, bool),
    Start,
    End,
    Split(usize, usize),
    Jmp(usize),
    Match,
}

/// A compiled pattern.
#[derive(Clone, Debug)]
pub struct Regex {
    prog: Vec<Inst>,
    icase: bool,
}

struct P<'a> {
    c: &'a [char],
    i: usize,
    depth: usize,
}

fn class_escape(ch: char) -> Option<Vec<(char, char)>> {
    match ch {
        'd' => Some(alloc::vec![('0', '9')]),
        'w' => Some(alloc::vec![('a', 'z'), ('A', 'Z'), ('0', '9'), ('_', '_')]),
        's' => Some(alloc::vec![
            (' ', ' '),
            ('\t', '\t'),
            ('\n', '\n'),
            ('\r', '\r')
        ]),
        _ => None,
    }
}

impl P<'_> {
    fn alt(&mut self) -> Result<Node, RegexErr> {
        let mut alts = alloc::vec![self.cat()?];
        while self.c.get(self.i) == Some(&'|') {
            self.i += 1;
            alts.push(self.cat()?);
        }
        Ok(if alts.len() == 1 {
            alts.pop().unwrap_or(Node::Cat(Vec::new()))
        } else {
            Node::Alt(alts)
        })
    }

    fn cat(&mut self) -> Result<Node, RegexErr> {
        let mut items: Vec<Node> = Vec::new();
        while let Some(&ch) = self.c.get(self.i) {
            if ch == '|' || ch == ')' {
                break;
            }
            let atom = self.atom()?;
            let node = match self.c.get(self.i) {
                Some('*') => {
                    self.i += 1;
                    Node::Star(alloc::boxed::Box::new(atom))
                }
                Some('+') => {
                    self.i += 1;
                    Node::Plus(alloc::boxed::Box::new(atom))
                }
                Some('?') => {
                    self.i += 1;
                    Node::Opt(alloc::boxed::Box::new(atom))
                }
                _ => atom,
            };
            items.push(node);
        }
        Ok(Node::Cat(items))
    }

    fn atom(&mut self) -> Result<Node, RegexErr> {
        let ch = self.c[self.i];
        self.i += 1;
        match ch {
            '.' => Ok(Node::Any),
            '^' => Ok(Node::Start),
            '$' => Ok(Node::End),
            '*' | '+' | '?' => Err(RegexErr::NothingToRepeat),
            '(' => {
                self.depth += 1;
                if self.depth > 32 {
                    return Err(RegexErr::TooLong);
                }
                let n = self.alt()?;
                self.depth -= 1;
                if self.c.get(self.i) != Some(&')') {
                    return Err(RegexErr::UnbalancedParen);
                }
                self.i += 1;
                Ok(n)
            }
            '[' => self.class(),
            '\\' => {
                let Some(&n) = self.c.get(self.i) else {
                    return Err(RegexErr::TrailingBackslash);
                };
                self.i += 1;
                Ok(match class_escape(n) {
                    Some(r) => Node::Class(r, false),
                    None => Node::Char(match n {
                        't' => '\t',
                        'n' => '\n',
                        o => o,
                    }),
                })
            }
            c => Ok(Node::Char(c)),
        }
    }

    fn class(&mut self) -> Result<Node, RegexErr> {
        let mut neg = false;
        if self.c.get(self.i) == Some(&'^') {
            neg = true;
            self.i += 1;
        }
        let mut ranges: Vec<(char, char)> = Vec::new();
        let mut first = true;
        loop {
            let Some(&ch) = self.c.get(self.i) else {
                return Err(RegexErr::UnterminatedClass);
            };
            self.i += 1;
            if ch == ']' && !first {
                break;
            }
            first = false;
            let lo = if ch == '\\' {
                let Some(&n) = self.c.get(self.i) else {
                    return Err(RegexErr::UnterminatedClass);
                };
                self.i += 1;
                if let Some(r) = class_escape(n) {
                    ranges.extend(r);
                    continue;
                }
                n
            } else {
                ch
            };
            if self.c.get(self.i) == Some(&'-') && self.c.get(self.i + 1).is_some_and(|&n| n != ']')
            {
                let hi = self.c[self.i + 1];
                self.i += 2;
                ranges.push((lo.min(hi), lo.max(hi)));
            } else {
                ranges.push((lo, lo));
            }
        }
        Ok(Node::Class(ranges, neg))
    }
}

fn emit(n: &Node, prog: &mut Vec<Inst>) {
    match n {
        Node::Char(c) => prog.push(Inst::Char(*c)),
        Node::Any => prog.push(Inst::Any),
        Node::Class(r, neg) => prog.push(Inst::Class(r.clone(), *neg)),
        Node::Start => prog.push(Inst::Start),
        Node::End => prog.push(Inst::End),
        Node::Cat(items) => {
            for i in items {
                emit(i, prog);
            }
        }
        Node::Alt(alts) => {
            let mut jumps = Vec::new();
            for (k, a) in alts.iter().enumerate() {
                if k + 1 < alts.len() {
                    let split = prog.len();
                    prog.push(Inst::Split(split + 1, 0));
                    emit(a, prog);
                    jumps.push(prog.len());
                    prog.push(Inst::Jmp(0));
                    let next = prog.len();
                    prog[split] = Inst::Split(split + 1, next);
                } else {
                    emit(a, prog);
                }
            }
            let end = prog.len();
            for j in jumps {
                prog[j] = Inst::Jmp(end);
            }
        }
        Node::Star(x) => {
            let split = prog.len();
            prog.push(Inst::Split(split + 1, 0));
            emit(x, prog);
            prog.push(Inst::Jmp(split));
            let end = prog.len();
            prog[split] = Inst::Split(split + 1, end);
        }
        Node::Plus(x) => {
            let start = prog.len();
            emit(x, prog);
            let split = prog.len();
            prog.push(Inst::Split(start, split + 1));
        }
        Node::Opt(x) => {
            let split = prog.len();
            prog.push(Inst::Split(split + 1, 0));
            emit(x, prog);
            let end = prog.len();
            prog[split] = Inst::Split(split + 1, end);
        }
    }
}

fn fold(c: char) -> char {
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

impl Regex {
    /// Compile `pattern`. `icase` makes matching case-insensitive.
    pub fn new(pattern: &str, icase: bool) -> Result<Self, RegexErr> {
        if pattern.len() > 1024 {
            return Err(RegexErr::TooLong);
        }
        let chars: Vec<char> = pattern.chars().collect();
        let mut p = P {
            c: &chars,
            i: 0,
            depth: 0,
        };
        let ast = p.alt()?;
        if p.i < chars.len() {
            // A stray `)`.
            return Err(RegexErr::UnbalancedParen);
        }
        let mut prog = Vec::new();
        emit(&ast, &mut prog);
        prog.push(Inst::Match);
        Ok(Self { prog, icase })
    }

    /// Does the pattern match anywhere in `text`?
    pub fn is_match(&self, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        let mut budget = STEP_BUDGET;
        for start in 0..=chars.len() {
            match self.run(&chars, start, &mut budget) {
                Some(true) => return true,
                Some(false) => {}
                None => return false,
            }
        }
        false
    }

    /// Match anchored at `start`; `None` when the budget ran out.
    fn run(&self, text: &[char], start: usize, budget: &mut usize) -> Option<bool> {
        let mut stack: Vec<(usize, usize)> = alloc::vec![(0, start)];
        while let Some((mut pc, mut pos)) = stack.pop() {
            loop {
                if *budget == 0 {
                    return None;
                }
                *budget -= 1;
                match &self.prog[pc] {
                    Inst::Match => return Some(true),
                    Inst::Char(c) => {
                        match text.get(pos) {
                            Some(&t) if t == *c || (self.icase && fold(t) == fold(*c)) => {
                                pc += 1;
                                pos += 1;
                            }
                            _ => break,
                        };
                    }
                    Inst::Any => {
                        if pos < text.len() {
                            pc += 1;
                            pos += 1;
                        } else {
                            break;
                        }
                    }
                    Inst::Class(r, neg) => {
                        let Some(&t) = text.get(pos) else { break };
                        let hit = |x: char| r.iter().any(|&(lo, hi)| lo <= x && x <= hi);
                        let mut m = hit(t);
                        if !m && self.icase {
                            m = t.to_lowercase().any(hit) || t.to_uppercase().any(hit);
                        }
                        if m != *neg {
                            pc += 1;
                            pos += 1;
                        } else {
                            break;
                        }
                    }
                    Inst::Start => {
                        if pos == 0 {
                            pc += 1;
                        } else {
                            break;
                        }
                    }
                    Inst::End => {
                        if pos == text.len() {
                            pc += 1;
                        } else {
                            break;
                        }
                    }
                    Inst::Jmp(t) => pc = *t,
                    Inst::Split(a, b) => {
                        stack.push((*b, pos));
                        pc = *a;
                    }
                }
            }
        }
        Some(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, t: &str) -> bool {
        Regex::new(p, false).unwrap().is_match(t)
    }

    #[test]
    fn literals_and_dot() {
        assert!(m("abc", "xxabcxx"));
        assert!(!m("abc", "ab"));
        assert!(m("a.c", "abc"));
        assert!(!m("a.c", "ac"));
        assert!(m("", "anything"));
    }

    #[test]
    fn anchors() {
        assert!(m("^abc", "abcdef"));
        assert!(!m("^abc", "xabc"));
        assert!(m("def$", "abcdef"));
        assert!(!m("def$", "defx"));
        assert!(m("^$", ""));
        assert!(!m("^$", "x"));
    }

    #[test]
    fn quantifiers() {
        assert!(m("ab*c", "ac"));
        assert!(m("ab*c", "abbbc"));
        assert!(m("ab+c", "abc"));
        assert!(!m("ab+c", "ac"));
        assert!(m("ab?c", "ac"));
        assert!(m("^a.*z$", "a123z"));
    }

    #[test]
    fn classes() {
        assert!(m("[abc]x", "bx"));
        assert!(!m("[abc]x", "dx"));
        assert!(m("[a-c]+$", "abcabc"));
        assert!(m("[^0-9]", "a"));
        assert!(!m("^[^0-9]+$", "ab1"));
        assert!(m("[]x]", "]"));
        assert!(m("[a-]", "-"));
    }

    #[test]
    fn escapes() {
        assert!(m("a\\.b", "a.b"));
        assert!(!m("a\\.b", "axb"));
        assert!(m("\\d+", "abc123"));
        assert!(m("^\\w+$", "foo_bar1"));
        assert!(m("a\\sb", "a b"));
        assert!(m("\\(x\\)", "(x)"));
    }

    #[test]
    fn alternation_and_groups() {
        assert!(m("cat|dog", "hotdog"));
        assert!(!m("cat|dog", "bird"));
        assert!(m("^(ab)+$", "ababab"));
        assert!(!m("^(ab)+$", "aba"));
        assert!(m("gr(a|e)y", "grey"));
        assert!(m("a|b|c", "c"));
    }

    #[test]
    fn case_insensitive() {
        let r = Regex::new("hello", true).unwrap();
        assert!(r.is_match("Say HeLLo"));
        let r = Regex::new("[a-c]x", true).unwrap();
        assert!(r.is_match("BX"));
        assert!(!Regex::new("hello", false).unwrap().is_match("HELLO"));
    }

    #[test]
    fn unicode_text() {
        assert!(m("ñ.", "añb"));
        assert!(m("^.$", "€"));
    }

    #[test]
    fn syntax_errors() {
        assert_eq!(
            Regex::new("(a", false).unwrap_err(),
            RegexErr::UnbalancedParen
        );
        assert_eq!(
            Regex::new("a)", false).unwrap_err(),
            RegexErr::UnbalancedParen
        );
        assert_eq!(
            Regex::new("[a", false).unwrap_err(),
            RegexErr::UnterminatedClass
        );
        assert_eq!(
            Regex::new("*a", false).unwrap_err(),
            RegexErr::NothingToRepeat
        );
        assert_eq!(
            Regex::new("a\\", false).unwrap_err(),
            RegexErr::TrailingBackslash
        );
        assert_eq!(
            Regex::new(&"a".repeat(2000), false).unwrap_err(),
            RegexErr::TooLong
        );
        for e in [
            RegexErr::UnbalancedParen,
            RegexErr::UnterminatedClass,
            RegexErr::NothingToRepeat,
            RegexErr::TrailingBackslash,
            RegexErr::TooLong,
        ] {
            assert!(!e.message().is_empty());
        }
    }

    #[test]
    fn pathological_pattern_terminates() {
        let r = Regex::new("(a*)*b", false).unwrap();
        let text = "a".repeat(5000);
        let _ = r.is_match(&text);
        let r = Regex::new("(a|aa)+$", false).unwrap();
        let _ = r.is_match(&alloc::format!("{}b", "a".repeat(5000)));
    }

    #[test]
    fn deep_group_nesting_is_rejected_not_overflowed() {
        let p = alloc::format!("{}a{}", "(".repeat(100), ")".repeat(100));
        assert_eq!(Regex::new(&p, false).unwrap_err(), RegexErr::TooLong);
    }
}
