//! File manager logic: everything about the Arquivos window that is not pixels.
//!
//! The kernel draws and routes input; this module decides:
//!
//! * [`FileView`]: one window's state (current path, rows, sort, selection, scroll,
//!   history) and how it reloads from a [`Backend`]. The trash is the pseudo path
//!   [`TRASH_PATH`], so navigation history, the address bar and the sidebar treat it
//!   like any other place.
//! * Sorting ([`natural_cmp`], [`Sort`]), multi-selection ([`Selection`]: click,
//!   Ctrl+click, Shift+click, Ctrl+A, Shift+arrows), breadcrumbs, [`History`].
//! * [`Layout`]: the window's geometry and hit-testing, shared by the renderer and
//!   the mouse handler so they cannot disagree.
//! * Small pure helpers: [`format_size`], [`format_datetime`], [`display_ascii`]
//!   (UTF-8 names for the ASCII-only bitmap font), [`TextInput`] (the inline name
//!   editor), [`PathClip`] (the shared copy/cut clipboard), [`classify`] (what
//!   "open" means for a file), [`context_menu`].

use crate::vfs::{self, Backend, Entry, EntryKind, VfsError};
use crate::window::Rect;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

/// The trash as a location.
pub const TRASH_PATH: &[u8] = b"/.trash";
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
    /// Trash key (empty outside the trash).
    pub id: Vec<u8>,
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

/// The crumbs of `path`: `Raiz`, then one per folder (`Lixeira` for the trash).
pub fn breadcrumbs(path: &[u8]) -> Vec<Crumb> {
    let mut out = alloc::vec![Crumb {
        label: b"Raiz".to_vec(),
        path: b"/".to_vec(),
    }];
    if path == TRASH_PATH {
        out.push(Crumb {
            label: b"Lixeira".to_vec(),
            path: TRASH_PATH.to_vec(),
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

/// Index of the first crumb to show so the bar fits in `max_chars` columns
/// (labels joined by a 3-column separator, plus a 4-column `... ` marker when
/// leading crumbs are hidden). The last crumb is always shown.
pub fn first_visible_crumb(label_lens: &[usize], max_chars: usize) -> usize {
    let n = label_lens.len();
    if n == 0 {
        return 0;
    }
    let width = |from: usize| -> usize {
        let body: usize = label_lens[from..].iter().sum::<usize>() + 3 * (n - from - 1);
        if from > 0 { body + 4 } else { body }
    };
    let mut from = 0;
    while from + 1 < n && width(from) > max_chars {
        from += 1;
    }
    from
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

/// `"512 B"`, `"1,5 KiB"`, `"3,0 MiB"`, `"2,3 GiB"` (one decimal, comma, binary units).
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return alloc::format!("{bytes} B");
    }
    let mut unit = 0;
    let mut scaled = bytes as u128 * 10; // tenths of the current unit
    scaled /= 1024;
    while scaled >= 10_240 && unit + 1 < UNITS.len() {
        scaled /= 1024;
        unit += 1;
    }
    alloc::format!("{},{} {}", scaled / 10, scaled % 10, UNITS[unit])
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

/// `"dd/mm/aaaa hh:mm"` of a Unix time shifted by `tz_secs` (local time). `0` (no
/// clock when the file was written) shows `"--"`.
pub fn format_datetime(unix: u64, tz_secs: i32) -> String {
    if unix == 0 {
        return String::from("--");
    }
    let t = unix as i64 + tz_secs as i64;
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    alloc::format!(
        "{:02}/{:02}/{:04} {:02}:{:02}",
        d,
        m,
        y,
        secs / 3600,
        (secs % 3600) / 60
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

/// A one-line text field (new name, rename, save-as).
#[derive(Clone, Debug)]
pub struct TextInput {
    buf: Vec<u8>,
    /// Byte offset of the caret, always on a UTF-8 boundary.
    cur: usize,
    max: usize,
}

impl TextInput {
    /// A field holding `initial`, caret at the end, at most `max` bytes.
    pub fn new(initial: &[u8], max: usize) -> Self {
        let buf = initial[..initial.len().min(max)].to_vec();
        TextInput {
            cur: buf.len(),
            buf,
            max,
        }
    }

    pub fn text(&self) -> &[u8] {
        &self.buf
    }

    /// The caret's byte offset.
    pub fn caret(&self) -> usize {
        self.cur
    }

    /// Insert a printable byte at the caret (control bytes and `/` are ignored).
    pub fn insert(&mut self, b: u8) {
        if b < 0x20 || b == 0x7F || b == b'/' || self.buf.len() >= self.max {
            return;
        }
        self.buf.insert(self.cur, b);
        self.cur += 1;
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

    /// Delete the character before the caret.
    pub fn backspace(&mut self) {
        let p = self.prev_boundary(self.cur);
        self.buf.drain(p..self.cur);
        self.cur = p;
    }

    /// Delete the character at the caret.
    pub fn delete(&mut self) {
        let n = self.next_boundary(self.cur);
        self.buf.drain(self.cur..n);
    }

    pub fn left(&mut self) {
        self.cur = self.prev_boundary(self.cur);
    }

    pub fn right(&mut self) {
        self.cur = self.next_boundary(self.cur);
    }

    pub fn home(&mut self) {
        self.cur = 0;
    }

    pub fn end(&mut self) {
        self.cur = self.buf.len();
    }

    /// Empty the field.
    pub fn clear(&mut self) {
        self.buf.clear();
        self.cur = 0;
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
}

/// What the context menu is about to be shown for.
#[derive(Clone, Copy, Debug)]
pub struct MenuCtx {
    pub in_trash: bool,
    /// Number of selected rows under the cursor click (0 = empty space).
    pub selected: usize,
    /// The single selected row is an image.
    pub image: bool,
    pub clip_has_items: bool,
}

/// The entries of the context menu, in order, with their Portuguese labels.
pub fn context_menu(ctx: MenuCtx) -> Vec<(Cmd, &'static str)> {
    let mut m = Vec::new();
    if ctx.in_trash {
        if ctx.selected > 0 {
            m.push((Cmd::Restore, "Restaurar"));
            m.push((Cmd::DeletePermanent, "Excluir permanentemente"));
            m.push((Cmd::Properties, "Propriedades"));
        }
        m.push((Cmd::EmptyTrash, "Esvaziar lixeira"));
        m.push((Cmd::SelectAll, "Selecionar tudo"));
        return m;
    }
    if ctx.selected > 0 {
        if ctx.selected == 1 {
            m.push((Cmd::Open, "Abrir"));
        }
        if ctx.selected == 1 && ctx.image {
            m.push((Cmd::SetWallpaper, "Definir como papel de parede"));
        }
        m.push((Cmd::Cut, "Recortar"));
        m.push((Cmd::Copy, "Copiar"));
        if ctx.selected == 1 {
            m.push((Cmd::Rename, "Renomear"));
        }
        m.push((Cmd::Delete, "Excluir"));
        m.push((Cmd::DeletePermanent, "Excluir permanentemente"));
        m.push((Cmd::Properties, "Propriedades"));
    } else {
        m.push((Cmd::NewFile, "Novo arquivo"));
        m.push((Cmd::NewFolder, "Nova pasta"));
        if ctx.clip_has_items {
            m.push((Cmd::Paste, "Colar"));
        }
        m.push((Cmd::SelectAll, "Selecionar tudo"));
        m.push((Cmd::Refresh, "Atualizar"));
        m.push((Cmd::Properties, "Propriedades"));
    }
    m
}

// ---------------------------------------------------------------------------
// Layout and hit testing
// ---------------------------------------------------------------------------

/// Sidebar places.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Root,
    Documents,
    Trash,
    /// The disk entry (shows usage); opens the root.
    Disk,
}

/// What a click landed on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    Back,
    Forward,
    Up,
    /// Crumb index (into the full crumb list).
    Crumb(usize),
    /// Empty part of the address bar.
    Address,
    Place(Place),
    Header(SortKey),
    /// A list row (absolute index; may be past the last row: check the count).
    Row(usize),
    /// The scrollbar track: fraction of the way down in permille.
    Scroll(u32),
    /// Empty space in the list.
    Blank,
}

pub const TITLE_H: i32 = crate::window::TITLE_H;
pub const TOOLBAR_H: i32 = 40;
pub const SIDEBAR_W: i32 = 164;
pub const HEADER_H: i32 = 24;
pub const ROW_H: i32 = 24;
pub const STATUS_H: i32 = 30;
pub const SCROLL_W: i32 = 12;
pub const SIZE_COL_W: i32 = 104;
pub const DATE_COL_W: i32 = 204;
pub const SIDE_ROW_H: i32 = 28;
/// Advance of one character of the 2x font.
pub const CELL: i32 = 12;

/// Geometry of a file-manager window, all in screen coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub window: Rect,
    pub back: Rect,
    pub forward: Rect,
    pub up: Rect,
    pub address: Rect,
    pub sidebar: Rect,
    pub header: Rect,
    pub list: Rect,
    pub status: Rect,
    pub scrollbar: Rect,
    /// X of the size column's left edge and of the date column's left edge.
    pub size_x: i32,
    pub date_x: i32,
    pub name_x: i32,
}

impl Layout {
    /// Geometry for a window at `r`.
    pub fn of(r: Rect) -> Layout {
        let top = r.y + TITLE_H;
        let b = 28;
        let by = top + (TOOLBAR_H - b) / 2;
        let main_x = r.x + SIDEBAR_W;
        let right = r.right();
        let bottom = r.bottom();
        let list_top = top + TOOLBAR_H + HEADER_H;
        let list_h = (bottom - STATUS_H - list_top).max(0);
        let date_x = right - SCROLL_W - DATE_COL_W;
        Layout {
            window: r,
            back: Rect::new(r.x + 10, by, b, b),
            forward: Rect::new(r.x + 10 + b + 6, by, b, b),
            up: Rect::new(r.x + 10 + 2 * (b + 6), by, b, b),
            address: Rect::new(
                r.x + 10 + 3 * (b + 6) + 4,
                by,
                (right - 10 - (r.x + 10 + 3 * (b + 6) + 4)).max(0),
                b,
            ),
            sidebar: Rect::new(
                r.x,
                top + TOOLBAR_H,
                SIDEBAR_W,
                (bottom - STATUS_H - top - TOOLBAR_H).max(0),
            ),
            header: Rect::new(main_x, top + TOOLBAR_H, (right - main_x).max(0), HEADER_H),
            list: Rect::new(main_x, list_top, (right - main_x - SCROLL_W).max(0), list_h),
            status: Rect::new(r.x, bottom - STATUS_H, r.w, STATUS_H),
            scrollbar: Rect::new(right - SCROLL_W, list_top, SCROLL_W, list_h),
            size_x: date_x - SIZE_COL_W,
            date_x,
            name_x: main_x + 34,
        }
    }

    /// Rows that fit in the list area.
    pub fn visible_rows(&self) -> usize {
        (self.list.h / ROW_H).max(0) as usize
    }

    /// Y of row `i` given the first visible row.
    pub fn row_y(&self, i: usize, scroll: usize) -> i32 {
        self.list.y + (i as i32 - scroll as i32) * ROW_H
    }

    /// Sidebar row rectangles `(place, rect)`; the disk entry is taller (usage bar).
    pub fn places(&self) -> [(Place, Rect); 4] {
        let x = self.sidebar.x + 8;
        let w = SIDEBAR_W - 16;
        let y0 = self.sidebar.y + 26;
        [
            (Place::Root, Rect::new(x, y0, w, SIDE_ROW_H)),
            (
                Place::Documents,
                Rect::new(x, y0 + SIDE_ROW_H + 2, w, SIDE_ROW_H),
            ),
            (
                Place::Trash,
                Rect::new(x, y0 + 2 * (SIDE_ROW_H + 2), w, SIDE_ROW_H),
            ),
            (
                Place::Disk,
                Rect::new(x, y0 + 3 * (SIDE_ROW_H + 2) + 28, w, 46),
            ),
        ]
    }

    /// Where the address bar draws each visible crumb: `(index, x, width_px)`, and
    /// whether leading crumbs were folded into a `...` marker.
    pub fn crumb_spans(&self, labels: &[usize]) -> (Vec<(usize, i32, i32)>, bool) {
        let inner = (self.address.w - 20).max(0) / CELL;
        let first = first_visible_crumb(labels, inner as usize);
        let mut x = self.address.x + 10;
        if first > 0 {
            x += 4 * CELL;
        }
        let mut out = Vec::new();
        for (i, &len) in labels.iter().enumerate().skip(first) {
            let w = len as i32 * CELL;
            out.push((i, x, w));
            x += w + 3 * CELL;
        }
        (out, first > 0)
    }

    /// Resolve a press at `(px, py)`. `scroll` is the first visible row and
    /// `labels` the crumb label lengths.
    pub fn hit(&self, px: i32, py: i32, scroll: usize, labels: &[usize]) -> Option<Hit> {
        if !self.window.contains(px, py) {
            return None;
        }
        if self.back.contains(px, py) {
            return Some(Hit::Back);
        }
        if self.forward.contains(px, py) {
            return Some(Hit::Forward);
        }
        if self.up.contains(px, py) {
            return Some(Hit::Up);
        }
        if self.address.contains(px, py) {
            let (spans, _) = self.crumb_spans(labels);
            for (i, x, w) in spans {
                if px >= x && px < x + w {
                    return Some(Hit::Crumb(i));
                }
            }
            return Some(Hit::Address);
        }
        if self.sidebar.contains(px, py) {
            return self
                .places()
                .iter()
                .find(|(_, r)| r.contains(px, py))
                .map(|&(p, _)| Hit::Place(p));
        }
        if self.header.contains(px, py) {
            let key = if px >= self.date_x {
                SortKey::Modified
            } else if px >= self.size_x {
                SortKey::Size
            } else {
                SortKey::Name
            };
            return Some(Hit::Header(key));
        }
        if self.scrollbar.contains(px, py) {
            let frac = ((py - self.scrollbar.y) as i64 * 1000 / self.scrollbar.h.max(1) as i64)
                .clamp(0, 1000) as u32;
            return Some(Hit::Scroll(frac));
        }
        if self.list.contains(px, py) {
            let i = scroll + ((py - self.list.y) / ROW_H) as usize;
            return Some(Hit::Row(i));
        }
        None
    }

    /// First visible row that a scrollbar press at `permille` maps to.
    pub fn scroll_for(&self, permille: u32, rows: usize) -> usize {
        let max = rows.saturating_sub(self.visible_rows());
        ((max as u64 * permille as u64) / 1000) as usize
    }

    /// Scrollbar thumb `(y, height)` for the given scroll position.
    pub fn thumb(&self, scroll: usize, rows: usize) -> (i32, i32) {
        let vis = self.visible_rows();
        let h = self.scrollbar.h;
        if rows <= vis || rows == 0 {
            return (self.scrollbar.y, h);
        }
        let th = ((h as i64 * vis as i64) / rows as i64).max(24) as i32;
        let th = th.min(h);
        let max = (rows - vis) as i64;
        let y = self.scrollbar.y + ((h - th) as i64 * scroll as i64 / max) as i32;
        (y, th)
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
}

/// One file-manager window's state.
pub struct FileView {
    /// `/`, a folder path, or [`TRASH_PATH`].
    pub cwd: Vec<u8>,
    pub rows: Vec<Row>,
    pub sort: Sort,
    pub sel: Selection,
    /// First visible row.
    pub scroll: usize,
    pub history: History,
    /// More rows existed than [`MAX_ROWS`].
    pub truncated: bool,
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
            sort: Sort::DEFAULT,
            sel: Selection::new(),
            scroll: 0,
            history: History::new(b"/"),
            truncated: false,
        }
    }

    pub fn in_trash(&self) -> bool {
        self.cwd == TRASH_PATH
    }

    /// Reload the rows of the current place from `b`, keeping the selection by name.
    /// A folder that no longer exists moves the view to the closest existing parent.
    pub fn refresh<B: Backend + ?Sized>(&mut self, b: &mut B) -> Result<(), VfsError> {
        let keep: Vec<Vec<u8>> = self
            .sel
            .selected()
            .into_iter()
            .filter_map(|i| self.rows.get(i).map(|r| r.name.clone()))
            .collect();
        let cursor = self.rows.get(self.sel.cursor()).map(|r| r.name.clone());
        let mut rows: Vec<Row> = if self.in_trash() {
            b.trash_list()?
                .into_iter()
                .map(|t| Row {
                    name: t.name,
                    kind: t.kind,
                    size: t.size,
                    mtime: t.deleted_at,
                    id: t.id,
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
                    }
                    Err(e) => return Err(e),
                }
            }
        };
        self.truncated = rows.len() > MAX_ROWS;
        rows.truncate(MAX_ROWS);
        sort_rows(&mut rows, self.sort);
        self.sel.restore(&rows, &keep, cursor.as_deref());
        self.rows = rows;
        self.clamp_scroll();
        Ok(())
    }

    /// Go to `path` (a folder or the trash), pushing history; the view is reloaded.
    /// On failure the view stays where it was.
    pub fn navigate<B: Backend + ?Sized>(
        &mut self,
        b: &mut B,
        path: &[u8],
    ) -> Result<(), VfsError> {
        if path != TRASH_PATH {
            match b.stat(path)? {
                i if i.kind == EntryKind::Dir => {}
                _ => return Err(VfsError::NotDir),
            }
        }
        let old = core::mem::replace(&mut self.cwd, path.to_vec());
        self.scroll = 0;
        self.sel.reset(0);
        if let Err(e) = self.refresh(b) {
            self.cwd = old;
            let _ = self.refresh(b);
            return Err(e);
        }
        self.history.push(&self.cwd);
        self.select_first();
        Ok(())
    }

    /// Put the cursor (and the selection) on the first row, if there is one.
    pub fn select_first(&mut self) {
        if !self.rows.is_empty() {
            self.sel.only(0);
        }
    }

    /// The parent folder (from the trash: the root).
    pub fn go_up<B: Backend + ?Sized>(&mut self, b: &mut B) -> Result<(), VfsError> {
        let up = if self.in_trash() {
            b"/".to_vec()
        } else {
            vfs::parent(&self.cwd)
        };
        if up == self.cwd {
            return Ok(());
        }
        let r = self.navigate(b, &up);
        // Land on the folder we came from.
        r
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
        self.scroll = 0;
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

    fn resort(&mut self) {
        let keep: Vec<Vec<u8>> = self
            .sel
            .selected()
            .into_iter()
            .filter_map(|i| self.rows.get(i).map(|r| r.name.clone()))
            .collect();
        let cursor = self.rows.get(self.sel.cursor()).map(|r| r.name.clone());
        sort_rows(&mut self.rows, self.sort);
        self.sel.restore(&self.rows, &keep, cursor.as_deref());
    }

    /// Absolute path of row `i` (folder views only; the trash has none).
    pub fn path_of(&self, i: usize) -> Option<Vec<u8>> {
        if self.in_trash() {
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

    /// Select (and scroll to) the row called `name`.
    pub fn select_name(&mut self, name: &[u8], visible: usize) {
        if let Some(i) = self.rows.iter().position(|r| r.name == name) {
            self.sel.only(i);
            self.ensure_visible(visible);
        }
    }

    /// Select every row whose name is in `names` (after a paste: the new items), the
    /// cursor on the first of them, scrolled into view. Does nothing if none match.
    pub fn select_names(&mut self, names: &[Vec<u8>], visible: usize) {
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
        self.ensure_visible(visible);
    }

    /// Keep the cursor row on screen.
    pub fn ensure_visible(&mut self, visible: usize) {
        let c = self.sel.cursor();
        if visible == 0 {
            return;
        }
        if c < self.scroll {
            self.scroll = c;
        } else if c >= self.scroll + visible {
            self.scroll = c + 1 - visible;
        }
        self.clamp_scroll_to(visible);
    }

    /// Scroll by `delta` rows.
    pub fn scroll_by(&mut self, delta: isize, visible: usize) {
        let max = self.rows.len().saturating_sub(visible);
        self.scroll = (self.scroll as isize + delta).clamp(0, max as isize) as usize;
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.min(self.rows.len().saturating_sub(1));
    }

    fn clamp_scroll_to(&mut self, visible: usize) {
        let max = self.rows.len().saturating_sub(visible);
        self.scroll = self.scroll.min(max);
    }

    /// Status text: item count, or selection count and size.
    pub fn summary(&self) -> String {
        let n = self.rows.len();
        let k = self.sel.count();
        if k == 0 {
            return if n == 1 {
                String::from("1 item")
            } else {
                alloc::format!("{n} itens")
            };
        }
        let bytes: u64 = self
            .selected_rows()
            .iter()
            .filter(|r| !r.is_dir())
            .map(|r| r.size)
            .sum();
        if k == 1 {
            alloc::format!("1 selecionado ({})", format_size(bytes))
        } else {
            alloc::format!("{k} selecionados ({})", format_size(bytes))
        }
    }
}

#[cfg(test)]
mod tests;
