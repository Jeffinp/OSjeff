//! Desktop-side services the apps use: the virtual file system front end, the kernel
//! implementations of the system-management traits, and the shell worker threads.

pub(super) mod shellhost;
pub(super) mod sysstore;
pub(crate) mod vfs;
