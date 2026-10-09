//! Windows: geometry and hit testing (`window`), the dynamic window table and its animations
//! (`winman`, `wm`), snapping (`snap`), the compositor's damage engine (`compositor`), the taskbar
//! model (`taskbar`) and the launcher (`launcher`).
//!
//! May depend on: `ui` (style tokens and motion), `format` (the substring matcher), `i18n`. See
//! `docs/design/code-structure.md`.

pub mod compositor;
pub mod launcher;
pub mod snap;
pub mod taskbar;
pub mod window;
pub mod winman;
pub mod wm;
