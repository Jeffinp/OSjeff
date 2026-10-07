//! Kernel implementations of the `osjeff_core::sysif` traits that the
//! system-management apps use, backed by what exists today: the RAM image of
//! the FS v2 root directory. The FS v3 front replaces these with `/var/log/`,
//! `/etc/osjeff.conf` and real volume accounting by implementing the same
//! traits (see `docs/design/sysmgmt.md`).

use super::*;
use alloc::vec::Vec;
use osjeff_core::sysif::{
    DiskUsage, DiskUsageInfo, LogSink, NetCounters, NetStats, SettingsStore, SinkError,
};

fn map_fs_error(e: fs::FsError) -> SinkError {
    match e {
        fs::FsError::NoSpace => SinkError::NoSpace,
        fs::FsError::NameTooLong | fs::FsError::EmptyName => SinkError::BadName,
        _ => SinkError::Unavailable,
    }
}

/// Writes top-level files of FS v2. A v2 file holds at most
/// [`fs::MAX_FILE_SIZE`] bytes, so a longer dump keeps its newest whole lines
/// and the call reports [`SinkError::Truncated`] (the file *is* written).
pub(crate) struct FsV2Sink;

impl LogSink for FsV2Sink {
    fn write_file(&mut self, name: &[u8], data: &[u8]) -> Result<(), SinkError> {
        let (body, cut) = osjeff_core::klog::tail_lines(data, fs::MAX_FILE_SIZE);
        fs::write(disk(), name, body).map_err(map_fs_error)?;
        flush_disk();
        if cut {
            Err(SinkError::Truncated { kept: body.len() })
        } else {
            Ok(())
        }
    }
}

/// Space accounting of the FS v2 image: 48 slots of up to 1 KiB each (a
/// trashed file still holds its slot until it is purged).
pub(crate) struct FsV2Usage;

impl DiskUsage for FsV2Usage {
    fn label(&self) -> &str {
        "FS v2 (IDE 1)"
    }

    fn usage(&self) -> DiskUsageInfo {
        let img = disk();
        let used_slots = (0..fs::MAX_FILES).filter(|&i| fs::is_used(img, i));
        let (mut slots, mut bytes) = (0u32, 0u64);
        for i in used_slots {
            slots += 1;
            bytes += fs::size_at(img, i) as u64;
        }
        DiskUsageInfo {
            total_bytes: Some((fs::MAX_FILES * fs::MAX_FILE_SIZE) as u64),
            used_bytes: Some(bytes),
            items_used: Some(slots),
            items_total: Some(fs::MAX_FILES as u32),
        }
    }
}

/// The NE2000 byte counters (`crate::netstats`); `None` when there is no NIC.
pub(crate) struct KernelNetStats;

impl NetStats for KernelNetStats {
    fn counters(&self) -> Option<NetCounters> {
        crate::netstats::counters()
    }
}

/// Stores the settings text as the top-level FS v2 file `osjeff.conf` (the FS v3
/// front maps it to `/etc/osjeff.conf`). A first boot has no file: `load` is `None`.
pub(crate) struct FsV2Store;

impl SettingsStore for FsV2Store {
    fn load(&mut self) -> Option<Vec<u8>> {
        fs::read(disk(), osjeff_core::settings::FILE_NAME).map(<[u8]>::to_vec)
    }

    fn save(&mut self, text: &[u8]) -> Result<(), SinkError> {
        fs::write(disk(), osjeff_core::settings::FILE_NAME, text).map_err(map_fs_error)?;
        flush_disk();
        Ok(())
    }
}

/// The contents of the file at `path` (`name`, or `folder/name`, folders
/// separated by `/`) in the FS v2 image, if it exists and is a file.
pub(crate) fn read_path(path: &[u8]) -> Option<&'static [u8]> {
    let img: &'static [u8] = disk();
    let mut parent = fs::ROOT;
    let mut parts = path
        .split(|&b| b == b'/')
        .filter(|p| !p.is_empty())
        .peekable();
    while let Some(part) = parts.next() {
        let slot = fs::find_in(img, parent, part)?;
        if parts.peek().is_some() {
            if !fs::is_dir(img, slot) {
                return None;
            }
            parent = slot as u8;
        } else if fs::is_dir(img, slot) {
            return None;
        } else {
            return fs::read_slot(img, slot);
        }
    }
    None
}
