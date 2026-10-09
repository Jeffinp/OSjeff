//! Text the file manager shows: dates, breadcrumbs and property lines.

use crate::desktop::*;
use core::sync::atomic::{AtomicU64, Ordering};
use kitsune_core::fileman::ui::{self};
use kitsune_core::fileman::{self, Crumb};
use kitsune_core::t;

/// A Unix time as a local date and time in the language in effect.
pub(crate) fn local_time(t: u64) -> String {
    fileman::format_datetime(t, crate::rtc::tz_minutes() * 60, crate::settings::clock24())
}

static NOW_UNIX: AtomicU64 = AtomicU64::new(0);
static NOW_AT: AtomicU64 = AtomicU64::new(0);

/// The current Unix time, read from the clock chip at most every few seconds (a list shows
/// a date per row; each read is a handful of port accesses).
fn now_unix_cached() -> u64 {
    let t = appui::ticks();
    let at = NOW_AT.load(Ordering::Relaxed);
    if NOW_UNIX.load(Ordering::Relaxed) == 0 || t.saturating_sub(at) > 1250 {
        NOW_UNIX.store(crate::rtc::now_unix(), Ordering::Relaxed);
        NOW_AT.store(t, Ordering::Relaxed);
    }
    NOW_UNIX.load(Ordering::Relaxed)
}

/// A time for the list: `Hoje, 14:32`, `Ontem, 09:10`, else the date.
pub(crate) fn modified_label(t: u64) -> String {
    ui::format_modified(
        t,
        now_unix_cached(),
        crate::rtc::tz_minutes() * 60,
        crate::settings::clock24(),
    )
}

/// The label of a crumb: the well-known folders take their name in the language in effect.
fn crumb_label(c: &Crumb) -> String {
    match &c.path[..] {
        b"/home" => String::from(t!("files.place.home")),
        b"/Documentos" => String::from(t!("files.place.documents")),
        b"/Imagens" => String::from(t!("files.place.images")),
        _ => String::from_utf8_lossy(&c.label).into_owned(),
    }
}

/// `label: value` line of the information sheet.
pub(super) fn prop(label_key: &str, value: &str) -> String {
    t!(
        "files.prop.line",
        label = kitsune_core::i18n::tr(label_key),
        value = value
    )
}

/// The crumbs of `cwd` with their display labels and measured widths (the last one is drawn
/// Medium, the others Regular).
pub(crate) fn crumbs_of(cwd: &[u8]) -> (Vec<Crumb>, Vec<String>, Vec<i32>) {
    let crumbs = fileman::breadcrumbs(cwd);
    let n = crumbs.len();
    let labels: Vec<String> = crumbs.iter().map(crumb_label).collect();
    let widths: Vec<i32> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let w = if i + 1 == n {
                crate::text::Weight::Medium
            } else {
                crate::text::Weight::Regular
            };
            // The disk crumb carries a glyph in front.
            crate::text::measure(l, crate::text::BODY, w) + if i == 0 { 20 } else { 0 }
        })
        .collect();
    (crumbs, labels, widths)
}

/// `usize` to a string without importing `ToString` everywhere.
pub(super) trait ToStringLossy {
    fn to_string_lossy(self) -> String;
}

impl ToStringLossy for usize {
    fn to_string_lossy(self) -> String {
        alloc::format!("{self}")
    }
}
