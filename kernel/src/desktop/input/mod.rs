//! Keyboard and pointer dispatch: raw PS/2 events in, calls into the shell, the windows and
//! the apps out.

pub(super) mod dispatch;

pub(crate) use dispatch::Special;
