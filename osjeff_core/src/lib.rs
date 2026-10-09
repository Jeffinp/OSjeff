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

pub mod activity;
pub mod anim;
pub mod appabi;
pub mod appfs;
pub mod appinstall;
pub mod appmanifest;
pub mod appnet;
pub mod base64;
pub mod blockcache;
pub mod blockdev;
pub mod bmp;
pub mod browser;
pub mod calc;
pub mod chrome;
pub mod clipboard;
pub mod compositor;
pub mod cursor;
pub mod deflate;
pub mod dns;
pub mod editor2;
pub mod entropy;
pub mod fileman;
pub mod fontcache;
pub mod fs;
pub mod fs3;
pub mod gfx;
pub mod glyph;
pub mod gzip;
pub mod heap;
pub mod hw;
pub mod icmp;
pub mod iconart;
pub mod image;
pub mod inflate;
pub mod input;
pub mod keymap;
pub mod klog;
pub mod launcher;
pub mod layout;
pub mod lease;
pub mod net;
pub mod netstats;
pub mod notify;
pub mod paging;
pub mod png;
pub mod pointer;
pub mod ppm;
pub mod process;
pub mod raster;
pub mod redirect;
pub mod rng;
pub mod schedule;
pub mod search;
pub mod settings;
pub mod shell;
pub mod snap;
pub mod sntp;
pub mod style;
pub mod sysif;
pub mod sysmon;
pub mod taskbar;
#[cfg(test)]
pub(crate) mod testutil;
pub mod textlayout;
pub mod tlsverify;
pub mod ttf;
pub mod unixtime;
pub mod vfs;
pub mod viewer;
pub mod wallpaper;
pub mod wasmsec;
pub mod web;
pub mod widgets;
pub mod window;
pub mod winman;
pub mod wm;
pub mod x509;

pub use anim::Anim;
pub use browser::Browser;
pub use calc::Calc;
pub use clipboard::Clipboard;
pub use hw::rtc::Time;
pub use keymap::{Key, Keymap};
pub use process::{ProcKind, ProcState, Process, ProcessTable};
pub use window::Rect;
