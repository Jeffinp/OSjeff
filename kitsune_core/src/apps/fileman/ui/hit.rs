//! hit (split out of `ui.rs`).

use super::*;

/// What a click landed on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    Back,
    Forward,
    /// Crumb index (into the full crumb list).
    Crumb(usize),
    /// The `...` marker of folded crumbs (goes to the parent of the first visible crumb).
    CrumbFold,
    /// Empty part of the path bar.
    PathBlank,
    View(ViewMode),
    SortButton,
    Search,
    PreviewButton,
    Place(Place),
    Header(SortKey),
    /// A row or icon (absolute index).
    Item(usize),
    /// Empty space in the list or icon area (a rubber band can start here).
    Blank,
    PreviewPane,
    /// Toolbar, sidebar or status space that does nothing.
    Dead,
}

/// What the hit test needs besides the layout.
pub struct HitCtx<'a> {
    pub mode: ViewMode,
    /// Scroll offset in pixels.
    pub scroll: i32,
    /// Number of rows shown.
    pub count: usize,
    pub crumbs: &'a CrumbLayout,
}

impl Layout {
    /// Resolve a press at `(px, py)`; `None` outside the window.
    pub fn hit(&self, px: i32, py: i32, ctx: &HitCtx<'_>) -> Option<Hit> {
        if !self.window.contains(px, py) {
            return None;
        }
        if self.sidebar.contains(px, py) {
            let side = SideLayout::of(self.sidebar);
            return Some(side.place_at(px, py).map_or(Hit::Dead, Hit::Place));
        }
        if self.back.contains(px, py) {
            return Some(Hit::Back);
        }
        if self.forward.contains(px, py) {
            return Some(Hit::Forward);
        }
        if self.path.contains(px, py) {
            if ctx.crumbs.fold_hit(px, py) {
                return Some(Hit::CrumbFold);
            }
            return Some(
                ctx.crumbs
                    .crumb_at(px, py)
                    .map_or(Hit::PathBlank, Hit::Crumb),
            );
        }
        if self.view_switch.contains(px, py) {
            let seg = if px < self.view_switch.x + self.view_switch.w / 2 {
                0
            } else {
                1
            };
            return Some(Hit::View(ViewMode::from_index(seg)));
        }
        if self.sort.contains(px, py) {
            return Some(Hit::SortButton);
        }
        if self.search.contains(px, py) {
            return Some(Hit::Search);
        }
        if self.preview_btn.contains(px, py) {
            return Some(Hit::PreviewButton);
        }
        if self.toolbar.contains(px, py) || self.status.contains(px, py) {
            return Some(Hit::Dead);
        }
        if self.header.h > 0 && self.header.contains(px, py) {
            return Some(Hit::Header(self.columns().key_at(px)));
        }
        if self.list.contains(px, py) {
            let (lx, ly) = (px - self.list.x, py - self.list.y);
            return Some(
                item_at(ctx.mode, self.list.w, ctx.scroll, lx, ly, ctx.count)
                    .map_or(Hit::Blank, Hit::Item),
            );
        }
        if self.preview.is_some_and(|p| p.contains(px, py)) {
            return Some(Hit::PreviewPane);
        }
        Some(Hit::Dead)
    }
}
