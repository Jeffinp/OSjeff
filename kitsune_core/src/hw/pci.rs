//! PCI configuration-space decoding: addresses, BARs, capability-list start and
//! bus enumeration. The kernel supplies the port I/O; this module supplies the
//! arithmetic.

/// virtio PCI vendor id, and the modern/transitional virtio-gpu device ids.
pub const VIRTIO_VENDOR: u16 = 0x1AF4;
pub const VIRTIO_GPU_MODERN: u16 = 0x1050; // 0x1040 + virtio device type 16 (GPU)
pub const VIRTIO_GPU_LEGACY: u16 = 0x1010;

/// virtio-net device ids: transitional (also what QEMU's default `virtio-net-pci`
/// reports) and modern (0x1040 + device type 1).
pub const VIRTIO_NET_LEGACY: u16 = 0x1000;
pub const VIRTIO_NET_MODERN: u16 = 0x1041;

/// virtio-rng (entropy source) device ids: transitional (QEMU's default `virtio-rng-pci`)
/// and modern (0x1040 + device type 4).
pub const VIRTIO_RNG_LEGACY: u16 = 0x1005;
pub const VIRTIO_RNG_MODERN: u16 = 0x1044;

/// `true` for a modern or transitional virtio-rng `vendor:device` pair.
pub fn is_virtio_rng(vendor: u16, device: u16) -> bool {
    vendor == VIRTIO_VENDOR && (device == VIRTIO_RNG_MODERN || device == VIRTIO_RNG_LEGACY)
}

/// `true` for a modern or transitional virtio-net `vendor:device` pair.
pub fn is_virtio_net(vendor: u16, device: u16) -> bool {
    vendor == VIRTIO_VENDOR && (device == VIRTIO_NET_MODERN || device == VIRTIO_NET_LEGACY)
}

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

/// Number of base address registers in a type-0 header.
pub const BAR_COUNT: u8 = 6;

/// Config-space offset of BAR `index`, or `None` if it is not one of the six.
pub fn bar_offset(index: u8) -> Option<u8> {
    (index < BAR_COUNT).then(|| 0x10 + index * 4)
}

/// Memory base address of BAR `index` from its low dword and the dword of the
/// next slot (`hi`, only used by 64-bit BARs). `None` when the BAR cannot hold
/// an MMIO window: bad index, an I/O-space BAR, a 64-bit BAR in the last slot
/// (no room for its high half), or an unassigned (zero) base.
pub fn memory_bar_base(index: u8, lo: u32, hi: u32) -> Option<u64> {
    if index >= BAR_COUNT || lo & 1 != 0 {
        return None;
    }
    if bar_is_64bit(lo) && index + 1 >= BAR_COUNT {
        return None;
    }
    let base = bar_address(lo, hi);
    (base != 0).then_some(base)
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
mod tests;
