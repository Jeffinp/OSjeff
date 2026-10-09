//! The app platform: the WASM app ABI and security rules (`appabi`, `wasmsec`), manifests and
//! packages (`appmanifest`, `appinstall`), the per-app file system (`appfs`) and the network
//! permission broker (`appnet`).
//!
//! May depend on: `format`, `storage`, `browsing` (the HTTP response parsing `appnet` reuses),
//! `i18n`. See `docs/design/code-structure.md`.

pub mod appabi;
pub mod appfs;
pub mod appinstall;
pub mod appmanifest;
pub mod appnet;
pub mod wasmsec;
