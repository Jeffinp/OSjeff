//! Keys, wheel, clicks, hover and per-frame state of Tarefas.

use super::layout::columns;
use super::layout::lay;
use super::layout::tab_split;
use super::state::H_CANCEL;
use super::state::H_DOWN;
use super::state::H_END;
use super::state::H_HEAD;
use super::state::H_OK;
use super::state::H_RESTART;
use super::state::H_ROW;
use super::state::H_SAMPLE;
use super::state::H_SEARCH;
use super::state::H_TAB;
use super::state::ROW_H;
use super::state::TAB_PROCESSES;
use crate::desktop::kit::{self};
use crate::desktop::*;
use kitsune_core::activity::{Column, sample_under};
use kitsune_core::sysmon::HIST;

impl Desktop {
    // ------------------------------------------------------------ input

    pub(crate) fn tarefas_key(&mut self, id: WindowId, key: Key) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let l = lay(rect);
        let visible = (l.list.h / ROW_H).max(1) as usize;
        let Some(t) = self.tarefas_mut(id) else {
            return;
        };
        if t.confirm.is_some() {
            match key {
                Key::Esc => t.confirm = None,
                Key::Enter => self.tarefas_end(id, true),
                _ => {}
            }
            return;
        }
        if t.search_focus {
            match key {
                Key::Esc => {
                    t.query.clear();
                    t.search_focus = false;
                }
                Key::Enter | Key::Down => t.search_focus = false,
                Key::Backspace => {
                    t.query.pop();
                }
                Key::Char(b) if (0x20..0x7F).contains(&b) && t.query.len() < 32 => {
                    t.query.push(b as char);
                }
                _ => return,
            }
            self.tarefas_rebuild(id);
            return;
        }
        let tab = t.tab;
        match key {
            Key::Esc => {
                if !t.query.is_empty() {
                    t.query.clear();
                    self.tarefas_rebuild(id);
                } else {
                    self.request_close(id);
                }
                return;
            }
            Key::Tab | Key::Right => t.tab = (t.tab + 1) % 5,
            Key::Left => t.tab = (t.tab + 4) % 5,
            Key::Char(c @ b'1'..=b'5') => t.tab = c - b'1',
            Key::Char(b'/') | Key::Char(b'f') if tab == TAB_PROCESSES => t.search_focus = true,
            Key::Up | Key::Down if tab == TAB_PROCESSES => {
                let n = t.rows.len();
                if n == 0 {
                    return;
                }
                let cur = t.sel_index();
                let next = match (key, cur) {
                    (Key::Up, Some(i)) => i.saturating_sub(1),
                    (Key::Up, None) => n - 1,
                    (_, Some(i)) => (i + 1).min(n - 1),
                    (_, None) => 0,
                };
                t.sel = t.rows.get(next).map(|r| r.id);
                // Keep the selection in view.
                let top = t.scroll.value();
                let (y0, y1) = (next as i32 * ROW_H, (next as i32 + 1) * ROW_H);
                if y0 < top {
                    t.scroll.set(y0);
                } else if y1 > top + visible as i32 * ROW_H {
                    t.scroll.set(y1 - visible as i32 * ROW_H);
                }
                t.sb.touch(crate::desktop::shell::toasts::now_ms());
                return;
            }
            Key::Delete | Key::Backspace if tab == TAB_PROCESSES => {
                self.tarefas_end(id, false);
                return;
            }
            Key::Enter if tab == TAB_PROCESSES => {
                // Bring the selected app to the front.
                if let Some(pid) = t.sel_index().and_then(|i| t.rows.get(i)).map(|r| r.pid)
                    && let Some(w) = self.window_of_pid(pid)
                {
                    self.wm.activate(w);
                }
                return;
            }
            Key::Char(b'r' | b'R') if tab == TAB_PROCESSES => {
                self.tarefas_restart(id);
                return;
            }
            _ => return,
        }
        self.tarefas_rebuild(id);
    }

    pub(crate) fn tarefas_wheel(&mut self, id: WindowId, notches: i32) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let l = lay(rect);
        let Some(t) = self.tarefas_mut(id) else {
            return;
        };
        if t.tab != TAB_PROCESSES {
            return;
        }
        let max = (t.rows.len() as i32 * ROW_H - l.list.h).max(0);
        let target = (t.scroll_target() + notches * ROW_H * 2).clamp(0, max);
        t.scroll.set(target);
        t.sb.touch(crate::desktop::shell::toasts::now_ms());
    }

    pub(crate) fn tarefas_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        let l = lay(rect);
        let Some(t) = self.tarefas_mut(id) else {
            return;
        };
        if t.confirm.is_some() {
            if l.sheet_ok.contains(px, py) {
                self.tarefas_end(id, true);
            } else if l.sheet_cancel.contains(px, py) || !l.sheet.contains(px, py) {
                t.confirm = None;
            }
            return;
        }
        t.search_focus = false;
        if let Some(i) = kitsune_core::widgets::segmented_hit(l.tabs, 5, px, py) {
            if t.tab != i as u8 {
                t.tab = i as u8;
                self.tarefas_rebuild(id);
            }
            return;
        }
        if t.tab != TAB_PROCESSES {
            return;
        }
        if l.search.contains(px, py) {
            t.search_focus = true;
            return;
        }
        if l.btn_end.contains(px, py) {
            self.tarefas_end(id, false);
            return;
        }
        if l.btn_restart.contains(px, py) {
            self.tarefas_restart(id);
            return;
        }
        if l.head.contains(px, py) {
            if let Some(i) = columns(l.head).iter().position(|c| c.contains(px, py)) {
                let col = Column::ALL[i];
                if t.sort == col {
                    t.desc = !t.desc;
                } else {
                    t.sort = col;
                    t.desc = col.default_desc();
                }
                self.tarefas_rebuild(id);
            }
            return;
        }
        if l.list.contains(px, py) {
            let i = ((py - l.list.y + t.scroll.value()) / ROW_H) as usize;
            t.sel = t.rows.get(i).map(|r| r.id);
        }
    }

    // ------------------------------------------------------------ hover

    /// What the pointer is over in window `rect` (0 for nothing special).
    fn tarefas_hover_key(&self, rect: Rect, st: &TarefasState, cx: i32, cy: i32) -> u32 {
        let l = lay(rect);
        if st.confirm.is_some() {
            return if l.sheet_ok.contains(cx, cy) {
                H_OK
            } else if l.sheet_cancel.contains(cx, cy) {
                H_CANCEL
            } else {
                0
            };
        }
        if !rect.body().contains(cx, cy) {
            return 0;
        }
        if let Some(i) = kitsune_core::widgets::segmented_hit(l.tabs, 5, cx, cy) {
            return H_TAB + i as u32;
        }
        match st.tab {
            TAB_PROCESSES => {
                if l.search.contains(cx, cy) {
                    return H_SEARCH;
                }
                if l.btn_end.contains(cx, cy) {
                    return H_END;
                }
                if l.btn_restart.contains(cx, cy) {
                    return H_RESTART;
                }
                if l.head.contains(cx, cy) {
                    return columns(l.head)
                        .iter()
                        .position(|c| c.contains(cx, cy))
                        .map_or(0, |i| H_HEAD + i as u32);
                }
                if l.list.contains(cx, cy) {
                    let i = (cy - l.list.y + st.scroll.value()) / ROW_H;
                    if (i as usize) < st.rows.len() {
                        return H_ROW + i as u32;
                    }
                }
                0
            }
            0..=3 => {
                let sp = tab_split(l.content, st.tab);
                let plot = kit::plot_of(sp.chart);
                let n = self.sysmon.cpu_total.len();
                sample_under(cx, plot.x, plot.w, HIST, n)
                    .filter(|_| cy >= plot.y - 8 && cy < plot.bottom() + 8)
                    .map_or(0, |i| H_SAMPLE + i as u32)
            }
            _ => 0,
        }
    }

    /// Update the hover key of every Tarefas window for a pointer at `(cx, cy)` with the
    /// primary button `down`. Returns whether a window needs repainting.
    pub(crate) fn tarefas_hover(&mut self, cx: i32, cy: i32, down: bool) -> bool {
        let top = if self.overlay_open() {
            None
        } else {
            self.topmost_at(cx, cy)
        };
        let mut changed = false;
        let mut dirty = Vec::new();
        for w in self.wm.windows() {
            let App::Tarefas(st) = &w.app.app else {
                continue;
            };
            if !w.shown() {
                continue;
            }
            let mut key = if Some(w.id) == top {
                self.tarefas_hover_key(w.rect, st, cx, cy)
            } else {
                0
            };
            if key != 0 && down {
                key |= H_DOWN;
            }
            if key != st.hover.get() {
                st.hover.set(key);
                changed = true;
                dirty.push(self.window_box(w));
            }
        }
        for r in dirty {
            self.mark_dirty(r);
        }
        changed
    }

    /// Advance the animations of the Tarefas windows (called by `Desktop::live_step`).
    pub(crate) fn tarefas_step(&mut self, dt_ms: u32) {
        if !self
            .wm
            .windows()
            .iter()
            .any(|w| matches!(w.app.app, App::Tarefas(_)))
        {
            return;
        }
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Tarefas(_)))
            .map(|w| w.id)
            .collect();
        for id in ids {
            if let Some(t) = self.tarefas_mut(id) {
                t.scroll.step(dt_ms, 90);
            }
        }
    }

    /// What window `w` repaints while it animates: the heading and chart of the graph tabs
    /// (the curve scrolls, the headline number glides), the table of Processos (scrolling).
    /// Everything else waits for the settle frame at the end of the glide.
    pub(crate) fn tarefas_live_rect(&self, w: &Win) -> Rect {
        let App::Tarefas(t) = &w.app.app else {
            return w.rect;
        };
        let l = lay(w.rect);
        if t.tab == TAB_PROCESSES {
            l.head.union(&l.list)
        } else {
            let sp = tab_split(l.content, t.tab);
            sp.title.union(&sp.chart)
        }
    }

    pub(crate) fn tarefas_busy_one(&self, w: &Win) -> bool {
        let App::Tarefas(t) = &w.app.app else {
            return false;
        };
        w.shown()
            && ((matches!(t.tab, 0..=3) && self.sysmon.gliding())
                || t.scroll.moving()
                || t.sb.active(crate::desktop::shell::toasts::now_ms()))
    }
}
