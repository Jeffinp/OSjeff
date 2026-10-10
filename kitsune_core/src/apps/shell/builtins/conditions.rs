//! conditions (split out of `builtins.rs`).

use super::*;

pub(super) fn is_unary(s: &str) -> bool {
    matches!(
        s,
        "-e" | "-f" | "-d" | "-s" | "-z" | "-n" | "-r" | "-w" | "-x"
    )
}

pub(super) fn is_binary(s: &str) -> bool {
    matches!(
        s,
        "=" | "==" | "!=" | "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge" | "<" | ">"
    )
}

impl TestEval<'_, '_> {
    pub(super) fn peek(&self) -> Option<&str> {
        self.t.get(self.i).map(String::as_str)
    }

    pub(super) fn or(&mut self) -> Result<bool, &'static str> {
        let mut v = self.and()?;
        while self.peek() == Some("-o") {
            self.i += 1;
            let r = self.and()?;
            v = v || r;
        }
        Ok(v)
    }

    pub(super) fn and(&mut self) -> Result<bool, &'static str> {
        let mut v = self.not()?;
        while self.peek() == Some("-a") {
            self.i += 1;
            let r = self.not()?;
            v = v && r;
        }
        Ok(v)
    }

    pub(super) fn not(&mut self) -> Result<bool, &'static str> {
        if self.peek() == Some("!") && self.i + 1 < self.t.len() {
            self.i += 1;
            return Ok(!self.not()?);
        }
        self.primary()
    }

    pub(super) fn primary(&mut self) -> Result<bool, &'static str> {
        let Some(tok) = self.peek().map(String::from) else {
            return Err(tk!("sh.test.arg_expected"));
        };
        if tok == "(" {
            self.i += 1;
            let v = self.or()?;
            if self.peek() != Some(")") {
                return Err(tk!("sh.test.missing_paren"));
            }
            self.i += 1;
            return Ok(v);
        }
        let next_is_binary = self.t.get(self.i + 1).is_some_and(|s| is_binary(s));
        if is_unary(&tok)
            && self.i + 1 < self.t.len()
            && !(next_is_binary && self.i + 2 < self.t.len())
        {
            let arg = self.t[self.i + 1].clone();
            self.i += 2;
            return Ok(match tok.as_str() {
                "-z" => arg.is_empty(),
                "-n" => !arg.is_empty(),
                "-e" | "-r" | "-w" | "-x" => self.cx.fs.stat(&arg).is_ok(),
                "-f" => self.cx.fs.stat(&arg).is_ok_and(|s| s.kind == Kind::File),
                "-d" => self.cx.fs.stat(&arg).is_ok_and(|s| s.kind == Kind::Dir),
                _ => self.cx.fs.stat(&arg).is_ok_and(|s| s.size > 0),
            });
        }
        if next_is_binary && self.i + 2 < self.t.len() {
            let op = self.t[self.i + 1].clone();
            let rhs = self.t[self.i + 2].clone();
            self.i += 3;
            let ints = |a: &str, b: &str| -> Result<(i64, i64), &'static str> {
                match (parse_num(a), parse_num(b)) {
                    (Some(x), Some(y)) => Ok((x, y)),
                    _ => Err(tk!("sh.test.int_expected")),
                }
            };
            return Ok(match op.as_str() {
                "=" | "==" => tok == rhs,
                "!=" => tok != rhs,
                "<" => tok < rhs,
                ">" => tok > rhs,
                "-eq" => {
                    let (a, b) = ints(&tok, &rhs)?;
                    a == b
                }
                "-ne" => {
                    let (a, b) = ints(&tok, &rhs)?;
                    a != b
                }
                "-lt" => {
                    let (a, b) = ints(&tok, &rhs)?;
                    a < b
                }
                "-le" => {
                    let (a, b) = ints(&tok, &rhs)?;
                    a <= b
                }
                "-gt" => {
                    let (a, b) = ints(&tok, &rhs)?;
                    a > b
                }
                _ => {
                    let (a, b) = ints(&tok, &rhs)?;
                    a >= b
                }
            });
        }
        self.i += 1;
        Ok(!tok.is_empty())
    }
}

pub(super) fn run_test(cx: &mut CmdCtx<'_>, tokens: &[String]) -> i32 {
    if tokens.is_empty() {
        return 1;
    }
    let mut ev = TestEval {
        t: tokens,
        i: 0,
        cx,
    };
    match ev.or() {
        Ok(v) if ev.i == tokens.len() => i32::from(!v),
        Ok(_) => {
            ev.cx.error(t!("sh.err.too_many_args"));
            2
        }
        Err(m) => {
            ev.cx.error(i18n::tr(m));
            2
        }
    }
}

pub(super) fn test(cx: &mut CmdCtx<'_>) -> i32 {
    let t: Vec<String> = cx.args[1..].to_vec();
    run_test(cx, &t)
}

pub(super) fn bracket(cx: &mut CmdCtx<'_>) -> i32 {
    let mut t: Vec<String> = cx.args[1..].to_vec();
    if t.last().map(String::as_str) != Some("]") {
        cx.error(t!("sh.test.missing_bracket"));
        return 2;
    }
    t.pop();
    run_test(cx, &t)
}
