//! Storage: block devices and the cache over them (`blockdev`, `blockcache`), the OJFS v2/v3
//! on-disk formats (`fs`, `fs3`) and the path-based virtual file system over them (`vfs`), plus `homes` (the folders accounts rely on) and `secured`, the VFS wrapper that enforces
//! ownership and permissions for one user.
//!
//! May depend on: `security` (permission rules, used by `secured`). Storage never knows about drawing, windows, networking
//! or apps. See `docs/design/code-structure.md`.

pub mod blockcache;
pub mod blockdev;
pub mod fs;
pub mod fs3;
pub mod homes;
pub mod secured;
pub mod vfs;
