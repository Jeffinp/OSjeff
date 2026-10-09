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
