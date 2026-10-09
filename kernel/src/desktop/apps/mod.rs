//! The apps. Each folder holds one app: its state, input handling and drawing, so an app can
//! be understood and changed in one place. The logic that does not need the framebuffer lives
//! in `kitsune_core`.

pub(super) mod ajustes;
pub(super) mod browser;
pub(super) mod calculadora;
pub(super) mod editor;
pub(super) mod files;
pub(super) mod gallery;
pub(super) mod registro;
pub(super) mod tarefas;
pub(super) mod terminal;
pub(super) mod viewer;
pub(super) mod wasm;
