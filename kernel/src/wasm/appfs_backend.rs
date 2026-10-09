//! The filesystem behind the app sandbox: the **single swap point**.
//!
//! Apps and the installer talk to an [`AppFs`] through [`with`]. It is
//! [`VolumeFs`] over the desktop VFS volume (`desktop/vfs.rs`): the OJFS v3 disk
//! when it is mounted, so `/apps/<id>.wasm`, `/data/<id>` and `/home` **persist
//! across reboots** and live in the same namespace as the user's files; when only
//! the RAM volume exists (no disk, a 64 KiB disk, an unknown or failed one) the
//! very same RAM volume the file manager uses, so the two stay consistent (and
//! nothing persists, which the file manager already says). Nothing above this
//! function changes: the sandbox, quotas, descriptors, installer and launcher only
//! see `&mut dyn AppFs`.
//!
//! # Locking
//!
//! One call of [`with`] is one critical section of the volume's `YieldMutex`
//! (`storage::with_fs` for the disk, the RAM volume's own lock otherwise). A
//! thread may hold it across disk I/O (the ATA driver yields between sectors); the
//! compositor and `appd` simply wait for each other. The rules:
//!
//! * `f` does file-system work only: **never run guest code, take another lock or
//!   call `with` again** from inside it (the lock is not re-entrant: a nested call
//!   returns [`FsError::Io`] instead of deadlocking);
//! * interrupts stay enabled: the lock yields, it does not mask;
//! * a lock that cannot be had (re-entered, or its holder's thread died) is an
//!   [`FsError::Io`] to the caller, never a hang;
//! * the sandbox state (descriptors, quota counters) lives in each app's own
//!   `HostState`, not here, so a guest call never holds the lock between host calls.
//!
//! Timestamps come from the lock-free boot-RTC clock (`clock::local_unix_ms`):
//! `appd` must not touch the CMOS ports the compositor reads.

use crate::desktop::vfs;
use kitsune_core::appfs::{AppFs, FsError, VolumeFs};

/// Unix seconds for new files (0 when the RTC was unreadable at boot).
fn now() -> u64 {
    crate::clock::local_unix_ms().map_or(0, |ms| ms / 1000)
}

/// Run `f` on the app filesystem. `Err(FsError::Io)` when the volume is not
/// available (lock re-entered, owner thread dead, no volume at all). Do not call
/// `with` again from inside `f`.
pub(crate) fn with<R>(f: impl FnOnce(&mut dyn AppFs) -> R) -> Result<R, FsError> {
    let t = now();
    let mut mutated = false;
    let r = vfs::with_backend(|be| {
        let mut fs = VolumeFs::new(be, t);
        let r = f(&mut fs);
        mutated = fs.mutated();
        r
    })
    .map_err(|_| FsError::Io);
    if mutated {
        // Lets the file manager notice that an app changed files.
        vfs::touch();
    }
    r
}

/// [`with`] for a closure that itself returns a `Result` whose error can hold an
/// [`FsError`] (the sandbox, the installer): the two layers are flattened.
pub(crate) fn try_with<T, E: From<FsError>>(
    f: impl FnOnce(&mut dyn AppFs) -> Result<T, E>,
) -> Result<T, E> {
    with(f).map_err(E::from)?
}
