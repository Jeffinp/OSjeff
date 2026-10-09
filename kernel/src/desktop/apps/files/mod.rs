//! Arquivos, the file manager.
//!
//! Everything that reaches the filesystem from the desktop goes through
//! [`vfs`](crate::desktop::services::vfs): the terminal commands (`shellhost`), the editor's
//! load and save, and the file manager's commands (here).
//!
//! The file manager's state and decisions live in `kitsune_core::fileman` (tested on the host:
//! rows, sort, search, selection, the geometry and hit testing of `fileman::ui`, the
//! drag-and-drop rules); this folder wires them to the VFS, the window table, the shell's menus
//! and the clipboard of paths, and runs long copies in steps from `animate`.
//!
//! - `state` the window state; `labels` text helpers
//! - `nav`, `menus` navigation and menus; `mouse`, `drag`, `keys` input; `commands` the commands
//! - `jobs` long copies and per-frame stepping; `props` the sheets; `preview` the preview pane
//! - `paint` drawing

mod commands;
mod drag;
mod jobs;
mod keys;
mod labels;
mod menus;
mod mouse;
mod nav;
pub(super) mod paint;
mod preview;
mod props;
mod state;

pub(crate) use labels::{local_time, modified_label};
pub(crate) use state::*;
