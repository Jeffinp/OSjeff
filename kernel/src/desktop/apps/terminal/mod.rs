//! Terminal: `Desktop` methods around [`TermState`].
//!
//! The behaviour (line editing, history, Tab, Ctrl+C / Ctrl+L, scrollback) is
//! `kitsune_core::shell::Term`, tested on the host. This folder connects it to the window: the
//! character grid that fits the window, keys in, pixels out, and the hand-off of command lines
//! to the `shelld` thread ([`shellhost`](crate::desktop::services::shellhost)). A command that
//! waits (`sleep`, `ping`, `curl`) never blocks the compositor: the terminal shows "executando"
//! and the result is picked up by [`Desktop::step_shell_jobs`] on a later frame.

mod input;
mod mouse;
mod paint;
mod run;
mod state;

pub(crate) use state::{TermState, term_grid};
