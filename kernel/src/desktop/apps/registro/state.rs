//! State and layout of the log viewer window.

use crate::desktop::*;
use crate::text::{self, BODY, Weight};
use core::cell::Cell;
use kitsune_core::activity::Glide;
use kitsune_core::i18n;
use kitsune_core::klog::{Filter, Level, LogView};
use kitsune_core::widgets::ScrollbarFade;
use kitsune_core::{t, tk};

const ROW_H: i32 = 24;
pub(super) const HEAD_H: i32 = 28;
/// Mono size of the time and message columns.
pub(super) const MONO: u16 = 12;
/// Longest file name the dump is saved under.
pub(super) const SAVE_NAME: &[u8] = b"syslog.txt";
/// The level segments (catalog keys, looked up when drawn).
pub(super) const LEVELS: [&str; 4] = [
    tk!("log.seg.all"),
    tk!("log.seg.info"),
    tk!("log.seg.warn"),
    tk!("log.seg.error"),
];
/// The minimum level of each segment.
pub(super) const SEG_LEVEL: [Level; 4] = [Level::Trace, Level::Info, Level::Warn, Level::Error];

// Hover keys.
pub(super) const H_DOWN: u32 = 1 << 31;
pub(super) const H_CLEAR: u32 = 1;
pub(super) const H_SAVE: u32 = 2;
pub(super) const H_SEARCH: u32 = 3;
pub(super) const H_FOLLOW: u32 = 4;
pub(super) const H_ROW: u32 = 0x100;

/// Per-window state of a log viewer.
pub(crate) struct LogState {
    pub(super) snap: Vec<u8>,
    pub(super) view: LogView,
    pub(super) filter: Filter,
    /// `klog::seq()` when `snap` was taken.
    seen: u32,
    /// The last action's message (a catalog key, so it follows the language) and whether it is an
    /// error.
    pub(super) status: Option<(&'static str, bool)>,
    pub(super) scroll: Glide,
    pub(super) sb: ScrollbarFade,
    /// The auto-follow switch (0..=256), animated.
    pub(super) knob: Glide,
    pub(super) hover: Cell<u32>,
}

impl LogState {
    pub(crate) fn new() -> Self {
        let mut s = Self {
            snap: Vec::new(),
            view: LogView::new(),
            filter: Filter::new(),
            seen: 0,
            status: None,
            scroll: Glide::at(0),
            sb: ScrollbarFade::new(),
            knob: Glide::at(256),
            hover: Cell::new(0),
        };
        s.reload(24);
        s
    }

    /// Approximate heap held by this window (for the memory tab).
    pub(crate) fn heap_bytes(&self) -> usize {
        self.snap.capacity()
    }

    pub(super) fn reload(&mut self, rows: usize) {
        crate::klog::snapshot(&mut self.snap);
        self.seen = crate::klog::seq();
        self.view.rebuild(&self.snap, &self.filter, rows);
    }

    /// Take a new snapshot if messages arrived since the last one.
    pub(super) fn refresh(&mut self, rows: usize) -> bool {
        if crate::klog::seq() == self.seen {
            return false;
        }
        self.reload(rows);
        true
    }

    pub(super) fn say(&mut self, key: &'static str, error: bool) {
        self.status = Some((key, error));
    }

    /// Largest scroll position in pixels for a list `h` high.
    pub(super) fn max_px(&self, h: i32) -> i32 {
        (self.view.len() as i32 * ROW_H - h).max(0)
    }

    /// Re-aim the scroll: pinned to the end while following, else kept in range.
    pub(super) fn aim(&mut self, h: i32) {
        let max = self.max_px(h);
        let t = if self.view.follow {
            max
        } else {
            self.scroll.target().clamp(0, max)
        };
        self.scroll.set(t);
        self.knob.set(if self.view.follow { 256 } else { 0 });
    }

    pub(super) fn seg_index(&self) -> usize {
        match self.filter.min {
            Level::Trace | Level::Debug => 0,
            Level::Info => 1,
            Level::Warn => 2,
            Level::Error | Level::Fatal => 3,
        }
    }
}

/// Geometry of a log window, shared by drawing and hit-testing.
pub(super) struct LogLayout {
    pub(super) search: Rect,
    pub(super) seg: Rect,
    pub(super) follow_label: Rect,
    pub(super) follow_sw: Rect,
    pub(super) clear: Rect,
    pub(super) save: Rect,
    pub(super) card: Rect,
    pub(super) head: Rect,
    pub(super) list: Rect,
    pub(super) status: Rect,
    /// Time, level, source, message.
    pub(super) cols: [Rect; 4],
    pub(super) rows: usize,
}

impl LogLayout {
    pub(super) fn of(r: Rect) -> LogLayout {
        let body = r.body();
        let pad = 16;
        let y = body.y + 12;
        let save = Rect::new(body.right() - pad - 80, y, 80, 28);
        let clear = Rect::new(save.x - 8 - 80, y, 80, 28);
        let follow_sw = kitsune_core::widgets::switch_rect(clear.x - 14 - 38, y + 3);
        // The widths follow the words of the language in effect.
        let follow_w = text::measure(t!("log.follow"), BODY, Weight::Regular).max(46) + 2;
        let seg_label = LEVELS
            .iter()
            .map(|k| text::measure(i18n::tr(k), BODY, Weight::Medium))
            .max()
            .unwrap_or(0);
        let seg_w = (4 * (seg_label + 28)).max(232);
        let follow_label = Rect::new(follow_sw.x - 8 - follow_w, y, follow_w, 28);
        let seg = Rect::new(follow_label.x - 16 - seg_w, y, seg_w, 28);
        let search = Rect::new(body.x + pad, y, (seg.x - 12 - (body.x + pad)).max(60), 28);
        let top = y + 28 + 12;
        let status = Rect::new(body.x + pad, body.bottom() - 16 - 18, body.w - 2 * pad, 18);
        let card = Rect::new(
            body.x + pad,
            top,
            body.w - 2 * pad,
            (status.y - 8 - top).max(HEAD_H + ROW_H),
        );
        let head = Rect::new(card.x, card.y, card.w, HEAD_H);
        let list = Rect::new(
            card.x,
            head.bottom(),
            card.w,
            (card.h - HEAD_H - 6).max(ROW_H),
        );
        let src_w = if card.w >= 620 { 128 } else { 0 };
        let time = Rect::new(card.x + 12, list.y, 92, list.h);
        let level = Rect::new(time.right(), list.y, 72, list.h);
        let source = Rect::new(level.right(), list.y, src_w, list.h);
        let msg_x = source.right();
        let msg = Rect::new(msg_x, list.y, (card.right() - 16 - msg_x).max(0), list.h);
        let rows = (list.h / ROW_H).max(1) as usize;
        LogLayout {
            search,
            seg,
            follow_label,
            follow_sw,
            clear,
            save,
            card,
            head,
            list,
            status,
            cols: [time, level, source, msg],
            rows,
        }
    }
}
