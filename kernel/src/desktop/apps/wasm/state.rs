//! State of a WASM app window.

use crate::desktop::*;

/// A WASM app window: the `AppManager` instance behind it and which package it is.
pub(crate) struct WasmWin {
    /// Handle in the manager (`0` = none).
    pub id: crate::wasm::AppId,
    /// Manifest id of the package (`snake`, `notes`, ...).
    pub app_id: String,
}
