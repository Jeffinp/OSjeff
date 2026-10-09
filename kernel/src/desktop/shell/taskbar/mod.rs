//! The taskbar: a floating rounded bar at the bottom with the Apps button, the pinned apps (they
//! can be dragged to reorder), the apps that run without being pinned, running indicators (a long
//! pill for the focused app, a dot for the others), a tooltip, the launch hop, a context menu
//! with the app's windows, and a *Mostrar área de trabalho* sliver at the right end.
//!
//! Geometry and rules are pure (`kitsune_core::taskbar`). The bar is a plain translucent surface
//! (nothing is blurred) painted live over the cached scene; icons are cached scaled surfaces.
//! There is no magnification: an icon lifts a little under the pointer (a spring per icon) and a
//! dragged icon makes its neighbours slide (a spring per icon too).

mod layout;
mod paint;
mod pointer;
mod state;

pub(crate) use state::*;
