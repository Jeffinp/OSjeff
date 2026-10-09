//! Tests of the Arquivos geometry and models.

use super::*;
use alloc::vec;

fn win() -> Rect {
    Rect::new(100, 100, 860, 520)
}

fn lay(mode: ViewMode, preview: bool) -> Layout {
    Layout::of(win(), mode, preview, false)
}

fn crumbs_for(l: &Layout, widths: &[i32]) -> CrumbLayout {
    crumb_layout(l.path, widths)
}

fn hit(l: &Layout, mode: ViewMode, scroll: i32, n: usize, p: (i32, i32)) -> Option<Hit> {
    let crumbs = crumbs_for(l, &[40, 60, 50]);
    l.hit(
        p.0,
        p.1,
        &HitCtx {
            mode,
            scroll,
            count: n,
            crumbs: &crumbs,
        },
    )
}

fn centre(r: Rect) -> (i32, i32) {
    (r.x + r.w / 2, r.y + r.h / 2)
}

fn paths(p: &[&str]) -> Vec<Vec<u8>> {
    p.iter().map(|s| s.as_bytes().to_vec()).collect()
}

mod drag_drop;
mod hit_testing;
mod list_grid_models;
mod path_bar;
mod regions;
mod scrolling;
mod search_kinds_previews;
