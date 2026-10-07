//! Everything the shell needs from the running system, as one trait.
//!
//! The kernel implements [`SysInfo`] once (RTC, PIT ticks, heap statistics,
//! scheduler, network stack) and hands it to the shell with the filesystem.
//! Only [`SysInfo::now`], [`SysInfo::uptime_ms`] and [`SysInfo::mem`] are
//! required; the others default to "not supported" / no-ops so a partial kernel
//! implementation still works and the command prints a clear message.
//!
//! All numbers are integers (the kernel is soft-float).

use alloc::string::String;
use alloc::vec::Vec;

/// Calendar date and wall-clock time (UTC or local, the kernel decides).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// Heap/physical memory in bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MemInfo {
    pub total: u64,
    pub used: u64,
}

impl MemInfo {
    pub fn free(&self) -> u64 {
        self.total.saturating_sub(self.used)
    }
}

/// One mounted volume for `df`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiskInfo {
    pub name: String,
    pub mount: String,
    pub total: u64,
    pub used: u64,
}

/// One process/thread for `ps`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    /// Short state word such as `run`, `ready`, `sleep`.
    pub state: String,
    pub mem_bytes: u64,
}

/// Outcome of a `ping`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PingStats {
    pub sent: u32,
    pub received: u32,
    /// Average round trip in microseconds (0 when nothing came back).
    pub avg_rtt_us: u64,
}

/// Why a system call failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysErr {
    /// The kernel does not provide this.
    Unsupported,
    /// No such pid.
    NoSuchProcess,
    /// The process may not be signalled.
    Denied,
    /// Name resolution or network failure.
    Network,
}

impl SysErr {
    pub const fn message(self) -> &'static str {
        match self {
            SysErr::Unsupported => "not supported on this system",
            SysErr::NoSuchProcess => "no such process",
            SysErr::Denied => "operation not permitted",
            SysErr::Network => "network is unreachable",
        }
    }
}

/// System services behind `date`, `uptime`, `free`, `df`, `ps`, `kill`,
/// `ping`, `sleep` and `clear`.
pub trait SysInfo {
    /// Current date and time (`date`).
    fn now(&self) -> DateTime;

    /// Milliseconds since boot (`uptime`).
    fn uptime_ms(&self) -> u64;

    /// Memory totals (`free`).
    fn mem(&self) -> MemInfo;

    /// Mounted volumes (`df`). The default is empty; `df` then reports the
    /// [`crate::shell::ShellFs::usage`] of the root filesystem.
    fn disks(&self) -> Vec<DiskInfo> {
        Vec::new()
    }

    /// Running processes (`ps`).
    fn procs(&self) -> Vec<ProcInfo> {
        Vec::new()
    }

    /// Send `signal` (9 = kill, 15 = terminate) to `pid` (`kill`).
    fn kill(&mut self, _pid: u32, _signal: i32) -> Result<(), SysErr> {
        Err(SysErr::Unsupported)
    }

    /// Send `count` echo requests to `host` (`ping`); blocks until done.
    fn ping(&mut self, _host: &str, _count: u32) -> Result<PingStats, SysErr> {
        Err(SysErr::Unsupported)
    }

    /// Wait `ms` milliseconds (`sleep`). The shell already caps the value with
    /// [`crate::shell::Limits::max_sleep_ms`].
    fn sleep_ms(&mut self, _ms: u64) {}

    /// Clear the terminal (`clear`).
    fn clear_screen(&mut self) {}

    /// Host name for the prompt.
    fn hostname(&self) -> String {
        String::from("osjeff")
    }
}

/// A deterministic [`SysInfo`] for tests and fuzzing.
#[derive(Clone, Debug)]
pub struct MockSys {
    pub time: DateTime,
    pub uptime: u64,
    pub memory: MemInfo,
    pub disk_list: Vec<DiskInfo>,
    pub process_list: Vec<ProcInfo>,
    /// Every `sleep_ms` call, in order.
    pub slept: Vec<u64>,
    pub cleared: u32,
    pub killed: Vec<(u32, i32)>,
    pub ping_ok: bool,
}

impl Default for MockSys {
    fn default() -> Self {
        Self {
            time: DateTime {
                year: 2026,
                month: 10,
                day: 7,
                hour: 13,
                minute: 5,
                second: 9,
            },
            uptime: 93_784_000,
            memory: MemInfo {
                total: 64 * 1024 * 1024,
                used: 20 * 1024 * 1024,
            },
            disk_list: Vec::new(),
            process_list: alloc::vec![
                ProcInfo {
                    pid: 1,
                    name: String::from("init"),
                    state: String::from("run"),
                    mem_bytes: 4096,
                },
                ProcInfo {
                    pid: 7,
                    name: String::from("shell"),
                    state: String::from("ready"),
                    mem_bytes: 8192,
                },
            ],
            slept: Vec::new(),
            cleared: 0,
            killed: Vec::new(),
            ping_ok: true,
        }
    }
}

impl SysInfo for MockSys {
    fn now(&self) -> DateTime {
        self.time
    }

    fn uptime_ms(&self) -> u64 {
        self.uptime
    }

    fn mem(&self) -> MemInfo {
        self.memory
    }

    fn disks(&self) -> Vec<DiskInfo> {
        self.disk_list.clone()
    }

    fn procs(&self) -> Vec<ProcInfo> {
        self.process_list.clone()
    }

    fn kill(&mut self, pid: u32, signal: i32) -> Result<(), SysErr> {
        if self.process_list.iter().any(|p| p.pid == pid) {
            self.killed.push((pid, signal));
            self.process_list.retain(|p| p.pid != pid);
            Ok(())
        } else {
            Err(SysErr::NoSuchProcess)
        }
    }

    fn ping(&mut self, _host: &str, count: u32) -> Result<PingStats, SysErr> {
        if self.ping_ok {
            Ok(PingStats {
                sent: count,
                received: count,
                avg_rtt_us: 1500,
            })
        } else {
            Err(SysErr::Network)
        }
    }

    fn sleep_ms(&mut self, ms: u64) {
        self.slept.push(ms);
        self.uptime += ms;
    }

    fn clear_screen(&mut self) {
        self.cleared += 1;
    }
}

/// A [`SysInfo`] with only the required methods (everything else defaults),
/// for tests of the "not supported" paths.
#[derive(Clone, Copy, Debug, Default)]
pub struct MinimalSys;

impl SysInfo for MinimalSys {
    fn now(&self) -> DateTime {
        DateTime::default()
    }

    fn uptime_ms(&self) -> u64 {
        0
    }

    fn mem(&self) -> MemInfo {
        MemInfo::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_free_saturates() {
        let m = MemInfo { total: 5, used: 9 };
        assert_eq!(m.free(), 0);
        assert_eq!(MemInfo { total: 10, used: 4 }.free(), 6);
    }

    #[test]
    fn mock_kill_removes_process() {
        let mut s = MockSys::default();
        assert_eq!(s.kill(7, 9), Ok(()));
        assert_eq!(s.kill(7, 9), Err(SysErr::NoSuchProcess));
        assert_eq!(s.killed, [(7, 9)]);
    }

    #[test]
    fn mock_sleep_advances_uptime() {
        let mut s = MockSys::default();
        let before = s.uptime_ms();
        s.sleep_ms(500);
        assert_eq!(s.uptime_ms(), before + 500);
    }

    #[test]
    fn minimal_sys_defaults_are_unsupported() {
        let mut s = MinimalSys;
        assert_eq!(s.kill(1, 9), Err(SysErr::Unsupported));
        assert_eq!(s.ping("x", 1), Err(SysErr::Unsupported));
        assert!(s.procs().is_empty());
        assert_eq!(s.hostname(), "osjeff");
    }

    #[test]
    fn syserr_messages() {
        for e in [
            SysErr::Unsupported,
            SysErr::NoSuchProcess,
            SysErr::Denied,
            SysErr::Network,
        ] {
            assert!(!e.message().is_empty());
        }
    }
}
