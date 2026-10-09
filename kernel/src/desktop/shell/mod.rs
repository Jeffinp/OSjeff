//! System UI around the windows: the panel, the app bar (dock), the launcher and search
//! overlays, menus and popovers, the quick settings, the calendar / notification centre, toasts
//! and the language-change hook.
//!
//! The system shell state is `Shell` (`model`): the menu bar menus, popovers, the confirmation
//! sheet, the Apps overlay, Busca and the app bar, the commands they issue (`commands`) and the
//! per-frame stepping of their animations (`step`). Drawing lives in `panel/`, `taskbar/` and
//! `overlays/`; this folder is the model and the glue to the rest of the desktop.

mod commands;
pub(super) mod lang;
pub(super) mod model;
pub(super) mod overlays;
pub(super) mod panel;
mod step;
pub(super) mod taskbar;
pub(super) mod toasts;

pub(crate) use model::*;
