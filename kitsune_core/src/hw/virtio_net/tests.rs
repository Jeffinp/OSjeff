use super::*;

#[test]
fn header_round_trips_little_endian() {
    let h = NetHdr {
        flags: 1,
        gso_type: 3,
        hdr_len: 0x0102,
        gso_size: 0x0304,
        csum_start: 0x0506,
        csum_offset: 0x0708,
        num_buffers: 0x090A,
    };
    let mut b = [0xEEu8; 16];
    assert!(h.encode(&mut b));
    assert_eq!(&b[..12], &[1, 3, 2, 1, 4, 3, 6, 5, 8, 7, 10, 9]);
    assert_eq!(&b[12..], &[0xEE; 4], "nothing written past the header");
    assert_eq!(NetHdr::decode(&b), Some(h));
    assert!(!h.encode(&mut b[..11]));
    assert_eq!(NetHdr::decode(&b[..11]), None);
}

#[test]
fn plain_header_is_all_zero() {
    let mut b = [0xFFu8; 12];
    assert!(NetHdr::plain().encode(&mut b));
    assert_eq!(b, [0; 12]);
}

#[test]
fn layout_of_a_16_entry_queue() {
    let l = QueueLayout::new(16).unwrap();
    assert_eq!((l.desc, l.avail), (0, 256));
    // avail: flags + idx + 16 entries + used_event = 38 bytes -> 294, used at 296.
    assert_eq!(l.used, 296);
    assert_eq!(l.total, 296 + 4 + 8 * 16 + 2);
    assert!(l.total <= 4096, "one page per queue");
    assert_eq!(l.desc_off(3), 48);
    assert_eq!(l.avail_idx_off(), 258);
    assert_eq!(l.avail_ring_off(2), 256 + 4 + 4);
    assert_eq!(l.used_idx_off(), 298);
    assert_eq!(l.used_elem_off(1), 296 + 4 + 8);
    // Offsets wrap modulo the size, like ring slots do.
    assert_eq!(l.desc_off(16), l.desc_off(0));
    assert_eq!(l.avail_ring_off(17), l.avail_ring_off(1));
}

#[test]
fn layout_alignment_and_no_overlap_for_every_size() {
    for shift in 0..=15u32 {
        let q = 1u16 << shift;
        let l = QueueLayout::new(q).unwrap();
        let n = usize::from(q);
        assert_eq!(l.desc % 16, 0);
        assert_eq!(l.avail % 2, 0);
        assert_eq!(l.used % 4, 0);
        let desc_end = l.desc + 16 * n;
        let avail_end = l.avail + 4 + 2 * n + 2;
        let used_end = l.used + 4 + 8 * n + 2;
        assert!(desc_end <= l.avail && avail_end <= l.used, "q={q}");
        assert_eq!(l.total, used_end);
        // Last elements stay inside their parts.
        assert!(l.desc_off(q - 1) + 16 <= desc_end);
        assert!(l.avail_ring_off(q - 1) + 2 <= avail_end);
        assert!(l.used_elem_off(q - 1) + 8 <= used_end);
    }
}

#[test]
fn layout_rejects_bad_sizes() {
    for bad in [0u16, 3, 6, 100, 1000, 0x8001, 0xFFFF] {
        assert_eq!(QueueLayout::new(bad), None, "{bad}");
    }
    assert!(QueueLayout::new(32768).is_some());
    assert!(QueueLayout::new(1).is_some());
}

#[test]
fn descriptor_encoding() {
    let d = encode_desc(0x1122_3344_5566_7788, 0x0A0B_0C0D, DESC_F_WRITE, 5);
    assert_eq!(&d[0..8], &0x1122_3344_5566_7788u64.to_le_bytes());
    assert_eq!(&d[8..12], &0x0A0B_0C0Du32.to_le_bytes());
    assert_eq!(&d[12..14], &[2, 0]);
    assert_eq!(&d[14..16], &[5, 0]);
}

#[test]
fn pending_completions_wrap_correctly() {
    assert_eq!(used_pending(0, 0, 16), Ok(0));
    assert_eq!(used_pending(5, 2, 16), Ok(3));
    // 16-bit wrap: device index wrapped past 0xFFFF, ours has not.
    assert_eq!(used_pending(3, 0xFFFE, 16), Ok(5));
    assert_eq!(used_pending(16, 0, 16), Ok(16), "all buffers returned");
    // More completions than the queue holds: a broken device.
    assert_eq!(used_pending(17, 0, 16), Err(RingCorrupt));
    assert_eq!(
        used_pending(0, 1, 16),
        Err(RingCorrupt),
        "index went backwards"
    );
}

#[test]
fn used_elements_are_validated() {
    assert_eq!(decode_used(0, 60, 16), Some((0, 60)));
    assert_eq!(decode_used(15, 1514, 16), Some((15, 1514)));
    assert_eq!(decode_used(16, 0, 16), None);
    assert_eq!(decode_used(0x1_0000, 0, 16), None, "id does not fit u16");
    assert_eq!(decode_used(u32::MAX, 0, 16), None);
}

#[test]
fn rx_frame_extraction() {
    // Runts: anything under header + 14 bytes.
    for n in [0u32, 1, 11, 12, 25] {
        assert_eq!(rx_frame(n, 1600), Rx::Runt, "{n}");
    }
    assert_eq!(
        rx_frame(26, 1600),
        Rx::Frame {
            copy: 14,
            truncated: false
        }
    );
    // A typical ARP/ping frame.
    assert_eq!(
        rx_frame(12 + 60, 1600),
        Rx::Frame {
            copy: 60,
            truncated: false
        }
    );
    // Caller's buffer too small: truncated copy.
    assert_eq!(
        rx_frame(12 + 1514, 1000),
        Rx::Frame {
            copy: 1000,
            truncated: true
        }
    );
    // A device claiming more than the buffer holds is clamped to the buffer.
    assert_eq!(
        rx_frame(u32::MAX, 4096),
        Rx::Frame {
            copy: BUF_LEN - HDR_LEN,
            truncated: false
        }
    );
}

#[test]
fn tx_total_pads_and_limits() {
    assert_eq!(tx_total(0), None);
    assert_eq!(tx_total(1), Some(12 + 60));
    assert_eq!(tx_total(42), Some(12 + 60), "ARP is padded");
    assert_eq!(tx_total(60), Some(72));
    assert_eq!(tx_total(1514), Some(12 + 1514));
    assert_eq!(tx_total(1515), None);
    assert!(tx_total(MAX_FRAME).unwrap() <= BUF_LEN);
}

#[test]
fn slot_set_allocates_lowest_and_detects_misuse() {
    let mut s = SlotSet::new(3);
    assert_eq!(
        (s.alloc(), s.alloc(), s.alloc()),
        (Some(0), Some(1), Some(2))
    );
    assert_eq!(s.alloc(), None, "full");
    assert_eq!(s.in_use(), 3);
    assert!(s.free(1));
    assert!(!s.free(1), "double free");
    assert!(!s.free(7), "out of range");
    assert!(s.is_used(0) && !s.is_used(1) && !s.is_used(9));
    assert_eq!(s.alloc(), Some(1), "reuses the lowest hole");
    // The widest set.
    let mut w = SlotSet::new(100);
    for i in 0..32 {
        assert_eq!(w.alloc(), Some(i));
    }
    assert_eq!(w.alloc(), None);
}

#[test]
fn mac_comes_from_config_only_when_negotiated_and_sane() {
    let cfg = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56, 1, 0];
    assert_eq!(
        config_mac(F_MAC, &cfg),
        Some(Mac([0x52, 0x54, 0, 0x12, 0x34, 0x56]))
    );
    assert_eq!(config_mac(0, &cfg), None, "feature not negotiated");
    assert_eq!(config_mac(F_MAC, &[0; 8]), None, "all zero");
    assert_eq!(config_mac(F_MAC, &[0xFF; 8]), None, "broadcast");
    assert_eq!(
        config_mac(F_MAC, &[0x01, 0, 0x5E, 0, 0, 1]),
        None,
        "multicast"
    );
    assert_eq!(config_mac(F_MAC, &cfg[..5]), None, "short config window");
}

#[test]
fn link_state() {
    assert!(link_up(F_STATUS, 1));
    assert!(!link_up(F_STATUS, 0));
    assert!(
        !link_up(F_STATUS | F_MAC, 2),
        "other status bits are not link"
    );
    assert!(link_up(0, 0), "no status feature: assume up");
}

#[test]
fn wanted_features_are_just_mac_and_status() {
    assert_eq!(WANTED_FEATURES_LO, (1 << 5) | (1 << 16));
}
