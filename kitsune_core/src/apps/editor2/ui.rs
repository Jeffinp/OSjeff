//! The pixels of the editor window as numbers: the text grid with its gutter, the find bar, the
//! status bar, the Open / Save sheet and the close question. Pure and host tested; the kernel
//! draws and routes the mouse through these rectangles so they cannot disagree.

use crate::windowing::window::Rect;
use alloc::string::String;
use alloc::vec::Vec;

pub const TITLE_H: i32 = crate::windowing::window::TITLE_H;
/// Padding left and right of the text grid, and the gap between the gutter and the text.
pub const PAD: i32 = 12;
pub const GAP: i32 = 10;
pub const STATUS_H: i32 = 28;
/// Height of the find bar (one field) and of the find-and-replace bar (two).
pub const FIND_H: i32 = 44;
pub const REPLACE_H: i32 = 80;
/// Top margin of the text under the title bar (or the find bar).
pub const TOP_PAD: i32 = 6;

/// Character cell of the monospace face: advance and line height.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Metrics {
    pub cw: i32,
    pub lh: i32,
}

/// Geometry of an editor window.
#[derive(Clone, Copy, Debug)]
pub struct Lay {
    pub window: Rect,
    /// Everything below the title bar and the find bar down to the status bar.
    pub area: Rect,
    /// The gutter band (line numbers), the full height of the text rows.
    pub gutter: Rect,
    /// X of the first text column (right of the gutter and its gap).
    pub text_x: i32,
    /// Y of the first row.
    pub top: i32,
    pub bar: Option<Rect>,
    pub status: Rect,
    /// Rows and columns of the grid (the columns include the gutter's).
    pub rows: usize,
    pub cols: usize,
    /// Columns the gutter takes.
    pub gutter_cols: usize,
    pub m: Metrics,
}

/// Lay out a window `r`. `bar_h` is 0 with no find bar, else [`FIND_H`] or [`REPLACE_H`];
/// `gutter_cols` is the engine's gutter width in columns (0 with line numbers off).
pub fn layout(r: Rect, m: Metrics, bar_h: i32, gutter_cols: usize) -> Lay {
    let body_top = r.y + TITLE_H;
    let status = Rect::new(
        r.x,
        (r.bottom() - STATUS_H).max(body_top),
        r.w,
        STATUS_H.min(r.h),
    );
    let bar = (bar_h > 0).then(|| Rect::new(r.x, body_top, r.w, bar_h));
    let top = body_top + bar_h + TOP_PAD;
    let avail_h = (status.y - top).max(0);
    let rows = (avail_h / m.lh.max(1)).max(1) as usize;
    let gutter_px = gutter_cols as i32 * m.cw;
    let gap = if gutter_cols > 0 { GAP } else { 0 };
    let cols = (((r.w - 2 * PAD - gap) / m.cw.max(1)).max(2)) as usize;
    let text_x = r.x + PAD + gutter_px + gap;
    Lay {
        window: r,
        area: Rect::new(
            r.x,
            body_top + bar_h,
            r.w,
            (status.y - body_top - bar_h).max(0),
        ),
        gutter: Rect::new(r.x, top, PAD + gutter_px + gap / 2, rows as i32 * m.lh),
        text_x,
        top,
        bar,
        status,
        rows,
        cols,
        gutter_cols,
        m,
    }
}

impl Lay {
    /// The grid cell `(row, column)` under `(px, py)`, columns counted from the gutter's left
    /// edge as the engine does; a press in the gutter lands on the first text column. Cells
    /// outside the grid are clamped to its edge.
    pub fn cell_at(&self, px: i32, py: i32) -> (usize, usize) {
        let row = ((py - self.top).max(0) / self.m.lh.max(1)) as usize;
        let row = row.min(self.rows - 1);
        let col = if px < self.text_x {
            self.gutter_cols
        } else {
            self.gutter_cols + ((px - self.text_x) / self.m.cw.max(1)) as usize
        };
        (row, col.min(self.cols - 1))
    }

    /// Whether `(px, py)` is inside the text rows (gutter included).
    pub fn in_text(&self, px: i32, py: i32) -> bool {
        px >= self.window.x
            && px < self.window.right()
            && py >= self.top
            && py < self.top + self.rows as i32 * self.m.lh
    }

    /// Top-left pixel of the cell `(row, col)` (columns as in [`cell_at`](Self::cell_at)).
    pub fn cell_xy(&self, row: usize, col: usize) -> (i32, i32) {
        (
            self.text_x + (col as i32 - self.gutter_cols as i32) * self.m.cw,
            self.top + row as i32 * self.m.lh,
        )
    }

    /// X (right edge) a line number is aligned to.
    pub fn number_right(&self) -> i32 {
        self.window.x + PAD + self.gutter_cols as i32 * self.m.cw - self.m.cw / 2
    }
}

/// Runs of selected cells in a row: `(first column, length)` of each, columns relative to the
/// iterator's start.
pub fn selection_runs(selected: impl Iterator<Item = bool>) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut n = 0;
    for (i, s) in selected.enumerate() {
        n = i + 1;
        match (s, start) {
            (true, None) => start = Some(i),
            (false, Some(a)) => {
                out.push((a, i - a));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(a) = start {
        out.push((a, n - a));
    }
    out
}

/// The display columns of the leading whitespace of `line` (tabs advance to the next multiple
/// of `tab`).
pub fn leading_indent(line: &str, tab: usize) -> usize {
    let tab = tab.max(1);
    let mut col = 0;
    for ch in line.chars() {
        match ch {
            ' ' => col += 1,
            '\t' => col = (col / tab + 1) * tab,
            _ => break,
        }
    }
    col
}

/// The columns (relative to the text's left edge) where indent guides are drawn for a line
/// indented by `indent` columns: one at each indent level the line is past, never at column 0.
pub fn indent_guides(indent: usize, tab: usize) -> Vec<usize> {
    let tab = tab.max(1);
    (1..).map(|k| k * tab).take_while(|&c| c < indent).collect()
}

// ---------------------------------------------------------------------------
// The Open / Save sheet
// ---------------------------------------------------------------------------

/// Row height of the file list in the sheet.
pub const PICK_ROW_H: i32 = 26;
/// Width of the sheet's sidebar.
pub const PICK_SIDE_W: i32 = 132;
/// Padding of the sheet.
pub const SHEET_PAD: i32 = 20;
/// Height of the buttons.
pub const BUTTON_H: i32 = 28;

/// The size of the Open / Save sheet inside a window `r`.
pub fn picker_size(r: Rect, save: bool) -> (i32, i32) {
    let w = (r.w - 40).clamp(380, 600);
    let h = (r.h - TITLE_H - 16).clamp(if save { 300 } else { 260 }, 400);
    (w, h)
}

/// Geometry of the sheet whose panel is `panel`.
#[derive(Clone, Copy, Debug)]
pub struct PickLay {
    pub panel: Rect,
    pub title: Rect,
    pub sidebar: Rect,
    /// The current folder shown above the list.
    pub path: Rect,
    pub list: Rect,
    /// The name field (Save as only; empty height for Open).
    pub field: Rect,
    /// The hint, error or overwrite question line.
    pub hint: Rect,
    /// Rows the list shows at once.
    pub rows: usize,
}

/// Lay the sheet out in `panel`.
pub fn picker_layout(panel: Rect, save: bool) -> PickLay {
    let x = panel.x + SHEET_PAD;
    let title = Rect::new(x, panel.y + SHEET_PAD, panel.w - 2 * SHEET_PAD, 24);
    let top = title.bottom() + 10;
    let buttons_y = panel.bottom() - SHEET_PAD - BUTTON_H;
    let hint = Rect::new(x, buttons_y - 4 - 18, panel.w - 2 * SHEET_PAD - 190, 18);
    let field_h = if save { 30 } else { 0 };
    let field = Rect::new(
        x + PICK_SIDE_W + 12,
        hint.y - 8 - field_h,
        panel.w - 2 * SHEET_PAD - PICK_SIDE_W - 12,
        field_h,
    );
    let list_bottom = if save { field.y - 10 } else { hint.y - 8 };
    let path = Rect::new(
        x + PICK_SIDE_W + 12,
        top,
        panel.w - 2 * SHEET_PAD - PICK_SIDE_W - 12,
        22,
    );
    let list_top = path.bottom() + 4;
    let rows = (((list_bottom - list_top) / PICK_ROW_H).max(1)) as usize;
    let list = Rect::new(path.x, list_top, path.w, rows as i32 * PICK_ROW_H);
    let sidebar = Rect::new(x, top, PICK_SIDE_W, (list.bottom() - top).max(0));
    PickLay {
        panel,
        title,
        sidebar,
        path,
        list,
        field,
        hint,
        rows,
    }
}

impl PickLay {
    /// The list row under `(px, py)` for a list scrolled to `scroll` (an index into the rows).
    pub fn row_at(&self, scroll: usize, px: i32, py: i32) -> Option<usize> {
        self.list
            .contains(px, py)
            .then(|| scroll + ((py - self.list.y) / PICK_ROW_H) as usize)
    }

    /// The sidebar place under `(px, py)`.
    pub fn place_at(&self, px: i32, py: i32) -> Option<usize> {
        (0..PLACES.len()).find(|&i| self.place_rect(i).contains(px, py))
    }

    /// The rectangle of sidebar place `i`.
    pub fn place_rect(&self, i: usize) -> Rect {
        Rect::new(
            self.sidebar.x,
            self.sidebar.y + 22 + i as i32 * 28,
            self.sidebar.w,
            28,
        )
    }
}

/// The sidebar places of the sheet: label keys (look one up with [`place_label`]); the folders are
/// [`place_dir`].
pub const PLACES: [&str; 4] = [
    crate::tk!("files.place.home"),
    crate::tk!("files.place.documents"),
    crate::tk!("files.place.images"),
    crate::tk!("files.place.disk"),
];

/// The folder of sidebar place `i`: the signed-in user's home and the two folders in it, then the
/// root of the volume.
pub fn place_dir(i: usize) -> String {
    use crate::apps::fileman::Place;
    let p = match i {
        0 => Place::Home,
        1 => Place::Documents,
        2 => Place::Images,
        _ => Place::Disk,
    };
    String::from_utf8_lossy(&p.path()).into_owned()
}

/// The label of sidebar place `i` in the language in effect.
pub fn place_label(i: usize) -> &'static str {
    PLACES.get(i).map_or("", |key| crate::i18n::tr(key))
}

/// The place that holds `dir` (the sheet highlights it), if any: the deepest match.
pub fn place_of(dir: &str) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for i in 0..PLACES.len() {
        let p = place_dir(i);
        let inside = if p == "/" {
            dir == "/"
        } else {
            dir == p || (dir.starts_with(p.as_str()) && dir.as_bytes().get(p.len()) == Some(&b'/'))
        };
        if inside && best.is_none_or(|(_, l)| p.len() > l) {
            best = Some((i, p.len()));
        }
    }
    best.map(|(i, _)| i)
}

/// The three buttons of the close question in a panel: Descartar at the left, Cancelar and
/// Salvar at the right, in that order.
pub fn close_buttons(panel: Rect, widths: [i32; 3]) -> [Rect; 3] {
    let y = panel.bottom() - SHEET_PAD - BUTTON_H;
    let save = Rect::new(
        panel.right() - SHEET_PAD - widths[2],
        y,
        widths[2],
        BUTTON_H,
    );
    let cancel = Rect::new(save.x - 8 - widths[1], y, widths[1], BUTTON_H);
    let discard = Rect::new(panel.x + SHEET_PAD, y, widths[0], BUTTON_H);
    [discard, cancel, save]
}

// ---------------------------------------------------------------------------
// The find bar
// ---------------------------------------------------------------------------

/// Rectangles inside the find bar `bar` (one or two rows).
#[derive(Clone, Copy, Debug)]
pub struct FindLay {
    pub find: Rect,
    /// The replacement field and its two buttons (replace mode).
    pub replace: Option<Rect>,
    pub replace_one: Option<Rect>,
    pub replace_all: Option<Rect>,
    pub prev: Rect,
    pub next: Rect,
    pub case: Rect,
    pub close: Rect,
    /// Where the notice text goes (to the right of the buttons of the first row).
    pub notice: Rect,
}

/// Lay the bar out; `replace` is the two-row variant.
pub fn find_layout(bar: Rect, replace: bool, one_w: i32, all_w: i32) -> FindLay {
    let row1_y = bar.y + 8;
    let h = 28;
    let close = Rect::new(bar.right() - PAD - 28, row1_y, 28, h);
    let case = Rect::new(close.x - 4 - 32, row1_y, 32, h);
    let next = Rect::new(case.x - 4 - 28, row1_y, 28, h);
    let prev = Rect::new(next.x - 28, row1_y, 28, h);
    // The notice gets what is left after the field's minimum width, at most 150 px.
    let notice_w = (prev.x - 8 - (bar.x + PAD + 60) - 8).clamp(0, 150);
    let field_right = prev.x - 8 - notice_w;
    let find = Rect::new(bar.x + PAD, row1_y, (field_right - bar.x - PAD).max(60), h);
    let notice = Rect::new(find.right() + 8, row1_y, notice_w, h);
    let (rep, one, all) = if replace {
        let y = row1_y + h + 8;
        let all = Rect::new(close.right() - all_w, y, all_w, h);
        let one = Rect::new(all.x - 8 - one_w, y, one_w, h);
        (
            Some(Rect::new(find.x, y, (one.x - 8 - find.x).max(60), h)),
            Some(one),
            Some(all),
        )
    } else {
        (None, None, None)
    };
    FindLay {
        find,
        replace: rep,
        replace_one: one,
        replace_all: all,
        prev,
        next,
        case,
        close,
        notice,
    }
}

/// What a press inside the find bar landed on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FindHit {
    Find,
    Replace,
    Prev,
    Next,
    Case,
    Close,
    ReplaceOne,
    ReplaceAll,
}

impl FindLay {
    /// The control under `(px, py)`. The go-to-line bar (`goto`) has only its field and the
    /// close button.
    pub fn hit(&self, px: i32, py: i32, goto: bool) -> Option<FindHit> {
        if self.close.contains(px, py) {
            return Some(FindHit::Close);
        }
        if self.find.contains(px, py) {
            return Some(FindHit::Find);
        }
        if goto {
            return None;
        }
        let rects = [
            (Some(self.prev), FindHit::Prev),
            (Some(self.next), FindHit::Next),
            (Some(self.case), FindHit::Case),
            (self.replace, FindHit::Replace),
            (self.replace_one, FindHit::ReplaceOne),
            (self.replace_all, FindHit::ReplaceAll),
        ];
        rects
            .into_iter()
            .find_map(|(r, h)| r.filter(|r| r.contains(px, py)).map(|_| h))
    }
}

#[cfg(test)]
mod tests;
