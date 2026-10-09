//! Registro: the system-log viewer, a window onto the kernel's log ring (`crate::klog`).
//!
//! A table (time, level chip, source, message in the mono font) with a search field, a
//! level filter, an auto-follow switch, and "Limpar" / "Salvar" buttons. The text scrolls
//! smoothly by pixels (wheel, keys), with an overlay scrollbar that fades.
//!
//! The window works on a private copy (snapshot) of the ring, refreshed once a second and
//! after any input, so drawing never touches the live ring or holds interrupts off. The
//! filter / indexing logic is `kitsune_core::klog`; the saved file goes through the
//! `LogSink` trait (`/var/log/syslog.txt`).

use super::kit;
use super::ui::{self, ButtonKind};
use super::*;
use crate::text::{self, BODY, FOOTNOTE, Weight};
use core::cell::Cell;
use kitsune_core::activity::{self, Glide};
use kitsune_core::i18n;
use kitsune_core::klog::{Filter, Level, LogView, render_text};
use kitsune_core::sysif::{LogSink, SinkError};
use kitsune_core::widgets::ScrollbarFade;
use kitsune_core::{t, tk, tp};

const ROW_H: i32 = 24;
const HEAD_H: i32 = 28;
/// Mono size of the time and message columns.
const MONO: u16 = 12;
/// Longest file name the dump is saved under.
const SAVE_NAME: &[u8] = b"syslog.txt";
/// The level segments (catalog keys, looked up when drawn).
const LEVELS: [&str; 4] = [
    tk!("log.seg.all"),
    tk!("log.seg.info"),
    tk!("log.seg.warn"),
    tk!("log.seg.error"),
];
/// The minimum level of each segment.
const SEG_LEVEL: [Level; 4] = [Level::Trace, Level::Info, Level::Warn, Level::Error];

// Hover keys.
const H_DOWN: u32 = 1 << 31;
const H_CLEAR: u32 = 1;
const H_SAVE: u32 = 2;
const H_SEARCH: u32 = 3;
const H_FOLLOW: u32 = 4;
const H_ROW: u32 = 0x100;

/// Per-window state of a log viewer.
pub(crate) struct LogState {
    snap: Vec<u8>,
    view: LogView,
    filter: Filter,
    /// `klog::seq()` when `snap` was taken.
    seen: u32,
    /// The last action's message (a catalog key, so it follows the language) and whether it is an
    /// error.
    status: Option<(&'static str, bool)>,
    scroll: Glide,
    sb: ScrollbarFade,
    /// The auto-follow switch (0..=256), animated.
    knob: Glide,
    hover: Cell<u32>,
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

    fn reload(&mut self, rows: usize) {
        crate::klog::snapshot(&mut self.snap);
        self.seen = crate::klog::seq();
        self.view.rebuild(&self.snap, &self.filter, rows);
    }

    /// Take a new snapshot if messages arrived since the last one.
    fn refresh(&mut self, rows: usize) -> bool {
        if crate::klog::seq() == self.seen {
            return false;
        }
        self.reload(rows);
        true
    }

    fn say(&mut self, key: &'static str, error: bool) {
        self.status = Some((key, error));
    }

    /// Largest scroll position in pixels for a list `h` high.
    fn max_px(&self, h: i32) -> i32 {
        (self.view.len() as i32 * ROW_H - h).max(0)
    }

    /// Re-aim the scroll: pinned to the end while following, else kept in range.
    fn aim(&mut self, h: i32) {
        let max = self.max_px(h);
        let t = if self.view.follow {
            max
        } else {
            self.scroll.target().clamp(0, max)
        };
        self.scroll.set(t);
        self.knob.set(if self.view.follow { 256 } else { 0 });
    }

    fn seg_index(&self) -> usize {
        match self.filter.min {
            Level::Trace | Level::Debug => 0,
            Level::Info => 1,
            Level::Warn => 2,
            Level::Error | Level::Fatal => 3,
        }
    }
}

/// Geometry of a log window, shared by drawing and hit-testing.
struct LogLayout {
    search: Rect,
    seg: Rect,
    follow_label: Rect,
    follow_sw: Rect,
    clear: Rect,
    save: Rect,
    card: Rect,
    head: Rect,
    list: Rect,
    status: Rect,
    /// Time, level, source, message.
    cols: [Rect; 4],
    rows: usize,
}

impl LogLayout {
    fn of(r: Rect) -> LogLayout {
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

fn level_color(l: Level) -> Color {
    match l {
        Level::Trace => Color::rgb(0x8E, 0x8E, 0x93),
        Level::Debug => Color::rgb(0x64, 0x8F, 0xD8),
        Level::Info => theme::accent(),
        Level::Warn => kit::amber(),
        Level::Error => kit::red(),
        Level::Fatal => Color::rgb(0xFF, 0x2D, 0x92),
    }
}

impl Desktop {
    /// Refresh every visible log window whose ring changed (called each second and after
    /// input). Returns whether any window needs a repaint.
    pub(crate) fn refresh_logs(&mut self) -> bool {
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| w.shown() && matches!(w.app.app, App::Log(_)))
            .map(|w| w.id)
            .collect();
        let mut changed = false;
        for id in ids {
            let Some(w) = self.wm.get_mut(id) else {
                continue;
            };
            let rect = w.rect;
            if let App::Log(l) = &mut w.app.app {
                let lay = LogLayout::of(rect);
                if l.refresh(lay.rows) {
                    l.aim(lay.list.h);
                    changed = true;
                }
            }
        }
        changed
    }

    fn log_mut(&mut self, id: WindowId) -> Option<&mut LogState> {
        match self.app_mut(id) {
            Some(App::Log(l)) => Some(l),
            _ => None,
        }
    }

    pub(crate) fn log_key(&mut self, id: WindowId, key: Key) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let lay = LogLayout::of(rect);
        let (rows, list_h) = (lay.rows, lay.list.h);
        let Some(l) = self.log_mut(id) else {
            return;
        };
        let now = super::toasts_ui::now_ms();
        let mut rebuild = false;
        let page = (list_h / ROW_H - 1).max(1) * ROW_H;
        let scroll_by = |l: &mut LogState, d: i32| {
            let max = l.max_px(list_h);
            let t = (l.scroll.target() + d).clamp(0, max);
            l.scroll.set(t);
            l.view.follow = t >= max;
            l.knob.set(if l.view.follow { 256 } else { 0 });
            l.sb.touch(now);
        };
        match key {
            Key::Esc => {
                if l.filter.needle().is_empty() {
                    self.request_close(id);
                    return;
                }
                l.filter.clear_needle();
                rebuild = true;
            }
            Key::Tab => {
                let next = (l.seg_index() + 1) % 4;
                l.filter.min = SEG_LEVEL[next];
                rebuild = true;
            }
            Key::Backspace => rebuild = l.filter.backspace(),
            Key::Delete => {
                l.filter.clear_needle();
                rebuild = true;
            }
            Key::Char(b) => rebuild = l.filter.push_char(b),
            Key::Up => scroll_by(l, -ROW_H),
            Key::Down => scroll_by(l, ROW_H),
            Key::PageUp => scroll_by(l, -page),
            Key::PageDown => scroll_by(l, page),
            Key::Home => scroll_by(l, -i32::MAX / 2),
            Key::End => scroll_by(l, i32::MAX / 2),
            _ => {}
        }
        if rebuild {
            l.view.rebuild(&l.snap, &l.filter, rows);
            l.aim(list_h);
        }
    }

    pub(crate) fn log_wheel(&mut self, id: WindowId, notches: i32) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let h = LogLayout::of(rect).list.h;
        let Some(l) = self.log_mut(id) else {
            return;
        };
        let max = l.max_px(h);
        let t = (l.scroll.target() + notches * ROW_H * 3).clamp(0, max);
        l.scroll.set(t);
        l.view.follow = t >= max;
        l.knob.set(if l.view.follow { 256 } else { 0 });
        l.sb.touch(super::toasts_ui::now_ms());
    }

    pub(crate) fn log_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        let lay = LogLayout::of(rect);
        let Some(l) = self.log_mut(id) else {
            return;
        };
        if let Some(i) = kitsune_core::widgets::segmented_hit(lay.seg, 4, px, py) {
            l.filter.min = SEG_LEVEL[i];
            l.view.rebuild(&l.snap, &l.filter, lay.rows);
            l.aim(lay.list.h);
        } else if lay.follow_sw.inflated(6).contains(px, py) || lay.follow_label.contains(px, py) {
            l.view.follow = !l.view.follow;
            l.aim(lay.list.h);
        } else if lay.clear.contains(px, py) {
            crate::klog::clear();
            l.reload(lay.rows);
            l.aim(lay.list.h);
            l.say(tk!("log.cleared"), false);
        } else if lay.save.contains(px, py) {
            let mut text = Vec::new();
            render_text(
                &l.snap,
                &l.filter,
                |o| crate::sched::thread_name(o as usize),
                &mut text,
            );
            let (msg, err): (&'static str, bool) = match VfsSink.write_file(SAVE_NAME, &text) {
                Ok(()) => (tk!("log.saved"), false),
                Err(SinkError::Truncated { .. }) => (tk!("log.saved_tail"), false),
                Err(SinkError::NoSpace) => (tk!("log.disk_full"), true),
                Err(_) => (tk!("log.save_failed"), true),
            };
            l.say(msg, err);
            crate::klog::log_quiet(Level::Info, format_args!("log saved to syslog.txt"));
        }
    }

    /// The scroll glides of every log window (called by `live_step`).
    pub(crate) fn log_step(&mut self, dt_ms: u32) {
        if !self
            .wm
            .windows()
            .iter()
            .any(|w| matches!(w.app.app, App::Log(_)))
        {
            return;
        }
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Log(_)))
            .map(|w| w.id)
            .collect();
        for id in ids {
            // While following, the end of the log is where the view heads (also after a resize).
            let h = self
                .wm
                .get(id)
                .map(|w| LogLayout::of(w.rect).list.h)
                .unwrap_or(0);
            if let Some(l) = self.log_mut(id) {
                if l.view.follow {
                    l.aim(h);
                }
                l.scroll.step(dt_ms, 80);
                l.knob.step(dt_ms, 60);
            }
        }
    }

    pub(crate) fn log_busy_one(&self, w: &Win) -> bool {
        let App::Log(l) = &w.app.app else {
            return false;
        };
        w.shown()
            && (l.scroll.moving() || l.knob.moving() || l.sb.active(super::toasts_ui::now_ms()))
    }

    /// Hover keys of every log window for a pointer at `(cx, cy)`; `true` when one changed.
    pub(crate) fn log_hover(&mut self, cx: i32, cy: i32, down: bool) -> bool {
        let top = if self.overlay_open() {
            None
        } else {
            self.topmost_at(cx, cy)
        };
        let mut changed = false;
        let mut dirty = Vec::new();
        for w in self.wm.windows() {
            let App::Log(l) = &w.app.app else { continue };
            if !w.shown() {
                continue;
            }
            let mut key = 0;
            if Some(w.id) == top && w.rect.body().contains(cx, cy) {
                let lay = LogLayout::of(w.rect);
                key = if lay.clear.contains(cx, cy) {
                    H_CLEAR
                } else if lay.save.contains(cx, cy) {
                    H_SAVE
                } else if lay.search.contains(cx, cy) {
                    H_SEARCH
                } else if lay.follow_sw.inflated(6).contains(cx, cy)
                    || lay.follow_label.contains(cx, cy)
                {
                    H_FOLLOW
                } else if lay.list.contains(cx, cy) {
                    let i = (cy - lay.list.y + l.scroll.value()) / ROW_H;
                    if (i as usize) < l.view.len() {
                        H_ROW + i as u32
                    } else {
                        0
                    }
                } else {
                    0
                };
                if key != 0 && down {
                    key |= H_DOWN;
                }
            }
            if key != l.hover.get() {
                l.hover.set(key);
                changed = true;
                dirty.push(self.window_box(w));
            }
        }
        for r in dirty {
            self.mark_dirty(r);
        }
        changed
    }

    pub(crate) fn draw_log(&self, c: &mut Canvas, r: Rect, l: &LogState) {
        let p = theme::pal();
        let lay = LogLayout::of(r);
        let hv = l.hover.get();
        let key = hv & !H_DOWN;
        let down = hv & H_DOWN != 0;

        // Toolbar.
        let focused = true;
        let needle = String::from_utf8_lossy(l.filter.needle()).into_owned();
        kit::search_field(c, lay.search, &needle, t!("log.search"), focused, focused);
        let levels = LEVELS.map(i18n::tr);
        ui::segmented(c, lay.seg, &levels, l.seg_index());
        text::draw_right(
            c,
            lay.follow_label,
            t!("log.follow"),
            BODY,
            Weight::Regular,
            kit::ink(),
        );
        ui::switch(c, lay.follow_sw, l.knob.value(), true);
        kit::icon_button(
            c,
            lay.clear,
            kitsune_core::iconart::Glyph::Trash,
            t!("log.clear"),
            ButtonKind::Secondary,
            kit::control_state(key == H_CLEAR, down, true),
        );
        kit::icon_button(
            c,
            lay.save,
            kitsune_core::iconart::Glyph::Save,
            t!("log.save"),
            ButtonKind::Secondary,
            kit::control_state(key == H_SAVE, down, true),
        );

        // The table.
        kit::card(c, lay.card);
        let heads = [
            t!("log.col.time"),
            t!("log.col.level"),
            t!("log.col.source"),
            t!("log.col.message"),
        ];
        for (i, h) in heads.iter().enumerate() {
            let col = lay.cols[i];
            if col.w == 0 {
                continue;
            }
            text::draw_left(
                c,
                Rect::new(col.x, lay.head.y, col.w, HEAD_H),
                h,
                FOOTNOTE,
                Weight::Medium,
                kit::ink2(),
            );
        }
        ui::separator(
            c,
            Rect::new(lay.head.x, lay.head.bottom() - 1, lay.head.w, 1),
        );

        let saved = kit::clip_to(c, lay.list);
        let scroll = l.scroll.value().clamp(0, l.max_px(lay.list.h));
        let first = (scroll / ROW_H).max(0) as usize;
        let frac = scroll % ROW_H;
        let mono_pitch = text::measure("0", MONO, Weight::Mono).max(1);
        let msg_cols = (lay.cols[3].w / mono_pitch).max(0) as usize;
        let mut y = lay.list.y - frac;
        for (k, e) in l
            .view
            .visible_from(&l.snap, first, lay.rows + 2)
            .enumerate()
        {
            let idx = first + k;
            let row = Rect::new(lay.list.x, y, lay.list.w, ROW_H);
            if key == H_ROW + idx as u32 {
                ui::fill_token(c, row.inflated(-4), 6, p.hover);
            } else if idx % 2 == 1 {
                ui::fill_token(
                    c,
                    row.inflated(-4),
                    6,
                    if theme::dark() {
                        0x0AFF_FFFF
                    } else {
                        0x0600_0000
                    },
                );
            }
            // Time, in the mono font so the digits line up.
            let t = activity::fmt_log_time(e.ts_ms);
            text::draw_mono(
                c,
                lay.cols[0].x,
                text::center_y(y, ROW_H, MONO, Weight::Mono),
                kit::fb_str(&t),
                MONO,
                kit::ink2(),
            );
            // The level chip.
            kit::chip(
                c,
                lay.cols[1].x,
                y + (ROW_H - 18) / 2,
                18,
                activity::level_name(e.level),
                level_color(e.level),
            );
            // Who wrote it.
            if lay.cols[2].w > 0 {
                let raw = crate::sched::thread_name(e.origin as usize);
                let name = if raw.is_empty() {
                    String::from("—")
                } else {
                    activity::friendly_name(raw.as_bytes())
                };
                text::draw_left(
                    c,
                    Rect::new(lay.cols[2].x, y, lay.cols[2].w - 8, ROW_H),
                    &name,
                    BODY,
                    Weight::Regular,
                    kit::ink2(),
                );
            }
            // The message.
            let msg = text::from_bytes(e.text);
            let shown: String = if msg.chars().count() > msg_cols && msg_cols > 1 {
                let mut s: String = msg.chars().take(msg_cols - 1).collect();
                s.push('…');
                s
            } else {
                msg.into_owned()
            };
            text::draw_mono(
                c,
                lay.cols[3].x,
                text::center_y(y, ROW_H, MONO, Weight::Mono),
                &shown,
                MONO,
                kit::ink(),
            );
            y += ROW_H;
        }
        if l.view.is_empty() {
            let msg = if l.snap.is_empty() {
                t!("log.empty")
            } else {
                t!("log.no_match")
            };
            text::draw_left(
                c,
                Rect::new(lay.list.x + 12, lay.list.y + 8, lay.list.w - 24, 24),
                msg,
                BODY,
                Weight::Regular,
                kit::ink2(),
            );
        }
        c.restore_clip(saved);
        ui::overlay_scrollbar(
            c,
            Rect::new(lay.list.right() - 10, lay.list.y, 10, lay.list.h),
            (scroll / ROW_H).max(0) as usize,
            l.view.len(),
            lay.rows,
            l.sb.alpha(super::toasts_ui::now_ms()),
        );

        // Status line: counts on the left, the last action on the right.
        let total = kitsune_core::klog::records(&l.snap).count();
        let counts = if l.view.len() == total {
            tp!("log.count", total)
        } else {
            tp!("log.count_of", total, shown = l.view.len())
        };
        text::draw_left(
            c,
            lay.status,
            &counts,
            FOOTNOTE,
            Weight::Regular,
            kit::ink2(),
        );
        if let Some((m, err)) = &l.status {
            text::draw_right(
                c,
                lay.status,
                i18n::tr(m),
                FOOTNOTE,
                Weight::Regular,
                if *err { kit::red() } else { kit::ink2() },
            );
        }
    }
}
