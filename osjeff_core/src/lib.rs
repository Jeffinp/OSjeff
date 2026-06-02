//! `osjeff_core` — pure, allocation-free OS logic shared by the kernel.
//!
//! Everything here is `no_std` in production but compiles against `std` under
//! `cargo test`, so the entire module tree is unit-testable on the host. The
//! kernel keeps only hardware glue (framebuffer, port I/O, PS/2, RTC, fonts);
//! all decision logic lives here and is covered by tests.
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

pub mod anim;
pub mod editor;
pub mod heap;
pub mod keymap;
pub mod process;
pub mod terminal;
pub mod window;

pub use anim::Anim;
pub use editor::Editor;
pub use keymap::{Key, Keymap};
pub use process::{ProcKind, ProcState, Process, ProcessTable};
pub use terminal::{Action, Terminal, Time};
pub use window::Rect;
