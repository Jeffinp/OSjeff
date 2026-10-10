//! The pixels of Arquivos as numbers: window regions, the sidebar, the breadcrumb bar, the
//! list and icon-grid geometry (hit testing, visible range, rubber band), the smooth scroller,
//! the drag-and-drop rules and the preview helpers.
//!
//! The kernel draws and routes input; everything it needs to *decide* lives here so it is
//! tested on the host and the renderer and the mouse handler cannot disagree. Text widths are
//! never guessed: callers pass measured widths in pixels.

use super::{APPS_PATH, Place, SortKey, TRASH_PATH};
use crate::ui::anim::Spring;
use crate::windowing::window::Rect;
use alloc::string::String;
use alloc::vec::Vec;

mod dnd;
mod drags;
mod hit;
mod names;
mod pathbar;
mod regions;
mod rows;
mod scroll;
pub use dnd::*;
pub use drags::*;
pub use hit::*;
pub use names::*;
pub use pathbar::*;
pub use regions::*;
pub use rows::*;
pub use scroll::*;

pub const TITLE_H: i32 = crate::windowing::window::TITLE_H;
pub const SIDEBAR_W: i32 = 188;
pub const TOOLBAR_H: i32 = 44;
pub const HEADER_H: i32 = 28;
pub const ROW_H: i32 = 28;
pub const STATUS_H: i32 = 28;
pub const PREVIEW_W: i32 = 252;
/// Side of a square toolbar button.
pub const BTN: i32 = 28;
/// Padding above the first row and below the last.
pub const LIST_PAD: i32 = 4;
/// Horizontal inset of the selection pill of a row.
pub const ROW_INSET: i32 = 8;
/// Icon grid: cell size and outer padding.
pub const CELL_W: i32 = 104;
pub const CELL_H: i32 = 96;
pub const GRID_PAD: i32 = 12;
/// Pixels the pointer must travel with the button down before a press becomes a drag.
pub const DRAG_THRESHOLD: i32 = 5;
/// Width of the size and date columns of the list.
pub const SIZE_COL_W: i32 = 84;
pub const DATE_COL_W: i32 = 152;
/// Below this list width the date column is dropped.
pub const NARROW_LIST: i32 = 420;

/// How the folder is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    List,
    Icons,
}

impl ViewMode {
    pub fn index(self) -> usize {
        match self {
            ViewMode::List => 0,
            ViewMode::Icons => 1,
        }
    }

    pub fn from_index(i: usize) -> ViewMode {
        if i == 1 {
            ViewMode::Icons
        } else {
            ViewMode::List
        }
    }
}

// ---------------------------------------------------------------------------
// Window regions
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// The path bar
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// What a press landed on
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Rows and icons
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Rubber band and drags
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Drag and drop
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Smooth scrolling
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Names, kinds and dates
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
