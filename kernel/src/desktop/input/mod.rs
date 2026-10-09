//! Keyboard and pointer dispatch: raw PS/2 events in, calls into the shell, the windows and the
//! apps out.
//!
//! - `keys` scancodes to keys, global shortcuts, routing to the focused app; `clipboard`
//! - `pointer` every mouse packet (right press, left press, release, drag, hover); `click` a
//!   press on a window; `wheel` the wheel

mod click;
mod clipboard;
mod drag;
mod hover;
mod keys;
mod pointer;
mod press;
mod wheel;

pub(crate) use keys::Special;
