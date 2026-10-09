//! Pure logic of the Busca overlay: ranking of names against a query and the
//! calculator that answers arithmetic typed into the field.
//!
//! The calculator is exact decimal arithmetic on `i128` scaled by 10^6 (no floating
//! point): `+ - * / %`, unary minus, parentheses, decimal points or commas, results
//! with up to six decimals, trailing zeros removed.

use alloc::string::String;

const SCALE: i128 = 1_000_000;
const MAX_ABS: i128 = 1_000_000_000_000 * SCALE;

/// How well `name` matches `query` (both compared case-insensitively, ASCII and
/// Latin-1 letters folded): `Some(0)` prefix of the name, `Some(1)` prefix of a word,
/// `Some(2)` anywhere, `None` no match. An empty query matches everything (rank 2).
pub fn rank(query: &str, name: &str) -> Option<u8> {
    let q = fold(query);
    let n = fold(name);
    if q.is_empty() {
        return Some(2);
    }
    if n.starts_with(q.as_str()) {
        return Some(0);
    }
    if n.split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(q.as_str()))
    {
        return Some(1);
    }
    n.contains(q.as_str()).then_some(2)
}

/// Lower-case with the Portuguese accents removed.
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

/// Evaluate an arithmetic expression; `None` when it is not one (so ordinary
/// searches never turn into answers) or it overflows / divides by zero.
pub fn eval(expr: &str) -> Option<String> {
    let mut p = Parser {
        s: expr.as_bytes(),
        i: 0,
        depth: 0,
    };
    // Needs at least one operator or parenthesis to count as a calculation.
    if !expr
        .bytes()
        .any(|b| matches!(b, b'+' | b'-' | b'*' | b'/' | b'%' | b'(' | b'x' | b'X'))
    {
        return None;
    }
    let v = p.expr()?;
    p.skip();
    if p.i != p.s.len() || v.abs() > MAX_ABS {
        return None;
    }
    Some(format(v))
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    depth: u8,
}

impl Parser<'_> {
    fn skip(&mut self) {
        while self.s.get(self.i) == Some(&b' ') {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.s.get(self.i).copied()
    }

    fn expr(&mut self) -> Option<i128> {
        let mut v = self.term()?;
        while let Some(op @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            let r = self.term()?;
            v = if op == b'+' {
                v.checked_add(r)?
            } else {
                v.checked_sub(r)?
            };
            if v.abs() > MAX_ABS {
                return None;
            }
        }
        Some(v)
    }

    fn term(&mut self) -> Option<i128> {
        let mut v = self.factor()?;
        while let Some(op @ (b'*' | b'/' | b'%' | b'x' | b'X')) = self.peek() {
            self.i += 1;
            let r = self.factor()?;
            v = match op {
                b'/' => {
                    if r == 0 {
                        return None;
                    }
                    v.checked_mul(SCALE)?.checked_div(r)?
                }
                b'%' => {
                    if r == 0 {
                        return None;
                    }
                    v.checked_rem(r)?
                }
                _ => v.checked_mul(r)? / SCALE,
            };
            if v.abs() > MAX_ABS {
                return None;
            }
        }
        Some(v)
    }

    fn factor(&mut self) -> Option<i128> {
        match self.peek()? {
            b'-' => {
                self.i += 1;
                self.factor().map(|v| -v)
            }
            b'+' => {
                self.i += 1;
                self.factor()
            }
            b'(' => {
                self.i += 1;
                self.depth += 1;
                if self.depth > 16 {
                    return None;
                }
                let v = self.expr()?;
                if self.peek()? != b')' {
                    return None;
                }
                self.i += 1;
                self.depth -= 1;
                Some(v)
            }
            b'0'..=b'9' | b'.' | b',' => self.number(),
            _ => None,
        }
    }

    fn number(&mut self) -> Option<i128> {
        let mut int: i128 = 0;
        let mut seen = false;
        while let Some(&b @ b'0'..=b'9') = self.s.get(self.i) {
            int = int.checked_mul(10)?.checked_add((b - b'0') as i128)?;
            if int > MAX_ABS {
                return None;
            }
            self.i += 1;
            seen = true;
        }
        let mut frac: i128 = 0;
        let mut digits = 0;
        if matches!(self.s.get(self.i), Some(b'.' | b',')) {
            self.i += 1;
            while let Some(&b @ b'0'..=b'9') = self.s.get(self.i) {
                if digits < 6 {
                    frac = frac * 10 + (b - b'0') as i128;
                    digits += 1;
                }
                self.i += 1;
                seen = true;
            }
        }
        if !seen {
            return None;
        }
        for _ in digits..6 {
            frac *= 10;
        }
        Some(int.checked_mul(SCALE)? + frac)
    }
}

fn format(v: i128) -> String {
    let neg = v < 0;
    let a = v.unsigned_abs();
    let (int, frac) = (a / SCALE as u128, a % SCALE as u128);
    let mut s = String::new();
    if neg && a != 0 {
        s.push('-');
    }
    s.push_str(&alloc::format!("{int}"));
    if frac != 0 {
        let mut f = alloc::format!("{frac:06}");
        while f.ends_with('0') {
            f.pop();
        }
        s.push(',');
        s.push_str(&f);
    }
    s
}

#[cfg(test)]
mod tests;
