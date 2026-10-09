//! Key dispatch: shortcuts and prompt handling.
//!
//! | Keys | Action |
//! |---|---|
//! | arrows, Home/End, PageUp/PageDown | move (add Shift to select) |
//! | Ctrl+Left/Right | by word |
//! | Ctrl+Home/End | document start/end |
//! | Ctrl+Up/Down | scroll one line |
//! | Ctrl+A / Ctrl+L | select all / current line |
//! | Ctrl+C / Ctrl+X / Ctrl+V (Shift+Delete = cut) | clipboard |
//! | Ctrl+Z / Ctrl+Y (Ctrl+Shift+Z) | undo / redo |
//! | Ctrl+F / F3 / Shift+F3 | find / next / previous |
//! | Ctrl+H | find and replace |
//! | Ctrl+G | go to line |
//! | Ctrl+Backspace / Ctrl+Delete | delete word left / right |
//! | Tab / Shift+Tab | indent / outdent |
//! | Ctrl+S / Ctrl+Q | [`Event::SaveRequested`] / [`Event::QuitRequested`] |
//!
//! Inside a prompt: Enter = next (Find) / replace (Replace field) / go (Goto),
//! Shift+Enter = previous, Tab = switch field, Alt+C = toggle case
//! sensitivity, Alt+A = replace all, Ctrl+U = clear field, Esc = close.

use super::{Editor, Event};
use crate::system::clipboard::Clipboard;
use crate::system::input::{KeyCode, KeyEvent};

impl Editor {
    /// Handle one key press. `clip` is the shared clipboard.
    pub fn handle_key(&mut self, ev: KeyEvent, clip: &mut Clipboard) -> Event {
        if self.prompt.is_some() {
            return self.prompt_key(ev);
        }
        let (ctrl, shift, alt) = (ev.mods.ctrl, ev.mods.shift, ev.mods.alt);
        match ev.code {
            KeyCode::Char(c) if ctrl && !alt => return self.ctrl_char(c, shift, clip),
            KeyCode::Char(_) if alt => return Event::Ignored,
            KeyCode::Char(c) => self.insert_char(c),
            KeyCode::Enter => self.newline(),
            KeyCode::Backspace => {
                if ctrl {
                    self.delete_word_left();
                } else {
                    self.backspace();
                }
            }
            KeyCode::Delete => {
                if ctrl {
                    self.delete_word_right();
                } else if shift {
                    self.cut(clip);
                } else {
                    self.delete_forward();
                }
            }
            KeyCode::Tab => {
                if shift {
                    self.outdent();
                } else {
                    self.tab();
                }
            }
            KeyCode::Esc => self.clear_selection(),
            KeyCode::Left => {
                if ctrl {
                    self.move_word_left(shift);
                } else {
                    self.move_left(shift);
                }
            }
            KeyCode::Right => {
                if ctrl {
                    self.move_word_right(shift);
                } else {
                    self.move_right(shift);
                }
            }
            KeyCode::Up => {
                if ctrl {
                    self.scroll_by(-1);
                } else {
                    self.move_vertical(-1, shift);
                }
            }
            KeyCode::Down => {
                if ctrl {
                    self.scroll_by(1);
                } else {
                    self.move_vertical(1, shift);
                }
            }
            KeyCode::Home => {
                if ctrl {
                    self.move_doc_start(shift);
                } else {
                    self.move_home(shift);
                }
            }
            KeyCode::End => {
                if ctrl {
                    self.move_doc_end(shift);
                } else {
                    self.move_end(shift);
                }
            }
            KeyCode::PageUp => self.page(-1, shift),
            KeyCode::PageDown => self.page(1, shift),
            KeyCode::F(3) => {
                if shift {
                    self.find_prev();
                } else {
                    self.find_next();
                }
            }
            KeyCode::F(_) => return Event::Ignored,
        }
        Event::Handled
    }

    fn ctrl_char(&mut self, c: char, shift: bool, clip: &mut Clipboard) -> Event {
        match c.to_ascii_lowercase() {
            'a' => self.select_all(),
            'l' => {
                let c = self.cursor;
                self.select_line_at(c);
            }
            'c' => {
                self.copy(clip);
            }
            'x' => {
                self.cut(clip);
            }
            'v' => {
                self.paste(clip);
            }
            'z' => {
                if shift {
                    self.redo();
                } else {
                    self.undo();
                }
            }
            'y' => {
                self.redo();
            }
            'f' => self.open_find(),
            'h' => self.open_replace(),
            'g' => self.open_goto(),
            's' => return Event::SaveRequested,
            'q' => return Event::QuitRequested,
            _ => return Event::Ignored,
        }
        Event::Handled
    }

    fn prompt_key(&mut self, ev: KeyEvent) -> Event {
        let (ctrl, shift, alt) = (ev.mods.ctrl, ev.mods.shift, ev.mods.alt);
        match ev.code {
            KeyCode::Esc => self.close_prompt(),
            KeyCode::Enter => {
                if self.prompt_enter(shift) {
                    self.close_prompt();
                }
            }
            KeyCode::Backspace => self.prompt_backspace(),
            KeyCode::Tab => self.prompt_switch_field(),
            KeyCode::Up => {
                self.find_prev();
            }
            KeyCode::Down => {
                self.find_next();
            }
            KeyCode::F(3) => {
                if shift {
                    self.find_prev();
                } else {
                    self.find_next();
                }
            }
            KeyCode::Char(c) if alt => match c.to_ascii_lowercase() {
                'c' => self.cfg.case_sensitive = !self.cfg.case_sensitive,
                'a' => {
                    self.replace_all();
                }
                'r' => {
                    self.replace_current();
                }
                _ => {}
            },
            KeyCode::Char(c) if ctrl => match c.to_ascii_lowercase() {
                'u' => self.prompt_clear_field(),
                'f' => {
                    self.find_next();
                }
                'g' => self.open_goto(),
                'h' => self.open_replace(),
                _ => {}
            },
            KeyCode::Char(c) => self.prompt_type(c),
            _ => {}
        }
        Event::Handled
    }
}
