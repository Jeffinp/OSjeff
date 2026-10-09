//! Navegador, the web browser window.

pub(super) mod input;
pub(super) mod logic;
pub(super) mod paint;
pub(super) mod ui;

pub(crate) use paint::PaintCache;
mod state;

pub(crate) use state::*;
