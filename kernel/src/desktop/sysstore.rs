//! Kernel implementations of the `osjeff_core::sysif` traits that the
//! system-management apps use, backed by what exists today: the RAM image of
//! the FS v2 root directory. The FS v3 front replaces these with `/var/log/`,
//! `/etc/osjeff.conf` and real volume accounting by implementing the same
//! traits (see `docs/design/sysmgmt.md`).

use super::*;
use osjeff_core::sysif::{LogSink, SinkError};

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
