//! PS/2 byte-stream decoding: set-1 scancodes and 3- or 4-byte mouse packets.
//!
//! The kernel's IRQ handlers only push raw bytes into a ring; [`Decoder`] turns
//! that byte stream into [`Event`]s. It is a pair of tiny state machines (the
//! `0xE0` extended-key prefix and the mouse packet cycle).
//!
//! # Wheel mouse (IntelliMouse)
//!
//! A plain PS/2 mouse sends 3-byte packets (`flags, dx, dy`). The *IntelliMouse*
//! extension adds a 4th byte with the wheel delta `dz`. It is negotiated by
//! setting the sample rate to 200, 100 and 80 (the "magic" sequence,
//! [`WHEEL_MAGIC`]) and then reading the device id (`0xF2`): a mouse that took
//! the sequence answers `3`, any other answers `0` (and must keep sending
//! 3-byte packets). [`MouseMode::from_id`] maps the answer and
//! [`Decoder::set_mode`] switches the packet size. The kernel does the port
//! I/O; everything that can be decided without hardware is here.
//!
//! Sign convention: a positive `dz` is "wheel toward the user" (scroll down),
//! as QEMU and real mice report it.

/// The sample-rate sequence that unlocks the wheel (`0xF3 <rate>` for each).
pub const WHEEL_MAGIC: [u8; 3] = [200, 100, 80];

/// Largest wheel magnitude a packet reports (the byte is a signed 8-bit value;
/// `-128` saturates to `-127` so the range is symmetric).
pub const WHEEL_MAX: i32 = 127;

/// Largest horizontal / vertical magnitude when the hardware flags an overflow.
pub const MOTION_MAX: i32 = 255;

/// How the attached mouse frames its packets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseMode {
    /// 3-byte packets (device id 0, or anything not understood).
    #[default]
    Basic,
    /// 4-byte IntelliMouse packets with the wheel axis (device id 3).
    Wheel,
}

impl MouseMode {
    /// The mode for the answer to "get device id" after the magic sequence.
    /// `None` (no answer in time) and every id other than 3 mean [`Basic`]:
    /// id 4 (Explorer, 5 buttons) is not negotiated, and a mouse that kept
    /// answering 0 simply has no wheel.
    ///
    /// [`Basic`]: MouseMode::Basic
    pub fn from_id(id: Option<u8>) -> Self {
        match id {
            Some(3) => MouseMode::Wheel,
            _ => MouseMode::Basic,
        }
    }

    /// Bytes per packet in this mode.
    pub const fn packet_len(self) -> u8 {
        match self {
            MouseMode::Basic => 3,
            MouseMode::Wheel => 4,
        }
    }
}

/// A decoded mouse packet (relative motion, wheel and button state).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Packet {
    pub dx: i32,
    pub dy: i32,
    /// Wheel delta, positive = toward the user (scroll down). Always `0` for a
    /// mouse in [`MouseMode::Basic`]. Within `-WHEEL_MAX..=WHEEL_MAX`.
    pub dz: i32,
    pub left: bool,
    pub right: bool,
    pub middle: bool,
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

/// Decode one motion byte. With the overflow flag the 9-bit value cannot be
/// trusted, so it saturates in the direction of its sign.
fn motion(raw: u8, negative: bool, overflow: bool) -> i32 {
    if overflow {
        return if negative { -MOTION_MAX } else { MOTION_MAX };
    }
    let v = raw as i32;
    if negative { v - 256 } else { v }
}

/// Decode the 4th byte of an IntelliMouse (id 3) packet: a signed 8-bit
/// delta, `-128` saturating to `-127`.
pub fn wheel_delta(byte: u8) -> i32 {
    (byte as i8 as i32).max(-WHEEL_MAX)
}

/// Incremental decoder state for the scancode / mouse-packet state machines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Decoder {
    mouse_cycle: u8,
    mouse_buf: [u8; 4],
    mode: MouseMode,
    key_extended: bool,
}

impl Decoder {
    pub const fn new() -> Self {
        Self {
            mouse_cycle: 0,
            mouse_buf: [0; 4],
            mode: MouseMode::Basic,
            key_extended: false,
        }
    }

    /// The current mouse framing.
    pub const fn mode(&self) -> MouseMode {
        self.mode
    }

    /// Switch the packet framing (after the wheel negotiation). A packet in
    /// progress is dropped: its bytes belong to the old framing.
    pub fn set_mode(&mut self, mode: MouseMode) {
        self.mode = mode;
        self.mouse_cycle = 0;
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

    /// Feed one mouse byte. Emits a [`Packet`] on the last byte of each packet
    /// (the third, or the fourth in [`MouseMode::Wheel`]). A first byte with
    /// bit 3 clear is treated as out of sync and dropped, so a lost byte costs
    /// at most the packet it belonged to.
    pub fn mouse(&mut self, data: u8) -> Option<Event> {
        let len = self.mode.packet_len();
        if self.mouse_cycle == 0 {
            if data & 0x08 == 0 {
                return None; // out of sync; bit3 of byte0 is always 1
            }
            self.mouse_buf[0] = data;
            self.mouse_cycle = 1;
            return None;
        }
        self.mouse_buf[self.mouse_cycle as usize] = data;
        self.mouse_cycle += 1;
        if self.mouse_cycle < len {
            return None;
        }
        self.mouse_cycle = 0;
        let flags = self.mouse_buf[0];
        let dz = if self.mode == MouseMode::Wheel {
            wheel_delta(self.mouse_buf[3])
        } else {
            0
        };
        Some(Event::Mouse(Packet {
            dx: motion(self.mouse_buf[1], flags & 0x10 != 0, flags & 0x40 != 0),
            dy: motion(self.mouse_buf[2], flags & 0x20 != 0, flags & 0x80 != 0),
            dz,
            left: flags & 0x01 != 0,
            right: flags & 0x02 != 0,
            middle: flags & 0x04 != 0,
        }))
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
            ..Packet::default()
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

    // ---- wheel (IntelliMouse) ----

    fn wheel_decoder() -> Decoder {
        let mut d = Decoder::new();
        d.set_mode(MouseMode::Wheel);
        d
    }

    fn mouse4(d: &mut Decoder, a: u8, b: u8, c: u8, z: u8) -> Option<Event> {
        assert_eq!(d.mouse(a), None);
        assert_eq!(d.mouse(b), None);
        assert_eq!(d.mouse(c), None);
        d.mouse(z)
    }

    fn dz_of(e: Option<Event>) -> i32 {
        match e {
            Some(Event::Mouse(p)) => p.dz,
            other => panic!("expected a mouse packet, got {other:?}"),
        }
    }

    #[test]
    fn magic_sequence_is_200_100_80() {
        assert_eq!(WHEEL_MAGIC, [200, 100, 80]);
    }

    #[test]
    fn device_id_3_selects_the_wheel_mode() {
        assert_eq!(MouseMode::from_id(Some(3)), MouseMode::Wheel);
    }

    #[test]
    fn device_id_0_keeps_three_byte_packets() {
        assert_eq!(MouseMode::from_id(Some(0)), MouseMode::Basic);
    }

    #[test]
    fn unknown_or_missing_device_id_falls_back() {
        for id in [Some(1), Some(2), Some(4), Some(0xFA), Some(0xFF), None] {
            assert_eq!(MouseMode::from_id(id), MouseMode::Basic, "{id:?}");
        }
    }

    #[test]
    fn packet_lengths() {
        assert_eq!(MouseMode::Basic.packet_len(), 3);
        assert_eq!(MouseMode::Wheel.packet_len(), 4);
        assert_eq!(MouseMode::default(), MouseMode::Basic);
    }

    #[test]
    fn four_byte_packet_carries_motion_buttons_and_wheel() {
        let mut d = wheel_decoder();
        let e = mouse4(&mut d, 0x08 | 0x01, 5, 7, 1);
        assert_eq!(
            e,
            Some(Event::Mouse(Packet {
                dx: 5,
                dy: 7,
                dz: 1,
                left: true,
                right: false,
                middle: false,
            }))
        );
    }

    #[test]
    fn wheel_up_is_negative_and_down_positive() {
        let mut d = wheel_decoder();
        assert_eq!(dz_of(mouse4(&mut d, 0x08, 0, 0, 0xFF)), -1);
        assert_eq!(dz_of(mouse4(&mut d, 0x08, 0, 0, 0x01)), 1);
    }

    #[test]
    fn wheel_delta_is_a_signed_byte() {
        assert_eq!(wheel_delta(0), 0);
        assert_eq!(wheel_delta(1), 1);
        assert_eq!(wheel_delta(127), 127);
        assert_eq!(wheel_delta(0xFF), -1);
        assert_eq!(wheel_delta(0xF8), -8);
        assert_eq!(wheel_delta(0x81), -127);
    }

    #[test]
    fn wheel_delta_saturates_the_most_negative_value() {
        assert_eq!(wheel_delta(0x80), -127);
        let mut d = wheel_decoder();
        assert_eq!(dz_of(mouse4(&mut d, 0x08, 0, 0, 0x80)), -WHEEL_MAX);
    }

    #[test]
    fn wheel_delta_stays_symmetric_over_every_byte() {
        for b in 0..=255u8 {
            let v = wheel_delta(b);
            assert!((-WHEEL_MAX..=WHEEL_MAX).contains(&v), "{b:#x} -> {v}");
        }
    }

    #[test]
    fn three_byte_mode_never_reports_a_wheel() {
        let mut d = Decoder::new();
        assert_eq!(dz_of(mouse3(&mut d, 0x08, 1, 1)), 0);
        assert_eq!(d.mode(), MouseMode::Basic);
    }

    #[test]
    fn middle_button_is_bit_two() {
        let mut d = wheel_decoder();
        match mouse4(&mut d, 0x08 | 0x04, 0, 0, 0) {
            Some(Event::Mouse(p)) => assert!(p.middle && !p.left && !p.right),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn all_three_buttons_together() {
        let mut d = wheel_decoder();
        match mouse4(&mut d, 0x08 | 0x07, 0, 0, 0) {
            Some(Event::Mouse(p)) => assert!(p.left && p.right && p.middle),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn four_byte_packet_does_not_emit_after_three_bytes() {
        let mut d = wheel_decoder();
        assert_eq!(d.mouse(0x08), None);
        assert_eq!(d.mouse(1), None);
        assert_eq!(d.mouse(2), None);
        assert!(d.mouse(3).is_some());
    }

    #[test]
    fn wheel_mode_resyncs_on_a_first_byte_without_bit3() {
        let mut d = wheel_decoder();
        assert_eq!(d.mouse(0x00), None);
        assert_eq!(d.mouse(0x37), None);
        assert_eq!(dz_of(mouse4(&mut d, 0x08, 1, 1, 0xFE)), -2);
    }

    #[test]
    fn wheel_byte_is_not_checked_for_bit3() {
        // The 4th byte is data: a value without bit 3 is a valid delta, not a
        // resync trigger.
        let mut d = wheel_decoder();
        assert_eq!(dz_of(mouse4(&mut d, 0x08, 0, 0, 0x01)), 1);
        assert_eq!(dz_of(mouse4(&mut d, 0x08, 0, 0, 0x02)), 2);
    }

    #[test]
    fn switching_the_mode_drops_a_packet_in_progress() {
        let mut d = Decoder::new();
        assert_eq!(d.mouse(0x08), None);
        assert_eq!(d.mouse(5), None);
        d.set_mode(MouseMode::Wheel);
        // The old bytes are gone: a fresh 4-byte packet decodes cleanly.
        assert_eq!(dz_of(mouse4(&mut d, 0x08, 1, 1, 3)), 3);
    }

    #[test]
    fn switching_back_to_basic_restores_three_byte_framing() {
        let mut d = wheel_decoder();
        d.set_mode(MouseMode::Basic);
        assert_eq!(mouse3(&mut d, 0x09, 2, 3), pkt(2, 3, true, false));
    }

    #[test]
    fn overflow_saturates_motion_with_its_sign() {
        let mut d = Decoder::new();
        // X overflow, positive; Y overflow with the Y sign bit set.
        let e = mouse3(&mut d, 0x08 | 0x40 | 0x80 | 0x20, 0x12, 0x34);
        assert_eq!(e, pkt(MOTION_MAX, -MOTION_MAX, false, false));
    }

    #[test]
    fn overflow_x_negative() {
        let mut d = Decoder::new();
        let e = mouse3(&mut d, 0x08 | 0x40 | 0x10, 0x00, 0x05);
        assert_eq!(e, pkt(-MOTION_MAX, 5, false, false));
    }

    #[test]
    fn motion_and_wheel_decode_together() {
        let mut d = wheel_decoder();
        let e = mouse4(&mut d, 0x08 | 0x10 | 0x20, 0xFE, 0xFD, 0xFC);
        assert_eq!(
            e,
            Some(Event::Mouse(Packet {
                dx: -2,
                dy: -3,
                dz: -4,
                ..Packet::default()
            }))
        );
    }

    #[test]
    fn long_wheel_stream_decodes_every_packet() {
        let mut d = wheel_decoder();
        let mut n = 0;
        let mut sum = 0;
        for i in 0..300u32 {
            for b in [0x08u8, (i & 0x7F) as u8, 1, 1] {
                if let Some(Event::Mouse(p)) = d.mouse(b) {
                    n += 1;
                    sum += p.dz;
                }
            }
        }
        assert_eq!((n, sum), (300, 300));
    }

    #[test]
    fn keyboard_bytes_do_not_disturb_a_wheel_packet() {
        let mut d = wheel_decoder();
        assert_eq!(d.mouse(0x08), None);
        assert_eq!(d.keyboard(0x1E), key(0x1E, true, false));
        assert_eq!(d.mouse(1), None);
        assert_eq!(d.mouse(1), None);
        assert_eq!(dz_of(d.mouse(0xFF)), -1);
    }

    #[test]
    fn a_dropped_byte_costs_one_packet_then_resyncs() {
        let mut d = wheel_decoder();
        // Packet 1 loses its last byte; the next packet's first byte (0x09 has
        // bit 3) is then taken as payload, so one packet is garbage, and the
        // stream recovers at the next first byte lacking... bit 3 is always set
        // in a flags byte, so recovery needs a payload byte without it.
        let mut out = std::vec::Vec::new();
        let stream = [
            0x08u8, 1, 1, /* lost 4th byte */
            0x08, 2, 2, 0, 0x08, 3, 3, 0, 0x08, 4, 4, 0,
        ];
        for b in stream {
            if let Some(Event::Mouse(p)) = d.mouse(b) {
                out.push(p);
            }
        }
        // 15 bytes at 4 per packet: 3 packets, none of them panicked or stuck.
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn default_decoder_is_basic_and_clean() {
        let d = Decoder::default();
        assert_eq!(d, Decoder::new());
        assert_eq!(d.mode(), MouseMode::Basic);
    }

    #[test]
    fn packet_default_is_all_zero() {
        let p = Packet::default();
        assert_eq!((p.dx, p.dy, p.dz), (0, 0, 0));
        assert!(!p.left && !p.right && !p.middle);
    }
}
