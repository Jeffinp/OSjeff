//! System UI around the windows: the panel, the app bar (dock), the launcher and search
//! overlays, menus and popovers, the quick settings, the calendar / notification centre,
//! toasts and the language-change hook.

pub(super) mod lang;
pub(super) mod overlays;
pub(super) mod panel;
pub(super) mod state;
pub(super) mod taskbar;
pub(super) mod toasts;

pub(crate) use state::*;
