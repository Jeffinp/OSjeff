//! Keyboard events with modifiers, shared by the v2 editor and the shell.
//!
//! [`crate::system::keymap::Key`] carries no modifier state (the keymap tracks Ctrl and
//! Shift itself) and has no `PageUp`/`PageDown`. Changing that enum would break
//! every exhaustive `match` in the kernel, so this module wraps it instead:
//! [`KeyEvent::from_key`] combines a `Key` with the modifiers the caller read from
//! [`crate::system::keymap::Keymap::ctrl`] / [`crate::system::keymap::Keymap::shift`], and
//! [`KeyCode`] adds the keys the old enum lacks.

use crate::system::keymap::Key;

/// Modifier keys held while a key was pressed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: false,
    };
    pub const CTRL: Mods = Mods {
        ctrl: true,
        shift: false,
        alt: false,
    };
    pub const SHIFT: Mods = Mods {
        ctrl: false,
        shift: true,
        alt: false,
    };
    pub const ALT: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: true,
    };
    pub const CTRL_SHIFT: Mods = Mods {
        ctrl: true,
        shift: true,
        alt: false,
    };

    /// True when no modifier is held.
    pub const fn is_none(self) -> bool {
        !self.ctrl && !self.shift && !self.alt
    }
}

/// A logical key, independent of modifiers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyCode {
    /// A printable character (already case-resolved by the keymap).
    Char(char),
    Enter,
    Backspace,
    Delete,
    Tab,
    Esc,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    /// Function key `F1..=F12`.
    F(u8),
}

/// A key press with its modifiers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub mods: Mods,
}

impl KeyEvent {
    pub const fn new(code: KeyCode, mods: Mods) -> Self {
        Self { code, mods }
    }

    /// A key with no modifiers.
    pub const fn plain(code: KeyCode) -> Self {
        Self::new(code, Mods::NONE)
    }

    /// `Ctrl` + a character key.
    pub const fn ctrl(c: char) -> Self {
        Self::new(KeyCode::Char(c), Mods::CTRL)
    }

    /// A printable character with no modifiers.
    pub const fn ch(c: char) -> Self {
        Self::plain(KeyCode::Char(c))
    }

    /// The same key with `Shift` added.
    pub const fn shifted(self) -> Self {
        Self::new(
            self.code,
            Mods {
                ctrl: self.mods.ctrl,
                shift: true,
                alt: self.mods.alt,
            },
        )
    }

    /// The same key with `Ctrl` added.
    pub const fn with_ctrl(self) -> Self {
        Self::new(
            self.code,
            Mods {
                ctrl: true,
                shift: self.mods.shift,
                alt: self.mods.alt,
            },
        )
    }

    /// Translate a legacy [`Key`] plus the modifier state the caller tracks.
    pub fn from_key(key: Key, mods: Mods) -> Self {
        let code = match key {
            Key::Char(b) => {
                let c = char::from(b);
                // With Ctrl held, letters are reported lower-case so `Ctrl+A`
                // and `Ctrl+Shift+A` can be told apart by `mods.shift`.
                KeyCode::Char(if mods.ctrl { c.to_ascii_lowercase() } else { c })
            }
            Key::Enter => KeyCode::Enter,
            Key::Backspace => KeyCode::Backspace,
            Key::Tab => KeyCode::Tab,
            Key::Esc => KeyCode::Esc,
            Key::Delete => KeyCode::Delete,
            Key::Left => KeyCode::Left,
            Key::Right => KeyCode::Right,
            Key::Up => KeyCode::Up,
            Key::Down => KeyCode::Down,
            Key::Home => KeyCode::Home,
            Key::End => KeyCode::End,
            Key::PageUp => KeyCode::PageUp,
            Key::PageDown => KeyCode::PageDown,
        };
        Self { code, mods }
    }
}

impl From<Key> for KeyEvent {
    fn from(key: Key) -> Self {
        Self::from_key(key, Mods::NONE)
    }
}

#[cfg(test)]
mod tests;
