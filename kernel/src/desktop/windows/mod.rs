//! Windows: what a window is, how it looks and how it is opened, moved and closed.
//!
//! Chrome (title bar, shadow, focus cross-fade), the per-window app instance, the pointer
//! shapes and the Alt+Tab switcher. The apps that live inside the windows are under
//! `desktop::apps`; the system UI around them is under `desktop::shell`.

pub(super) mod chrome;
pub(super) mod cursor;
pub(super) mod instance;
pub(super) mod switcher;
