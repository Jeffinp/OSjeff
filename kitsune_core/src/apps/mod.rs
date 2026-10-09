//! The logic of the apps, free of the framebuffer: the file manager (`fileman`), the image viewer
//! (`viewer`), the editor (`editor2`), the terminal grid (`termui`) and the shell engine (`shell`),
//! the calculator (`calc`) and the activity model (`activity`).
//!
//! May depend on: `ui`, `windowing`, `storage`, `system`, `format`, `platform` (app manifests
//! listed by the file manager), `i18n`. Nothing depends on `apps` except the kernel (and `i18n`
//! tests). See `docs/design/code-structure.md`.

pub mod activity;
pub mod calc;
pub mod editor2;
pub mod fileman;
pub mod shell;
pub mod termui;
pub mod viewer;
