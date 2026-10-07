//! The live system settings (`osjeff_core::settings::Settings`) and how they
//! reach the parts of the kernel that read them.
//!
//! The compositor owns the settings: it loads them at boot (before the
//! wallpaper is painted), the settings app changes them, and [`set`] pushes the
//! pieces other modules read on their own (accent colour, time zone, clock
//! format) into their atomics.

use crate::sync::RacyCell;
use core::sync::atomic::{AtomicBool, Ordering};
use osjeff_core::settings::Settings;

static CURRENT: RacyCell<Settings> = RacyCell::new(Settings::new());
static CLOCK24: AtomicBool = AtomicBool::new(true);

/// The settings in effect.
pub fn get() -> Settings {
    // SAFETY: written only by `set` (compositor thread, at boot and from the settings app) and
    // read from the same thread (wallpaper painting, desktop); `Settings` is `Copy`, so this
    // is a plain copy with no reference kept.
    // NOTE: not guaranteed by the type; a second writer thread would race.
    unsafe { *CURRENT.get() }
}

/// Make `s` the settings in effect and apply what other modules read directly.
pub fn set(s: Settings) {
    // SAFETY: as in `get`: single writer thread.
    unsafe { *CURRENT.get() = s };
    crate::theme::set_accent(s.accent_rgb());
    crate::rtc::set_tz_minutes(s.tz_minutes as i32);
    CLOCK24.store(s.clock24, Ordering::Relaxed);
}

/// 24-hour clock (otherwise 12-hour with AM/PM).
pub fn clock24() -> bool {
    CLOCK24.load(Ordering::Relaxed)
}
