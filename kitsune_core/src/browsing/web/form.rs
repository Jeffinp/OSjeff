//! HTML forms: what the parser finds ([`FormInfo`], [`FieldInfo`]), the state
//! the user edits ([`FormState`]), dead-key accent composition ([`Compose`])
//! and the GET query ([`FormState::target`]).
//!
//! Submitting builds `name=value&...` with `application/x-www-form-urlencoded`
//! escaping of the UTF-8 bytes: for `method=get` the browser navigates to
//! `action?query`, for `method=post` it sends the query as the request body
//! ([`FormState::submission`]). Supported controls: text-like inputs (`text`,
//! `search`, `url`, `email`, `tel`, `number` and no type), `password`,
//! `hidden`, `submit` and `<button>`, check boxes and radio buttons (a radio
//! group is the buttons of one form that share a `name`), `<select>` (a single
//! choice, changed by click or keys: there is no drop-down list) and `<textarea>`
//! (several lines; Enter inserts a line break). Files are not drawn.

use crate::Key;
use crate::tk;
use alloc::string::String;
use alloc::vec::Vec;

/// Forms recorded per page.
pub const MAX_FORMS: usize = 16;
/// Controls recorded per form.
pub const MAX_FIELDS: usize = 32;
/// Longest value a text control holds, in bytes.
pub const MAX_VALUE: usize = 256;
/// Longest text a `<textarea>` holds, in bytes.
pub const MAX_TEXTAREA: usize = 4096;
/// Options kept per `<select>`.
pub const MAX_OPTIONS: usize = 256;
/// Longest query string built from a form for a GET, in bytes.
pub const MAX_QUERY: usize = 380;
/// Longest body a POST form sends, in bytes.
pub const MAX_POST: usize = 16 * 1024;

/// The kinds of control we draw and submit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// A one-line text box.
    Text,
    /// A text box that shows bullets.
    Password,
    /// Not drawn; always submitted.
    Hidden,
    /// A button; submitted only when it is the one pressed.
    Submit,
    /// A button that submits nothing (`type=button`, `type=reset`).
    PushButton,
    /// A check box: submitted (with its `value`) when checked.
    Checkbox,
    /// A radio button: one per group is checked.
    Radio,
    /// A `<select>`: one of its options is chosen.
    Select,
    /// A multi-line text box.
    TextArea,
}

impl FieldKind {
    /// Can the user type into it?
    pub fn is_text(self) -> bool {
        matches!(
            self,
            FieldKind::Text | FieldKind::Password | FieldKind::TextArea
        )
    }
}

/// One `<option>` of a `<select>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectOption {
    /// What is sent (the `value` attribute, else the text).
    pub value: String,
    /// What is shown.
    pub label: String,
}

/// One control of a form, as written in the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldInfo {
    pub name: String,
    pub kind: FieldKind,
    /// The `value` attribute (the initial text of a text box).
    pub value: String,
    /// Width in characters of a text box.
    pub size: usize,
    /// The text on a button.
    pub label: String,
    /// The `checked` attribute (check boxes and radio buttons).
    pub checked: bool,
    /// The `placeholder` attribute of a text box.
    pub placeholder: String,
    /// The options of a `<select>`.
    pub options: Vec<SelectOption>,
    /// The option a `<select>` starts on.
    pub selected: usize,
    /// Visible lines of a `<textarea>`.
    pub rows: usize,
}

/// A `<form>` and its controls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormInfo {
    /// `method=post`.
    pub post: bool,
    /// The `action` attribute (empty = the current page).
    pub action: String,
    pub fields: Vec<FieldInfo>,
}

impl FormInfo {
    /// A form from its `method` and `action` attributes.
    pub fn from_attrs(method: Option<&str>, action: Option<String>) -> FormInfo {
        FormInfo {
            post: method.is_some_and(|m| m.trim().eq_ignore_ascii_case("post")),
            action: action.unwrap_or_default(),
            fields: Vec::new(),
        }
    }
}

/// Why a form could not be submitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormError {
    /// The query would not fit in a URL (or the body in a request).
    TooLong,
    /// No such form.
    NoForm,
}

impl FormError {
    /// Catalog key of the message shown to the user.
    pub fn message_key(self) -> &'static str {
        match self {
            FormError::TooLong => tk!("web.form.too_long"),
            FormError::NoForm => tk!("web.form.invalid"),
        }
    }

    /// The message in the language in effect.
    pub fn message(self) -> &'static str {
        crate::i18n::tr(self.message_key())
    }
}

/// Percent-encode `s` as an `application/x-www-form-urlencoded` component:
/// letters, digits and `*-._` stay, a space is `+`, every other UTF-8 byte is
/// `%XX` (upper-case hex).
pub fn urlencode(s: &str, out: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 15) as usize] as char);
            }
        }
    }
}

// ---- dead-key composition ----

/// US-International style accents for typing Portuguese on a US keyboard:
/// `'` `` ` `` `~` `^` `"` are *dead keys*. Typed before a vowel (or `c`, `n`,
/// `y`) they make the accented letter (`'` then `a` is `\u{e1}`, `'` then `c`
/// is `\u{e7}`); before a space or the same key they give the plain
/// character; before anything else both come out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Compose {
    pending: Option<char>,
}

/// What a keystroke produced: up to two characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Composed {
    chars: [Option<char>; 2],
}

impl Composed {
    fn none() -> Self {
        Composed { chars: [None; 2] }
    }
    fn one(a: char) -> Self {
        Composed {
            chars: [Some(a), None],
        }
    }
    fn two(a: char, b: char) -> Self {
        Composed {
            chars: [Some(a), Some(b)],
        }
    }
    /// The produced characters, in order.
    pub fn iter(&self) -> impl Iterator<Item = char> + '_ {
        self.chars.iter().flatten().copied()
    }
}

fn is_dead(c: char) -> bool {
    matches!(c, '\'' | '`' | '~' | '^' | '"')
}

/// The accented letter for dead key `d` followed by `c`, if there is one.
pub fn accent(d: char, c: char) -> Option<char> {
    let (lower, upper): (&str, &str) = match d {
        '\'' => (
            "a\u{e1}e\u{e9}i\u{ed}o\u{f3}u\u{fa}c\u{e7}y\u{fd}",
            "A\u{c1}E\u{c9}I\u{cd}O\u{d3}U\u{da}C\u{c7}Y\u{dd}",
        ),
        '`' => (
            "a\u{e0}e\u{e8}i\u{ec}o\u{f2}u\u{f9}",
            "A\u{c0}E\u{c8}I\u{cc}O\u{d2}U\u{d9}",
        ),
        '~' => ("a\u{e3}o\u{f5}n\u{f1}", "A\u{c3}O\u{d5}N\u{d1}"),
        '^' => (
            "a\u{e2}e\u{ea}i\u{ee}o\u{f4}u\u{fb}",
            "A\u{c2}E\u{ca}I\u{ce}O\u{d4}U\u{db}",
        ),
        '"' => (
            "a\u{e4}e\u{eb}i\u{ef}o\u{f6}u\u{fc}y\u{ff}",
            "A\u{c4}E\u{cb}I\u{cf}O\u{d6}U\u{dc}",
        ),
        _ => return None,
    };
    for table in [lower, upper] {
        let mut it = table.chars();
        while let (Some(base), Some(acc)) = (it.next(), it.next()) {
            if base == c {
                return Some(acc);
            }
        }
    }
    None
}

impl Compose {
    pub const fn new() -> Self {
        Compose { pending: None }
    }

    /// True while a dead key is waiting for the next character.
    pub fn pending(&self) -> Option<char> {
        self.pending
    }

    /// Feed one typed character.
    pub fn feed(&mut self, c: char) -> Composed {
        match self.pending.take() {
            None if is_dead(c) => {
                self.pending = Some(c);
                Composed::none()
            }
            None => Composed::one(c),
            Some(d) => {
                if c == ' ' || c == d {
                    Composed::one(d)
                } else if let Some(a) = accent(d, c) {
                    Composed::one(a)
                } else if is_dead(c) {
                    self.pending = Some(c);
                    Composed::one(d)
                } else {
                    Composed::two(d, c)
                }
            }
        }
    }

    /// A non-character key arrived: a waiting dead key comes out as itself.
    pub fn flush(&mut self) -> Composed {
        match self.pending.take() {
            Some(d) => Composed::one(d),
            None => Composed::none(),
        }
    }
}

// ---- editing state ----

/// What a key did to the focused control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormOutcome {
    /// Not handled (no focused control, or a key that means nothing here).
    Ignored,
    /// The control changed (repaint).
    Changed,
    /// Enter / a button: submit `form`, with `submitter` the button pressed (if any).
    Submit {
        form: usize,
        submitter: Option<usize>,
    },
    /// Esc: focus left the control.
    Blur,
}

/// The values the user typed and which control has the focus.
#[derive(Clone, Debug, Default)]
pub struct FormState {
    /// `values[form][field]`, starting as the `value` attributes.
    values: Vec<Vec<String>>,
    focus: Option<(usize, usize)>,
    /// Caret as a byte offset into the focused value (always a char boundary).
    caret: usize,
    compose: Compose,
}

/// Split a `<textarea>` value into the rows it shows: byte ranges (without the line break) of at
/// most `max_w` pixels, where `width_of` is the width of one character. A row breaks after the
/// last space that fits, or in the middle of a word wider than the box. Every character falls in
/// one row, so a caret can always be placed.
pub fn wrap_rows(value: &str, max_w: i32, width_of: impl Fn(char) -> i32) -> Vec<(usize, usize)> {
    let mut rows = Vec::new();
    let mut line_start = 0;
    loop {
        let line_end = value[line_start..]
            .find('\n')
            .map_or(value.len(), |i| line_start + i);
        let mut a = line_start;
        loop {
            let mut w = 0;
            let mut cut = None; // the last place a row may end: just after a space
            let mut end = line_end;
            for (i, ch) in value[a..line_end].char_indices() {
                let cw = width_of(ch);
                // A space may hang past the edge, so a row ends after it, not before it.
                if w + cw > max_w && i > 0 && ch != ' ' {
                    end = cut.unwrap_or(a + i);
                    break;
                }
                w += cw;
                if ch == ' ' {
                    cut = Some(a + i + 1);
                }
            }
            rows.push((a, end));
            if end >= line_end {
                break;
            }
            a = end;
        }
        if line_end >= value.len() {
            return rows;
        }
        line_start = line_end + 1;
    }
}

/// The row of `rows` (from [`wrap_rows`]) that holds byte offset `caret`, and the offset into it.
pub fn caret_row(rows: &[(usize, usize)], caret: usize) -> (usize, usize) {
    for (k, &(a, b)) in rows.iter().enumerate() {
        let soft_wrap_next = rows.get(k + 1).is_some_and(|n| n.0 == b);
        if caret < b || (caret == b && !soft_wrap_next) {
            return (k, caret.saturating_sub(a));
        }
    }
    rows.last()
        .map_or((0, 0), |&(a, b)| (rows.len() - 1, b - a))
}

/// Longest value of a control, in bytes.
fn max_len(kind: FieldKind) -> usize {
    if kind == FieldKind::TextArea {
        MAX_TEXTAREA
    } else {
        MAX_VALUE
    }
}

/// Byte offset of the start of the line holding `caret`.
fn line_start(s: &str, caret: usize) -> usize {
    let c = caret.min(s.len());
    s[..c].rfind('\n').map_or(0, |i| i + 1)
}

/// Byte offset of the end (before the `\n`) of the line holding `caret`.
fn line_end(s: &str, caret: usize) -> usize {
    let c = caret.min(s.len());
    s[c..].find('\n').map_or(s.len(), |i| c + i)
}

/// The caret one line up or down, in the same column (counted in characters).
fn vertical(s: &str, caret: usize, up: bool) -> usize {
    let c = caret.min(s.len());
    let start = line_start(s, c);
    let col = s[start..c].chars().count();
    let target_start = if up {
        if start == 0 {
            return 0;
        }
        line_start(s, start - 1)
    } else {
        let end = line_end(s, c);
        if end >= s.len() {
            return s.len();
        }
        end + 1
    };
    let target_end = line_end(s, target_start);
    let line = &s[target_start..target_end];
    let off = line.char_indices().nth(col).map_or(line.len(), |(i, _)| i);
    target_start + off
}

fn prev_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    if i == 0 {
        return 0;
    }
    i -= 1;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn next_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    if i >= s.len() {
        return s.len();
    }
    i += 1;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

impl FormState {
    /// State for `forms`, every control at its initial value, nothing focused.
    pub fn new(forms: &[FormInfo]) -> Self {
        FormState {
            values: forms
                .iter()
                .map(|f| {
                    f.fields
                        .iter()
                        .map(|fi| {
                            if matches!(fi.kind, FieldKind::Checkbox | FieldKind::Radio) {
                                // The state of a toggle: "1" when checked.
                                return String::from(if fi.checked { "1" } else { "" });
                            }
                            if fi.kind == FieldKind::Select {
                                // The index of the chosen option.
                                let i = fi.selected.min(fi.options.len().saturating_sub(1));
                                return alloc::format!("{i}");
                            }
                            let mut v = fi.value.clone();
                            truncate_at_boundary(&mut v, max_len(fi.kind));
                            v
                        })
                        .collect()
                })
                .collect(),
            focus: None,
            caret: 0,
            compose: Compose::new(),
        }
    }

    /// The text of a control (empty for an unknown one).
    pub fn value(&self, form: usize, field: usize) -> &str {
        self.values
            .get(form)
            .and_then(|f| f.get(field))
            .map_or("", String::as_str)
    }

    /// Is the check box or radio button checked?
    pub fn is_checked(&self, form: usize, field: usize) -> bool {
        !self.value(form, field).is_empty()
    }

    /// The index of the option a `<select>` shows.
    pub fn select_index(&self, form: usize, field: usize) -> usize {
        self.value(form, field).parse().unwrap_or(0)
    }

    /// The label of the option a `<select>` shows (empty when it has none).
    pub fn select_label<'a>(&self, forms: &'a [FormInfo], form: usize, field: usize) -> &'a str {
        forms
            .get(form)
            .and_then(|f| f.fields.get(field))
            .and_then(|fi| fi.options.get(self.select_index(form, field)))
            .map_or("", |o| o.label.as_str())
    }

    /// Choose another option of a `<select>`: `delta` steps, wrapping round. Returns whether
    /// the choice changed.
    pub fn step_select(
        &mut self,
        forms: &[FormInfo],
        form: usize,
        field: usize,
        delta: i32,
    ) -> bool {
        let Some(fi) = forms.get(form).and_then(|f| f.fields.get(field)) else {
            return false;
        };
        let n = fi.options.len() as i32;
        if fi.kind != FieldKind::Select || n < 2 {
            return false;
        }
        let now = self.select_index(form, field) as i32;
        let next = (now + delta).rem_euclid(n) as usize;
        if let Some(v) = self.values.get_mut(form).and_then(|r| r.get_mut(field)) {
            *v = alloc::format!("{next}");
        }
        true
    }

    /// Jump to the next option whose label starts with `c` (any case), after the current one.
    fn select_by_letter(&mut self, forms: &[FormInfo], form: usize, field: usize, c: char) {
        let Some(fi) = forms.get(form).and_then(|f| f.fields.get(field)) else {
            return;
        };
        let n = fi.options.len();
        let now = self.select_index(form, field);
        let c = c.to_ascii_lowercase();
        for k in 1..=n {
            let i = (now + k) % n;
            if fi.options[i]
                .label
                .chars()
                .next()
                .is_some_and(|f| f.to_ascii_lowercase() == c)
            {
                if let Some(v) = self.values.get_mut(form).and_then(|r| r.get_mut(field)) {
                    *v = alloc::format!("{i}");
                }
                return;
            }
        }
    }

    /// Flip a check box, or select a radio button (clearing the others of its group).
    /// Returns whether anything changed.
    pub fn toggle(&mut self, forms: &[FormInfo], form: usize, field: usize) -> bool {
        let Some(f) = forms.get(form) else {
            return false;
        };
        let Some(fi) = f.fields.get(field) else {
            return false;
        };
        let set = |s: &mut Self, i: usize, on: bool| {
            if let Some(v) = s.values.get_mut(form).and_then(|r| r.get_mut(i)) {
                *v = String::from(if on { "1" } else { "" });
            }
        };
        match fi.kind {
            FieldKind::Checkbox => {
                let now = !self.is_checked(form, field);
                set(self, field, now);
                true
            }
            FieldKind::Radio => {
                if self.is_checked(form, field) {
                    return false;
                }
                for (i, other) in f.fields.iter().enumerate() {
                    if other.kind == FieldKind::Radio && other.name == fi.name {
                        set(self, i, i == field);
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// The focused control, if any.
    pub fn focus(&self) -> Option<(usize, usize)> {
        self.focus
    }

    /// The caret (byte offset) in the focused control.
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// Focus a control (text controls take the caret at their end; a button
    /// takes the focus for Enter). Returns whether the control exists.
    pub fn set_focus(&mut self, forms: &[FormInfo], form: usize, field: usize) -> bool {
        let Some(fi) = forms.get(form).and_then(|f| f.fields.get(field)) else {
            return false;
        };
        if fi.kind == FieldKind::Hidden {
            return false;
        }
        self.compose = Compose::new();
        self.focus = Some((form, field));
        self.caret = self.value(form, field).len();
        true
    }

    /// Drop the focus.
    pub fn blur(&mut self) {
        self.compose = Compose::new();
        self.focus = None;
        self.caret = 0;
    }

    /// Move the focus to the next (or previous) visible control in document
    /// order, across forms. From the last one it returns `false` and clears
    /// the focus (Tab leaves the page's controls).
    pub fn tab(&mut self, forms: &[FormInfo], back: bool) -> bool {
        let order: Vec<(usize, usize)> = forms
            .iter()
            .enumerate()
            .flat_map(|(i, f)| {
                f.fields
                    .iter()
                    .enumerate()
                    .filter(|(_, fi)| fi.kind != FieldKind::Hidden)
                    .map(move |(j, _)| (i, j))
            })
            .collect();
        if order.is_empty() {
            return false;
        }
        let pos = self.focus.and_then(|f| order.iter().position(|&o| o == f));
        let next = match (pos, back) {
            (None, false) => Some(0),
            (None, true) => Some(order.len() - 1),
            (Some(p), false) if p + 1 < order.len() => Some(p + 1),
            (Some(p), true) if p > 0 => Some(p - 1),
            _ => None,
        };
        match next {
            Some(n) => self.set_focus(forms, order[n].0, order[n].1),
            None => {
                self.blur();
                false
            }
        }
    }

    fn focused_text<'a>(&'a mut self, forms: &[FormInfo]) -> Option<&'a mut String> {
        let (f, i) = self.focus?;
        if !forms.get(f)?.fields.get(i)?.kind.is_text() {
            return None;
        }
        self.values.get_mut(f)?.get_mut(i)
    }

    /// Insert `s` at the caret of the focused text control (a paste, or the
    /// result of composition). Returns whether anything was inserted.
    pub fn insert_str(&mut self, forms: &[FormInfo], s: &str) -> bool {
        let caret = self.caret;
        let multi = self
            .focus
            .and_then(|(f, i)| forms.get(f)?.fields.get(i))
            .is_some_and(|fi| fi.kind == FieldKind::TextArea);
        let max = if multi { MAX_TEXTAREA } else { MAX_VALUE };
        let Some(v) = self.focused_text(forms) else {
            return false;
        };
        let mut at = caret.min(v.len());
        while !v.is_char_boundary(at) {
            at -= 1;
        }
        let mut added = 0;
        for ch in s.chars() {
            if (ch < ' ' && !(multi && ch == '\n')) || ch == '\u{7f}' {
                continue; // no control characters in a field
            }
            if v.len() + ch.len_utf8() > max {
                break;
            }
            v.insert(at + added, ch);
            added += ch.len_utf8();
        }
        self.caret = at + added;
        added > 0
    }

    /// Handle a key for the focused control.
    pub fn on_key(&mut self, forms: &[FormInfo], key: Key) -> FormOutcome {
        let Some((form, field)) = self.focus else {
            return FormOutcome::Ignored;
        };
        let kind = match forms.get(form).and_then(|f| f.fields.get(field)) {
            Some(fi) => fi.kind,
            None => return FormOutcome::Ignored,
        };
        // Any key but a printable one lets a waiting dead key out as itself.
        if !matches!(key, Key::Char(_)) {
            let pending: Vec<char> = self.compose.flush().iter().collect();
            if !pending.is_empty() {
                let s: String = pending.into_iter().collect();
                self.insert_str(forms, &s);
            }
        }
        if kind == FieldKind::Select {
            return match key {
                Key::Up | Key::Left => {
                    self.step_select(forms, form, field, -1);
                    FormOutcome::Changed
                }
                Key::Down | Key::Right | Key::Char(b' ') => {
                    self.step_select(forms, form, field, 1);
                    FormOutcome::Changed
                }
                Key::Char(b) if b.is_ascii_graphic() => {
                    self.select_by_letter(forms, form, field, b as char);
                    FormOutcome::Changed
                }
                Key::Enter => FormOutcome::Submit {
                    form,
                    submitter: None,
                },
                Key::Esc => {
                    self.blur();
                    FormOutcome::Blur
                }
                _ => FormOutcome::Ignored,
            };
        }
        if matches!(kind, FieldKind::Checkbox | FieldKind::Radio) {
            return match key {
                Key::Char(b' ') => {
                    self.toggle(forms, form, field);
                    FormOutcome::Changed
                }
                Key::Enter => FormOutcome::Submit {
                    form,
                    submitter: None,
                },
                Key::Esc => {
                    self.blur();
                    FormOutcome::Blur
                }
                _ => FormOutcome::Ignored,
            };
        }
        if kind == FieldKind::PushButton {
            return match key {
                Key::Esc => {
                    self.blur();
                    FormOutcome::Blur
                }
                _ => FormOutcome::Ignored,
            };
        }
        if kind == FieldKind::Submit {
            return match key {
                Key::Enter | Key::Char(b' ') => FormOutcome::Submit {
                    form,
                    submitter: Some(field),
                },
                Key::Esc => {
                    self.blur();
                    FormOutcome::Blur
                }
                _ => FormOutcome::Ignored,
            };
        }
        match key {
            Key::Char(b) => {
                let c = b as char;
                if !b.is_ascii() || b < b' ' || b == 0x7f {
                    return FormOutcome::Ignored;
                }
                let out: Vec<char> = self.compose.feed(c).iter().collect();
                let s: String = out.into_iter().collect();
                if s.is_empty() {
                    return FormOutcome::Changed; // dead key waiting
                }
                self.insert_str(forms, &s);
                FormOutcome::Changed
            }
            Key::Backspace => {
                let caret = self.caret;
                if let Some(v) = self.focused_text(forms)
                    && caret > 0
                {
                    let p = prev_boundary(v, caret);
                    v.replace_range(p..caret, "");
                    self.caret = p;
                }
                FormOutcome::Changed
            }
            Key::Delete => {
                let caret = self.caret;
                if let Some(v) = self.focused_text(forms)
                    && caret < v.len()
                {
                    let n = next_boundary(v, caret);
                    v.replace_range(caret..n, "");
                }
                FormOutcome::Changed
            }
            Key::Left => {
                let caret = self.caret;
                if let Some(v) = self.focused_text(forms) {
                    self.caret = prev_boundary(v, caret);
                }
                FormOutcome::Changed
            }
            Key::Right => {
                let caret = self.caret;
                if let Some(v) = self.focused_text(forms) {
                    self.caret = next_boundary(v, caret);
                }
                FormOutcome::Changed
            }
            Key::Home => {
                let v = self.value(form, field);
                self.caret = if kind == FieldKind::TextArea {
                    line_start(v, self.caret)
                } else {
                    0
                };
                FormOutcome::Changed
            }
            Key::End => {
                let v = self.value(form, field);
                self.caret = if kind == FieldKind::TextArea {
                    line_end(v, self.caret)
                } else {
                    v.len()
                };
                FormOutcome::Changed
            }
            Key::Up | Key::Down if kind == FieldKind::TextArea => {
                let v = self.value(form, field);
                self.caret = vertical(v, self.caret, key == Key::Up);
                FormOutcome::Changed
            }
            Key::Enter if kind == FieldKind::TextArea => {
                self.insert_str(forms, "\n");
                FormOutcome::Changed
            }
            Key::Enter => FormOutcome::Submit {
                form,
                submitter: None,
            },
            Key::Esc => {
                self.blur();
                FormOutcome::Blur
            }
            // Tab belongs to the caller (`tab`).
            _ => FormOutcome::Ignored,
        }
    }

    /// The `name=value&...` query of `form` when `submitter` was pressed.
    pub fn query(
        &self,
        forms: &[FormInfo],
        form: usize,
        submitter: Option<usize>,
    ) -> Result<String, FormError> {
        self.encode(forms, form, submitter, MAX_QUERY)
    }

    fn encode(
        &self,
        forms: &[FormInfo],
        form: usize,
        submitter: Option<usize>,
        limit: usize,
    ) -> Result<String, FormError> {
        let f = forms.get(form).ok_or(FormError::NoForm)?;
        let mut q = String::new();
        for (i, fi) in f.fields.iter().enumerate() {
            let include = match fi.kind {
                FieldKind::Hidden | FieldKind::Text | FieldKind::Password | FieldKind::TextArea => {
                    !fi.name.is_empty()
                }
                FieldKind::Select => !fi.name.is_empty() && !fi.options.is_empty(),
                FieldKind::Submit => submitter == Some(i) && !fi.name.is_empty(),
                FieldKind::Checkbox | FieldKind::Radio => {
                    !fi.name.is_empty() && self.is_checked(form, i)
                }
                FieldKind::PushButton => false,
            };
            if !include {
                continue;
            }
            if !q.is_empty() {
                q.push('&');
            }
            urlencode(&fi.name, &mut q);
            q.push('=');
            match fi.kind {
                FieldKind::Hidden | FieldKind::Submit | FieldKind::Checkbox | FieldKind::Radio => {
                    urlencode(fi.value.as_str(), &mut q)
                }
                FieldKind::Select => {
                    let o = fi.options.get(self.select_index(form, i));
                    urlencode(o.map_or("", |o| o.value.as_str()), &mut q)
                }
                // A browser sends a line break as CR LF.
                FieldKind::TextArea => {
                    urlencode(&self.value(form, i).replace('\n', "\r\n"), &mut q)
                }
                _ => urlencode(self.value(form, i), &mut q),
            }
            if q.len() > limit {
                return Err(FormError::TooLong);
            }
        }
        Ok(q)
    }

    /// What submitting `form` sends: the address (relative to the page URL: the form's action
    /// without its own query or fragment; empty stays on the current page) and, for
    /// `method=post`, the body. A GET puts the query in the address after `?`.
    pub fn submission(
        &self,
        forms: &[FormInfo],
        form: usize,
        submitter: Option<usize>,
    ) -> Result<Submission, FormError> {
        let f = forms.get(form).ok_or(FormError::NoForm)?;
        let action = f.action.trim();
        let end = action.find(['?', '#']).unwrap_or(action.len());
        let mut href = String::from(&action[..end]);
        if f.post {
            // A POST keeps the query string of its action (it is part of the address).
            let q_end = action.find('#').unwrap_or(action.len());
            href = String::from(&action[..q_end]);
            let body = self.encode(forms, form, submitter, MAX_POST)?;
            return Ok(Submission {
                href,
                body: Some(body),
            });
        }
        let q = self.encode(forms, form, submitter, MAX_QUERY)?;
        href.push('?');
        href.push_str(&q);
        Ok(Submission { href, body: None })
    }

    /// The `href` to navigate to on submit for a GET form (see [`FormState::submission`]).
    pub fn target(
        &self,
        forms: &[FormInfo],
        form: usize,
        submitter: Option<usize>,
    ) -> Result<String, FormError> {
        self.submission(forms, form, submitter).map(|s| s.href)
    }
}

/// What a form sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submission {
    /// Where to, relative to the page URL.
    pub href: String,
    /// The urlencoded body of a POST (`None`: a GET, the data is in `href`).
    pub body: Option<String>,
}

fn truncate_at_boundary(s: &mut String, max: usize) {
    if s.len() <= max {
        return;
    }
    let mut n = max;
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    s.truncate(n);
}
