//! Kernel implementations of the `osjeff_core::sysif` traits that the
//! system-management apps use, backed by the desktop VFS (OJFS v3 on the disk,
//! or the RAM volume when no v3 disk is mounted): the log goes to
//! `/var/log/`, the settings to `/etc/osjeff.conf`, and the space accounting
//! is the volume's `statfs`.

use alloc::vec::Vec;
use osjeff_core::sysif::{
    DiskUsage, DiskUsageInfo, LogSink, NetCounters, NetStats, SettingsStore, SinkError,
};

use super::vfs::{self, VfsError};

/// Largest file a `LogSink` writes: a longer dump keeps its newest whole lines.
const MAX_LOG_FILE: usize = 256 * 1024;

fn map_err(e: VfsError) -> SinkError {
    match e {
        VfsError::NoSpace => {
            crate::klog!(Warn, "disk full: the file could not be written");
            SinkError::NoSpace
        }
        VfsError::InvalidName | VfsError::InvalidPath | VfsError::NameTooLong => SinkError::BadName,
        other => {
            crate::klog!(Warn, "sysstore: write failed: {:?}", other);
            SinkError::Unavailable
        }
    }
}

/// Ensure `dir` (a top-level folder) exists.
fn ensure_dir(dir: &[u8]) {
    if !vfs::exists(dir) {
        let _ = vfs::mkdir(dir);
    }
}

/// Writes files into `/var/log/`. A dump over [`MAX_LOG_FILE`] bytes keeps its
/// newest whole lines and the call reports [`SinkError::Truncated`] (the file
/// *is* written).
pub(crate) struct VfsSink;

impl LogSink for VfsSink {
    fn write_file(&mut self, name: &[u8], data: &[u8]) -> Result<(), SinkError> {
        let (body, cut) = osjeff_core::klog::tail_lines(data, MAX_LOG_FILE);
        ensure_dir(b"/var");
        ensure_dir(b"/var/log");
        let path = vfs::join(b"/var/log", name);
        vfs::write_file(&path, body).map_err(map_err)?;
        if cut {
            Err(SinkError::Truncated { kept: body.len() })
        } else {
            Ok(())
        }
    }
}

/// Space accounting of the volume the desktop is on.
pub(crate) struct VfsUsage;

impl DiskUsage for VfsUsage {
    fn label(&self) -> &str {
        match vfs::volume() {
            vfs::Volume::Disk => "OJFS v3 (IDE)",
            vfs::Volume::Memory => "Memoria (sem disco v3)",
        }
    }

    fn usage(&self) -> DiskUsageInfo {
        let u = vfs::statfs();
        DiskUsageInfo {
            total_bytes: Some(u.total),
            used_bytes: Some(u.used()),
            items_used: None,
            items_total: None,
        }
    }
}

/// The NIC byte counters, from the network owner's statistics (`netd::stats`: every
/// driver, NE2000 and virtio-net alike, feeds them); `None` when there is no NIC.
pub(crate) struct KernelNetStats;

impl NetStats for KernelNetStats {
    fn counters(&self) -> Option<NetCounters> {
        let s = crate::netd::stats();
        (s.nic != osjeff_core::netstats::NicKind::None).then_some(NetCounters {
            rx_bytes: s.rx_bytes,
            tx_bytes: s.tx_bytes,
            rx_frames: s.rx_packets,
            tx_frames: s.tx_packets,
        })
    }
}

/// Stores the settings text as `/etc/osjeff.conf`. A first boot has no file:
/// `load` is `None`.
pub(crate) struct VfsStore;

const CONF_PATH: &[u8] = b"/etc/osjeff.conf";

impl SettingsStore for VfsStore {
    fn load(&mut self) -> Option<Vec<u8>> {
        vfs::read_file(CONF_PATH).ok()
    }

    fn save(&mut self, text: &[u8]) -> Result<(), SinkError> {
        ensure_dir(b"/etc");
        vfs::write_file(CONF_PATH, text).map_err(map_err)
    }
}

/// The contents of the file at `path` (a volume path, or a bare name relative to the
/// root as the settings page and older settings files store it), if it exists and is
/// a file.
pub(crate) fn read_path(path: &[u8]) -> Option<Vec<u8>> {
    vfs::read_file(&osjeff_core::settings::absolute_path(path)).ok()
}
