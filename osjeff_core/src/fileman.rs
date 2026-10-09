//! File manager logic: everything about the Arquivos window that is not pixels.
//!
//! The kernel draws and routes input; this module decides:
//!
//! * [`FileView`]: one window's state (current path, rows, sort, search filter, selection,
//!   history) and how it reloads from a [`Backend`]. The trash is the pseudo path
//!   [`TRASH_PATH`], so navigation history, the path bar and the sidebar treat it like any
//!   other place.
//! * Sorting ([`natural_cmp`], [`Sort`]), multi-selection ([`Selection`]: click,
//!   Ctrl+click, Shift+click, Ctrl+A, Shift+arrows), breadcrumbs, [`History`], [`Place`].
//! * [`ui`]: the window's geometry and hit-testing, the list and icon-grid models, the
//!   path bar, rubber band, drag-and-drop rules, the smooth scroller and the preview
//!   helpers, shared by the renderer and the mouse handler so they cannot disagree.
//! * Small pure helpers: [`format_size`], [`format_datetime`], [`display_ascii`]
//!   (folding names for the accent-insensitive search), [`TextInput`] (the inline name
//!   editor with selection), [`PathClip`] (the shared copy/cut clipboard), [`classify`]
//!   (what "open" means for a file), [`context_menu`].

pub mod apps;
pub mod ui;

use crate::vfs::{self, Backend, Entry, EntryKind, VfsError};
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

/// The trash as a location.
pub const TRASH_PATH: &[u8] = b"/.trash";
/// The Apps place (installed and bundled packages) as a location, like the trash a
/// pseudo path: history, the address bar and the sidebar treat it as any other place.
pub const APPS_PATH: &[u8] = b"/.apps";
/// Most rows a view loads (a folder with more shows the first ones).
pub const MAX_ROWS: usize = 20_000;

// ---------------------------------------------------------------------------
// Rows and sorting
// ---------------------------------------------------------------------------

/// One line of the list.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    pub name: Vec<u8>,
    pub kind: EntryKind,
    pub size: u64,
    /// Modified time; in the trash, the time it was deleted.
    pub mtime: u64,
    /// Trash key (empty outside the trash); in the Apps place, the app id.
    pub id: Vec<u8>,
    /// Apps place only: the package is installed (always `false` elsewhere).
    pub installed: bool,
}

impl Row {
    /// True for a folder.
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Dir
    }
}

impl From<Entry> for Row {
    fn from(e: Entry) -> Self {
        Row {
            name: e.name,
            kind: e.kind,
            size: e.size,
            mtime: e.mtime,
            id: Vec::new(),
            installed: false,
        }
    }
}

/// A sortable column.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKey {
    Name,
    Size,
    Modified,
}

/// Column and direction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sort {
    pub key: SortKey,
    pub asc: bool,
}

impl Sort {
    /// Name, A to Z.
    pub const DEFAULT: Sort = Sort {
        key: SortKey::Name,
        asc: true,
    };

    /// A click on a column header: the same column flips direction, another
    /// column starts ascending.
    pub fn click(&mut self, key: SortKey) {
        if self.key == key {
            self.asc = !self.asc;
        } else {
            self.key = key;
            self.asc = true;
        }
    }
}

/// Case-insensitive "natural" order: digit runs compare by value, so `f2` comes before
/// `f10`; other bytes compare by ASCII-lowercase. Ties break on the raw bytes, so the
/// order is total and stable.
pub fn natural_cmp(a: &[u8], b: &[u8]) -> Ordering {
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let si = i;
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            let sj = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let (da, db) = (&a[si..i], &b[sj..j]);
            let (ta, tb) = (trim_zeros(da), trim_zeros(db));
            let c = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
            if c != Ordering::Equal {
                return c;
            }
            // Same value: fewer leading zeros first.
            let c = da.len().cmp(&db.len());
            if c != Ordering::Equal {
                return c;
            }
        } else {
            let c = a[i].to_ascii_lowercase().cmp(&b[j].to_ascii_lowercase());
            if c != Ordering::Equal {
                return c;
            }
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j)).then_with(|| a.cmp(b))
}

fn trim_zeros(d: &[u8]) -> &[u8] {
    let n = d.iter().take_while(|&&c| c == b'0').count();
    &d[n.min(d.len().saturating_sub(1))..]
}

/// Order of two rows under `sort`: folders always first; then the chosen column
/// (ties broken by name, always ascending).
pub fn compare(a: &Row, b: &Row, sort: Sort) -> Ordering {
    let by_name = natural_cmp(&a.name, &b.name);
    match (a.is_dir(), b.is_dir()) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }
    let primary = match sort.key {
        SortKey::Name => by_name,
        SortKey::Size => {
            if a.is_dir() {
                Ordering::Equal // folders have no meaningful size: by name
            } else {
                a.size.cmp(&b.size)
            }
        }
        SortKey::Modified => a.mtime.cmp(&b.mtime),
    };
    let primary = if sort.asc { primary } else { primary.reverse() };
    primary.then(by_name)
}

/// Sort rows in place.
pub fn sort_rows(rows: &mut [Row], sort: Sort) {
    rows.sort_by(|a, b| compare(a, b, sort));
}

// ---------------------------------------------------------------------------
// Selection
// ---------------------------------------------------------------------------

/// Multi-selection over `n` rows with a cursor and a range anchor.
#[derive(Clone, Debug, Default)]
pub struct Selection {
    mask: Vec<bool>,
    count: usize,
    cursor: usize,
    anchor: usize,
}

impl Selection {
    /// Empty selection over zero rows.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start over with `n` unselected rows (cursor on the first).
    pub fn reset(&mut self, n: usize) {
        self.mask.clear();
        self.mask.resize(n, false);
        self.count = 0;
        self.cursor = 0;
        self.anchor = 0;
    }

    /// Number of rows covered.
    pub fn len(&self) -> usize {
        self.mask.len()
    }

    /// True when there are no rows.
    pub fn is_empty(&self) -> bool {
        self.mask.is_empty()
    }

    /// Number of selected rows.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Whether row `i` is selected.
    pub fn is_selected(&self, i: usize) -> bool {
        self.mask.get(i).copied().unwrap_or(false)
    }

    /// The keyboard cursor row.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Deselect everything (the cursor stays).
    pub fn clear(&mut self) {
        self.mask.iter_mut().for_each(|m| *m = false);
        self.count = 0;
    }

    /// Select every row (Ctrl+A).
    pub fn select_all(&mut self) {
        self.mask.iter_mut().for_each(|m| *m = true);
        self.count = self.mask.len();
    }

    fn set(&mut self, i: usize, v: bool) {
        if let Some(m) = self.mask.get_mut(i)
            && *m != v
        {
            *m = v;
            if v {
                self.count += 1;
            } else {
                self.count -= 1;
            }
        }
    }

    /// Select exactly row `i` (plain click or arrow key).
    pub fn only(&mut self, i: usize) {
        if self.mask.is_empty() {
            return;
        }
        let i = i.min(self.mask.len() - 1);
        self.clear();
        self.set(i, true);
        self.cursor = i;
        self.anchor = i;
    }

    /// Select exactly the rows `idx` (ascending, in range), cursor and anchor on the first.
    pub fn select_set(&mut self, idx: &[usize]) {
        self.clear();
        for &i in idx {
            self.set(i, true);
        }
        if let Some(&f) = idx.first() {
            self.cursor = f.min(self.mask.len().saturating_sub(1));
            self.anchor = self.cursor;
        }
    }

    /// A mouse click on row `i` with the modifier state: plain selects only it,
    /// Ctrl toggles it, Shift selects the range from the anchor (Ctrl+Shift adds the
    /// range to what is selected).
    pub fn click(&mut self, i: usize, ctrl: bool, shift: bool) {
        if i >= self.mask.len() {
            return;
        }
        match (ctrl, shift) {
            (false, false) => self.only(i),
            (true, false) => {
                let v = !self.is_selected(i);
                self.set(i, v);
                self.cursor = i;
                self.anchor = i;
            }
            (_, true) => {
                if !ctrl {
                    self.clear();
                }
                let (a, b) = (self.anchor.min(i), self.anchor.max(i));
                for k in a..=b {
                    self.set(k, true);
                }
                self.cursor = i;
            }
        }
    }

    /// Move the cursor by `delta` rows; with Shift the selection grows from the
    /// anchor, without it only the new row is selected.
    pub fn move_cursor(&mut self, delta: isize, shift: bool) {
        if self.mask.is_empty() {
            return;
        }
        let target = (self.cursor as isize + delta).clamp(0, self.mask.len() as isize - 1) as usize;
        if shift {
            self.clear();
            let (a, b) = (self.anchor.min(target), self.anchor.max(target));
            for k in a..=b {
                self.set(k, true);
            }
            self.cursor = target;
        } else {
            self.only(target);
        }
    }

    /// Indices of the selected rows, ascending.
    pub fn selected(&self) -> Vec<usize> {
        self.mask
            .iter()
            .enumerate()
            .filter(|&(_, &m)| m)
            .map(|(i, _)| i)
            .collect()
    }

    /// Select the rows in `names` (by index of `rows`), cursor on `cursor_name`.
    fn restore(&mut self, rows: &[Row], names: &[Vec<u8>], cursor_name: Option<&[u8]>) {
        self.reset(rows.len());
        let wanted: alloc::collections::BTreeSet<&[u8]> = names.iter().map(|n| &n[..]).collect();
        for (i, r) in rows.iter().enumerate() {
            if wanted.contains(&r.name[..]) {
                self.set(i, true);
            }
            if cursor_name == Some(&r.name[..]) {
                self.cursor = i;
                self.anchor = i;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Paths, breadcrumbs, history
// ---------------------------------------------------------------------------

/// One clickable part of the address bar.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Crumb {
    pub label: Vec<u8>,
    pub path: Vec<u8>,
}

/// The crumbs of `path`: the disk (`Disco` / `Disk`), then one per folder (the trash for the
/// trash). The special labels follow the language in effect.
pub fn breadcrumbs(path: &[u8]) -> Vec<Crumb> {
    let mut out = alloc::vec![Crumb {
        label: crate::t!("files.place.disk").as_bytes().to_vec(),
        path: b"/".to_vec(),
    }];
    if path == TRASH_PATH {
        out.push(Crumb {
            label: crate::t!("files.place.trash").as_bytes().to_vec(),
            path: TRASH_PATH.to_vec(),
        });
        return out;
    }
    if path == APPS_PATH {
        out.push(Crumb {
            label: b"Apps".to_vec(),
            path: APPS_PATH.to_vec(),
        });
        return out;
    }
    let mut acc: Vec<u8> = Vec::new();
    for c in vfs::components(path) {
        acc.push(b'/');
        acc.extend_from_slice(c);
        out.push(Crumb {
            label: c.to_vec(),
            path: acc.clone(),
        });
    }
    out
}

/// Back/forward history of locations.
#[derive(Clone, Debug)]
pub struct History {
    stack: Vec<Vec<u8>>,
    pos: usize,
}

/// Most locations remembered.
const HISTORY_MAX: usize = 64;

impl History {
    /// History starting at `start`.
    pub fn new(start: &[u8]) -> Self {
        History {
            stack: alloc::vec![start.to_vec()],
            pos: 0,
        }
    }

    /// Visit `path`: drops the forward part; visiting the current place again does nothing.
    pub fn push(&mut self, path: &[u8]) {
        if self.stack[self.pos] == path {
            return;
        }
        self.stack.truncate(self.pos + 1);
        self.stack.push(path.to_vec());
        if self.stack.len() > HISTORY_MAX {
            self.stack.remove(0);
        }
        self.pos = self.stack.len() - 1;
    }

    pub fn can_back(&self) -> bool {
        self.pos > 0
    }

    pub fn can_forward(&self) -> bool {
        self.pos + 1 < self.stack.len()
    }

    /// Step back; the new current place.
    pub fn back(&mut self) -> Option<&[u8]> {
        if self.can_back() {
            self.pos -= 1;
            Some(&self.stack[self.pos])
        } else {
            None
        }
    }

    /// Step forward; the new current place.
    pub fn forward(&mut self) -> Option<&[u8]> {
        if self.can_forward() {
            self.pos += 1;
            Some(&self.stack[self.pos])
        } else {
            None
        }
    }

    /// The current place.
    pub fn current(&self) -> &[u8] {
        &self.stack[self.pos]
    }

    /// Replace the current place (it moved or vanished).
    pub fn replace_current(&mut self, path: &[u8]) {
        self.stack[self.pos] = path.to_vec();
    }
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/// `"512 B"`, `"1,5 KiB"` (pt) / `"1.5 KiB"` (en): one decimal, binary units, the language's
/// decimal separator (see [`crate::i18n::format_size`]).
pub fn format_size(bytes: u64) -> String {
    crate::i18n::format_size(bytes)
}

/// Days since 1970-01-01 to `(year, month, day)` (proleptic Gregorian).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A Unix time shifted by `tz_secs` (local time) as the language writes a date and a time
/// (`08/10/2026 23:49` / `10/08/2026 11:49 PM`; `clock24` picks the clock). `0` (no clock when
/// the file was written) shows `--`.
pub fn format_datetime(unix: u64, tz_secs: i32, clock24: bool) -> String {
    if unix == 0 {
        return String::from("--");
    }
    let t = unix as i64 + tz_secs as i64;
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let civil = crate::i18n::Civil {
        year: y.clamp(0, 9999) as i32,
        month: m as u8,
        day: d as u8,
        // 1970-01-01 was a Thursday.
        weekday: (days + 4).rem_euclid(7) as u8,
        hour: (secs / 3600) as u8,
        minute: ((secs % 3600) / 60) as u8,
        second: (secs % 60) as u8,
    };
    crate::t!(
        "files.when.datetime",
        date = &crate::i18n::format_date(civil, crate::i18n::DateStyle::Short, clock24),
        time = &crate::i18n::format_time(civil, clock24, false)
    )
}

/// Fold a UTF-8 name to what the ASCII bitmap font can draw: accented Latin letters
/// become their base letter, any other non-ASCII character becomes one `?`, control
/// bytes become `?`. Invalid UTF-8 bytes become `?` too. Never longer than the input.
pub fn display_ascii(name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len());
    let mut i = 0;
    while i < name.len() {
        let b = name[i];
        if b < 0x80 {
            out.push(if b < 0x20 || b == 0x7F { b'?' } else { b });
            i += 1;
            continue;
        }
        let len = match b {
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => 1,
        };
        let end = (i + len).min(name.len());
        let ch = core::str::from_utf8(&name[i..end])
            .ok()
            .and_then(|s| s.chars().next());
        out.push(ch.map_or(b'?', fold_char));
        i = end.max(i + 1);
    }
    out
}

fn fold_char(c: char) -> u8 {
    match c {
        'à'..='å' => b'a',
        'À'..='Å' => b'A',
        'ç' => b'c',
        'Ç' => b'C',
        'è'..='ë' => b'e',
        'È'..='Ë' => b'E',
        'ì'..='ï' => b'i',
        'Ì'..='Ï' => b'I',
        'ñ' => b'n',
        'Ñ' => b'N',
        'ò'..='ö' | 'ø' => b'o',
        'Ò'..='Ö' | 'Ø' => b'O',
        'ù'..='ü' => b'u',
        'Ù'..='Ü' => b'U',
        'ý' | 'ÿ' => b'y',
        'Ý' => b'Y',
        _ => b'?',
    }
}

/// `text` cut to `max` columns with a trailing `...` when it does not fit.
pub fn ellipsize(text: &[u8], max: usize) -> Vec<u8> {
    if text.len() <= max {
        return text.to_vec();
    }
    if max <= 3 {
        return text[..max].to_vec();
    }
    let mut out = text[..max - 3].to_vec();
    out.extend_from_slice(b"...");
    out
}

// ---------------------------------------------------------------------------
// Inline text input
// ---------------------------------------------------------------------------

/// A one-line text field (new name, rename, save-as, search) with an optional selection.
#[derive(Clone, Debug)]
pub struct TextInput {
    buf: Vec<u8>,
    /// Byte offset of the caret, always on a UTF-8 boundary.
    cur: usize,
    /// The other end of the selection (the caret is the moving end).
    anchor: Option<usize>,
    max: usize,
}

impl TextInput {
    /// A field holding `initial`, caret at the end, at most `max` bytes.
    pub fn new(initial: &[u8], max: usize) -> Self {
        let mut end = initial.len().min(max);
        while end > 0 && end < initial.len() && initial[end] & 0xC0 == 0x80 {
            end -= 1;
        }
        let buf = initial[..end].to_vec();
        TextInput {
            cur: buf.len(),
            buf,
            anchor: None,
            max,
        }
    }

    pub fn text(&self) -> &[u8] {
        &self.buf
    }

    /// The text as a string (invalid UTF-8 shown as replacement characters).
    pub fn to_string_lossy(&self) -> String {
        String::from_utf8_lossy(&self.buf).into_owned()
    }

    /// The caret's byte offset.
    pub fn caret(&self) -> usize {
        self.cur
    }

    /// The selected byte range `(start, end)`, if any.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        (a != self.cur).then(|| (a.min(self.cur), a.max(self.cur)))
    }

    /// Select everything.
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cur = self.buf.len();
    }

    /// Select the name without its extension (what renaming starts with): up to the last dot,
    /// unless the dot starts the name; everything when there is no extension.
    pub fn select_stem(&mut self) {
        let end = match self.buf.iter().rposition(|&b| b == b'.') {
            Some(i) if i > 0 => i,
            _ => self.buf.len(),
        };
        self.anchor = Some(0);
        self.cur = end;
    }

    /// Delete the selected text, leaving the caret at its start. `true` when there was some.
    fn delete_selection(&mut self) -> bool {
        match self.selection() {
            Some((a, b)) => {
                self.buf.drain(a..b);
                self.cur = a;
                self.anchor = None;
                true
            }
            None => {
                self.anchor = None;
                false
            }
        }
    }

    /// Insert a printable byte at the caret, replacing the selection (control bytes and `/`
    /// are ignored). A byte above 127 is a Latin-1 character and is stored as UTF-8.
    pub fn insert(&mut self, b: u8) {
        if b < 0x20 || b == 0x7F || b == b'/' {
            return;
        }
        let mut tmp = [0u8; 4];
        let enc: &[u8] = if b < 0x80 {
            tmp[0] = b;
            &tmp[..1]
        } else {
            char::from(b).encode_utf8(&mut tmp).as_bytes()
        };
        let removed = self.selection().map_or(0, |(a, z)| z - a);
        if self.buf.len() - removed + enc.len() > self.max {
            return;
        }
        self.delete_selection();
        for (k, &x) in enc.iter().enumerate() {
            self.buf.insert(self.cur + k, x);
        }
        self.cur += enc.len();
    }

    fn prev_boundary(&self, mut i: usize) -> usize {
        while i > 0 {
            i -= 1;
            if self.buf[i] & 0xC0 != 0x80 {
                break;
            }
        }
        i
    }

    fn next_boundary(&self, mut i: usize) -> usize {
        while i < self.buf.len() {
            i += 1;
            if i >= self.buf.len() || self.buf[i] & 0xC0 != 0x80 {
                break;
            }
        }
        i
    }

    /// Delete the selection, else the character before the caret.
    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        let p = self.prev_boundary(self.cur);
        self.buf.drain(p..self.cur);
        self.cur = p;
    }

    /// Delete the selection, else the character at the caret.
    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        let n = self.next_boundary(self.cur);
        self.buf.drain(self.cur..n);
    }

    /// Move left; over a selection, collapse to its start.
    pub fn left(&mut self) {
        if let Some((a, _)) = self.selection() {
            self.cur = a;
        } else {
            self.cur = self.prev_boundary(self.cur);
        }
        self.anchor = None;
    }

    /// Move right; over a selection, collapse to its end.
    pub fn right(&mut self) {
        if let Some((_, b)) = self.selection() {
            self.cur = b;
        } else {
            self.cur = self.next_boundary(self.cur);
        }
        self.anchor = None;
    }

    pub fn home(&mut self) {
        self.anchor = None;
        self.cur = 0;
    }

    pub fn end(&mut self) {
        self.anchor = None;
        self.cur = self.buf.len();
    }

    /// Empty the field.
    pub fn clear(&mut self) {
        self.buf.clear();
        self.cur = 0;
        self.anchor = None;
    }

    /// Replace the whole text.
    pub fn set(&mut self, text: &[u8]) {
        *self = TextInput::new(text, self.max);
    }

    /// The caret's column in the folded display text ([`display_ascii`]).
    pub fn caret_column(&self) -> usize {
        display_ascii(&self.buf[..self.cur]).len()
    }
}

// ---------------------------------------------------------------------------
// Clipboard of paths
// ---------------------------------------------------------------------------

/// What Ctrl+C / Ctrl+X put aside for Ctrl+V; shared by all file-manager windows.
#[derive(Clone, Debug, Default)]
pub struct PathClip {
    paths: Vec<Vec<u8>>,
    cut: bool,
}

impl PathClip {
    pub const fn new() -> Self {
        PathClip {
            paths: Vec::new(),
            cut: false,
        }
    }

    pub fn set(&mut self, paths: Vec<Vec<u8>>, cut: bool) {
        self.paths = paths;
        self.cut = cut;
    }

    pub fn paths(&self) -> &[Vec<u8>] {
        &self.paths
    }

    pub fn is_cut(&self) -> bool {
        self.cut
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// After a successful move the clipboard is spent; a copy can be pasted again.
    pub fn after_paste(&mut self) {
        if self.cut {
            self.paths.clear();
            self.cut = false;
        }
    }

    /// Whether `path` is waiting to be moved (drawn dimmed).
    pub fn is_cut_path(&self, path: &[u8]) -> bool {
        self.cut && self.paths.iter().any(|p| p == path)
    }
}

// ---------------------------------------------------------------------------
// What "open" means
// ---------------------------------------------------------------------------

/// How a file opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileClass {
    /// PNG, BMP, PPM: the image viewer.
    Image,
    /// WebAssembly module: the apps platform.
    Wasm,
    /// Opens in the editor.
    Text,
    /// Unknown: the editor if the content looks like text.
    Other,
}

/// The lowercase extension of `name` (without the dot), if any.
pub fn extension(name: &[u8]) -> Vec<u8> {
    let (_, ext) = vfs::split_ext(name);
    ext.iter().skip(1).map(u8::to_ascii_lowercase).collect()
}

/// Classify a file by its name.
pub fn classify(name: &[u8]) -> FileClass {
    match &extension(name)[..] {
        b"png" | b"bmp" | b"ppm" => FileClass::Image,
        b"wasm" => FileClass::Wasm,
        b"txt" | b"md" | b"rs" | b"c" | b"h" | b"toml" | b"json" | b"log" | b"ini" | b"cfg"
        | b"csv" | b"html" | b"htm" | b"css" | b"js" | b"sh" | b"py" | b"yml" | b"yaml"
        | b"xml" | b"lock" | b"conf" | b"" => FileClass::Text,
        _ => FileClass::Other,
    }
}

/// Whether `head` (the first bytes of a file) looks like text: no NUL, mostly
/// printable or whitespace.
pub fn looks_like_text(head: &[u8]) -> bool {
    if head.contains(&0) {
        return false;
    }
    let odd = head
        .iter()
        .filter(|&&b| b < 0x20 && !matches!(b, b'\n' | b'\r' | b'\t'))
        .count();
    odd * 20 <= head.len()
}

/// Whether an image viewer can open `name`.
pub fn is_image(name: &[u8]) -> bool {
    classify(name) == FileClass::Image
}

// ---------------------------------------------------------------------------
// Commands and context menu
// ---------------------------------------------------------------------------

/// An action of the file manager (context menu entries and key shortcuts).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    Open,
    NewFile,
    NewFolder,
    Rename,
    Copy,
    Cut,
    Paste,
    Delete,
    DeletePermanent,
    Restore,
    EmptyTrash,
    Properties,
    SelectAll,
    Refresh,
    SetWallpaper,
    /// Apps place: install the selected bundled package.
    InstallApp,
    /// Apps place: remove the selected installed app.
    RemoveApp,
    /// Sort by a column (the sort menu): the same column again flips the direction.
    SortBy(SortKey),
    /// Set the sort direction (`true` = ascending).
    SortDir(bool),
    /// Switch between the list and the icon grid.
    SetView(ui::ViewMode),
    /// Show or hide the preview pane (Space).
    TogglePreview,
}

impl Cmd {
    /// Menu group: entries of different groups are separated by a line.
    pub fn group(self) -> u8 {
        match self {
            Cmd::Open | Cmd::Restore | Cmd::InstallApp | Cmd::TogglePreview => 0,
            Cmd::SetWallpaper => 1,
            Cmd::NewFile | Cmd::NewFolder => 2,
            Cmd::Cut | Cmd::Copy | Cmd::Paste | Cmd::Rename => 3,
            Cmd::Delete | Cmd::DeletePermanent | Cmd::EmptyTrash | Cmd::RemoveApp => 4,
            Cmd::SelectAll
            | Cmd::Refresh
            | Cmd::Properties
            | Cmd::SortBy(_)
            | Cmd::SortDir(_)
            | Cmd::SetView(_) => 5,
        }
    }

    /// The keyboard shortcut shown next to the entry.
    pub fn shortcut(self) -> &'static str {
        match self {
            Cmd::Open => "Enter",
            Cmd::Copy => "Ctrl+C",
            Cmd::Cut => "Ctrl+X",
            Cmd::Paste => "Ctrl+V",
            Cmd::Rename => "F2",
            Cmd::Delete => "Del",
            Cmd::SelectAll => "Ctrl+A",
            Cmd::Refresh => "F5",
            Cmd::TogglePreview => crate::t!("files.key.space"),
            Cmd::NewFolder => "N",
            _ => "",
        }
    }
}

/// What the context menu is about to be shown for.
#[derive(Clone, Copy, Debug)]
pub struct MenuCtx {
    pub in_trash: bool,
    /// The Apps place (the menu then offers run / install / remove).
    pub in_apps: bool,
    /// Apps place: the single selected app is installed.
    pub app_installed: bool,
    /// Number of selected rows under the cursor click (0 = empty space).
    pub selected: usize,
    /// The single selected row is an image.
    pub image: bool,
    pub clip_has_items: bool,
}

/// The entries of the context menu, in order, with their labels in the language in effect.
pub fn context_menu(ctx: MenuCtx) -> Vec<(Cmd, &'static str)> {
    let mut m = Vec::new();
    if ctx.in_apps {
        if ctx.selected == 1 {
            if ctx.app_installed {
                m.push((Cmd::Open, crate::t!("files.menu.open")));
                m.push((Cmd::RemoveApp, crate::t!("files.menu.remove")));
            } else {
                m.push((Cmd::Open, crate::t!("files.menu.install_open")));
                m.push((Cmd::InstallApp, crate::t!("files.menu.install")));
            }
            m.push((Cmd::Properties, crate::t!("files.menu.properties")));
        }
        m.push((Cmd::Refresh, crate::t!("files.menu.refresh")));
        return m;
    }
    if ctx.in_trash {
        if ctx.selected > 0 {
            m.push((Cmd::Restore, crate::t!("files.menu.restore")));
            m.push((
                Cmd::DeletePermanent,
                crate::t!("files.menu.delete_permanently"),
            ));
        }
        m.push((Cmd::EmptyTrash, crate::t!("files.menu.empty_trash")));
        if ctx.selected > 0 {
            m.push((Cmd::Properties, crate::t!("files.menu.properties")));
        }
        m.push((Cmd::SelectAll, crate::t!("files.menu.select_all")));
        return m;
    }
    if ctx.selected > 0 {
        if ctx.selected == 1 {
            m.push((Cmd::Open, crate::t!("files.menu.open")));
            m.push((Cmd::TogglePreview, crate::t!("files.menu.preview")));
        }
        if ctx.selected == 1 && ctx.image {
            m.push((Cmd::SetWallpaper, crate::t!("files.menu.set_wallpaper")));
        }
        m.push((Cmd::Cut, crate::t!("files.menu.cut")));
        m.push((Cmd::Copy, crate::t!("files.menu.copy")));
        if ctx.selected == 1 {
            m.push((Cmd::Rename, crate::t!("files.menu.rename")));
        }
        m.push((Cmd::Delete, crate::t!("files.menu.delete")));
        m.push((
            Cmd::DeletePermanent,
            crate::t!("files.menu.delete_permanently"),
        ));
        m.push((Cmd::Properties, crate::t!("files.menu.properties")));
    } else {
        m.push((Cmd::NewFile, crate::t!("files.menu.new_file")));
        m.push((Cmd::NewFolder, crate::t!("files.menu.new_folder")));
        if ctx.clip_has_items {
            m.push((Cmd::Paste, crate::t!("files.menu.paste")));
        }
        m.push((Cmd::SelectAll, crate::t!("files.menu.select_all")));
        m.push((Cmd::Refresh, crate::t!("files.menu.refresh")));
        m.push((Cmd::Properties, crate::t!("files.menu.properties")));
    }
    m
}

// ---------------------------------------------------------------------------
// Places
// ---------------------------------------------------------------------------

/// Sidebar places.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    /// The user's folder (`/home`).
    Home,
    Documents,
    Images,
    /// Installed and bundled apps (`APPS_PATH`).
    Apps,
    Trash,
    /// The volume (its root); the entry shows the usage bar.
    Disk,
}

impl Place {
    /// The location the place opens.
    pub fn path(self) -> &'static [u8] {
        match self {
            Place::Home => b"/home",
            Place::Documents => b"/Documentos",
            Place::Images => b"/Imagens",
            Place::Apps => APPS_PATH,
            Place::Trash => TRASH_PATH,
            Place::Disk => b"/",
        }
    }

    /// Whether the place has to exist as a folder on the volume (and is created when it does
    /// not).
    pub fn is_folder(self) -> bool {
        !matches!(self, Place::Apps | Place::Trash | Place::Disk)
    }

    /// The place that contains `cwd`, for highlighting the sidebar: the deepest favourite
    /// that is a prefix of the path, else the disk for any other folder.
    pub fn of_path(cwd: &[u8]) -> Place {
        if cwd == TRASH_PATH {
            return Place::Trash;
        }
        if cwd == APPS_PATH {
            return Place::Apps;
        }
        for p in [Place::Home, Place::Documents, Place::Images] {
            let base = p.path();
            if cwd == base || (cwd.starts_with(base) && cwd.get(base.len()) == Some(&b'/')) {
                return p;
            }
        }
        Place::Disk
    }
}

// ---------------------------------------------------------------------------
// The view
// ---------------------------------------------------------------------------

/// What activating (Enter, double click) a row asks the desktop to do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Activation {
    /// Nothing happened (empty list, trash item).
    None,
    /// A folder: the view navigated into it.
    Entered,
    /// Open this file.
    Open(Vec<u8>, FileClass),
    /// Apps place: run this app (installing the bundled package first when it is not
    /// installed yet).
    App { id: Vec<u8>, installed: bool },
}

/// One file-manager window's state.
pub struct FileView {
    /// `/`, a folder path, [`TRASH_PATH`] or [`APPS_PATH`].
    pub cwd: Vec<u8>,
    /// The rows shown: the folder's, filtered by the search and sorted.
    pub rows: Vec<Row>,
    /// While a search filter is active: every row of the place (sorted), of which `rows` is
    /// the part that matches. Empty otherwise, so an unfiltered view holds its rows once.
    all: Vec<Row>,
    filter: Vec<u8>,
    pub sort: Sort,
    pub sel: Selection,
    pub history: History,
    /// More rows existed than [`MAX_ROWS`].
    pub truncated: bool,
    /// Bumped each time the view moves to another place, so the window can reset its scroll.
    pub nav_gen: u32,
}

impl Default for FileView {
    fn default() -> Self {
        Self::new()
    }
}

impl FileView {
    /// A view at the root (call [`refresh`](Self::refresh) to fill it).
    pub fn new() -> Self {
        FileView {
            cwd: b"/".to_vec(),
            rows: Vec::new(),
            all: Vec::new(),
            filter: Vec::new(),
            sort: Sort::DEFAULT,
            sel: Selection::new(),
            history: History::new(b"/"),
            truncated: false,
            nav_gen: 0,
        }
    }

    pub fn in_trash(&self) -> bool {
        self.cwd == TRASH_PATH
    }

    /// In the Apps place (rows come from [`set_apps`](Self::set_apps), not a folder).
    pub fn in_apps(&self) -> bool {
        self.cwd == APPS_PATH
    }

    /// The search text (empty when not searching).
    pub fn filter(&self) -> &[u8] {
        &self.filter
    }

    /// How many rows the place has before the search filter.
    pub fn total_rows(&self) -> usize {
        if self.filter.is_empty() {
            self.rows.len()
        } else {
            self.all.len()
        }
    }

    /// Show only the rows whose name matches `query` (accent and case insensitive); an empty
    /// query shows all. The selection follows the rows by name.
    pub fn set_filter(&mut self, query: &[u8]) {
        if query == &self.filter[..] {
            return;
        }
        let keep = self.selected_names();
        let cursor = self.rows.get(self.sel.cursor()).map(|r| r.name.clone());
        // Back to the full list first, then filter it afresh.
        if !self.filter.is_empty() {
            self.rows = core::mem::take(&mut self.all);
        }
        self.filter = query.to_vec();
        self.apply_filter();
        self.sel.restore(&self.rows, &keep, cursor.as_deref());
    }

    /// Drop the search filter (the rows come back).
    pub fn clear_filter(&mut self) {
        self.set_filter(b"");
    }

    /// Filter `rows` (which hold the full list) in place, parking the full list in `all`.
    fn apply_filter(&mut self) {
        if self.filter.is_empty() {
            return;
        }
        let key = ui::query_key(&self.filter);
        let shown: Vec<Row> = self
            .rows
            .iter()
            .filter(|r| ui::matches_key(&r.name, &key))
            .cloned()
            .collect();
        self.all = core::mem::replace(&mut self.rows, shown);
    }

    fn selected_names(&self) -> Vec<Vec<u8>> {
        self.sel
            .selected()
            .into_iter()
            .filter_map(|i| self.rows.get(i).map(|r| r.name.clone()))
            .collect()
    }

    /// Replace the place's rows with `rows` (sorted here), keeping selection by name.
    fn load(&mut self, mut rows: Vec<Row>, keep: &[Vec<u8>], cursor: Option<&[u8]>) {
        self.truncated = rows.len() > MAX_ROWS;
        rows.truncate(MAX_ROWS);
        sort_rows(&mut rows, self.sort);
        self.all.clear();
        self.rows = rows;
        self.apply_filter();
        self.sel.restore(&self.rows, keep, cursor);
    }

    /// Reload the rows of the current place from `b`, keeping the selection by name.
    /// A folder that no longer exists moves the view to the closest existing parent.
    pub fn refresh<B: Backend + ?Sized>(&mut self, b: &mut B) -> Result<(), VfsError> {
        if self.in_apps() {
            // The app list is not on the volume: whoever owns the catalog refills it
            // with `set_apps`; the app rows already shown stay until then. Rows left
            // over from the folder we came from (app rows always have an id) go.
            if self.rows.iter().any(|r| r.id.is_empty()) {
                self.rows.clear();
                self.all.clear();
                self.sel.reset(0);
            }
            return Ok(());
        }
        let keep = self.selected_names();
        let cursor = self.rows.get(self.sel.cursor()).map(|r| r.name.clone());
        let rows: Vec<Row> = if self.in_trash() {
            b.trash_list()?
                .into_iter()
                .map(|t| Row {
                    name: t.name,
                    kind: t.kind,
                    size: t.size,
                    mtime: t.deleted_at,
                    id: t.id,
                    installed: false,
                })
                .collect()
        } else {
            loop {
                match b.readdir(&self.cwd) {
                    Ok(list) => break list.into_iter().map(Row::from).collect(),
                    Err(VfsError::NotFound | VfsError::NotDir) if !vfs::is_root(&self.cwd) => {
                        let up = vfs::parent(&self.cwd);
                        self.cwd = up;
                        self.history.replace_current(&self.cwd);
                        self.nav_gen = self.nav_gen.wrapping_add(1);
                    }
                    Err(e) => return Err(e),
                }
            }
        };
        self.load(rows, &keep, cursor.as_deref());
        Ok(())
    }

    /// Go to `path` (a folder or the trash), pushing history; the view is reloaded.
    /// On failure the view stays where it was.
    pub fn navigate<B: Backend + ?Sized>(
        &mut self,
        b: &mut B,
        path: &[u8],
    ) -> Result<(), VfsError> {
        if path != TRASH_PATH && path != APPS_PATH {
            match b.stat(path)? {
                i if i.kind == EntryKind::Dir => {}
                _ => return Err(VfsError::NotDir),
            }
        }
        let old = core::mem::replace(&mut self.cwd, path.to_vec());
        let old_filter = core::mem::take(&mut self.filter);
        let old_all = core::mem::take(&mut self.all);
        self.sel.reset(0);
        if let Err(e) = self.refresh(b) {
            self.cwd = old;
            self.filter = old_filter;
            self.all = old_all;
            let _ = self.refresh(b);
            return Err(e);
        }
        self.nav_gen = self.nav_gen.wrapping_add(1);
        self.history.push(&self.cwd);
        self.select_first();
        Ok(())
    }

    /// Fill the Apps place from the catalog (sorted by the current sort, the rows are
    /// [`apps::rows`]). The cursor stays on the same app (by id); if it is gone, or
    /// nothing was selected, it goes to the first row.
    pub fn set_apps(&mut self, items: &[apps::AppItem]) {
        let cursor_id = self.rows.get(self.sel.cursor()).map(|r| r.id.clone());
        let had_selection = self.sel.count() > 0;
        let was_empty = self.rows.is_empty();
        let rows = apps::rows(items);
        self.truncated = false;
        self.all.clear();
        self.rows = rows;
        sort_rows(&mut self.rows, self.sort);
        self.apply_filter();
        self.sel.reset(self.rows.len());
        if had_selection || was_empty {
            let at = cursor_id
                .and_then(|id| self.rows.iter().position(|r| r.id == id))
                .unwrap_or(0);
            if !self.rows.is_empty() {
                self.sel.only(at);
            }
        }
    }

    /// Put the cursor (and the selection) on the first row, if there is one.
    pub fn select_first(&mut self) {
        if !self.rows.is_empty() {
            self.sel.only(0);
        }
    }

    /// The parent folder (from the trash: the root).
    pub fn go_up<B: Backend + ?Sized>(&mut self, b: &mut B) -> Result<(), VfsError> {
        let up = if self.in_trash() || self.in_apps() {
            b"/".to_vec()
        } else {
            vfs::parent(&self.cwd)
        };
        if up == self.cwd {
            return Ok(());
        }
        self.navigate(b, &up)
    }

    /// History back.
    pub fn go_back<B: Backend + ?Sized>(&mut self, b: &mut B) -> Result<(), VfsError> {
        match self.history.back().map(<[u8]>::to_vec) {
            Some(p) => self.go_to_history(b, &p),
            None => Ok(()),
        }
    }

    /// History forward.
    pub fn go_forward<B: Backend + ?Sized>(&mut self, b: &mut B) -> Result<(), VfsError> {
        match self.history.forward().map(<[u8]>::to_vec) {
            Some(p) => self.go_to_history(b, &p),
            None => Ok(()),
        }
    }

    fn go_to_history<B: Backend + ?Sized>(&mut self, b: &mut B, p: &[u8]) -> Result<(), VfsError> {
        self.cwd = p.to_vec();
        self.filter.clear();
        self.all.clear();
        self.nav_gen = self.nav_gen.wrapping_add(1);
        self.sel.reset(0);
        let r = self.refresh(b);
        self.select_first();
        r
    }

    /// Change the sort column (header click) keeping the selection.
    pub fn click_header(&mut self, key: SortKey) {
        self.sort.click(key);
        self.resort();
    }

    /// Sort by `key`: the same key as now flips nothing (menu entries pick, headers toggle).
    pub fn set_sort(&mut self, key: SortKey) {
        if self.sort.key != key {
            self.sort = Sort { key, asc: true };
            self.resort();
        }
    }

    fn resort(&mut self) {
        let keep = self.selected_names();
        let cursor = self.rows.get(self.sel.cursor()).map(|r| r.name.clone());
        if self.filter.is_empty() {
            sort_rows(&mut self.rows, self.sort);
        } else {
            sort_rows(&mut self.all, self.sort);
            sort_rows(&mut self.rows, self.sort);
        }
        self.sel.restore(&self.rows, &keep, cursor.as_deref());
    }

    /// Absolute path of row `i` (folder views only; the trash has none).
    pub fn path_of(&self, i: usize) -> Option<Vec<u8>> {
        if self.in_trash() || self.in_apps() {
            return None;
        }
        self.rows.get(i).map(|r| vfs::join(&self.cwd, &r.name))
    }

    /// Paths of the selected rows (empty in the trash).
    pub fn selected_paths(&self) -> Vec<Vec<u8>> {
        self.sel
            .selected()
            .into_iter()
            .filter_map(|i| self.path_of(i))
            .collect()
    }

    /// The selected rows.
    pub fn selected_rows(&self) -> Vec<&Row> {
        self.sel
            .selected()
            .into_iter()
            .filter_map(|i| self.rows.get(i))
            .collect()
    }

    /// Enter / double click on row `i`.
    pub fn activate<B: Backend + ?Sized>(&mut self, b: &mut B, i: usize) -> Activation {
        let Some(row) = self.rows.get(i) else {
            return Activation::None;
        };
        if self.in_trash() {
            return Activation::None;
        }
        if self.in_apps() {
            return Activation::App {
                id: row.id.clone(),
                installed: row.installed,
            };
        }
        let name = row.name.clone();
        let is_dir = row.is_dir();
        let path = vfs::join(&self.cwd, &name);
        if is_dir {
            return match self.navigate(b, &path) {
                Ok(()) => Activation::Entered,
                Err(_) => Activation::None,
            };
        }
        Activation::Open(path, classify(&name))
    }

    /// Select the row called `name`.
    pub fn select_name(&mut self, name: &[u8]) {
        if let Some(i) = self.rows.iter().position(|r| r.name == name) {
            self.sel.only(i);
        }
    }

    /// Select every row whose name is in `names` (after a paste: the new items), the
    /// cursor on the first of them. Does nothing if none match.
    pub fn select_names(&mut self, names: &[Vec<u8>]) {
        let idx: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| names.contains(&r.name))
            .map(|(i, _)| i)
            .collect();
        if idx.is_empty() {
            return;
        }
        self.sel.select_set(&idx);
    }

    /// Status text: item count, or selection count and size.
    pub fn summary(&self) -> String {
        let n = self.rows.len();
        let k = self.sel.count();
        if k == 0 {
            return crate::tp!("files.count", n);
        }
        let bytes: u64 = self
            .selected_rows()
            .iter()
            .filter(|r| !r.is_dir())
            .map(|r| r.size)
            .sum();
        crate::tp!("files.selected", k, size = crate::i18n::bytes(bytes))
    }
}

#[cfg(test)]
mod tests;
