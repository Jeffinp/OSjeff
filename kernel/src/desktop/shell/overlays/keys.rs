//! Keyboard handling of the shell layers.

use crate::desktop::shell::*;
use crate::desktop::*;

impl Desktop {
    // ---- keys ----

    /// A key while a shell layer is up. Returns `true` when consumed.
    pub(crate) fn shell_key(&mut self, key: Key) -> bool {
        if self.shell.dialog.as_ref().is_some_and(|d| !d.closing) {
            self.dialog_key(key);
            return true;
        }
        if self.shell.apps.as_ref().is_some_and(|a| !a.closing) {
            self.apps_key(key);
            return true;
        }
        if self.shell.search.as_ref().is_some_and(|s| !s.closing) {
            self.search_key(key);
            return true;
        }
        if let Some(m) = self.shell.menu.as_ref().filter(|m| !m.closing) {
            let n = m.entries.len();
            let cur = m.hover;
            match key {
                Key::Esc => self.close_transients(),
                Key::Up | Key::Down if n > 0 => {
                    let mut i = cur.unwrap_or(if key == Key::Down { n - 1 } else { 0 });
                    for _ in 0..n {
                        i = if key == Key::Down {
                            (i + 1) % n
                        } else {
                            (i + n - 1) % n
                        };
                        if m.entries[i].cmd != Cmd::Sep && m.entries[i].enabled {
                            break;
                        }
                    }
                    if let Some(m) = self.shell.menu.as_mut() {
                        m.hover = Some(i);
                    }
                    self.force_full = true;
                }
                Key::Enter => {
                    if let Some(i) = cur {
                        self.menu_pick(i);
                    }
                }
                _ => {}
            }
            return true;
        }
        if self.shell.pop.as_ref().is_some_and(|p| !p.closing) {
            if key == Key::Esc {
                self.close_transients();
            }
            return true;
        }
        false
    }
}
