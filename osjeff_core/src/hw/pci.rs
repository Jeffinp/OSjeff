//! PCI configuration-space decoding: addresses, BARs, capability-list start and
//! bus enumeration. The kernel supplies the port I/O; this module supplies the
//! arithmetic.

/// virtio PCI vendor id, and the modern/transitional virtio-gpu device ids.
pub const VIRTIO_VENDOR: u16 = 0x1AF4;
pub const VIRTIO_GPU_MODERN: u16 = 0x1050; // 0x1040 + virtio device type 16 (GPU)
pub const VIRTIO_GPU_LEGACY: u16 = 0x1010;

/// `true` for a modern or transitional virtio-gpu `vendor:device` pair.
pub fn is_virtio_gpu(vendor: u16, device: u16) -> bool {
    vendor == VIRTIO_VENDOR && (device == VIRTIO_GPU_MODERN || device == VIRTIO_GPU_LEGACY)
}

/// Value for the `0xCF8` CONFIG_ADDRESS port selecting a dword.
pub fn config_address(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    0x8000_0000
        | (bus as u32) << 16
        | (slot as u32) << 11
        | (func as u32) << 8
        | (offset as u32 & 0xFC)
}

/// The 16-bit word at byte `offset` out of the dword read at `offset & 0xFC`.
pub fn extract_u16(dword: u32, offset: u8) -> u16 {
    ((dword >> ((offset as u32 & 2) * 8)) & 0xFFFF) as u16
}

/// Status register bit 4: a capability list exists.
const STATUS_CAP_LIST: u16 = 0x10;

/// Offset of the first PCI capability, given the status register (offset 0x06)
/// and the capability-pointer word (offset 0x34). `None` when the device has no
/// list. The pointer's low two bits are reserved and masked off.
pub fn cap_list_start(status: u16, cap_ptr_word: u16) -> Option<u8> {
    if status & STATUS_CAP_LIST == 0 {
        return None;
    }
    Some((cap_ptr_word & 0xFC) as u8)
}

/// `true` when a BAR's low dword describes a 64-bit memory BAR (type bits
/// `0b10` in bits 2:1), whose high half lives in the next BAR slot.
pub fn bar_is_64bit(lo: u32) -> bool {
    lo & 0b110 == 0b100
}

/// Physical base address of a BAR from its low dword and (for 64-bit BARs) the
/// following dword; the four low flag bits are masked. `hi` is ignored for
/// 32-bit BARs.
pub fn bar_address(lo: u32, hi: u32) -> u64 {
    if bar_is_64bit(lo) {
        ((hi as u64) << 32) | (lo as u64 & !0xF)
    } else {
        lo as u64 & !0xF
    }
}

/// Walks bus 0: calls `f(slot, func, vendor, device)` for every present
/// function. `read16(slot, func, offset)` reads config space. A slot whose
/// function 0 is absent is skipped entirely; other absent functions are skipped
/// individually.
pub fn enumerate(mut read16: impl FnMut(u8, u8, u8) -> u16, mut f: impl FnMut(u8, u8, u16, u16)) {
    for slot in 0..32u8 {
        for func in 0..8u8 {
            let vendor = read16(slot, func, 0x00);
            if vendor == 0xFFFF {
                if func == 0 {
                    break; // no function 0 -> the whole slot is empty
                }
                continue;
            }
            let device = read16(slot, func, 0x02);
            f(slot, func, vendor, device);
        }
    }
}

#[cfg(test)]
mod tests {
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
    fn virtio_gpu_ids() {
        assert!(is_virtio_gpu(0x1AF4, 0x1050));
        assert!(is_virtio_gpu(0x1AF4, 0x1010));
        assert!(!is_virtio_gpu(0x1AF4, 0x1000)); // virtio-net transitional
        assert!(!is_virtio_gpu(0x8086, 0x1050)); // wrong vendor
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
}
