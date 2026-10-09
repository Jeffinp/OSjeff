//! Message templates: `{name}` / `{0}` placeholders filled with typed arguments.
//!
//! The grammar is deliberately tiny (ICU-lite) so that a hostile or merely wrong template
//! can only produce odd text, never a panic or unbounded work:
//!
//! * `{name}` takes the argument called `name`; `{0}`, `{1}`... take the argument at that
//!   *position* in the slice (so `&[("n", Arg::Int(3))]` answers both `{n}` and `{0}`);
//! * `{{` and `}}` are a literal `{` and `}`;
//! * a name whose first letter is upper case (`{Weekday_long}`) and that is not an argument
//!   itself prints the argument with the lower-case first letter and capitalises the first
//!   character of the result (Portuguese month and weekday names are lower case inside a
//!   sentence and capitalised at its start);
//! * an unterminated `{`, an empty `{}`, a name that is not in the slice or that is longer
//!   than [`MAX_NAME`] is written back **verbatim**, so the problem is visible on screen and
//!   rendering continues;
//! * a lone `}` is written as is.
//!
//! How a value prints is decided by its [`Arg`] type, not by a format spec in the template, so
//! a translator can reorder placeholders but never change what they mean:
//!
//! | `Arg` | Prints | pt | en |
//! |---|---|---|---|
//! | `Str(&str)` | the text | | |
//! | `Int(i64)` | plain digits (ids, ports, pixels) | `1234` | `1234` |
//! | `Num(i64)` | a quantity, grouped | `1.234` | `1,234` |
//! | `Pad(u64, w)` | zero padded to `w` digits | `07` | `07` |
//! | `Dec(v, places)` | `v / 10^places`, grouped | `1.234,5` | `1,234.5` |
//! | `Bytes(u64)` | a size, one decimal | `1,5 KiB` | `1.5 KiB` |
//! | `Display(&dyn Display)` | whatever it prints | | |

use super::{Lang, locale};
use core::fmt::{self, Write};

/// Longest placeholder name accepted; anything longer is written back as text.
pub const MAX_NAME: usize = 32;

/// A value for a placeholder. See the module docs for how each prints.
#[derive(Clone, Copy)]
pub enum Arg<'a> {
    Str(&'a str),
    Int(i64),
    Num(i64),
    Pad(u64, u8),
    Dec(i64, u8),
    Bytes(u64),
    Display(&'a dyn fmt::Display),
}

impl fmt::Debug for Arg<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Arg::Str(s) => write!(f, "Str({s:?})"),
            Arg::Int(n) => write!(f, "Int({n})"),
            Arg::Num(n) => write!(f, "Num({n})"),
            Arg::Pad(n, w) => write!(f, "Pad({n}, {w})"),
            Arg::Dec(n, p) => write!(f, "Dec({n}, {p})"),
            Arg::Bytes(n) => write!(f, "Bytes({n})"),
            Arg::Display(_) => f.write_str("Display(..)"),
        }
    }
}

impl<'a> From<&'a str> for Arg<'a> {
    fn from(s: &'a str) -> Self {
        Arg::Str(s)
    }
}
impl<'a> From<&'a alloc::string::String> for Arg<'a> {
    fn from(s: &'a alloc::string::String) -> Self {
        Arg::Str(s.as_str())
    }
}
macro_rules! arg_from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for Arg<'_> {
            fn from(n: $t) -> Self {
                Arg::Int(n as i64)
            }
        }
    )*};
}
arg_from_int!(i8, i16, i32, i64, isize, u8, u16, u32, usize);
impl From<u64> for Arg<'_> {
    fn from(n: u64) -> Self {
        Arg::Int(i64::try_from(n).unwrap_or(i64::MAX))
    }
}

/// A named argument list: `&[("n", Arg::Int(3)), ("name", Arg::Str("x"))]`.
pub type Args<'a> = &'a [(&'a str, Arg<'a>)];

/// A quantity printed with the language's thousands separator.
pub const fn num<'a>(n: i64) -> Arg<'a> {
    Arg::Num(n)
}

/// `scaled / 10^places` printed with the language's decimal separator.
pub const fn dec<'a>(scaled: i64, places: u8) -> Arg<'a> {
    Arg::Dec(scaled, places)
}

/// A size in bytes printed as `512 B`, `1,5 KiB`.
pub const fn bytes<'a>(n: u64) -> Arg<'a> {
    Arg::Bytes(n)
}

fn write_arg<W: Write>(w: &mut W, lang: Lang, a: &Arg<'_>) -> fmt::Result {
    match *a {
        Arg::Str(s) => w.write_str(s),
        Arg::Int(n) => write!(w, "{n}"),
        Arg::Num(n) => locale::write_int(w, lang, n, true),
        Arg::Pad(n, width) => write!(w, "{:0>width$}", n, width = width.min(20) as usize),
        Arg::Dec(v, places) => locale::write_dec(w, lang, v, places),
        Arg::Bytes(n) => locale::write_size(w, lang, n),
        Arg::Display(d) => write!(w, "{d}"),
    }
}

fn find<'a>(args: Args<'a>, name: &str) -> Option<&'a Arg<'a>> {
    if let Some(a) = args.iter().find(|(n, _)| *n == name) {
        return Some(&a.1);
    }
    // Positional: all digits, and short enough that it cannot overflow.
    if !name.is_empty() && name.len() <= 4 && name.bytes().all(|b| b.is_ascii_digit()) {
        let idx: usize = name.parse().ok()?;
        return args.get(idx).map(|a| &a.1);
    }
    None
}

/// For `Name` where `name` is an argument: that argument.
fn find_capitalised<'a>(args: Args<'a>, name: &str) -> Option<&'a Arg<'a>> {
    let first = *name.as_bytes().first()?;
    if !first.is_ascii_uppercase() || name.len() > MAX_NAME {
        return None;
    }
    let mut buf = [0u8; MAX_NAME];
    buf[..name.len()].copy_from_slice(name.as_bytes());
    buf[0] = first.to_ascii_lowercase();
    let lower = core::str::from_utf8(&buf[..name.len()]).ok()?;
    args.iter().find(|(n, _)| *n == lower).map(|a| &a.1)
}

/// A writer that upper-cases the first character written through it.
struct CapFirst<'w, W: Write> {
    w: &'w mut W,
    done: bool,
}

impl<W: Write> Write for CapFirst<'_, W> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.done {
            return self.w.write_str(s);
        }
        let mut it = s.chars();
        match it.next() {
            None => Ok(()),
            Some(c) => {
                self.done = true;
                for u in c.to_uppercase() {
                    self.w.write_char(u)?;
                }
                self.w.write_str(it.as_str())
            }
        }
    }
}

/// Write `template` with its placeholders filled from `args`, numbers and sizes formatted
/// for `lang`. Never panics; see the module docs for malformed input.
pub fn render<W: Write>(w: &mut W, lang: Lang, template: &str, args: Args<'_>) -> fmt::Result {
    let mut rest = template;
    while let Some(i) = rest.find(['{', '}']) {
        w.write_str(&rest[..i])?;
        let tail = &rest[i..];
        // `{` and `}` are ASCII, so the byte offsets below are char boundaries.
        let b = tail.as_bytes();
        if b[0] == b'}' {
            // `}}` is one brace; a lone one is kept.
            w.write_char('}')?;
            rest = if b.get(1) == Some(&b'}') {
                &tail[2..]
            } else {
                &tail[1..]
            };
            continue;
        }
        if b.get(1) == Some(&b'{') {
            w.write_char('{')?;
            rest = &tail[2..];
            continue;
        }
        // A placeholder: the name runs to the next `}`, and may not contain a `{`.
        let end = tail[1..]
            .find(['}', '{'])
            .filter(|&e| tail.as_bytes()[1 + e] == b'}');
        match end {
            Some(e) => {
                let name = &tail[1..1 + e];
                if name.len() > MAX_NAME {
                    w.write_str(&tail[..e + 2])?;
                } else if let Some(a) = find(args, name) {
                    write_arg(w, lang, a)?;
                } else if let Some(a) = find_capitalised(args, name) {
                    write_arg(&mut CapFirst { w, done: false }, lang, a)?;
                } else {
                    w.write_str(&tail[..e + 2])?;
                }
                rest = &tail[e + 2..];
            }
            None => {
                // Unterminated: keep the brace as text and go on after it.
                w.write_char('{')?;
                rest = &tail[1..];
            }
        }
    }
    w.write_str(rest)
}

/// A template plus its arguments as a [`Display`](fmt::Display) value, so a message can be
/// used in `write!` / `format!` without an intermediate `String`.
pub struct Rendered<'a> {
    pub lang: Lang,
    pub template: &'a str,
    pub args: Args<'a>,
}

impl fmt::Display for Rendered<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        render(f, self.lang, self.template, self.args)
    }
}

/// The names of the placeholders in `template` in order of appearance (for the catalog
/// parity checks). Escaped braces and malformed placeholders are not placeholders.
pub fn placeholders(template: &str) -> alloc::vec::Vec<&str> {
    let mut out = alloc::vec::Vec::new();
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        let tail = &rest[i..];
        if tail.as_bytes().get(1) == Some(&b'{') {
            rest = &tail[2..];
            continue;
        }
        match tail[1..].find(['}', '{']) {
            Some(e) if tail.as_bytes()[1 + e] == b'}' && e > 0 => {
                out.push(&tail[1..1 + e]);
                rest = &tail[e + 2..];
            }
            _ => rest = &tail[1..],
        }
    }
    out
}

#[cfg(test)]
mod tests;
