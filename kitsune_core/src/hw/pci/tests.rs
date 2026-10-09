use super::*;

#[test]
fn config_address_layout() {
    assert_eq!(config_address(0, 0, 0, 0), 0x8000_0000);
    assert_eq!(
        config_address(1, 2, 3, 0x10),
        0x8000_0000 | 1 << 16 | 2 << 11 | 3 << 8 | 0x10
    );
    assert_eq!(config_address(255, 31, 7, 0xFC), 0x80FF_FFFC);
}

#[test]
fn config_address_masks_the_low_offset_bits() {
    assert_eq!(config_address(0, 0, 0, 0x13), config_address(0, 0, 0, 0x10));
    assert_eq!(config_address(0, 0, 0, 0xFF), config_address(0, 0, 0, 0xFC));
}

#[test]
fn extract_u16_picks_the_right_half() {
    let d = 0xAABB_CCDD;
    assert_eq!(extract_u16(d, 0x00), 0xCCDD);
    assert_eq!(extract_u16(d, 0x01), 0xCCDD); // bit 1 clear: low half
    assert_eq!(extract_u16(d, 0x02), 0xAABB);
    assert_eq!(extract_u16(d, 0x03), 0xAABB);
    assert_eq!(extract_u16(d, 0x06), 0xAABB);
}

#[test]
fn cap_list_requires_status_bit_4() {
    assert_eq!(cap_list_start(0x0000, 0x0040), None);
    assert_eq!(cap_list_start(0xFFEF, 0x0040), None); // every bit but bit 4
    assert_eq!(cap_list_start(0x0010, 0x0040), Some(0x40));
}

#[test]
fn cap_pointer_reserved_bits_and_high_byte_are_dropped() {
    assert_eq!(cap_list_start(0x10, 0x0043), Some(0x40));
    assert_eq!(cap_list_start(0x10, 0xFF9F), Some(0x9C));
    assert_eq!(cap_list_start(0x10, 0x0000), Some(0)); // zero = empty list, caller stops
}

#[test]
fn bar_types() {
    assert!(!bar_is_64bit(0xFEBC_0000)); // 32-bit memory
    assert!(bar_is_64bit(0xFE00_000C)); // 64-bit prefetchable
    assert!(bar_is_64bit(0x0000_0004));
    assert!(!bar_is_64bit(0x0000_C001)); // I/O BAR
    assert!(!bar_is_64bit(0x0000_0002)); // reserved type 01
    assert!(!bar_is_64bit(0x0000_0006)); // reserved type 11
}

#[test]
fn bar_address_32_and_64_bit() {
    assert_eq!(bar_address(0xFEBC_000F, 0xDEAD_BEEF), 0xFEBC_0000);
    assert_eq!(bar_address(0xFE00_000C, 0x0000_0001), 0x1_FE00_0000);
    assert_eq!(bar_address(0x0000_0004, 0xFFFF_FFFF), 0xFFFF_FFFF_0000_0000);
    assert_eq!(bar_address(0, 0), 0);
}

#[test]
fn bar_offsets_cover_exactly_six_registers() {
    assert_eq!(bar_offset(0), Some(0x10));
    assert_eq!(bar_offset(5), Some(0x24));
    assert_eq!(bar_offset(6), None);
    assert_eq!(bar_offset(255), None); // would have overflowed `0x10 + i * 4`
}

#[test]
fn memory_bar_base_accepts_mmio_bars() {
    assert_eq!(memory_bar_base(0, 0xFEBC_0000, 0), Some(0xFEBC_0000));
    assert_eq!(memory_bar_base(4, 0xFE00_000C, 1), Some(0x1_FE00_0000));
    assert_eq!(memory_bar_base(0, 0xFEBC_0008, 0xFFFF), Some(0xFEBC_0000)); // 32-bit: hi ignored
}

#[test]
fn memory_bar_base_rejects_unusable_bars() {
    assert_eq!(memory_bar_base(6, 0xFEBC_0000, 0), None); // no such BAR
    assert_eq!(memory_bar_base(0, 0x0000_C001, 0), None); // I/O space
    assert_eq!(memory_bar_base(0, 0, 0), None); // unassigned
    assert_eq!(memory_bar_base(0, 0x0000_000F, 0), None); // flags only
    assert_eq!(memory_bar_base(5, 0xFE00_000C, 1), None); // 64-bit in the last slot
    assert_eq!(memory_bar_base(0, 0x0000_0004, 0), None); // 64-bit, base 0
}

#[test]
fn virtio_gpu_ids() {
    assert!(is_virtio_gpu(0x1AF4, 0x1050));
    assert!(is_virtio_gpu(0x1AF4, 0x1010));
    assert!(!is_virtio_gpu(0x1AF4, 0x1000)); // virtio-net transitional
    assert!(!is_virtio_gpu(0x8086, 0x1050)); // wrong vendor
    assert!(is_virtio_net(0x1AF4, 0x1000));
    assert!(is_virtio_net(0x1AF4, 0x1041));
    assert!(!is_virtio_net(0x1AF4, 0x1050)); // virtio-gpu
    assert!(!is_virtio_net(0x10EC, 0x1000)); // wrong vendor
}

#[test]
fn virtio_rng_ids() {
    assert!(is_virtio_rng(0x1AF4, 0x1005)); // QEMU's default virtio-rng-pci (transitional)
    assert!(is_virtio_rng(0x1AF4, 0x1044)); // modern-only (disable-legacy=on)
    assert!(!is_virtio_rng(0x1AF4, 0x1000)); // virtio-net
    assert!(!is_virtio_rng(0x1AF4, 0x1045)); // virtio-balloon
    assert!(!is_virtio_rng(0x8086, 0x1005)); // wrong vendor
}

#[test]
fn enumerate_visits_present_functions_only() {
    // Slot 3 has functions 0 and 2 (1 absent); slot 5 only function 0.
    let present = |slot: u8, func: u8| matches!((slot, func), (3, 0) | (3, 2) | (5, 0));
    let mut seen = Vec::new();
    enumerate(
        |s, f, off| {
            if !present(s, f) {
                0xFFFF
            } else if off == 0 {
                0x1234
            } else {
                s as u16 * 16 + f as u16
            }
        },
        |s, f, v, d| seen.push((s, f, v, d)),
    );
    assert_eq!(
        seen,
        vec![(3, 0, 0x1234, 48), (3, 2, 0x1234, 50), (5, 0, 0x1234, 80)]
    );
}

#[test]
fn enumerate_skips_a_slot_when_function_0_is_missing() {
    // Function 1 "exists" but function 0 does not: the slot is not scanned.
    let mut seen = 0;
    enumerate(
        |_, f, _| if f == 1 { 0x8086 } else { 0xFFFF },
        |_, _, _, _| seen += 1,
    );
    assert_eq!(seen, 0);
}

#[test]
fn enumerate_on_an_empty_bus_reads_each_slot_once() {
    let mut reads = 0;
    enumerate(
        |_, _, _| {
            reads += 1;
            0xFFFF
        },
        |_, _, _, _| panic!("no devices"),
    );
    assert_eq!(reads, 32);
}

#[test]
fn enumerate_full_slot_visits_all_eight_functions() {
    let mut funcs = Vec::new();
    enumerate(
        |s, _, _| if s == 0 { 0x1111 } else { 0xFFFF },
        |s, f, _, _| {
            if s == 0 {
                funcs.push(f)
            }
        },
    );
    assert_eq!(funcs, (0..8).collect::<Vec<u8>>());
}
