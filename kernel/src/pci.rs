//! Minimal PCI configuration-space access (legacy mechanism #1) and bus
//! enumeration — the foundation for the virtio-gpu driver. It locates devices
//! by reading config space through the 0xCF8/0xCFC port pair.

use crate::io::{inl, outl};

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

/// virtio PCI vendor id, and the modern/transitional virtio-gpu device ids.
pub const VIRTIO_VENDOR: u16 = 0x1AF4;
pub const VIRTIO_GPU_MODERN: u16 = 0x1050; // 0x1040 + virtio device type 16 (GPU)
pub const VIRTIO_GPU_LEGACY: u16 = 0x1010;

/// A located PCI function.
#[derive(Clone, Copy, Debug)]
pub struct PciDevice {
    pub bus: u8,
    pub slot: u8,
    pub func: u8,
    pub vendor: u16,
    pub device: u16,
}

fn address(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    0x8000_0000
        | (bus as u32) << 16
        | (slot as u32) << 11
        | (func as u32) << 8
        | (offset as u32 & 0xFC)
}

/// Read a 32-bit dword from config space (`offset` is dword-aligned).
pub fn read32(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    outl(CONFIG_ADDRESS, address(bus, slot, func, offset));
    inl(CONFIG_DATA)
}

/// Read a 16-bit word from config space.
pub fn read16(bus: u8, slot: u8, func: u8, offset: u8) -> u16 {
    let dword = read32(bus, slot, func, offset & 0xFC);
    ((dword >> ((offset as u32 & 2) * 8)) & 0xFFFF) as u16
}

/// Write a 32-bit dword to config space.
pub fn write32(bus: u8, slot: u8, func: u8, offset: u8, value: u32) {
    outl(CONFIG_ADDRESS, address(bus, slot, func, offset));
    outl(CONFIG_DATA, value);
}

impl PciDevice {
    /// Raw value of base address register `i` (0..6).
    pub fn bar(&self, i: u8) -> u32 {
        read32(self.bus, self.slot, self.func, 0x10 + i * 4)
    }

    /// Set the memory-space + bus-master bits in the command register, required
    /// before a DMA-capable device (like virtio-gpu) can be used.
    pub fn enable_bus_master(&self) {
        let mut cmd = read32(self.bus, self.slot, self.func, 0x04);
        cmd |= 0x0006; // bit1 = memory space, bit2 = bus master
        write32(self.bus, self.slot, self.func, 0x04, cmd);
    }

    /// Offset of the first entry in the PCI capability list, or `None` if the
    /// device has none (status register bit 4 clears it).
    pub fn cap_list(&self) -> Option<u8> {
        let status = read16(self.bus, self.slot, self.func, 0x06);
        if status & 0x10 == 0 {
            return None;
        }
        Some((read16(self.bus, self.slot, self.func, 0x34) & 0xFC) as u8)
    }

    /// Read a config-space dword at `offset` (for walking capabilities).
    pub fn cap_read32(&self, offset: u8) -> u32 {
        read32(self.bus, self.slot, self.func, offset)
    }
}

/// Visit every present function on bus 0, calling `f` for each. QEMU places its
/// virtio devices on bus 0, so a single-bus scan suffices here.
pub fn for_each<F: FnMut(PciDevice)>(mut f: F) {
    for slot in 0..32u8 {
        for func in 0..8u8 {
            let vendor = read16(0, slot, func, 0x00);
            if vendor == 0xFFFF {
                if func == 0 {
                    break; // no function 0 -> the whole slot is empty
                }
                continue;
            }
            let device = read16(0, slot, func, 0x02);
            f(PciDevice {
                bus: 0,
                slot,
                func,
                vendor,
                device,
            });
        }
    }
}

/// Find the first function matching `vendor`/`device` on bus 0.
pub fn find(vendor: u16, device: u16) -> Option<PciDevice> {
    let mut found = None;
    for_each(|d| {
        if found.is_none() && d.vendor == vendor && d.device == device {
            found = Some(d);
        }
    });
    found
}

/// Locate a virtio-gpu device (modern, then transitional).
pub fn find_virtio_gpu() -> Option<PciDevice> {
    find(VIRTIO_VENDOR, VIRTIO_GPU_MODERN).or_else(|| find(VIRTIO_VENDOR, VIRTIO_GPU_LEGACY))
}
