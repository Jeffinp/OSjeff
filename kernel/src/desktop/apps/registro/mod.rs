//! Registro: the system-log viewer, a window onto the kernel's log ring (`crate::klog`).
//!
//! A table (time, level chip, source, message in the mono font) with a search field, a level
//! filter, an auto-follow switch, and "Limpar" / "Salvar" buttons. The text scrolls smoothly by
//! pixels (wheel, keys), with an overlay scrollbar that fades.
//!
//! The window works on a private copy (snapshot) of the ring, refreshed once a second and after
//! any input, so drawing never touches the live ring or holds interrupts off. The filter /
//! indexing logic is `kitsune_core::klog`; the saved file goes through the `LogSink` trait
//! (`/var/log/syslog.txt`).

mod input;
mod paint;
mod state;

pub(crate) use state::LogState;
