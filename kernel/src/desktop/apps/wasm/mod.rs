//! WASM app windows, the installed-app catalog and the install / remove actions.
//!
//! Every WASM window is an instance of the `AppManager` (`crate::wasm`); the window record only
//! keeps the instance handle ([`WasmWin`]). The catalog (what the launcher lists) is rebuilt
//! from `/apps` whenever something is installed or removed. See `docs/design/apps.md`.

mod catalog;
mod paint;
mod pointer;
mod state;
mod window;

pub(crate) use catalog::*;
pub(crate) use pointer::wasm_key_code;
pub(crate) use state::*;
pub(crate) use window::*;
