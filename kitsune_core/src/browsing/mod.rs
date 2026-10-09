//! The browser engine: the HTML / CSS / layout engine (`web`), the browser model (`browser`: URLs,
//! tabs, history, bookmarks, error pages, TLS status) and HTTP redirect policy (`redirect`).
//!
//! May depend on: `format` (decoders), `network` (TLS and redirects), `ui` (motion preferences),
//! `i18n`. See `docs/design/code-structure.md`.

pub mod browser;
pub mod redirect;
pub mod web;
