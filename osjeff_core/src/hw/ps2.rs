//! PS/2 byte-stream decoding: set-1 scancodes and 3-byte mouse packets.
//!
//! The kernel's IRQ handlers only push raw bytes into a ring; [`Decoder`] turns
//! that byte stream into [`Event`]s. It is a pair of tiny state machines (the
//! `0xE0` extended-key prefix and the 3-byte mouse packet cycle).

/// A decoded mouse packet (relative motion + button state).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet {
    pub dx: i32,
    pub dy: i32,
    pub left: bool,
    pub right: bool,
}

/// A decoded key press or release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub scan_code: u8,
    pub pressed: bool,
    /// Set when the scancode followed an `0xE0` prefix (arrows, Del, etc.).
    pub extended: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Mouse(Packet),
    Key(KeyEvent),
}

/// Incremental decoder state for the scancode / mouse-packet state machines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Decoder {
    mouse_cycle: u8,
    mouse_buf: [u8; 3],
    key_extended: bool,
}

impl Decoder {
    pub const fn new() -> Self {
        Self {
            mouse_cycle: 0,
            mouse_buf: [0; 3],
            key_extended: false,
        }
    }

    /// Feed one keyboard byte. `None` for a prefix byte (`0xE0`); the real
    /// scancode arrives next and picks up the `extended` flag.
    pub fn keyboard(&mut self, data: u8) -> Option<Event> {
        if data == 0xE0 {
            self.key_extended = true;
            return None;
        }
        let extended = self.key_extended;
        self.key_extended = false;
        Some(Event::Key(KeyEvent {
            scan_code: data & 0x7F,
            pressed: data & 0x80 == 0,
            extended,
        }))
    }

    /// Feed one mouse byte. Emits a [`Packet`] on every third byte; a first
    /// byte with bit 3 clear is treated as out of sync and dropped.
    pub fn mouse(&mut self, data: u8) -> Option<Event> {
        match self.mouse_cycle {
            0 => {
                if data & 0x08 == 0 {
                    return None; // out of sync; bit3 of byte0 is always 1
                }
                self.mouse_buf[0] = data;
                self.mouse_cycle = 1;
                None
            }
            1 => {
                self.mouse_buf[1] = data;
                self.mouse_cycle = 2;
                None
            }
            _ => {
                self.mouse_buf[2] = data;
                self.mouse_cycle = 0;
                let flags = self.mouse_buf[0];
                let mut dx = self.mouse_buf[1] as i32;
                let mut dy = self.mouse_buf[2] as i32;
                if flags & 0x10 != 0 {
                    dx -= 256;
                }
                if flags & 0x20 != 0 {
                    dy -= 256;
                }
                Some(Event::Mouse(Packet {
                    dx,
                    dy,
                    left: flags & 0x01 != 0,
                    right: flags & 0x02 != 0,
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(sc: u8, pressed: bool, extended: bool) -> Option<Event> {
        Some(Event::Key(KeyEvent {
            scan_code: sc,
            pressed,
            extended,
        }))
    }

    fn pkt(dx: i32, dy: i32, left: bool, right: bool) -> Option<Event> {
        Some(Event::Mouse(Packet {
            dx,
            dy,
            left,
            right,
        }))
    }

    fn mouse3(d: &mut Decoder, a: u8, b: u8, c: u8) -> Option<Event> {
        assert_eq!(d.mouse(a), None);
        assert_eq!(d.mouse(b), None);
        d.mouse(c)
    }

    #[test]
    fn make_and_break_codes() {
        let mut d = Decoder::new();
        assert_eq!(d.keyboard(0x1E), key(0x1E, true, false)); // 'A' down
        assert_eq!(d.keyboard(0x9E), key(0x1E, false, false)); // 'A' up
    }

    #[test]
    fn extended_prefix_applies_to_next_byte_only() {
        let mut d = Decoder::new();
        assert_eq!(d.keyboard(0xE0), None);
        assert_eq!(d.keyboard(0x48), key(0x48, true, true)); // Up arrow
        assert_eq!(d.keyboard(0x48), key(0x48, true, false)); // flag cleared
    }

    #[test]
    fn extended_release() {
        let mut d = Decoder::new();
        assert_eq!(d.keyboard(0xE0), None);
        assert_eq!(d.keyboard(0xC8), key(0x48, false, true));
    }

    #[test]
    fn double_prefix_is_idempotent() {
        let mut d = Decoder::new();
        assert_eq!(d.keyboard(0xE0), None);
        assert_eq!(d.keyboard(0xE0), None);
        assert_eq!(d.keyboard(0x4B), key(0x4B, true, true));
    }

    #[test]
    fn boundary_scancodes() {
        let mut d = Decoder::new();
        assert_eq!(d.keyboard(0x00), key(0, true, false));
        assert_eq!(d.keyboard(0x7F), key(0x7F, true, false));
        assert_eq!(d.keyboard(0x80), key(0, false, false));
        assert_eq!(d.keyboard(0xFF), key(0x7F, false, false));
    }

    #[test]
    fn mouse_packet_positive_motion_and_buttons() {
        let mut d = Decoder::new();
        assert_eq!(mouse3(&mut d, 0x08 | 0x01, 5, 7), pkt(5, 7, true, false));
        assert_eq!(mouse3(&mut d, 0x08 | 0x02, 0, 0), pkt(0, 0, false, true));
        assert_eq!(mouse3(&mut d, 0x08 | 0x03, 1, 1), pkt(1, 1, true, true));
    }

    #[test]
    fn mouse_packet_sign_bits_extend_to_negative() {
        let mut d = Decoder::new();
        // X sign (0x10) and Y sign (0x20): raw 0xFF = -1, 0x80 = -128.
        let e = mouse3(&mut d, 0x08 | 0x10 | 0x20, 0xFF, 0x80);
        assert_eq!(e, pkt(-1, -128, false, false));
    }

    #[test]
    fn mouse_sign_bits_are_independent() {
        let mut d = Decoder::new();
        let e = mouse3(&mut d, 0x08 | 0x10, 0xFE, 0xFE);
        assert_eq!(e, pkt(-2, 254, false, false)); // only X is negative
    }

    #[test]
    fn mouse_resyncs_when_first_byte_lacks_bit3() {
        let mut d = Decoder::new();
        // Garbage bytes with bit 3 clear are dropped without advancing the cycle.
        assert_eq!(d.mouse(0x00), None);
        assert_eq!(d.mouse(0x07), None);
        assert_eq!(d.mouse(0xF7), None);
        assert_eq!(mouse3(&mut d, 0x08, 1, 2), pkt(1, 2, false, false));
    }

    #[test]
    fn mouse_payload_bytes_are_not_checked_for_bit3() {
        let mut d = Decoder::new();
        assert!(mouse3(&mut d, 0x08, 0x00, 0x00).is_some());
    }

    #[test]
    fn keyboard_and_mouse_state_do_not_interfere() {
        let mut d = Decoder::new();
        assert_eq!(d.keyboard(0xE0), None);
        assert_eq!(d.mouse(0x09), None); // starts a packet
        assert_eq!(d.keyboard(0x1C), key(0x1C, true, true)); // prefix still pending
        assert_eq!(d.mouse(3), None);
        assert!(matches!(d.mouse(4), Some(Event::Mouse(_))));
    }

    #[test]
    fn long_stream_decodes_every_packet() {
        let mut d = Decoder::new();
        let mut n = 0;
        for i in 0..300u32 {
            for b in [0x08u8, (i & 0x7F) as u8, 1] {
                if d.mouse(b).is_some() {
                    n += 1;
                }
            }
        }
        assert_eq!(n, 300);
    }
}
