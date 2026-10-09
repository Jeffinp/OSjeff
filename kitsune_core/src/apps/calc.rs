//! Pure calculator with immediate-execution semantics: the four operations, percent,
//! sign change, one memory register and a short history of finished operations.
//!
//! UI-agnostic: the kernel feeds button/key bytes via [`Calc::input`] and renders
//! the [`Calc::display`] string. All math is `f64`, but the decimal formatter
//! avoids `std`-only float intrinsics (`trunc`/`round`/`abs`) by using integer
//! casts, so it builds under `no_std`.

const ENTRY_MAX: usize = 16;
/// Finished operations kept for the history strip.
pub const HISTORY: usize = 4;
/// Longest history line: `a op b = result`.
const LINE_MAX: usize = 56;

/// Input bytes of the memory keys (they never collide with typed characters).
pub const KEY_MC: u8 = 0x01;
pub const KEY_MR: u8 = 0x02;
pub const KEY_MSUB: u8 = 0x03;
pub const KEY_MADD: u8 = 0x04;
/// The sign-change key.
pub const KEY_NEG: u8 = b'n';
/// The percent key.
pub const KEY_PCT: u8 = b'%';

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
    /// The memory register and whether anything was stored in it.
    mem: f64,
    mem_set: bool,
    /// The last finished operations, newest in slot `hist_head - 1` (a ring).
    hist: [[u8; LINE_MAX]; HISTORY],
    hist_len: [u8; HISTORY],
    hist_head: usize,
    hist_count: usize,
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
            mem: 0.0,
            mem_set: false,
            hist: [[0; LINE_MAX]; HISTORY],
            hist_len: [0; HISTORY],
            hist_head: 0,
            hist_count: 0,
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

    /// Feed one input byte: a digit `0-9`, `.` (or `,`), an operator `+ - * /`, `=`,
    /// `%`, `n` (change sign), a memory key (`KEY_MC`..`KEY_MADD`), or `c`/`C` to clear.
    /// Anything else is ignored.
    pub fn input(&mut self, b: u8) {
        match b {
            b'0'..=b'9' => self.push_digit(b),
            b'.' | b',' => self.push_dot(),
            b'+' | b'-' | b'*' | b'/' => self.apply_op(b),
            b'=' => self.equals(),
            KEY_PCT => self.percent(),
            KEY_NEG | b'N' => self.negate(),
            KEY_MC => self.memory_clear(),
            KEY_MR => self.memory_recall(),
            KEY_MSUB => self.memory_add(-1.0),
            KEY_MADD => self.memory_add(1.0),
            b'c' | b'C' => self.clear(),
            _ => {}
        }
    }

    /// Clear the entry, the pending operation and the history; the memory stays (as on a
    /// pocket calculator, `MC` clears that).
    pub fn clear(&mut self) {
        let (mem, mem_set) = (self.mem, self.mem_set);
        *self = Calc::new();
        self.mem = mem;
        self.mem_set = mem_set;
    }

    /// Is something stored in the memory register?
    pub fn has_memory(&self) -> bool {
        self.mem_set
    }

    /// Empty the memory register.
    fn memory_clear(&mut self) {
        self.mem = 0.0;
        self.mem_set = false;
    }

    /// Show the memory register as the current entry.
    fn memory_recall(&mut self) {
        if !self.mem_set {
            return;
        }
        if self.error {
            self.clear();
        }
        let v = self.mem;
        self.write_entry(v);
        self.fresh = true;
    }

    /// Add (`sign` = 1) or subtract (`sign` = -1) the entry to the memory register.
    fn memory_add(&mut self, sign: f64) {
        if self.error {
            return;
        }
        self.mem += sign * self.entry_value();
        self.mem_set = true;
        self.fresh = true;
    }

    /// Percent: with a pending `+` or `-` the entry becomes that share of the left operand
    /// (`200 + 10 %` adds 20); otherwise it is divided by 100.
    pub fn percent(&mut self) {
        if self.error {
            return;
        }
        let x = self.entry_value();
        let v = match self.pending {
            Some(b'+') | Some(b'-') => self.acc * x / 100.0,
            _ => x / 100.0,
        };
        self.write_entry(v);
        self.fresh = false;
    }

    /// Change the sign of the entry (`0` stays `0`).
    pub fn negate(&mut self) {
        if self.error || (self.len == 1 && self.buf[0] == b'0') {
            return;
        }
        if self.buf[0] == b'-' {
            self.buf.copy_within(1..self.len, 0);
            self.len -= 1;
        } else if self.len < ENTRY_MAX {
            self.buf.copy_within(0..self.len, 1);
            self.buf[0] = b'-';
            self.len += 1;
        }
    }

    /// The pending expression for the strip above the display (`12 *`), if any.
    pub fn pending_text(&self) -> Option<([u8; ENTRY_MAX + 2], usize)> {
        let op = self.pending?;
        let (n, nl) = render(self.acc)?;
        let mut out = [b' '; ENTRY_MAX + 2];
        out[..nl].copy_from_slice(&n[..nl]);
        out[nl + 1] = op;
        Some((out, nl + 2))
    }

    /// The finished operations, oldest first: `12 * 3 = 36` (ASCII, `.` decimal point).
    pub fn history(&self) -> impl Iterator<Item = &[u8]> + '_ {
        let n = self.hist_count.min(HISTORY);
        (0..n).map(move |i| {
            let slot = (self.hist_head + HISTORY - n + i) % HISTORY;
            &self.hist[slot][..self.hist_len[slot] as usize]
        })
    }

    fn push_history(&mut self, a: f64, op: u8, b: f64, r: f64) {
        let (Some((an, al)), Some((bn, bl)), Some((rn, rl))) = (render(a), render(b), render(r))
        else {
            return;
        };
        let mut line = [0u8; LINE_MAX];
        let mut n = 0;
        for part in [&an[..al], b" ", &[op], b" ", &bn[..bl], b" = ", &rn[..rl]] {
            for &c in part {
                if n < LINE_MAX {
                    line[n] = c;
                    n += 1;
                }
            }
        }
        self.hist[self.hist_head] = line;
        self.hist_len[self.hist_head] = n as u8;
        self.hist_head = (self.hist_head + 1) % HISTORY;
        self.hist_count = (self.hist_count + 1).min(HISTORY);
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
            self.commit_pending_recorded();
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
        self.commit_pending_recorded();
        self.pending = None;
        self.write_entry(self.acc);
        self.fresh = true;
    }

    /// Finish the pending operation and remember it in the history (unless it failed).
    fn commit_pending_recorded(&mut self) {
        let (a, op, b) = (self.acc, self.pending.unwrap_or(b'+'), self.entry_value());
        self.commit_pending();
        if (self.acc.to_bits() >> 52) & 0x7ff != 0x7ff {
            self.push_history(a, op, b, self.acc);
        }
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
        match render(v) {
            Some((buf, len)) => {
                self.buf = buf;
                self.len = len;
                self.error = false;
            }
            None => {
                self.error = true;
                self.acc = 0.0;
                self.pending = None;
            }
        }
    }
}

/// Format `v` as `[-]digits[.digits]` (6 decimals at most, trailing zeros trimmed), or
/// `None` when it is not finite or too large for the display.
fn render(v: f64) -> Option<([u8; ENTRY_MAX], usize)> {
    // Finite check without `f64::is_finite` (std-only): a NaN/inf has all exponent bits set.
    let finite = (v.to_bits() >> 52) & 0x7ff != 0x7ff;
    let mag = if v < 0.0 { -v } else { v };
    if !finite || mag >= 1e12 {
        return None;
    }
    let mut buf = [b' '; ENTRY_MAX];
    let mut idx = 0;
    // Round to 6 decimals as fixed-point micro-units; the carry into the integer part is
    // handled for free. A value that rounds to zero loses its sign.
    let scaled = (mag * 1_000_000.0 + 0.5) as i64;
    if v < 0.0 && scaled != 0 {
        buf[0] = b'-';
        idx = 1;
    }
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
    Some((buf, idx.max(1)))
}

/// A display string for people, in the language in effect: the language's decimal
/// separator and thousands grouping, `Erro`/`Error` for the error state
/// (`"-1234567.5"` -> `"-1.234.567,5"` or `"-1,234,567.5"`, `"5."` -> `"5,"`). Text that
/// is not a number is returned as it is.
pub fn pretty(display: &[u8]) -> alloc::string::String {
    use alloc::string::String;
    if display == b"ERROR" {
        return String::from(crate::t!("calc.error"));
    }
    let (dec_sep, group_sep) = (
        crate::i18n::locale::decimal_sep(crate::i18n::lang()),
        crate::i18n::locale::group_sep(crate::i18n::lang()),
    );
    let mut out = String::new();
    let (neg, rest) = match display.split_first() {
        Some((b'-', r)) => (true, r),
        _ => (false, display),
    };
    let dot = rest.iter().position(|&b| b == b'.');
    let (int, frac) = match dot {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    if !int.iter().all(u8::is_ascii_digit) || !frac.is_none_or(|f| f.iter().all(u8::is_ascii_digit))
    {
        return String::from_utf8_lossy(display).into_owned();
    }
    if neg {
        out.push('-');
    }
    for (i, &d) in int.iter().enumerate() {
        if i > 0 && (int.len() - i).is_multiple_of(3) {
            out.push(group_sep);
        }
        out.push(d as char);
    }
    if let Some(f) = frac {
        out.push(dec_sep);
        out.extend(f.iter().map(|&d| d as char));
    }
    out
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
mod tests;
