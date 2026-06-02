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
}

/// Tracks modifier state and translates scancodes.
#[derive(Default)]
pub struct Keymap {
    shift: bool,
    caps: bool,
}

impl Keymap {
    pub const fn new() -> Self {
        Self {
            shift: false,
            caps: false,
        }
    }

    /// Current shift state (exposed for tests/UX).
    pub fn shift(&self) -> bool {
        self.shift
    }

    /// Current caps-lock state.
    pub fn caps(&self) -> bool {
        self.caps
    }

    /// Process one PS/2 event. Returns the produced key on key-down, or `None`
    /// for modifier changes, key-up, and unmapped codes.
    pub fn process(&mut self, scan: u8, extended: bool, pressed: bool) -> Option<Key> {
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
                0x53 => Some(Key::Delete),
                _ => None,
            };
        }

        match scan {
            0x1C => Some(Key::Enter),
            0x0E => Some(Key::Backspace),
            0x0F => Some(Key::Tab),
            0x01 => Some(Key::Esc),
            _ => self.translate_char(scan).map(Key::Char),
        }
    }

    fn translate_char(&self, scan: u8) -> Option<u8> {
        let (base, shifted) = base_shift(scan)?;
        let upper = if base.is_ascii_lowercase() {
            // Letters: shift XOR caps decides case.
            self.shift ^ self.caps
        } else {
            // Symbols/digits: caps has no effect.
            self.shift
        };
        Some(if upper { shifted } else { base })
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
mod tests {
    use super::*;

    fn down(km: &mut Keymap, scan: u8) -> Option<Key> {
        km.process(scan, false, true)
    }

    #[test]
    fn lowercase_by_default() {
        let mut km = Keymap::new();
        assert_eq!(down(&mut km, 0x1E), Some(Key::Char(b'a')));
        assert_eq!(down(&mut km, 0x32), Some(Key::Char(b'm')));
    }

    #[test]
    fn shift_uppercases_letters() {
        let mut km = Keymap::new();
        assert_eq!(km.process(0x2A, false, true), None); // shift down
        assert!(km.shift());
        assert_eq!(down(&mut km, 0x1E), Some(Key::Char(b'A')));
        assert_eq!(km.process(0x2A, false, false), None); // shift up
        assert!(!km.shift());
        assert_eq!(down(&mut km, 0x1E), Some(Key::Char(b'a')));
    }

    #[test]
    fn right_shift_also_works() {
        let mut km = Keymap::new();
        km.process(0x36, false, true);
        assert_eq!(down(&mut km, 0x1F), Some(Key::Char(b'S')));
    }

    #[test]
    fn caps_lock_toggles_letters_only() {
        let mut km = Keymap::new();
        km.process(0x3A, false, true); // caps on
        assert!(km.caps());
        assert_eq!(down(&mut km, 0x1E), Some(Key::Char(b'A')));
        // digit unaffected by caps
        assert_eq!(down(&mut km, 0x02), Some(Key::Char(b'1')));
        km.process(0x3A, false, true); // caps off
        assert!(!km.caps());
        assert_eq!(down(&mut km, 0x1E), Some(Key::Char(b'a')));
    }

    #[test]
    fn shift_plus_caps_is_lowercase_letter() {
        let mut km = Keymap::new();
        km.process(0x3A, false, true); // caps on
        km.process(0x2A, false, true); // shift on
        assert_eq!(down(&mut km, 0x1E), Some(Key::Char(b'a')));
    }

    #[test]
    fn shifted_symbols() {
        let mut km = Keymap::new();
        km.process(0x2A, false, true);
        assert_eq!(down(&mut km, 0x02), Some(Key::Char(b'!')));
        assert_eq!(down(&mut km, 0x0C), Some(Key::Char(b'_')));
        assert_eq!(down(&mut km, 0x35), Some(Key::Char(b'?')));
        assert_eq!(down(&mut km, 0x34), Some(Key::Char(b'>')));
    }

    #[test]
    fn control_keys() {
        let mut km = Keymap::new();
        assert_eq!(down(&mut km, 0x1C), Some(Key::Enter));
        assert_eq!(down(&mut km, 0x0E), Some(Key::Backspace));
        assert_eq!(down(&mut km, 0x0F), Some(Key::Tab));
        assert_eq!(down(&mut km, 0x01), Some(Key::Esc));
        assert_eq!(down(&mut km, 0x39), Some(Key::Char(b' ')));
    }

    #[test]
    fn extended_arrows_and_edit_keys() {
        let mut km = Keymap::new();
        assert_eq!(km.process(0x48, true, true), Some(Key::Up));
        assert_eq!(km.process(0x50, true, true), Some(Key::Down));
        assert_eq!(km.process(0x4B, true, true), Some(Key::Left));
        assert_eq!(km.process(0x4D, true, true), Some(Key::Right));
        assert_eq!(km.process(0x47, true, true), Some(Key::Home));
        assert_eq!(km.process(0x4F, true, true), Some(Key::End));
        assert_eq!(km.process(0x53, true, true), Some(Key::Delete));
    }

    #[test]
    fn key_up_produces_nothing() {
        let mut km = Keymap::new();
        assert_eq!(km.process(0x1E, false, false), None);
    }

    #[test]
    fn unmapped_scancode_is_none() {
        let mut km = Keymap::new();
        assert_eq!(down(&mut km, 0x7E), None);
        assert_eq!(km.process(0x99, true, true), None);
    }

    #[test]
    fn default_impl_matches_new() {
        let km = Keymap::default();
        assert!(!km.shift() && !km.caps());
    }

    const MODIFIERS: [u8; 3] = [0x2A, 0x36, 0x3A];

    #[test]
    fn whole_table_translates_both_cases() {
        // Exercise every mapped scancode unshifted and shifted (skipping the
        // modifier keys so caps/shift state stays controlled), asserting the
        // shifted form differs for everything except space.
        let mut shifted_km = Keymap::new();
        shifted_km.process(0x2A, false, true); // shift held

        for scan in 0x00u8..0x60 {
            if MODIFIERS.contains(&scan) {
                continue;
            }
            let unshifted = Keymap::new().process(scan, false, true);
            let shifted = shifted_km.process(scan, false, true);

            if let (Some(Key::Char(u)), Some(Key::Char(s))) = (unshifted, shifted) {
                assert!(u.is_ascii_graphic() || u == b' ');
                if u != b' ' {
                    assert_ne!(u, s, "scan {:#x} should change under shift", scan);
                }
            }
        }
    }
}
