//! Keyboard handling.

use crate::desktop::*;
use kitsune_core::fileman::apps::AppKey;
use kitsune_core::fileman::ui::{self, Dir, ViewMode};
use kitsune_core::fileman::{APPS_PATH, Cmd, TRASH_PATH};
use kitsune_core::t;

impl Desktop {
    // ---- keys ----

    /// Ctrl+letter shortcuts of the file manager. `true` when consumed.
    pub(crate) fn files_ctrl(&mut self, id: WindowId, ch: u8) -> bool {
        if self
            .files_mut(id)
            .is_some_and(|f| f.sheet_open() || f.input.is_some())
        {
            return false;
        }
        if self.files_mut(id).is_some_and(|f| f.search.focused) && matches!(ch, b'a' | b'A') {
            if let Some(f) = self.files_mut(id) {
                f.search.input.select_all();
            }
            return true;
        }
        let cmd = match ch.to_ascii_lowercase() {
            b'a' => Cmd::SelectAll,
            b'c' => Cmd::Copy,
            b'x' => Cmd::Cut,
            b'v' => Cmd::Paste,
            b'r' => Cmd::Refresh,
            b'i' => Cmd::Properties,
            b'1' => Cmd::SetView(ViewMode::List),
            b'2' => Cmd::SetView(ViewMode::Icons),
            b'f' => {
                if let Some(f) = self.files_mut(id) {
                    f.search.focused = true;
                    f.search.last_input = appui::ticks();
                }
                return true;
            }
            _ => return false,
        };
        self.files_cmd(id, cmd);
        true
    }

    /// Keys of the file manager (after the global shortcuts).
    pub(crate) fn files_key(&mut self, id: WindowId, key: Key) {
        let shift = self.keymap.shift();
        let ctrl = self.keymap.ctrl();
        let Some(lay) = self.files_layout(id) else {
            return;
        };
        let now = appui::ticks();
        let Some(f) = self.files_mut(id) else {
            return;
        };
        // 1. a sheet.
        if f.sheet_open() {
            if f.confirm.is_some() {
                match key {
                    Key::Enter => self.files_confirmed(id),
                    Key::Esc => {
                        f.confirm = None;
                        f.say(t!("files.msg.cancelled"), false);
                    }
                    _ => {}
                }
            } else if f.props.is_some() {
                f.props = None;
            } else if key == Key::Esc {
                self.files_cancel_job(id);
            }
            return;
        }
        // 2. the inline name field.
        if let Some(edit) = f.input.as_mut() {
            edit.last_input = now;
            match key {
                Key::Esc => f.input = None,
                Key::Enter => self.files_commit_input(id),
                Key::Backspace => edit.input.backspace(),
                Key::Delete => edit.input.delete(),
                Key::Left => edit.input.left(),
                Key::Right => edit.input.right(),
                Key::Home => edit.input.home(),
                Key::End => edit.input.end(),
                Key::Char(b) => edit.input.insert(b),
                _ => {}
            }
            return;
        }
        // 3. the search field.
        if f.search.focused {
            f.search.last_input = now;
            let before = f.search.input.text().to_vec();
            match key {
                Key::Esc => {
                    if f.search.input.text().is_empty() {
                        f.search.focused = false;
                    } else {
                        f.search.input.clear();
                    }
                }
                Key::Enter | Key::Down | Key::Tab => f.search.focused = false,
                Key::Backspace => f.search.input.backspace(),
                Key::Delete => f.search.input.delete(),
                Key::Left => f.search.input.left(),
                Key::Right => f.search.input.right(),
                Key::Home => f.search.input.home(),
                Key::End => f.search.input.end(),
                Key::Char(b) => f.search.input.insert(b),
                _ => {}
            }
            if f.search.input.text() != &before[..] {
                let text = f.search.input.text().to_vec();
                f.view.set_filter(&text);
                f.scroller.jump(0);
                f.view.select_first();
            }
            self.files_sync_preview(id);
            return;
        }
        // 4. normal keys.
        let (mode, vw) = (f.mode, lay.list.w);
        let n = f.view.rows.len();
        let in_trash = f.view.in_trash();
        let in_apps = f.view.in_apps();
        let cur = f.view.sel.cursor();
        let mut moved = false;
        match key {
            Key::Esc => {
                if f.view.sel.count() > 0 {
                    f.view.sel.clear();
                } else if !f.view.filter().is_empty() {
                    f.search.input.clear();
                    f.view.clear_filter();
                } else {
                    self.request_close(id);
                }
            }
            Key::Up if ctrl => self.files_history(id, 2),
            Key::Left if ctrl => self.files_history(id, 0),
            Key::Right if ctrl => self.files_history(id, 1),
            Key::Up | Key::Down | Key::Left | Key::Right
                if mode == ViewMode::Icons || matches!(key, Key::Up | Key::Down) =>
            {
                let dir = match key {
                    Key::Up => Dir::Up,
                    Key::Down => Dir::Down,
                    Key::Left => Dir::Left,
                    _ => Dir::Right,
                };
                let to = ui::step_index(mode, vw, cur, dir, n);
                f.view.sel.move_cursor(to as isize - cur as isize, shift);
                moved = true;
            }
            Key::Home => {
                f.view.sel.move_cursor(-(n as isize), shift);
                moved = true;
            }
            Key::End => {
                f.view.sel.move_cursor(n as isize, shift);
                moved = true;
            }
            Key::Left | Key::Backspace => self.files_history(id, 2),
            Key::Enter | Key::Right => {
                if n > 0 {
                    self.files_activate(id, cur);
                }
            }
            Key::Tab => {
                let target: &[u8] = if in_trash || in_apps {
                    b"/"
                } else {
                    TRASH_PATH
                };
                self.files_go(id, target);
            }
            // Space shows or hides the preview pane.
            Key::Char(b' ') => self.files_cmd(id, Cmd::TogglePreview),
            // The Apps place: `I` installs the bundled package, `Del` removes the app.
            Key::Char(b'i') | Key::Char(b'I') if in_apps => {
                self.files_app_key(id, cur, AppKey::Install);
            }
            Key::Char(b'a') | Key::Char(b'A') if !in_trash && !in_apps => {
                self.files_go(id, APPS_PATH);
            }
            Key::Delete => {
                let cmd = if shift || in_trash {
                    Cmd::DeletePermanent
                } else {
                    Cmd::Delete
                };
                self.files_cmd(id, cmd);
            }
            Key::Char(b'n') | Key::Char(b'N') if !in_trash && !in_apps => {
                self.files_cmd(id, Cmd::NewFolder)
            }
            Key::Char(b'f') | Key::Char(b'F') if !in_trash && !in_apps => {
                self.files_cmd(id, Cmd::NewFile)
            }
            _ => {}
        }
        if moved {
            self.files_reveal(id);
        }
        self.files_sync_preview(id);
    }

    /// F2 / F5 / PageUp / PageDown in a file manager. `true` when consumed.
    pub(crate) fn files_special(&mut self, id: WindowId, sp: Special) -> bool {
        let shift = self.keymap.shift();
        let Some(lay) = self.files_layout(id) else {
            return false;
        };
        if self
            .files_mut(id)
            .is_none_or(|f| f.sheet_open() || f.input.is_some() || f.search.focused)
        {
            return false;
        }
        match sp {
            Special::F2 => self.files_cmd(id, Cmd::Rename),
            Special::F5 => self.files_cmd(id, Cmd::Refresh),
            Special::PageUp | Special::PageDown => {
                if let Some(f) = self.files_mut(id) {
                    let d = ui::page_items(f.mode, lay.list.w, lay.list.h) as isize;
                    f.view
                        .sel
                        .move_cursor(if sp == Special::PageUp { -d } else { d }, shift);
                }
                self.files_reveal(id);
            }
            _ => return false,
        }
        self.files_sync_preview(id);
        true
    }
}
