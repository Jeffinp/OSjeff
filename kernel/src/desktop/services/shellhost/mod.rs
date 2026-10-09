//! What the shell engine (`kitsune_core::shell`) runs on: the filesystem, the system information
//! and the thread that executes command lines.
//!
//! * `fs`: [`VfsFs`] implements `ShellFs` over [`vfs`](crate::desktop::services::vfs), the
//!   desktop's only file API: absolute normalized paths, a working directory per terminal.
//! * `sys`: [`KSys`] implements `SysInfo`. The facts that live in the compositor (the process
//!   table, disk identity, heap use) arrive as a [`Snap`] taken when the command is posted;
//!   everything else (clock, uptime, network) is read from thread-safe sources.
//! * `jobs`: `shelld` ([`worker`]) is one kernel thread that runs command lines posted by any
//!   terminal, one at a time. `sleep`, `ping`, `nslookup` and `curl` can wait for seconds;
//!   running them off the compositor keeps the desktop alive. A terminal hands its shell and
//!   filesystem state ([`Ctx`]) to the job and gets them back with the result ([`Done`]); Ctrl+C
//!   is [`cancel`].
//! * `ui`: commands that must act on the desktop (`edit`, `tasks`, `reboot`...) cannot run on
//!   that thread: they leave a [`UiReq`] that the compositor applies.

mod fs;
mod jobs;
mod sys;
mod ui;

pub(crate) use jobs::*;
pub use jobs::{worker, worker2};
pub(crate) use sys::Snap;
pub(crate) use ui::{UiReq, take_ui};
