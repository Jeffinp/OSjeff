//! regions (split out of `ui.rs`).

use super::*;

/// Geometry of a file-manager window, all in screen coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub window: Rect,
    /// Everything below the title bar.
    pub body: Rect,
    pub sidebar: Rect,
    /// The column right of the sidebar (toolbar, list, status bar).
    pub main: Rect,
    pub toolbar: Rect,
    pub back: Rect,
    pub forward: Rect,
    /// The path bar (breadcrumbs).
    pub path: Rect,
    /// The two-segment view switch.
    pub view_switch: Rect,
    pub sort: Rect,
    pub search: Rect,
    pub preview_btn: Rect,
    /// Column titles of the list (empty height in icon mode).
    pub header: Rect,
    /// The scrolling viewport of the rows or icons.
    pub list: Rect,
    /// The preview pane, when open.
    pub preview: Option<Rect>,
    pub status: Rect,
}

impl Layout {
    /// Geometry for a window at `r`. `search_open` keeps the search field expanded even in a
    /// narrow window.
    pub fn of(r: Rect, mode: ViewMode, preview_open: bool, search_open: bool) -> Layout {
        let top = r.y + TITLE_H;
        let body = Rect::new(r.x, top, r.w, (r.h - TITLE_H).max(0));
        let sidebar = Rect::new(r.x, top, SIDEBAR_W.min(r.w), body.h);
        let main = Rect::new(r.x + SIDEBAR_W, top, (r.w - SIDEBAR_W).max(0), body.h);
        let toolbar = Rect::new(main.x, main.y, main.w, TOOLBAR_H.min(main.h));
        let by = toolbar.y + (TOOLBAR_H - BTN) / 2;
        let back = Rect::new(main.x + 12, by, BTN, BTN);
        let forward = Rect::new(back.right() + 2, by, BTN, BTN);
        // Right cluster, laid out from the right edge.
        let preview_btn = Rect::new(main.right() - 12 - BTN, by, BTN, BTN);
        let wide = main.w >= 620;
        let search_w = if search_open || wide {
            if main.w >= 820 { 188 } else { 148 }
        } else {
            BTN
        };
        let path_x = forward.right() + 12;
        let (path, view_switch, sort, search);
        if search_open && !wide {
            // Not enough room for everything: the field takes the place of the path bar and
            // of the view and sort buttons while it is open.
            let right = preview_btn.x - 8;
            search = Rect::new(path_x, by, (right - path_x).max(BTN), BTN);
            path = Rect::new(path_x, by, 0, BTN);
            view_switch = Rect::new(path_x, by, 0, BTN);
            sort = Rect::new(path_x, by, 0, BTN);
        } else {
            search = Rect::new(preview_btn.x - 8 - search_w, by, search_w, BTN);
            sort = Rect::new(search.x - 8 - BTN, by, BTN, BTN);
            view_switch = Rect::new(sort.x - 8 - 64, by, 64, BTN);
            path = Rect::new(path_x, by, (view_switch.x - 8 - path_x).max(0), BTN);
        }
        let status = Rect::new(
            main.x,
            (main.bottom() - STATUS_H).max(main.y),
            main.w,
            STATUS_H.min(main.h),
        );
        let content_top = toolbar.bottom();
        let pane_w = if preview_open {
            PREVIEW_W.min(main.w / 2)
        } else {
            0
        };
        let preview = (pane_w > 0).then(|| {
            Rect::new(
                main.right() - pane_w,
                content_top,
                pane_w,
                (status.y - content_top).max(0),
            )
        });
        let content_w = main.w - pane_w;
        let header_h = if mode == ViewMode::List { HEADER_H } else { 0 };
        let header = Rect::new(main.x, content_top, content_w, header_h);
        let list_y = content_top + header_h;
        let list = Rect::new(main.x, list_y, content_w, (status.y - list_y).max(0));
        Layout {
            window: r,
            body,
            sidebar,
            main,
            toolbar,
            back,
            forward,
            path,
            view_switch,
            sort,
            search,
            preview_btn,
            header,
            list,
            preview,
            status,
        }
    }

    /// Whether the search field is wide enough to be a field (else it is a magnifier button).
    pub fn search_is_field(&self) -> bool {
        self.search.w > BTN
    }

    /// The columns of the list for its current width.
    pub fn columns(&self) -> Columns {
        Columns::of(self.list.x, self.list.w)
    }
}

/// X positions of the list columns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Columns {
    /// Left edge of the name column's content (icon starts here).
    pub name_x: i32,
    /// Left edge of the size column and of the date column; the date column is absent when
    /// `date_x == right`.
    pub size_x: i32,
    pub date_x: i32,
    pub right: i32,
}

impl Columns {
    pub fn of(x: i32, w: i32) -> Columns {
        let right = x + w;
        let date_x = if w < NARROW_LIST {
            right
        } else {
            right - DATE_COL_W - 12
        };
        let size_x = date_x - SIZE_COL_W;
        Columns {
            name_x: x + ROW_INSET + 8,
            size_x,
            date_x,
            right,
        }
    }

    pub fn has_date(&self) -> bool {
        self.date_x < self.right
    }

    /// The sort key under header x `px`.
    pub fn key_at(&self, px: i32) -> SortKey {
        if self.has_date() && px >= self.date_x {
            SortKey::Modified
        } else if px >= self.size_x {
            SortKey::Size
        } else {
            SortKey::Name
        }
    }
}

/// A sidebar entry and where it sits.
#[derive(Clone, Copy, Debug)]
pub struct SideLayout {
    pub favorites_title: Rect,
    pub places_title: Rect,
    pub items: [(Place, Rect); 6],
}

impl SideLayout {
    pub fn of(sidebar: Rect) -> SideLayout {
        let x = sidebar.x + 8;
        let w = (sidebar.w - 16).max(0);
        let mut y = sidebar.y + 12;
        let favorites_title = Rect::new(x + 8, y, w - 8, 24);
        y += 24;
        let mut items = [(Place::Home, Rect::new(0, 0, 0, 0)); 6];
        for (i, p) in [
            Place::Home,
            Place::Documents,
            Place::Images,
            Place::Apps,
            Place::Trash,
        ]
        .into_iter()
        .enumerate()
        {
            items[i] = (p, Rect::new(x, y, w, 28));
            y += 30;
        }
        y += 8;
        let places_title = Rect::new(x + 8, y, w - 8, 24);
        y += 24;
        items[5] = (Place::Disk, Rect::new(x, y, w, 56));
        SideLayout {
            favorites_title,
            places_title,
            items,
        }
    }

    pub fn place_at(&self, px: i32, py: i32) -> Option<Place> {
        self.items
            .iter()
            .find(|(_, r)| r.contains(px, py))
            .map(|&(p, _)| p)
    }
}
