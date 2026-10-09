//! The pointer drag in progress: moving, un-snapping, resizing, panning, selecting.
//! The per-packet handling is in `input::drag`.

use crate::desktop::*;

/// A pointer drag in progress.
#[derive(Clone, Copy)]
pub(crate) enum DragMode {
    /// Moving the window: offset of the grab point inside it.
    Move { grab_dx: i32, grab_dy: i32 },
    /// A maximised or tiled window's title was pressed at `(ox, oy)`: once the pointer moves it
    /// is restored under the pointer and becomes a [`DragMode::Move`].
    Unsnap { ox: i32, oy: i32 },
    /// Dragging the image of a viewer window: last pointer position.
    Pan { last_x: i32, last_y: i32 },
    /// Extending a text selection in an editor window.
    Select,
    /// Resizing from `edge`; `start` is the window rect and `(ox, oy)` the
    /// pointer position when the drag began.
    Resize {
        edge: ResizeEdge,
        start: Rect,
        ox: i32,
        oy: i32,
    },
    /// Selecting text on a browser page (the anchor lives in the window's browser state).
    PageSelect,
    /// A control of a system app being dragged (a slider of Ajustes).
    Ui,
    /// A press in a file manager: a click, a drag of items or a rubber band (the gesture
    /// lives in the window's state).
    Files,
}

pub(crate) struct Drag {
    pub win: WindowId,
    pub mode: DragMode,
}
