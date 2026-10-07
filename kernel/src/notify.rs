//! The notification API: [`notify`] shows a toast in the corner of the desktop.
//!
//! Any thread (or an ISR: nothing here allocates or takes a lock) can call it.
//! The message is also written to the system log at the same level, so it can
//! be found in the viewer after the toast is gone. WARN, ERROR and FATAL log
//! records already become toasts by themselves (the compositor watches
//! `klog::take_warnings`), so `notify` only queues the lower levels, which is how
//! an INFO event ("novo lease DHCP", "app encerrado") reaches the screen
//! without being a warning. The compositor drains the queue every loop pass; with
//! nothing queued that costs one atomic load.
//!
//! The overlay itself (stacking, timing, drawing) is `osjeff_core::notify` and
//! `desktop/toasts_ui.rs`.

use crate::klog::{self, Level};
use crate::sync::RacyCell;
use core::fmt::{self, Write as _};
use core::sync::atomic::{AtomicUsize, Ordering};
use osjeff_core::klog::FixedBuf;

const CAP: usize = 8;
const TEXT: usize = 64;

#[derive(Clone, Copy)]
struct Pending {
    level: Level,
    text: [u8; TEXT],
    len: u8,
}

static QUEUE: RacyCell<[Option<Pending>; CAP]> = RacyCell::new([None; CAP]);
static COUNT: AtomicUsize = AtomicUsize::new(0);

/// Show `args` as a toast of `level` (and log it). A full queue drops the
/// message; the log still has it.
pub fn notify(level: Level, args: fmt::Arguments<'_>) {
    let mut buf = FixedBuf::<TEXT>::new();
    let _ = buf.write_fmt(args);
    klog::log_quiet(level, format_args!("{buf}"));
    if level >= Level::Warn {
        return; // the log watcher turns these into toasts
    }
    let mut text = [0u8; TEXT];
    let n = buf.as_bytes().len();
    text[..n].copy_from_slice(buf.as_bytes());
    x86_64::instructions::interrupts::without_interrupts(|| {
        // SAFETY: interrupts are off on a single core, so no other push or drain runs meanwhile.
        let q = unsafe { &mut *QUEUE.get() };
        if let Some(slot) = q.iter_mut().find(|s| s.is_none()) {
            *slot = Some(Pending {
                level,
                text,
                len: n as u8,
            });
            COUNT.fetch_add(1, Ordering::Release);
        }
    });
}

/// `notify(Info, "text {}", x)`.
#[macro_export]
macro_rules! notify {
    ($lvl:ident, $($arg:tt)*) => {
        $crate::notify::notify($crate::klog::Level::$lvl, format_args!($($arg)*))
    };
}

/// Is anything waiting for the compositor?
pub fn pending() -> bool {
    COUNT.load(Ordering::Acquire) != 0
}

/// Hand every queued message to `f` (oldest first) and empty the queue.
pub fn drain(mut f: impl FnMut(Level, &[u8])) {
    // Copy out under the critical section, call `f` outside it.
    let mut out = [None; CAP];
    x86_64::instructions::interrupts::without_interrupts(|| {
        // SAFETY: as in `notify`.
        let q = unsafe { &mut *QUEUE.get() };
        out = *q;
        *q = [None; CAP];
        COUNT.store(0, Ordering::Release);
    });
    for p in out.iter().flatten() {
        f(p.level, &p.text[..p.len as usize]);
    }
}
