//! `osjeff_core` — pure, allocation-free OS logic shared by the kernel.
//!
//! Everything here is `no_std` in production but compiles against `std` under
//! `cargo test`, so the entire module tree is unit-testable on the host. The
//! kernel keeps only hardware glue (framebuffer, port I/O, PS/2, RTC, fonts);
//! all decision logic lives here and is covered by tests.
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

// Most of the crate is fixed-buffer and allocation-free. The `web` rendering
// engine is the exception: an HTML/CSS box-model engine needs a dynamic DOM and
// display list, so it uses `alloc` (backed by the kernel's global allocator in
// production, and by `std` under tests).
extern crate alloc;

pub mod anim;
pub mod browser;
pub mod calc;
pub mod clipboard;
pub mod editor;
pub mod fs;
pub mod heap;
pub mod keymap;
pub mod net;
pub mod process;
pub mod terminal;
pub mod web;
pub mod window;

pub use anim::Anim;
pub use browser::Browser;
pub use calc::Calc;
pub use clipboard::Clipboard;
pub use editor::Editor;
pub use keymap::{Key, Keymap};
pub use process::{ProcKind, ProcState, Process, ProcessTable};
pub use terminal::{Action, FileName, Terminal, Time};
pub use window::Rect;
