//! Pure 4-function calculator with immediate-execution semantics.
//!
//! UI-agnostic: the kernel feeds button/key bytes via [`Calc::input`] and renders
//! the [`Calc::display`] string. All math is `f64`, but the decimal formatter
//! avoids `std`-only float intrinsics (`trunc`/`round`/`abs`) by using integer
//! casts, so it builds under `no_std`.

const ENTRY_MAX: usize = 16;

/// Calculator state machine. Holds the current entry text, an accumulator, and a
/// pending operator (classic infix calculator behavior).
pub struct Calc {
    buf: [u8; ENTRY_MAX],
    len: usize,
    acc: f64,
    pending: Option<u8>,
    /// When true the next digit starts a fresh entry (after an operator or `=`).
    fresh: bool,
    error: bool,
}

impl Default for Calc {
    fn default() -> Self {
        Self::new()
    }
}

impl Calc {
    pub const fn new() -> Self {
        let mut buf = [b' '; ENTRY_MAX];
        buf[0] = b'0';
        Self {
            buf,
            len: 1,
            acc: 0.0,
            pending: None,
            fresh: true,
            error: false,
        }
    }

    /// Current display string: the live entry, or `ERROR` after an invalid op
    /// (overflow / divide-by-zero).
    pub fn display(&self) -> &[u8] {
        if self.error {
            b"ERROR"
        } else {
            &self.buf[..self.len]
        }
    }

    /// Pending operator (`+ - * /`) for UI highlighting, or `None`.
    pub fn operator(&self) -> Option<u8> {
        self.pending
    }

    pub fn is_error(&self) -> bool {
        self.error
    }

    /// Feed one input byte: a digit `0-9`, `.`, an operator `+ - * /`, `=`, or
    /// `c`/`C` to clear. Anything else is ignored.
    pub fn input(&mut self, b: u8) {
        match b {
            b'0'..=b'9' => self.push_digit(b),
            b'.' => self.push_dot(),
            b'+' | b'-' | b'*' | b'/' => self.apply_op(b),
            b'=' => self.equals(),
            b'c' | b'C' => self.clear(),
            _ => {}
        }
    }

    pub fn clear(&mut self) {
        *self = Calc::new();
    }

    /// Erase the last entered character.
    pub fn backspace(&mut self) {
        if self.error {
            self.clear();
            return;
        }
        if self.fresh {
            return;
        }
        if self.len > 1 {
            self.len -= 1;
        } else {
            self.buf[0] = b'0';
            self.len = 1;
            self.fresh = true;
        }
    }

    // ---- input handling ----

    fn push_digit(&mut self, d: u8) {
        if self.error {
            self.clear();
        }
        if self.fresh {
            self.buf[0] = d;
            self.len = 1;
            self.fresh = false;
        } else if self.len == 1 && self.buf[0] == b'0' {
            // Replace a lone leading zero.
            self.buf[0] = d;
        } else if self.len < ENTRY_MAX {
            self.buf[self.len] = d;
            self.len += 1;
        }
    }

    fn push_dot(&mut self) {
        if self.error {
            self.clear();
        }
        if self.fresh {
            self.buf[0] = b'0';
            self.buf[1] = b'.';
            self.len = 2;
            self.fresh = false;
        } else if !self.has_dot() && self.len < ENTRY_MAX {
            self.buf[self.len] = b'.';
            self.len += 1;
        }
    }

    fn has_dot(&self) -> bool {
        self.buf[..self.len].contains(&b'.')
    }

    fn apply_op(&mut self, op: u8) {
        if self.error {
            return;
        }
        if self.pending.is_some() && !self.fresh {
            // Chain: fold the current entry into the accumulator and show it.
            self.commit_pending();
            self.write_entry(self.acc);
        } else if self.pending.is_none() {
            self.acc = self.entry_value();
        }
        // (pending set but fresh = operator pressed twice: just swap the op.)
        if !self.error {
            self.pending = Some(op);
            self.fresh = true;
        }
    }

    fn equals(&mut self) {
        if self.error || self.pending.is_none() {
            return;
        }
        self.commit_pending();
        self.pending = None;
        self.write_entry(self.acc);
        self.fresh = true;
    }

    fn commit_pending(&mut self) {
        let x = self.entry_value();
        self.acc = match self.pending {
            Some(b'+') => self.acc + x,
            Some(b'-') => self.acc - x,
            Some(b'*') => self.acc * x,
            Some(b'/') => self.acc / x, // x == 0 -> inf, caught by write_entry
            _ => x,
        };
    }

    fn entry_value(&self) -> f64 {
        parse_decimal(&self.buf[..self.len])
    }

    /// Render `v` into the entry buffer, or latch `ERROR` for non-finite /
    /// out-of-range results.
    fn write_entry(&mut self, v: f64) {
        // Finite check without `f64::is_finite` (std-only): a NaN/inf has all
        // exponent bits set. `f64::to_bits` is available in `core`.
        let finite = (v.to_bits() >> 52) & 0x7ff != 0x7ff;
        let mag = if v < 0.0 { -v } else { v };
        if !finite || mag >= 1e12 {
            self.error = true;
            self.acc = 0.0;
            self.pending = None;
            return;
        }

        let mut buf = [b' '; ENTRY_MAX];
        let mut idx = 0;
        if v < 0.0 {
            buf[0] = b'-';
            idx = 1;
        }

        // Round to 6 decimals as fixed-point micro-units; the carry into the
        // integer part is handled for free.
        let scaled = (mag * 1_000_000.0 + 0.5) as i64;
        let mut int_part = (scaled / 1_000_000) as u64;
        let frac = (scaled % 1_000_000) as u64;

        // Integer digits, generated low-to-high then reversed.
        let mut digits = [0u8; 20];
        let mut dn = 0;
        if int_part == 0 {
            digits[0] = b'0';
            dn = 1;
        }
        while int_part > 0 {
            digits[dn] = b'0' + (int_part % 10) as u8;
            int_part /= 10;
            dn += 1;
        }
        while dn > 0 {
            dn -= 1;
            if idx < ENTRY_MAX {
                buf[idx] = digits[dn];
                idx += 1;
            }
        }

        // Fractional part: 6 fixed digits, trailing zeros trimmed.
        if frac > 0 {
            let mut fd = [0u8; 6];
            let mut f = frac;
            for k in (0..6).rev() {
                fd[k] = b'0' + (f % 10) as u8;
                f /= 10;
            }
            let mut end = 6;
            while end > 0 && fd[end - 1] == b'0' {
                end -= 1;
            }
            if end > 0 && idx < ENTRY_MAX {
                buf[idx] = b'.';
                idx += 1;
                for &d in fd.iter().take(end) {
                    if idx < ENTRY_MAX {
                        buf[idx] = d;
                        idx += 1;
                    }
                }
            }
        }

        self.buf = buf;
        self.len = idx.max(1);
        self.error = false;
    }
}

/// Parse a `[-]digits[.digits]` byte string into `f64` (no `std` parser).
fn parse_decimal(s: &[u8]) -> f64 {
    let mut i = 0;
    let neg = !s.is_empty() && s[0] == b'-';
    if neg {
        i = 1;
    }
    let mut val = 0.0;
    while i < s.len() && s[i].is_ascii_digit() {
        val = val * 10.0 + (s[i] - b'0') as f64;
        i += 1;
    }
    if i < s.len() && s[i] == b'.' {
        i += 1;
        let mut scale = 0.1;
        while i < s.len() && s[i].is_ascii_digit() {
            val += (s[i] - b'0') as f64 * scale;
            scale *= 0.1;
            i += 1;
        }
    }
    if neg { -val } else { val }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(c: &mut Calc, s: &str) {
        for b in s.bytes() {
            c.input(b);
        }
    }

    fn shown(c: &Calc) -> &str {
        core::str::from_utf8(c.display()).unwrap()
    }

    #[test]
    fn starts_at_zero() {
        assert_eq!(shown(&Calc::new()), "0");
    }

    #[test]
    fn types_digits_replacing_leading_zero() {
        let mut c = Calc::new();
        feed(&mut c, "42");
        assert_eq!(shown(&c), "42");
    }

    #[test]
    fn simple_addition() {
        let mut c = Calc::new();
        feed(&mut c, "2+3=");
        assert_eq!(shown(&c), "5");
    }

    #[test]
    fn subtraction_and_multiplication() {
        let mut c = Calc::new();
        feed(&mut c, "9-4=");
        assert_eq!(shown(&c), "5");
        c.clear();
        feed(&mut c, "6*7=");
        assert_eq!(shown(&c), "42");
    }

    #[test]
    fn chained_operations_fold_left_to_right() {
        let mut c = Calc::new();
        feed(&mut c, "2+3*4="); // (2+3)*4
        assert_eq!(shown(&c), "20");
    }

    #[test]
    fn division_with_decimal_result() {
        let mut c = Calc::new();
        feed(&mut c, "10/4=");
        assert_eq!(shown(&c), "2.5");
    }

    #[test]
    fn decimal_entry() {
        let mut c = Calc::new();
        feed(&mut c, "1.5+2.5=");
        assert_eq!(shown(&c), "4");
    }

    #[test]
    fn only_one_dot_allowed() {
        let mut c = Calc::new();
        feed(&mut c, "1.2.3");
        assert_eq!(shown(&c), "1.23");
    }

    #[test]
    fn leading_dot_becomes_zero_dot() {
        let mut c = Calc::new();
        feed(&mut c, ".5=");
        assert_eq!(shown(&c), "0.5");
    }

    #[test]
    fn divide_by_zero_is_error() {
        let mut c = Calc::new();
        feed(&mut c, "5/0=");
        assert!(c.is_error());
        assert_eq!(shown(&c), "ERROR");
    }

    #[test]
    fn digit_after_error_starts_fresh() {
        let mut c = Calc::new();
        feed(&mut c, "5/0=");
        assert!(c.is_error());
        feed(&mut c, "7");
        assert_eq!(shown(&c), "7");
        assert!(!c.is_error());
    }

    #[test]
    fn clear_resets() {
        let mut c = Calc::new();
        feed(&mut c, "123+45");
        c.clear();
        assert_eq!(shown(&c), "0");
        assert_eq!(c.operator(), None);
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut c = Calc::new();
        feed(&mut c, "123");
        c.backspace();
        assert_eq!(shown(&c), "12");
        c.backspace();
        c.backspace();
        assert_eq!(shown(&c), "0");
    }

    #[test]
    fn negative_result() {
        let mut c = Calc::new();
        feed(&mut c, "3-8=");
        assert_eq!(shown(&c), "-5");
    }

    #[test]
    fn operator_is_exposed_while_pending() {
        let mut c = Calc::new();
        feed(&mut c, "7+");
        assert_eq!(c.operator(), Some(b'+'));
    }

    #[test]
    fn continue_after_equals_uses_result() {
        let mut c = Calc::new();
        feed(&mut c, "2+3=");
        feed(&mut c, "*2=");
        assert_eq!(shown(&c), "10");
    }

    #[test]
    fn overflow_is_error() {
        let mut c = Calc::new();
        feed(&mut c, "999999999*999999=");
        assert!(c.is_error());
    }

    #[test]
    fn parse_decimal_roundtrip() {
        assert!((parse_decimal(b"3.25") - 3.25).abs() < 1e-9);
        assert!((parse_decimal(b"-12") + 12.0).abs() < 1e-9);
        assert!((parse_decimal(b"0") - 0.0).abs() < 1e-9);
    }
}
