//! PS/2 Scan Code Set 1 → logical key translation, with shift/caps state.
//!
//! The kernel feeds raw `(scancode, extended, pressed)` triples; the keymap
//! owns modifier state and emits high-level [`Key`] values.

/// A logical key press produced by the keymap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A printable ASCII byte (already case-resolved).
    Char(u8),
    Enter,
    Backspace,
    Tab,
    Esc,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

/// Physical keyboard layout the scancodes are read with.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Layout {
    /// US QWERTY (the default).
    #[default]
    Us,
    /// Brazilian ABNT2: `ç`, dead accents (´ ` ~ ^ ¨), the extra `\|` and `/?` keys.
    Abnt2,
}

impl Layout {
    pub const ALL: [Layout; 2] = [Layout::Us, Layout::Abnt2];

    /// Short name used in the settings file (`us`, `abnt2`).
    pub const fn name(self) -> &'static str {
        match self {
            Layout::Us => "us",
            Layout::Abnt2 => "abnt2",
        }
    }

    /// Parse [`name`](Self::name) (ASCII case-insensitive).
    pub fn from_name(s: &[u8]) -> Option<Layout> {
        Layout::ALL
            .into_iter()
            .find(|l| s.eq_ignore_ascii_case(l.name().as_bytes()))
    }
}

/// A dead accent waiting for the next letter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dead {
    Acute,
    Grave,
    Tilde,
    Circumflex,
    Diaeresis,
}

impl Dead {
    /// The accent as a Latin-1 character (what a dead key + space types).
    const fn glyph(self) -> u8 {
        match self {
            Dead::Acute => 0xB4,
            Dead::Grave => b'`',
            Dead::Tilde => b'~',
            Dead::Circumflex => b'^',
            Dead::Diaeresis => 0xA8,
        }
    }

    /// The accented Latin-1 letter for `base`, if the accent combines with it.
    fn compose(self, base: u8) -> Option<u8> {
        // (base, acute, grave, tilde, circumflex, diaeresis); 0 = no such letter.
        const T: [(u8, [u8; 5]); 12] = [
            (b'a', [0xE1, 0xE0, 0xE3, 0xE2, 0xE4]),
            (b'e', [0xE9, 0xE8, 0, 0xEA, 0xEB]),
            (b'i', [0xED, 0xEC, 0, 0xEE, 0xEF]),
            (b'o', [0xF3, 0xF2, 0xF5, 0xF4, 0xF6]),
            (b'u', [0xFA, 0xF9, 0, 0xFB, 0xFC]),
            (b'n', [0, 0, 0xF1, 0, 0]),
            (b'A', [0xC1, 0xC0, 0xC3, 0xC2, 0xC4]),
            (b'E', [0xC9, 0xC8, 0, 0xCA, 0xCB]),
            (b'I', [0xCD, 0xCC, 0, 0xCE, 0xCF]),
            (b'O', [0xD3, 0xD2, 0xD5, 0xD4, 0xD6]),
            (b'U', [0xDA, 0xD9, 0, 0xDB, 0xDC]),
            (b'N', [0, 0, 0xD1, 0, 0]),
        ];
        let col = match self {
            Dead::Acute => 0,
            Dead::Grave => 1,
            Dead::Tilde => 2,
            Dead::Circumflex => 3,
            Dead::Diaeresis => 4,
        };
        T.iter()
            .find(|(b, _)| *b == base)
            .map(|(_, v)| v[col])
            .filter(|&c| c != 0)
    }
}

/// What an ABNT2 scancode types: unshifted / shifted, each a character or a
/// dead accent.
#[derive(Clone, Copy)]
enum Out {
    Ch(u8),
    Dead(Dead),
}

/// The scancodes where ABNT2 differs from [`base_shift`] (the US map).
fn abnt2_override(scan: u8) -> Option<(Out, Out)> {
    use Out::{Ch, Dead as D};
    Some(match scan {
        0x03 => (Ch(b'2'), Ch(b'@')),
        0x07 => (Ch(b'6'), D(Dead::Diaeresis)),
        0x29 => (Ch(b'\''), Ch(b'"')),
        0x1A => (D(Dead::Acute), D(Dead::Grave)),
        0x1B => (Ch(b'['), Ch(b'{')),
        0x27 => (Ch(0xE7), Ch(0xC7)),
        0x28 => (D(Dead::Tilde), D(Dead::Circumflex)),
        0x2B => (Ch(b']'), Ch(b'}')),
        0x56 => (Ch(b'\\'), Ch(b'|')),
        0x35 => (Ch(b';'), Ch(b':')),
        0x73 => (Ch(b'/'), Ch(b'?')),
        _ => return None,
    })
}

/// Tracks modifier state and translates scancodes.
#[derive(Default)]
pub struct Keymap {
    shift: bool,
    caps: bool,
    ctrl: bool,
    alt: bool,
    layout: Layout,
    /// A dead accent typed and not yet combined.
    dead: Option<Dead>,
    /// A second key produced by the same press (an accent followed by a letter
    /// it cannot combine with).
    pending: Option<Key>,
}

impl Keymap {
    pub const fn new() -> Self {
        Self {
            shift: false,
            caps: false,
            ctrl: false,
            alt: false,
            layout: Layout::Us,
            dead: None,
            pending: None,
        }
    }

    /// The layout in use.
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// Switch layout; a half-typed dead accent is dropped.
    pub fn set_layout(&mut self, layout: Layout) {
        self.layout = layout;
        self.dead = None;
        self.pending = None;
    }

    /// The second key of the last press, if it produced two (call after
    /// [`process`](Self::process) until it returns `None`).
    pub fn take_pending(&mut self) -> Option<Key> {
        self.pending.take()
    }

    /// Current shift state (exposed for tests/UX).
    pub fn shift(&self) -> bool {
        self.shift
    }

    /// Current caps-lock state.
    pub fn caps(&self) -> bool {
        self.caps
    }

    /// Current control state (left or right Ctrl held).
    pub fn ctrl(&self) -> bool {
        self.ctrl
    }

    /// Current alt state (left, or right = extended, Alt held).
    pub fn alt(&self) -> bool {
        self.alt
    }

    /// Process one PS/2 event. Returns the produced key on key-down, or `None`
    /// for modifier changes, key-up, and unmapped codes.
    pub fn process(&mut self, scan: u8, extended: bool, pressed: bool) -> Option<Key> {
        // Ctrl is scancode 0x1D for both left (normal) and right (extended).
        if scan == 0x1D {
            self.ctrl = pressed;
            return None;
        }

        // Alt is scancode 0x38 for both left (normal) and right (extended, AltGr).
        if scan == 0x38 {
            self.alt = pressed;
            return None;
        }

        if !extended {
            match scan {
                0x2A | 0x36 => {
                    self.shift = pressed;
                    return None;
                }
                0x3A => {
                    if pressed {
                        self.caps = !self.caps;
                    }
                    return None;
                }
                _ => {}
            }
        }

        if !pressed {
            return None;
        }

        if extended {
            return match scan {
                0x48 => Some(Key::Up),
                0x50 => Some(Key::Down),
                0x4B => Some(Key::Left),
                0x4D => Some(Key::Right),
                0x47 => Some(Key::Home),
                0x4F => Some(Key::End),
                0x49 => Some(Key::PageUp),
                0x51 => Some(Key::PageDown),
                0x53 => Some(Key::Delete),
                _ => None,
            };
        }

        // Control keys cancel a half-typed dead accent and pass through.
        let ctl = match scan {
            0x1C => Some(Key::Enter),
            0x0E => Some(Key::Backspace),
            0x0F => Some(Key::Tab),
            0x01 => Some(Key::Esc),
            _ => None,
        };
        if ctl.is_some() {
            self.dead = None;
            return ctl;
        }
        self.typed(scan)
    }

    /// What `scan` types with the current modifiers and layout (before any
    /// dead accent is applied).
    fn lookup(&self, scan: u8) -> Option<Out> {
        let (base, shifted) = match self.layout {
            Layout::Abnt2 => match abnt2_override(scan) {
                Some(pair) => pair,
                None => {
                    let (b, s) = base_shift(scan)?;
                    (Out::Ch(b), Out::Ch(s))
                }
            },
            Layout::Us => {
                let (b, s) = base_shift(scan)?;
                (Out::Ch(b), Out::Ch(s))
            }
        };
        let letter = matches!(base, Out::Ch(c) if c.is_ascii_lowercase() || c == 0xE7);
        // Letters: shift XOR caps decides case; symbols and digits: caps has no effect.
        let upper = if letter {
            self.shift ^ self.caps
        } else {
            self.shift
        };
        Some(if upper { shifted } else { base })
    }

    fn typed(&mut self, scan: u8) -> Option<Key> {
        let out = self.lookup(scan)?;
        let dead = self.dead.take();
        match (out, dead) {
            (Out::Ch(c), None) => Some(Key::Char(c)),
            (Out::Dead(d), None) => {
                self.dead = Some(d);
                None
            }
            // Same accent twice types the accent itself; a different one types the
            // first and starts the second.
            (Out::Dead(d), Some(prev)) => {
                if d != prev {
                    self.dead = Some(d);
                }
                Some(Key::Char(prev.glyph()))
            }
            (Out::Ch(b' '), Some(d)) => Some(Key::Char(d.glyph())),
            (Out::Ch(c), Some(d)) => match d.compose(c) {
                Some(x) => Some(Key::Char(x)),
                None => {
                    self.pending = Some(Key::Char(c));
                    Some(Key::Char(d.glyph()))
                }
            },
        }
    }
}

/// Maps a Set-1 scancode to its `(unshifted, shifted)` ASCII pair.
fn base_shift(scan: u8) -> Option<(u8, u8)> {
    let pair = match scan {
        0x02 => (b'1', b'!'),
        0x03 => (b'2', b'@'),
        0x04 => (b'3', b'#'),
        0x05 => (b'4', b'$'),
        0x06 => (b'5', b'%'),
        0x07 => (b'6', b'^'),
        0x08 => (b'7', b'&'),
        0x09 => (b'8', b'*'),
        0x0A => (b'9', b'('),
        0x0B => (b'0', b')'),
        0x0C => (b'-', b'_'),
        0x0D => (b'=', b'+'),
        0x10 => (b'q', b'Q'),
        0x11 => (b'w', b'W'),
        0x12 => (b'e', b'E'),
        0x13 => (b'r', b'R'),
        0x14 => (b't', b'T'),
        0x15 => (b'y', b'Y'),
        0x16 => (b'u', b'U'),
        0x17 => (b'i', b'I'),
        0x18 => (b'o', b'O'),
        0x19 => (b'p', b'P'),
        0x1A => (b'[', b'{'),
        0x1B => (b']', b'}'),
        0x1E => (b'a', b'A'),
        0x1F => (b's', b'S'),
        0x20 => (b'd', b'D'),
        0x21 => (b'f', b'F'),
        0x22 => (b'g', b'G'),
        0x23 => (b'h', b'H'),
        0x24 => (b'j', b'J'),
        0x25 => (b'k', b'K'),
        0x26 => (b'l', b'L'),
        0x27 => (b';', b':'),
        0x28 => (b'\'', b'"'),
        0x29 => (b'`', b'~'),
        0x2B => (b'\\', b'|'),
        0x2C => (b'z', b'Z'),
        0x2D => (b'x', b'X'),
        0x2E => (b'c', b'C'),
        0x2F => (b'v', b'V'),
        0x30 => (b'b', b'B'),
        0x31 => (b'n', b'N'),
        0x32 => (b'm', b'M'),
        0x33 => (b',', b'<'),
        0x34 => (b'.', b'>'),
        0x35 => (b'/', b'?'),
        0x39 => (b' ', b' '),
        _ => return None,
    };
    Some(pair)
}

#[cfg(test)]
mod tests;
