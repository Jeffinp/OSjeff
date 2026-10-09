//! Interfaces the system-management apps (resource monitor, settings, log
//! viewer) use to reach subsystems that other parts of the kernel own: disk
//! usage, network counters and control, log persistence, settings persistence.
//!
//! Each has a trivial default implementation here (`n/d`, RAM), and the kernel
//! provides one backed by whatever exists today. A new storage or network
//! front only has to implement the trait and hand the new object to the
//! desktop (see `docs/design/sysmgmt.md`).

use alloc::vec::Vec;

/// Space accounting of one volume. Any field the backend cannot know is `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiskUsageInfo {
    pub total_bytes: Option<u64>,
    pub used_bytes: Option<u64>,
    /// Directory entries (files + folders) in use / available.
    pub items_used: Option<u32>,
    pub items_total: Option<u32>,
}

impl DiskUsageInfo {
    /// Used share of the volume in tenths of a percent (`None` if unknown).
    pub fn used_permille(&self) -> Option<u32> {
        let (u, t) = (self.used_bytes?, self.total_bytes?);
        if t == 0 {
            return None;
        }
        Some((u.saturating_mul(1000) / t).min(1000) as u32)
    }
}

/// Disk space of the volume that holds user data.
pub trait DiskUsage {
    /// Short label of the volume ("FS v2 (IDE 1)").
    fn label(&self) -> &str;
    fn usage(&self) -> DiskUsageInfo;
}

/// Cumulative network counters (monotonic since boot).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetCounters {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_frames: u64,
    pub tx_frames: u64,
}

/// Source of network counters; `None` when there is no NIC.
pub trait NetStats {
    fn counters(&self) -> Option<NetCounters>;
}

/// Why a network control request was not carried out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetControlError {
    /// No NIC, or the network front does not offer this yet.
    Unsupported,
    /// The stack is busy (a fetch owns the NIC).
    Busy,
}

/// Network actions the settings app can ask for.
pub trait NetControl {
    /// Ask for a new DHCP lease.
    fn renew_dhcp(&mut self) -> Result<(), NetControlError>;
}

/// A `NetControl` for kernels whose network front does not expose the action
/// yet: every request answers `Unsupported`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoNetControl;

impl NetControl for NoNetControl {
    fn renew_dhcp(&mut self) -> Result<(), NetControlError> {
        Err(NetControlError::Unsupported)
    }
}

/// Why a file could not be stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SinkError {
    /// The volume or table is full.
    NoSpace,
    /// The name is not acceptable for this filesystem.
    BadName,
    /// No writable storage at all.
    Unavailable,
    /// Written, but cut to the filesystem's file size limit (`kept` bytes).
    Truncated { kept: usize },
}

/// Where "save log to file" puts the text. The FS v3 front will implement it
/// over `/var/log/`; today's implementation writes a root file of FS v2.
pub trait LogSink {
    /// Store `data` under `name` (replacing a previous file of that name).
    fn write_file(&mut self, name: &[u8], data: &[u8]) -> Result<(), SinkError>;
}

/// A `LogSink` that keeps the last file in memory (tests, no-disk boots).
#[derive(Default)]
pub struct MemSink {
    pub name: Vec<u8>,
    pub data: Vec<u8>,
}

impl LogSink for MemSink {
    fn write_file(&mut self, name: &[u8], data: &[u8]) -> Result<(), SinkError> {
        self.name = name.to_vec();
        self.data = data.to_vec();
        Ok(())
    }
}

/// Persistence of the settings text (`key=value` lines, see
/// [`crate::system::settings`]). The FS v3 front will back it with `/etc/kitsune.conf`.
pub trait SettingsStore {
    /// The stored text, if any.
    fn load(&mut self) -> Option<Vec<u8>>;
    fn save(&mut self, text: &[u8]) -> Result<(), SinkError>;
}

/// Path of the settings file.
pub const SETTINGS_PATH: &[u8] = b"/etc/kitsune.conf";
/// Path the settings file had when the system was called OSjeff. It is read when
/// the new file does not exist and removed after the first successful save.
pub const LEGACY_SETTINGS_PATH: &[u8] = b"/etc/osjeff.conf";

/// The few file operations [`MigratingStore`] needs from a volume.
pub trait ConfFiles {
    /// The contents of the file at `path`, if it exists.
    fn read(&mut self, path: &[u8]) -> Option<Vec<u8>>;
    /// Creates or replaces the file at `path` (the parent directory is made if needed).
    fn write(&mut self, path: &[u8], data: &[u8]) -> Result<(), SinkError>;
    /// Removes the file at `path`; a missing file is not an error.
    fn remove(&mut self, path: &[u8]);
}

/// A [`SettingsStore`] over [`SETTINGS_PATH`] that still understands the old
/// `/etc/osjeff.conf`: `load` falls back to it, and the first `save` writes the new
/// file and then drops the old one (never before the new one is safely written).
pub struct MigratingStore<F: ConfFiles>(pub F);

impl<F: ConfFiles> SettingsStore for MigratingStore<F> {
    fn load(&mut self) -> Option<Vec<u8>> {
        self.0
            .read(SETTINGS_PATH)
            .or_else(|| self.0.read(LEGACY_SETTINGS_PATH))
    }
    fn save(&mut self, text: &[u8]) -> Result<(), SinkError> {
        self.0.write(SETTINGS_PATH, text)?;
        self.0.remove(LEGACY_SETTINGS_PATH);
        Ok(())
    }
}

/// A `SettingsStore` in RAM (used when no disk is writable, and in tests).
#[derive(Default)]
pub struct MemStore {
    pub text: Option<Vec<u8>>,
}

impl SettingsStore for MemStore {
    fn load(&mut self) -> Option<Vec<u8>> {
        self.text.clone()
    }
    fn save(&mut self, text: &[u8]) -> Result<(), SinkError> {
        self.text = Some(text.to_vec());
        Ok(())
    }
}

#[cfg(test)]
mod tests;
