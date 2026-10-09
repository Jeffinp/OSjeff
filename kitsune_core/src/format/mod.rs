//! Data formats and small utilities: base64, the deflate family (`inflate`, `deflate`, `gzip`),
//! the image codecs (`png`, `bmp`, `ppm`, and `image` over them), Unix time and calendar maths
//! (`unixtime`) and the substring matcher (`search`).
//!
//! May depend on: nothing else in the crate (this is the leaf layer). See
//! `docs/design/code-structure.md`.

pub mod base64;
pub mod bmp;
pub mod deflate;
pub mod gzip;
pub mod image;
pub mod inflate;
pub mod png;
pub mod ppm;
pub mod search;
pub mod unixtime;
