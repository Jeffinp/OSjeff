//! The system-log viewer: a window onto the kernel's log ring (`crate::klog`)
//! with a minimum-level filter, text search, scrolling, "clear" and "save to
//! file" (through the `LogSink` trait, FS v2 today).
//!
//! The window works on a private copy (snapshot) of the ring, refreshed once a
//! second and after any input, so drawing never touches the live ring or holds
//! interrupts off. The filter / scroll / indexing logic is `osjeff_core::klog`.

use super::ui::*;
use super::*;
use osjeff_core::klog::{Filter, Level, LogView, PREFIX_LEN, format_prefix, render_text};
use osjeff_core::sysif::{LogSink, SinkError};

/// Height of one log line.
const ROW_H: i32 = 18;
/// Longest file name the dump is saved under.
const SAVE_NAME: &[u8] = b"syslog.txt";

/// Per-window state of a log viewer.
pub(crate) struct LogState {
    snap: Vec<u8>,
    view: LogView,
    filter: Filter,
    /// `klog::seq()` when `snap` was taken.
    seen: u32,
    status: [u8; 40],
    status_len: usize,
}

impl LogState {
    pub(crate) fn new() -> Self {
        let mut s = Self {
            snap: Vec::new(),
            view: LogView::new(),
            filter: Filter::new(),
            seen: 0,
            status: [0; 40],
            status_len: 0,
        };
        s.reload(24);
        s
    }

    /// Approximate heap held by this window (for the resource monitor).
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

    fn set_status(&mut self, msg: &[u8]) {
        let n = msg.len().min(self.status.len());
        self.status[..n].copy_from_slice(&msg[..n]);
        self.status_len = n;
    }
}

/// Geometry of a log window, shared by drawing and hit-testing.
struct LogLayout {
    level: Rect,
    search: Rect,
    clear: Rect,
    save: Rect,
    list: Rect,
    sbar: Rect,
    status: Rect,
    rows: usize,
}

impl LogLayout {
    fn of(r: Rect) -> LogLayout {
        let pad = 10;
        let x = r.x + pad;
        let w = r.w - 2 * pad;
        let ty = r.y + TITLE_H + 8;
        let bh = 28;
        let save = Rect::new(x + w - 84, ty, 84, bh);
        let clear = Rect::new(save.x - 8 - 96, ty, 96, bh);
        let level = Rect::new(x, ty, 156, bh);
        let search = Rect::new(
            level.right() + 8,
            ty,
            (clear.x - 8 - (level.right() + 8)).max(60),
            bh,
        );
        let status = Rect::new(x, r.bottom() - 26, w, 20);
        let list = Rect::new(x, ty + bh + 8, w, (status.y - 6 - (ty + bh + 8)).max(ROW_H));
        let sbar = Rect::new(list.right() - 12, list.y + 6, 8, list.h - 12);
        let rows = (((list.h - 12) / ROW_H).max(1)) as usize;
        LogLayout {
            level,
            search,
            clear,
            save,
            list,
            sbar,
            status,
            rows,
        }
    }
}

fn level_color(l: Level) -> Color {
    match l {
        Level::Trace => Color::rgb(0x6B, 0x77, 0x90),
        Level::Debug => Color::rgb(0x8C, 0x9A, 0xB6),
        Level::Info => Color::rgb(0xDC, 0xE3, 0xF2),
        Level::Warn => Color::rgb(0xFF, 0xC1, 0x4D),
        Level::Error => Color::rgb(0xFF, 0x6B, 0x6B),
        Level::Fatal => Color::rgb(0xFF, 0x4D, 0x9D),
    }
}

impl Desktop {
    /// Refresh every visible log window whose ring changed (called each second
    /// and after input). Returns whether any window needs a repaint.
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
                let rows = LogLayout::of(rect).rows;
                changed |= l.refresh(rows);
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
        let rows = LogLayout::of(rect).rows;
        let Some(l) = self.log_mut(id) else {
            return;
        };
        let rebuild = match key {
            Key::Esc => {
                if l.filter.needle().is_empty() {
                    self.request_close(id);
                    return;
                }
                l.filter.clear_needle();
                true
            }
            Key::Tab => {
                l.filter.cycle_level();
                true
            }
            Key::Backspace => l.filter.backspace(),
            Key::Delete => {
                l.filter.clear_needle();
                true
            }
            Key::Char(b) => l.filter.push_char(b),
            Key::Up => {
                l.view.scroll(-1, rows);
                false
            }
            Key::Down => {
                l.view.scroll(1, rows);
                false
            }
            Key::Home => {
                l.view.home();
                false
            }
            Key::End => {
                l.view.end(rows);
                false
            }
            _ => false,
        };
        if rebuild {
            l.view.rebuild(&l.snap, &l.filter, rows);
        }
    }

    pub(crate) fn log_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        let lay = LogLayout::of(rect);
        let Some(l) = self.log_mut(id) else {
            return;
        };
        if lay.level.contains(px, py) {
            l.filter.cycle_level();
            l.view.rebuild(&l.snap, &l.filter, lay.rows);
        } else if lay.clear.contains(px, py) {
            crate::klog::clear();
            l.reload(lay.rows);
            l.set_status(b"log limpo");
        } else if lay.save.contains(px, py) {
            let mut text = Vec::new();
            render_text(
                &l.snap,
                &l.filter,
                |o| crate::sched::thread_name(o as usize),
                &mut text,
            );
            let msg: &[u8] = match VfsSink.write_file(SAVE_NAME, &text) {
                Ok(()) => b"salvo em syslog.txt",
                Err(SinkError::Truncated { .. }) => b"salvo em syslog.txt (so o fim)",
                Err(SinkError::NoSpace) => b"erro: disco cheio",
                Err(_) => b"erro ao salvar",
            };
            l.set_status(msg);
            crate::klog::log_quiet(Level::Info, format_args!("log saved to syslog.txt"));
        } else if Rect::new(lay.sbar.x - 6, lay.sbar.y, lay.sbar.w + 12, lay.sbar.h)
            .contains(px, py)
        {
            let pos = scrollbar_pos(lay.sbar, py, l.view.len(), lay.rows);
            l.view.set_top(pos, lay.rows);
        } else if lay.list.contains(px, py) {
            // Click in the upper / lower half scrolls a few lines that way.
            let up = py < lay.list.y + lay.list.h / 2;
            l.view.scroll(if up { -4 } else { 4 }, lay.rows);
        }
    }

    pub(crate) fn draw_log(&self, c: &mut Canvas, r: Rect, l: &LogState) {
        let lay = LogLayout::of(r);

        // Toolbar.
        let mut lvl = [0u8; 16];
        let tag = l.filter.min.tag().trim_end().as_bytes();
        lvl[..7].copy_from_slice(b"Nivel: ");
        lvl[7..7 + tag.len()].copy_from_slice(tag);
        lvl[7 + tag.len()] = b'+';
        let lvl_state = if l.filter.min > Level::Trace {
            Btn::On
        } else {
            Btn::Normal
        };
        button(c, lay.level, &lvl[..8 + tag.len()], lvl_state);
        input_box(c, lay.search, l.filter.needle(), b"buscar...", true);
        button(c, lay.clear, b"Limpar", Btn::Normal);
        button(c, lay.save, b"Salvar", Btn::Normal);

        // Console panel.
        fill_round(c, lay.list, 10, PANEL_DARK);
        let text_w = lay.list.w - 32;
        let show_thread = lay.list.w >= 700;
        let mut y = lay.list.y + 6;
        for e in l.view.visible(&l.snap, lay.rows) {
            let col = level_color(e.level);
            let mut x = lay.list.x + 10;
            let mut p = [0u8; PREFIX_LEN];
            let n = format_prefix(&e, &mut p);
            let (pc, tc) = if e.level >= Level::Warn {
                (col, col)
            } else {
                (DIM_TEXT, col)
            };
            text(c, x, y, text_w, &p[..n], pc);
            x += n as i32 * CELL_W;
            if show_thread {
                let name = crate::sched::thread_name(e.origin as usize).as_bytes();
                let name = &name[..name.len().min(10)];
                text(c, x, y, text_w, name, DIM_TEXT);
                x += 11 * CELL_W;
            }
            let room = lay.list.x + 10 + text_w - x;
            text(c, x, y, room, e.text, tc);
            y += ROW_H;
        }
        if l.view.is_empty() {
            let msg: &[u8] = if l.snap.is_empty() {
                b"(log vazio)"
            } else {
                b"(nenhuma linha passa pelo filtro)"
            };
            text(c, lay.list.x + 14, lay.list.y + 10, text_w, msg, DIM_TEXT);
        }
        scrollbar(
            c,
            lay.sbar,
            l.view.top_for(lay.rows),
            l.view.len(),
            lay.rows,
        );

        // Status line: counts on the left, the last action on the right.
        let mut st = [0u8; 48];
        let mut b = osjeff_core::klog::FixedBuf::<48>::new();
        use core::fmt::Write as _;
        let _ = write!(b, "{} de {} linhas", l.view.len(), count_records(&l.snap));
        let n = b.as_bytes().len();
        st[..n].copy_from_slice(b.as_bytes());
        text(
            c,
            lay.status.x,
            lay.status.y,
            lay.status.w,
            &st[..n],
            theme::text_muted(),
        );
        text_right(
            c,
            lay.status,
            &l.status[..l.status_len],
            theme::text_muted(),
        );
    }
}

fn count_records(snap: &[u8]) -> usize {
    osjeff_core::klog::records(snap).count()
}
