//! Keys, wheel, clicks, hover and per-frame state of the log viewer.

use super::state::H_CLEAR;
use super::state::H_DOWN;
use super::state::H_FOLLOW;
use super::state::H_ROW;
use super::state::H_SAVE;
use super::state::H_SEARCH;
use super::state::LogLayout;
use super::state::SAVE_NAME;
use super::state::SEG_LEVEL;
use crate::desktop::*;
use kitsune_core::fileman::ui::ROW_H;
use kitsune_core::klog::{Level, render_text};
use kitsune_core::sysif::{LogSink, SinkError};
use kitsune_core::tk;

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
        let now = crate::desktop::shell::toasts::now_ms();
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
        l.sb.touch(crate::desktop::shell::toasts::now_ms());
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
            && (l.scroll.moving()
                || l.knob.moving()
                || l.sb.active(crate::desktop::shell::toasts::now_ms()))
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
}
