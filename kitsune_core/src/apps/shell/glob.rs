//! `*` and `?` globbing against a [`ShellFs`], and `$(( ))` arithmetic.

use super::fs::{Kind, ShellFs};
use alloc::string::String;
use alloc::vec::Vec;

/// A pattern character and whether it is an *active* wildcard (an unquoted
/// `*` or `?`).
pub type PatChar = (char, bool);

/// Match one path component against a pattern. Names starting with `.` only
/// match patterns that start with a literal `.`.
pub fn match_segment(pat: &[PatChar], name: &str) -> bool {
    let chars: Vec<char> = name.chars().collect();
    if chars.first() == Some(&'.') && !matches!(pat.first(), Some(('.', false))) {
        return false;
    }
    let (mut p, mut n) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while n < chars.len() {
        match pat.get(p) {
            Some(&('*', true)) => {
                star = Some((p, n));
                p += 1;
            }
            Some(&('?', true)) => {
                p += 1;
                n += 1;
            }
            Some(&(c, _)) if c == chars[n] => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some((sp, sn)) => {
                    p = sp + 1;
                    n = sn + 1;
                    star = Some((sp, sn + 1));
                }
                None => return false,
            },
        }
    }
    while matches!(pat.get(p), Some(&('*', true))) {
        p += 1;
    }
    p == pat.len()
}

fn join_user(base: &str, name: &str) -> String {
    if base.is_empty() {
        String::from(name)
    } else if base.ends_with('/') {
        alloc::format!("{base}{name}")
    } else {
        alloc::format!("{base}/{name}")
    }
}

/// Expand a pattern into sorted existing paths (as typed: relative patterns
/// give relative paths). At most `limit` results; empty when nothing matches.
pub fn expand(fs: &dyn ShellFs, pat: &[PatChar], limit: usize) -> Vec<String> {
    let absolute = pat.first().map(|c| c.0) == Some('/');
    let mut segs: Vec<Vec<PatChar>> = Vec::new();
    let mut cur: Vec<PatChar> = Vec::new();
    for &pc in pat {
        if pc.0 == '/' {
            segs.push(core::mem::take(&mut cur));
        } else {
            cur.push(pc);
        }
    }
    let dir_only = cur.is_empty() && !segs.is_empty();
    if !cur.is_empty() {
        segs.push(cur);
    }
    segs.retain(|s| !s.is_empty());
    if segs.is_empty() {
        return Vec::new();
    }
    let mut bases: Vec<String> = alloc::vec![if absolute {
        String::from("/")
    } else {
        String::new()
    }];
    let last = segs.len() - 1;
    for (idx, seg) in segs.iter().enumerate() {
        let wild = seg.iter().any(|c| c.1);
        let mut next: Vec<String> = Vec::new();
        for base in &bases {
            if !wild {
                let name: String = seg.iter().map(|c| c.0).collect();
                next.push(join_user(base, &name));
            } else {
                let dir = if base.is_empty() { "." } else { base.as_str() };
                let Ok(mut entries) = fs.list(dir) else {
                    continue;
                };
                entries.sort_by(|a, b| a.name.cmp(&b.name));
                for e in entries {
                    if (idx < last || dir_only) && e.kind != Kind::Dir {
                        continue;
                    }
                    if match_segment(seg, &e.name) {
                        next.push(join_user(base, &e.name));
                    }
                }
            }
            if next.len() >= limit {
                next.truncate(limit);
                break;
            }
        }
        bases = next;
        if bases.is_empty() {
            return bases;
        }
    }
    // Literal trailing segments were not checked while walking.
    bases.retain(|p| match fs.stat(p) {
        Ok(s) => !dir_only || s.kind == Kind::Dir,
        Err(_) => false,
    });
    if dir_only {
        for p in &mut bases {
            p.push('/');
        }
    }
    bases
}

/// Why arithmetic failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArithErr {
    DivZero,
    Syntax,
    TooDeep,
}

impl ArithErr {
    /// The catalog key of the text for this error.
    pub const fn key(self) -> &'static str {
        match self {
            ArithErr::DivZero => crate::tk!("sh.arith.div_zero"),
            ArithErr::Syntax => crate::tk!("sh.arith.syntax"),
            ArithErr::TooDeep => crate::tk!("sh.arith.too_deep"),
        }
    }

    /// The text of this error in the language in effect.
    pub fn message(self) -> &'static str {
        crate::i18n::tr(self.key())
    }
}

/// Evaluate an integer expression: `+ - * / %`, comparisons, `&&`, `||`,
/// `!`, unary minus and parentheses. Bare identifiers are looked up with
/// `var` (missing or non-numeric values count as 0). Arithmetic wraps on
/// overflow.
pub fn eval_arith(src: &str, var: &dyn Fn(&str) -> i64) -> Result<i64, ArithErr> {
    let mut p = ArithParser {
        s: src.as_bytes(),
        i: 0,
        var,
        depth: 0,
    };
    p.skip_ws();
    if p.i >= p.s.len() {
        return Ok(0);
    }
    let v = p.or()?;
    p.skip_ws();
    if p.i != p.s.len() {
        return Err(ArithErr::Syntax);
    }
    Ok(v)
}

struct ArithParser<'a> {
    s: &'a [u8],
    i: usize,
    var: &'a dyn Fn(&str) -> i64,
    depth: usize,
}

impl ArithParser<'_> {
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n') {
            self.i += 1;
        }
    }

    fn eat(&mut self, tok: &str) -> bool {
        self.skip_ws();
        if self.s[self.i..].starts_with(tok.as_bytes()) {
            self.i += tok.len();
            true
        } else {
            false
        }
    }

    fn or(&mut self) -> Result<i64, ArithErr> {
        let mut v = self.and()?;
        while self.eat("||") {
            let r = self.and()?;
            v = i64::from(v != 0 || r != 0);
        }
        Ok(v)
    }

    fn and(&mut self) -> Result<i64, ArithErr> {
        let mut v = self.cmp()?;
        while self.eat("&&") {
            let r = self.cmp()?;
            v = i64::from(v != 0 && r != 0);
        }
        Ok(v)
    }

    fn cmp(&mut self) -> Result<i64, ArithErr> {
        let mut v = self.add()?;
        loop {
            let op = if self.eat("==") {
                "=="
            } else if self.eat("!=") {
                "!="
            } else if self.eat("<=") {
                "<="
            } else if self.eat(">=") {
                ">="
            } else if self.eat("<") {
                "<"
            } else if self.eat(">") {
                ">"
            } else {
                return Ok(v);
            };
            let r = self.add()?;
            v = i64::from(match op {
                "==" => v == r,
                "!=" => v != r,
                "<=" => v <= r,
                ">=" => v >= r,
                "<" => v < r,
                _ => v > r,
            });
        }
    }

    fn add(&mut self) -> Result<i64, ArithErr> {
        let mut v = self.mul()?;
        loop {
            if self.eat("+") {
                v = v.wrapping_add(self.mul()?);
            } else if self.eat("-") {
                v = v.wrapping_sub(self.mul()?);
            } else {
                return Ok(v);
            }
        }
    }

    fn mul(&mut self) -> Result<i64, ArithErr> {
        let mut v = self.unary()?;
        loop {
            if self.eat("*") {
                v = v.wrapping_mul(self.unary()?);
            } else if self.eat("/") {
                let r = self.unary()?;
                if r == 0 {
                    return Err(ArithErr::DivZero);
                }
                v = v.wrapping_div(r);
            } else if self.eat("%") {
                let r = self.unary()?;
                if r == 0 {
                    return Err(ArithErr::DivZero);
                }
                v = v.wrapping_rem(r);
            } else {
                return Ok(v);
            }
        }
    }

    fn unary(&mut self) -> Result<i64, ArithErr> {
        self.depth += 1;
        if self.depth > 64 {
            return Err(ArithErr::TooDeep);
        }
        let r = self.unary_inner();
        self.depth -= 1;
        r
    }

    fn unary_inner(&mut self) -> Result<i64, ArithErr> {
        self.skip_ws();
        if self.eat("-") {
            return Ok(self.unary()?.wrapping_neg());
        }
        if self.eat("+") {
            return self.unary();
        }
        if self.eat("!") {
            return Ok(i64::from(self.unary()? == 0));
        }
        if self.eat("(") {
            let v = self.or()?;
            if !self.eat(")") {
                return Err(ArithErr::Syntax);
            }
            return Ok(v);
        }
        self.skip_ws();
        let start = self.i;
        if self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
            let mut v: i64 = 0;
            while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                v = v
                    .wrapping_mul(10)
                    .wrapping_add(i64::from(self.s[self.i] - b'0'));
                self.i += 1;
            }
            return Ok(v);
        }
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_')
        {
            self.i += 1;
        }
        if self.i == start {
            return Err(ArithErr::Syntax);
        }
        let name = core::str::from_utf8(&self.s[start..self.i]).map_err(|_| ArithErr::Syntax)?;
        Ok((self.var)(name))
    }
}

#[cfg(test)]
mod tests;
