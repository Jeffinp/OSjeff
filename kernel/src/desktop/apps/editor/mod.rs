//! Editor, the text editor window: `Desktop` methods around [`EditorState`].
//!
//! The text engine is `kitsune_core::editor2` (gap buffer, UTF-8, selection, undo/redo,
//! find/replace, line numbers) and the questions it does not own (the Open / Save-as picker,
//! "save changes?") are `editor2::dialog`; all of it is tested on the host, and so is the window
//! geometry (`editor2::ui`). This folder feeds keys and the mouse in, keeps the animation state
//! (eased caret, selection, sheets, scrollbar) and reaches files only through
//! [`vfs`](crate::desktop::services::vfs).
//!
//! A window with unsaved changes never closes silently: every way to close it (title-bar button,
//! Ctrl+Q, the task manager, the terminal's `kill`, power actions) goes through
//! [`Desktop::request_close`], which asks first.
//!
//! - `state`, `geometry` data and rectangles; `sync` window glue
//! - `keys`, `mouse` input; `fileio` open / save / close guard; `step` animation
//! - `paint`, `sheets` drawing

mod fileio;
mod geometry;
mod keys;
mod mouse;
mod paint;
mod sheets;
mod state;
mod step;
mod sync;

pub(crate) use state::EditorState;
