//! `logd`: the thread that persists the system log.
//!
//! The kernel log lives in a 64 KiB RAM ring (`klog`). Writing it to disk takes
//! the volume lock and a few journaled transactions (tens of milliseconds on a
//! slow disk), so it must **never** run from an interrupt handler and must not
//! stall the compositor: it runs on its own thread, which sleeps (`sched::block`,
//! no time slice at all) until it is asked.
//!
//! One job today: the **boot-time flush**. The compositor calls
//! [`request_boot_flush`] after the first desktop frame (so the log holds the whole
//! boot); `logd` renders the ring ([`kitsune_core::klog::dump_bounded`], at most
//! [`BOOT_LOG_MAX`] bytes, newest lines win) and writes `/var/log/boot.log` through
//! the same `LogSink` the log viewer's "Salvar" uses (`/var/log/syslog.txt`).
//! Each boot replaces the previous file. With no disk volume (the RAM fallback) the
//! flush is skipped: a log in RAM next to a log in RAM would persist nothing.
//!
//! The thread takes the volume's `YieldMutex` like any other user: while it holds it
//! the compositor, if it needs the disk at that moment, yields and waits for one
//! write. It never calls into the compositor and holds no other lock.

use crate::desktop::{VfsSink, vfs};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use kitsune_core::sysif::{LogSink, SinkError};

/// Largest `boot.log` written (the ring holds 64 KiB of records; with the
/// timestamp, level and thread prefixes the text can reach about 100 KiB).
pub const BOOT_LOG_MAX: usize = 96 * 1024;
/// File name inside `/var/log/`.
pub const BOOT_LOG_NAME: &[u8] = b"boot.log";

/// Scheduler slot of the thread (`usize::MAX` until it runs).
static TID: AtomicUsize = AtomicUsize::new(usize::MAX);
static BOOT_FLUSH: AtomicBool = AtomicBool::new(false);

/// Ask `logd` to write the boot log. Callable from any thread (not from an IRQ
/// handler: it does not need to be, and nothing here is meant for that context).
pub fn request_boot_flush() {
    BOOT_FLUSH.store(true, Ordering::Release);
    let tid = TID.load(Ordering::Acquire);
    if tid != usize::MAX {
        crate::sched::wake(tid);
    }
}

/// Thread entry.
pub extern "C" fn worker() -> ! {
    TID.store(crate::sched::current(), Ordering::Release);
    loop {
        if BOOT_FLUSH.swap(false, Ordering::AcqRel) {
            flush_boot_log();
        }
        // Nothing else to do until the next request: no deadline, woken by `wake`.
        crate::sched::block(crate::sched::FOREVER, || {
            !BOOT_FLUSH.load(Ordering::Acquire)
        });
    }
}

fn flush_boot_log() {
    if vfs::volume() != vfs::Volume::Disk {
        crate::klog::log_quiet(
            crate::klog::Level::Info,
            format_args!("logd: no disk volume, boot log not saved"),
        );
        return;
    }
    let mut snap = Vec::new();
    crate::klog::snapshot(&mut snap);
    let (text, cut) = kitsune_core::klog::dump_bounded(
        &snap,
        |o| crate::sched::thread_name(o as usize),
        BOOT_LOG_MAX,
    );
    drop(snap);
    let n = text.len();
    match VfsSink.write_file(BOOT_LOG_NAME, &text) {
        Ok(()) => crate::klog!(
            Info,
            "logd: boot log saved to /var/log/boot.log ({} bytes{})",
            n,
            if cut { ", oldest lines dropped" } else { "" }
        ),
        Err(SinkError::Truncated { kept }) => crate::klog!(
            Warn,
            "logd: boot log saved to /var/log/boot.log, cut to {} bytes",
            kept
        ),
        Err(e) => crate::klog!(Warn, "logd: boot log not saved ({:?})", e),
    }
}
