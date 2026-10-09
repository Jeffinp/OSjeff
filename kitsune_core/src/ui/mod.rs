//! Drawing and visual identity: the framebuffer primitives (`gfx`, `raster`), fonts and text
//! (`ttf`, `glyph`, `fontcache`, `textlayout`), vector icons and art (`iconart`, `appart`, `brand`,
//! `pointer`, `cursor`), motion (`anim`), the design tokens (`style`), widgets and window chrome
//! geometry (`widgets`, `chrome`, `layout`) and the wallpaper.
//!
//! May depend on: `format` (the wallpaper decodes images), `i18n`, `windowing` (chrome and layout
//! are drawn around windows: `Rect`, the taskbar geometry, the launcher grid). See
//! `docs/design/code-structure.md`.

pub mod anim;
pub mod appart;
pub mod brand;
pub mod chrome;
pub mod cursor;
pub mod fontcache;
pub mod gfx;
pub mod glyph;
pub mod iconart;
pub mod layout;
pub mod pointer;
pub mod raster;
pub mod style;
pub mod textlayout;
pub mod ttf;
pub mod wallpaper;
pub mod widgets;
