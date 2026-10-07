//! The filesystem behind the app sandbox: the **single swap point**.
//!
//! Apps and the installer talk to an [`AppFs`] through [`with`]. Today that is an
//! in-memory [`MemFs`] (`/apps`, `/data/<id>`, `/home` live in RAM and are lost
//! at reboot) because `kernel::storage` (OJFS v3 on the ATA disk) is not wired
//! yet.
//!
//! **To switch to the disk:** make [`with`] call `storage::with_fs(|fs| ...)` and
//! implement `AppFs` for the OJFS v3 handle (the trait is path-addressed over
//! canonical absolute paths; `osjeff_core::fs3` already has `stat/read_at/
//! write_at/mkdir/unlink/rename/readdir`). Nothing above this function changes:
//! the sandbox, quotas, descriptors, installer and launcher only see
//! `&mut dyn AppFs`.
//!
//! Mutual exclusion: the kernel is single-core, and the compositor thread (installer,
//! launcher) and `appd` (guest file calls) both come here, so access runs with
//! interrupts masked (short operations: one `read_at`/`write_at` of <= 64 KiB, or
//! a package install).

use crate::sync::RacyCell;
use osjeff_core::appfs::{AppFs, MemFs};

/// Cap on the RAM filesystem (bytes of file data).
const MEMFS_BYTES: u64 = 24 << 20;

static FS: RacyCell<Option<MemFs>> = RacyCell::new(None);

/// Run `f` on the app filesystem. Do not call `with` again from inside `f`.
pub(crate) fn with<R>(f: impl FnOnce(&mut dyn AppFs) -> R) -> R {
    x86_64::instructions::interrupts::without_interrupts(|| {
        // SAFETY: single core with interrupts masked for the whole closure, and `f` does not re-enter
        // `with`, so this is the only live reference to the cell's contents.
        let slot = unsafe { &mut *FS.get() };
        let fs = slot.get_or_insert_with(|| MemFs::new(MEMFS_BYTES));
        f(fs)
    })
}
