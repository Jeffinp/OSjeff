//! Storage: block devices and the cache over them (`blockdev`, `blockcache`), the OJFS v2/v3
//! on-disk formats (`fs`, `fs3`) and the path-based virtual file system over them (`vfs`).
//!
//! May depend on: nothing else in the crate. Storage never knows about drawing, windows, networking
//! or apps. See `docs/design/code-structure.md`.

pub mod blockcache;
pub mod blockdev;
pub mod fs;
pub mod fs3;
pub mod vfs;
