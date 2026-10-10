//! rows (split out of `fileman.rs`).

use super::*;

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

pub(super) fn trim_zeros(d: &[u8]) -> &[u8] {
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
